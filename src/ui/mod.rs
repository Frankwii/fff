use crate::{
    App, EnterOutcome,
    bookmarks::{Change as BookmarkChange, Draft as BookmarkDraft, EditField as BookmarkField},
    discovery::Discovery,
    editor::{EditView, Field, Nvim},
    normal_keys::{ListAction, Step},
};
use crossterm::{
    cursor::{Hide, SetCursorStyle, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M,
        KeyboardEnhancementFlags as Flags, MouseButton, MouseEvent, MouseEventKind,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        self, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{Terminal as TuiTerminal, backend::CrosstermBackend};
use std::{fs::OpenOptions, io, path::PathBuf, time::Duration};

mod actions;
mod input;
mod screen;
#[cfg(test)]
mod tests;
pub(crate) use actions::Button;
pub use actions::ScopedAction;
use actions::*;
use input::*;
pub use input::{StatusTimer, alt_numbered_key, numbered_key, start_editor};
use screen::*;

#[derive(Default)]
pub(crate) enum Focus {
    #[default]
    Results,
    Marks {
        rows: Vec<PathBuf>,
        index: usize,
    },
    /// F1 overlay: the key guide, or session details when `keys` is false.
    Info {
        offset: usize,
        keys: bool,
    },
    Exclusions(String),
    /// Alt+f: the file filters (`--glob`), one per line.
    Filters(String),
    Operation(crate::operations::Draft),
    DeleteConfirm(crate::operations::DeletePlan),
    /// Which paths an action applies to, asked only when ambiguous: the current
    /// entry is unmarked while other entries are marked.
    ScopeChoice {
        current: PathBuf,
        selected: Vec<PathBuf>,
        action: ScopedAction,
    },
    TransferConfirm(crate::operations::TransferPlan),
    /// gx / gX: the command line, then its review before it runs.
    Command(crate::run::Prompt),
    RunConfirm(crate::run::Plan),
    NewChoice,
    Location(crate::location::Location),
    Bookmarks {
        query: String,
        index: usize,
    },
    BookmarkEdit(BookmarkDraft),
    BookmarkIcons {
        draft: BookmarkDraft,
        query: String,
        index: usize,
    },
    BookmarkDelete {
        target: usize,
        query: String,
        index: usize,
    },
    /// Alt+p chooser; `original` is restored on Escape after live previews.
    Palettes {
        query: String,
        index: usize,
        original: crate::config::Palette,
    },
}
/// How a session ends.
pub enum Finish {
    /// Confirmed paths, or the directory of a quit key.
    Paths(Vec<PathBuf>),
    /// Normal-mode Enter: exit into this folder (printed with --output-paths).
    Folder(PathBuf),
    /// Ctrl+c.
    Cancelled,
    /// gX: the terminal is restored; the caller replaces fff with the program.
    Exec(crate::run::Plan),
}
/// How long a change to the current directory's listing waits for more
/// changes before the quiet rescan.
const RELIST_DELAY: Duration = Duration::from_millis(150);
/// Runs a session, then remembers where it was for the next one.
pub fn run(
    mut app: App,
    keys: crate::keys::KeyMap,
    cache: crate::config::CacheConfig,
    palettes: crate::reload::PaletteWatcher,
    performance: crate::config::PerformanceConfig,
) -> io::Result<Finish> {
    let finish = session(&mut app, keys, cache, palettes, performance);
    app.remember();
    if let Some(path) = crate::state::path() {
        let last = if app.records_location {
            Some(app.cwd.clone())
        } else {
            crate::state::load(&path).last
        };
        let state = crate::state::State {
            last,
            positions: std::mem::take(&mut app.positions),
        };
        let _ = crate::state::save(&path, &state);
    }
    finish
}
fn session(
    app: &mut App,
    keys: crate::keys::KeyMap,
    cache: crate::config::CacheConfig,
    palettes: crate::reload::PaletteWatcher,
    performance: crate::config::PerformanceConfig,
) -> io::Result<Finish> {
    let invocation = std::env::current_dir()?.canonicalize()?;
    let mut terminal = Terminal::enter(app.mouse)?;
    let (wake, waking) = std::sync::mpsc::sync_channel(1);
    crate::notify::set_waker(wake.clone());
    let mut input = Input::start(wake.clone());
    let mut discovery = Discovery::new(cache);
    discovery.set_waker(wake.clone());
    discovery.refresh_every = Duration::from_millis(performance.refresh_ms);
    app.cached_queries = performance.cached_queries;
    app.details.set_waker(wake.clone());
    let mut watch = app.watch.then(crate::watch::Watch::new).flatten();
    // When a change to the current directory's listing is reconciled.
    let mut relist_at: Option<std::time::Instant> = None;
    let mut details_generation = app.generation;
    // Starting a scan clears the status; keep a startup notice (resuming).
    let notice = std::mem::take(&mut app.status);
    discovery.start(app, true);
    app.status = notice;
    let mut editor = start_editor("nvim")?;
    // A --no-dir session is a file choice: start ready to type the query.
    if app.no_dir {
        editor.reset_insert(Field::Query);
    }
    let mut editor_directory = app.cwd.clone();
    warn_stranded(app);
    let mut cursor_shape = SetCursorStyle::DefaultUserShape;
    let mut focus = Focus::Results;
    let mut status = StatusTimer::default();
    let mut preview = PalettePreview::default();
    // Frames are drawn after input or a visible change, and at least once a
    // second so external changes (e.g. a deleted mark) still appear.
    let mut dirty = true;
    let mut drawn_at = std::time::Instant::now();
    let mut page = 1;
    let mut bookmark_targets = Vec::new();
    let mut results_area = ratatui::layout::Rect::default();
    let mut mouse = Mouse::default();
    let mut buttons = Vec::new();
    let mut tab_areas = Vec::new();
    loop {
        dirty |= status.tick(app);
        // Pop-up notifications from background work, shown for a while.
        let now = std::time::Instant::now();
        for notice in crate::notify::drain() {
            app.notices.push((notice, now));
            dirty = true;
        }
        let shown = app.notices.len();
        app.notices
            .retain(|(_, at)| now.duration_since(*at) < crate::notify::SHOWN_FOR);
        dirty |= app.notices.len() != shown;
        for update in palettes.updates() {
            dirty = true;
            match update {
                Ok(state) => palette_reloaded(app, &mut focus, state),
                Err(error) => {
                    app.status = format!("Palette reload failed; keeping current colors: {error}")
                }
            }
        }
        if let Some(watch) = &mut watch {
            watch.follow(&app.cwd);
            let changes = watch.poll();
            for path in &changes.touched {
                app.details.touched(path);
            }
            dirty |= !changes.touched.is_empty();
            // Coalesce bursts (an extraction, a copy) into one quiet rescan.
            if changes.listing && relist_at.is_none() {
                relist_at = Some(now + RELIST_DELAY);
            }
            if relist_at.is_some_and(|at| now >= at) {
                relist_at = None;
                discovery.changed(app);
            }
        }
        dirty |= discovery.drain(app);
        if app.generation != details_generation {
            details_generation = app.generation;
            app.details.forget();
        }
        dirty |= app.details.drain();
        discovery.idle(
            app,
            matches!(focus, Focus::Results),
            std::time::Instant::now(),
        );
        if editor_directory != app.cwd {
            editor_directory = app.cwd.clone();
            warn_stranded(app);
            // Carry the initiating field's editing mode into the new location.
            {
                let e = &mut editor;
                e.reset_preserving_mode(Field::Query);
            }
        }
        if let Some(view) = app.edit_view.as_ref()
            && view.closed
        {
            if let Focus::Operation(draft) = &focus {
                let request = view.saved.then(|| draft.request());
                focus = Focus::Results;
                if let Some(request) = request {
                    match request {
                        Ok(request) => perform_operation(app, &mut discovery, request),
                        Err(error) => app.status = error,
                    }
                }
            } else if matches!(focus, Focus::Exclusions(_) | Focus::Filters(_)) {
                if view.saved {
                    apply_globs(app, &mut focus, &mut discovery);
                } else {
                    focus = Focus::Results;
                }
            } else if let Focus::BookmarkEdit(draft) = &focus {
                // Enter and Escape are the form's keys; Neovim's own
                // ZZ/:wq and ZQ/:q! still save or cancel it.
                if view.saved {
                    save_bookmark(app, &mut focus);
                } else {
                    focus = Focus::Bookmarks {
                        query: draft.query.clone(),
                        index: draft.index,
                    };
                }
            }
            app.edit_view = None;
            dirty = true;
        }
        if let Some(error) = editor.broken() {
            return Err(io::Error::other(format!("Neovim stopped: {error}")));
        }
        if let Some((field, text)) = text_field(app, &focus) {
            {
                let e = &mut editor;
                match e.activate(field, &text) {
                    Ok(view) => app.edit_view = Some(view),
                    Err(error) => e.break_with(error),
                }
            }
        } else {
            app.edit_view = None;
        }
        if app.normal_keys.pending()
            && let Some(view) = &mut app.edit_view
        {
            // Show a partial list-key sequence like Neovim's own pending input.
            view.blocking = true;
        }
        dirty |= preview.tick(app, &focus, std::time::Instant::now());
        let desired = app
            .edit_view
            .as_ref()
            .map(|view| {
                if view.mode.starts_with('i') {
                    SetCursorStyle::SteadyBar
                } else {
                    SetCursorStyle::SteadyBlock
                }
            })
            .unwrap_or(SetCursorStyle::DefaultUserShape);
        if desired != cursor_shape {
            execute!(terminal.0.backend_mut(), desired)?;
            cursor_shape = desired;
        }
        if dirty || drawn_at.elapsed() >= Duration::from_secs(1) {
            let mut completed = None;
            terminal.synchronize(true)?;
            let drawn = terminal
                .0
                .draw(|frame| completed = Some(crate::view::render(frame, app, &mut focus)))
                .map(|_| ());
            let ended = terminal.synchronize(false);
            drawn?;
            ended?;
            let completed = completed.expect("terminal draw callback completes");
            // Only publish targets after Terminal::draw has flushed the frame.
            if matches!(focus, Focus::Results) {
                app.rendered = completed.targets;
            }
            results_area = completed.results;
            buttons = completed.buttons;
            tab_areas = completed.tabs;
            bookmark_targets = completed.bookmark_targets;
            page = completed.page.max(1);
            dirty = false;
            drawn_at = std::time::Instant::now();
        }
        // Both input and discovery wake this wait immediately. The timeout only
        // services due re-sorts, status expiry, palette reload and idle prefetch.
        let wait = if discovery.busy(app) || app.normal_keys.pending() {
            30
        } else {
            250
        };
        let wait = relist_at
            .map(|at| at.saturating_duration_since(std::time::Instant::now()))
            .map_or(wait, |delay| wait.min(delay.as_millis() as u64));
        let wait = preview
            .remaining(std::time::Instant::now())
            .map_or(Duration::from_millis(wait), |delay| {
                delay.min(Duration::from_millis(wait))
            });
        let event = match input.events.try_recv() {
            Ok(event) => event?,
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                let _ = waking.recv_timeout(wait);
                continue;
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                return Err(io::Error::other("terminal input reader stopped"));
            }
        };
        dirty = true;
        if let Event::Paste(text) = event {
            // A held-back Normal-mode prefix reaches Neovim before the paste.
            for pending in app.normal_keys.take_pending() {
                deliver(app, &mut focus, &mut editor, pending);
            }
            // Terminals paste the paths of files dropped onto them. While
            // browsing (Normal mode), existing paths are copied here after
            // confirmation; while typing, a paste is only text.
            if matches!(focus, Focus::Results)
                && !app.no_dir
                && app.edit_view.as_ref().is_some_and(|v| v.normal())
                && let Some(paths) = dropped_paths(&text, app.home.as_deref())
            {
                use crate::operations::{Transfer, TransferPlan};
                match TransferPlan::new(Transfer::Copy, paths, app.cwd.clone()) {
                    Ok(mut plan) => {
                        plan.dropped = true;
                        focus = Focus::TransferConfirm(plan);
                    }
                    Err(error) => app.status = error,
                }
                continue;
            }
            let field = text_field(app, &focus).map(|(field, _)| field);
            if let Some(field) = field {
                let text = if matches!(field, Field::Exclusions | Field::Filters | Field::Operation)
                {
                    text
                } else {
                    text.replace(['\r', '\n'], " ")
                };
                match &mut focus {
                    Focus::Results => {
                        paste(
                            &mut app.query,
                            &text,
                            field,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        app.offset = 0;
                        app.refresh();
                    }
                    Focus::Operation(draft) => paste(
                        &mut draft.text,
                        &text,
                        field,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    ),
                    Focus::Location(location) => {
                        location.selected = None;
                        paste(
                            &mut location.text,
                            &text,
                            field,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        )
                    }
                    Focus::Exclusions(value) | Focus::Filters(value) => paste(
                        value,
                        &text,
                        field,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    ),
                    Focus::Bookmarks { query, index } => {
                        paste(
                            query,
                            &text,
                            field,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        *index = 0;
                    }
                    Focus::BookmarkEdit(draft) => {
                        if let Some(field) = draft.field.editor() {
                            let value = if draft.field == BookmarkField::Name {
                                &mut draft.name
                            } else {
                                &mut draft.path
                            };
                            paste(
                                value,
                                &text,
                                field,
                                &mut editor,
                                &mut app.edit_view,
                                &mut app.status,
                            );
                        }
                    }
                    Focus::BookmarkIcons { query, index, .. } => {
                        paste(
                            query,
                            &text,
                            Field::BookmarkIcons,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        *index = 0;
                    }
                    Focus::Palettes { query, index, .. } => {
                        paste(
                            query,
                            &text,
                            field,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        *index = 0;
                    }
                    _ => {}
                }
            }
            continue;
        }
        // A button that stands for a key goes on as that key.
        let event = match event {
            Event::Mouse(event) => {
                app.mouse_used = true;
                if app.normal_keys.pending() {
                    continue;
                }
                // A click on a tab's number switches to it, keeping the marks.
                let position = ratatui::layout::Position::new(event.column, event.row);
                if event.kind == MouseEventKind::Down(MouseButton::Left)
                    && matches!(focus, Focus::Results)
                    && let Some(&(_, tab)) = tab_areas
                        .iter()
                        .find(|(area, _): &&(ratatui::layout::Rect, usize)| area.contains(position))
                {
                    if tab != app.tab && app.can_navigate() && app.select_tab(tab) {
                        discovery.start(app, true);
                    }
                    continue;
                }
                let button = (event.kind == MouseEventKind::Down(MouseButton::Left))
                    .then(|| {
                        buttons.iter().find(|(area, _)| {
                            area.contains(ratatui::layout::Position::new(event.column, event.row))
                        })
                    })
                    .flatten();
                match button {
                    Some(&(_, button)) => match button.key() {
                        Some(key) => Event::Key(key),
                        None => {
                            if let Some(finish) = press(app, &mut discovery, button) {
                                return Ok(finish);
                            }
                            continue;
                        }
                    },
                    None => {
                        if matches!(focus, Focus::Results) {
                            mouse_results(app, &mut discovery, event, results_area, &mut mouse);
                        }
                        continue;
                    }
                }
            }
            event => event,
        };
        let Event::Key(key) = event else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        status.before_key(app);
        let Some(key) = keys.translate(key) else {
            continue;
        };
        if key.modifiers.contains(M::CONTROL) && key.code == K::Char('c') {
            return Ok(Finish::Cancelled);
        }
        // Multiline buffers keep Neovim's write/close lifecycle; single-line
        // names and destinations take Enter/Escape like the bookmark form.
        let buffer_input = match &focus {
            Focus::Exclusions(_) | Focus::Filters(_) => true,
            Focus::Operation(draft) => draft.multiline(),
            _ => false,
        };
        if buffer_input {
            deliver(app, &mut focus, &mut editor, key);
            continue;
        }
        let mut operation = false;
        let key = match normal_list_key(app, &mut focus, &mut editor, key) {
            NormalKey::Pass(key) => key,
            // Configured sequences resolve to the operation's default letter.
            NormalKey::Operation(letter) => {
                operation = true;
                KeyEvent::new(K::Char(letter), M::NONE)
            }
            NormalKey::Consumed => continue,
            NormalKey::Depth(depth) => {
                if app.depth != depth {
                    app.depth = depth;
                    discovery.start(app, false);
                }
                continue;
            }
            NormalKey::Quit(current) => {
                if app.no_dir {
                    return Ok(Finish::Paths(Vec::new()));
                }
                return Ok(Finish::Paths(vec![if current {
                    app.cwd.clone()
                } else {
                    invocation
                }]));
            }
            NormalKey::Run(exit) => {
                let current = match &focus {
                    Focus::Marks { rows, index } => rows.get(*index).cloned(),
                    _ => app.highlight.clone(),
                };
                focus = match scope(&app.marks, current) {
                    Scope::Ready(paths) => start_run(&mut editor, paths, exit),
                    Scope::Ask(current, selected) => Focus::ScopeChoice {
                        current,
                        selected,
                        action: ScopedAction::Run(exit),
                    },
                    // A program may run on no files, in the browsing directory.
                    Scope::Nothing => start_run(&mut editor, vec![], exit),
                };
                continue;
            }
        };
        if operation {
            let current = match &focus {
                Focus::Marks { rows, index } => rows.get(*index).cloned(),
                _ => app.highlight.clone(),
            };
            match key.code {
                K::Char('v' | 'p' | 's') => {
                    let sources = app.marks.to_vec();
                    if sources.is_empty() {
                        app.status = "No files selected".into();
                    } else {
                        use crate::operations::Transfer;
                        // v/p/s are common Vim keys: every transfer is confirmed.
                        let kind = match key.code {
                            K::Char('v') => Transfer::Move,
                            K::Char('p') => Transfer::Copy,
                            _ => Transfer::Link,
                        };
                        match crate::operations::TransferPlan::new(kind, sources, app.cwd.clone()) {
                            Ok(plan) => focus = Focus::TransferConfirm(plan),
                            Err(error) => app.status = error,
                        }
                    }
                }
                K::Char('x') => match scope(&app.marks, current) {
                    Scope::Ready(paths) => focus = delete_confirmation(paths, &mut app.status),
                    Scope::Ask(current, selected) => {
                        focus = Focus::ScopeChoice {
                            current,
                            selected,
                            action: ScopedAction::Delete,
                        }
                    }
                    Scope::Nothing => app.status = "No current file".into(),
                },
                K::Char('n') => focus = Focus::NewChoice,
                // $EDITOR on the current or marked paths: no review, it only edits.
                K::Char('E') => match scope(&app.marks, current) {
                    Scope::Ask(current, selected) => {
                        focus = Focus::ScopeChoice {
                            current,
                            selected,
                            action: ScopedAction::Edit,
                        }
                    }
                    Scope::Ready(paths) => {
                        edit_files(&mut terminal, &mut input, &wake, app, &mut discovery, paths)?
                    }
                    Scope::Nothing => app.status = "Nothing to edit".into(),
                },
                // The opener takes over this terminal until it ends.
                K::Char('o') => match scope(&app.marks, current) {
                    Scope::Ask(current, selected) => {
                        focus = Focus::ScopeChoice {
                            current,
                            selected,
                            action: ScopedAction::Open,
                        }
                    }
                    Scope::Ready(paths) => {
                        open_here(&mut terminal, &mut input, &wake, app, &mut discovery, paths)?
                    }
                    Scope::Nothing => app.status = "Nothing to open".into(),
                },
                K::Char('r') => {
                    let sources = app
                        .rows
                        .iter()
                        .map(|row| row.path.clone())
                        .collect::<Vec<_>>();
                    if sources.is_empty() {
                        app.status = "No files to rename".into();
                    } else {
                        match crate::operations::rename_text(&sources, &app.cwd) {
                            Ok(text) => {
                                // Start on the highlighted row, ready to append.
                                let row = sources
                                    .iter()
                                    .position(|p| Some(p) == app.highlight.as_ref())
                                    .unwrap_or(0);
                                {
                                    let e = &mut editor;
                                    e.reset_at_row(Field::Operation, row);
                                }
                                focus = Focus::Operation(crate::operations::Draft {
                                    kind: crate::operations::EditKind::Rename(sources),
                                    cwd: app.cwd.clone(),
                                    text,
                                });
                            }
                            Err(error) => app.status = error,
                        }
                    }
                }
                K::Char('e') => match scope(&app.marks, current) {
                    Scope::Ready(paths) => focus = start_extract(app, &mut editor, paths),
                    Scope::Ask(current, selected) => {
                        focus = Focus::ScopeChoice {
                            current,
                            selected,
                            action: ScopedAction::Extract,
                        }
                    }
                    Scope::Nothing => app.status = "Select an extractable archive first".into(),
                },
                _ => {}
            }
            continue;
        }
        let capture = app.edit_view.as_ref().is_some_and(|v| {
            (key.code == K::Esc && !v.normal())
                || (v.command() && !key.modifiers.intersects(M::ALT | M::CONTROL))
        });
        if capture {
            match &mut focus {
                Focus::Results => {
                    edit(
                        &mut app.query,
                        key,
                        false,
                        Field::Query,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                    app.refresh();
                }
                Focus::Exclusions(value) => edit(
                    value,
                    key,
                    true,
                    Field::Exclusions,
                    &mut editor,
                    &mut app.edit_view,
                    &mut app.status,
                ),
                Focus::Filters(value) => edit(
                    value,
                    key,
                    true,
                    Field::Filters,
                    &mut editor,
                    &mut app.edit_view,
                    &mut app.status,
                ),
                Focus::Operation(draft) => {
                    let (field, multiline) = (draft.field(), draft.multiline());
                    edit(
                        &mut draft.text,
                        key,
                        multiline,
                        field,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    )
                }
                Focus::Location(location) => edit(
                    &mut location.text,
                    key,
                    false,
                    Field::Location,
                    &mut editor,
                    &mut app.edit_view,
                    &mut app.status,
                ),
                Focus::Bookmarks { query, index } => {
                    edit(
                        query,
                        key,
                        false,
                        Field::Bookmarks,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                    *index = 0;
                }
                Focus::BookmarkEdit(draft) => {
                    if let Some(field) = draft.field.editor() {
                        let text = if draft.field == BookmarkField::Name {
                            &mut draft.name
                        } else {
                            &mut draft.path
                        };
                        edit(
                            text,
                            key,
                            false,
                            field,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                    }
                }
                Focus::BookmarkIcons { query, index, .. } => {
                    let before = query.clone();
                    edit(
                        query,
                        key,
                        false,
                        Field::BookmarkIcons,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                    if *query != before {
                        *index = 0;
                    }
                }
                Focus::Palettes { query, index, .. } => {
                    let before = query.clone();
                    edit(
                        query,
                        key,
                        false,
                        Field::Palettes,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                    if *query != before {
                        *index = 0;
                    }
                }
                _ => {}
            }
            continue;
        }
        let ctrl = key.modifiers.contains(M::CONTROL);
        let alt = key.modifiers.contains(M::ALT);
        if matches!(focus, Focus::Results)
            && let Some(digit) = numbered_key(key)
        {
            // Static (--no-dir) sessions never fall back to the current directory.
            if app.no_dir && app.rendered.is_empty() {
                continue;
            }
            let target = app.numbered(digit);
            if app.ready_path(&target) {
                return Ok(Finish::Paths(vec![target]));
            }
            continue;
        }
        if matches!(focus, Focus::Results)
            && let Some(digit) = alt_numbered_key(key)
        {
            // Like l on that row: enter a directory, open a file in the background.
            discovery.checkpoint(app);
            if app.rendered.is_empty() {
                continue;
            }
            let path = app.numbered(digit);
            let Some(target) = app.rows.iter().find(|c| c.path == path).cloned() else {
                continue;
            };
            app.highlight = Some(target.path.clone());
            if target.directory {
                if app.ready_path(&target.path) && app.can_navigate() {
                    app.navigate(target.path.clone(), false);
                    discovery.start(app, true);
                }
            } else {
                open_in_background(app, &target.path);
            }
            continue;
        }
        if matches!(focus, Focus::Bookmarks { .. })
            && let Some(digit) = alt_numbered_key(key)
        {
            let index = if digit == 0 {
                usize::MAX
            } else {
                digit as usize - 1
            };
            if let Some(&target) =
                bookmark_targets.get(index.min(bookmark_targets.len().saturating_sub(1)))
            {
                discovery.checkpoint(app);
                if app.open_bookmark(target) {
                    discovery.start(app, true);
                    focus = Focus::Results;
                }
            }
            continue;
        }
        if let Focus::Bookmarks { query, index } = &focus {
            let matches = app.matching_bookmarks(query);
            let target = matches.get(*index).copied();
            // Plain letters act while the filter is in Normal mode, like the
            // results' file operations; Alt keeps them reachable while typing.
            let normal =
                key.modifiers.is_empty() && app.edit_view.as_ref().is_some_and(|v| v.normal());
            let command = |c| key.code == K::Char(c) && (alt || normal);
            let next = if command('n') {
                Some(BookmarkDraft::new(&app.cwd, None, query.clone(), *index))
            } else if command('e') {
                target.map(|i| {
                    BookmarkDraft::new(
                        &app.cwd,
                        Some((i, &app.bookmarks[i])),
                        query.clone(),
                        *index,
                    )
                })
            } else {
                None
            };
            if let Some(next) = next {
                match next {
                    Ok(draft) => {
                        {
                            let e = &mut editor;
                            e.reset(Field::BookmarkName);
                            e.reset(Field::BookmarkPath);
                        }
                        focus = Focus::BookmarkEdit(draft);
                    }
                    Err(error) => app.status = error,
                }
                continue;
            }
            if command('d') {
                if let Some(target) = target {
                    focus = Focus::BookmarkDelete {
                        target,
                        query: query.clone(),
                        index: *index,
                    };
                }
                continue;
            }
            if matches!(key.code, K::Up | K::Down) {
                // Matches keep configuration order, so a filtered list moves
                // the highlight past its visible neighbour.
                let neighbor = if key.code == K::Up {
                    index.checked_sub(1)
                } else {
                    Some(*index + 1)
                };
                if let (Some(target), Some((neighbor, &other))) =
                    (target, neighbor.and_then(|n| Some((n, matches.get(n)?))))
                {
                    match app
                        .bookmark_store
                        .commit(&app.bookmarks, &BookmarkChange::Swap(target, other))
                    {
                        Ok(bookmarks) => {
                            app.bookmarks = bookmarks;
                            focus = Focus::Bookmarks {
                                query: query.clone(),
                                index: neighbor,
                            };
                        }
                        Err(error) => app.status = error,
                    }
                }
                continue;
            }
            if command('r') {
                match app.bookmark_store.reload() {
                    Ok(bookmarks) => {
                        app.bookmarks = bookmarks;
                        {
                            let e = &mut editor;
                            e.reset(Field::Bookmarks);
                        }
                        focus = Focus::Bookmarks {
                            query: String::new(),
                            index: 0,
                        };
                        app.status = "Bookmarks reloaded".into();
                    }
                    Err(error) => app.status = error,
                }
                continue;
            }
        }
        let navigation =
            key.code == K::Enter || (alt && matches!(key.code, K::Char('h' | 'l' | '0'..='9')));
        if navigation
            && matches!(
                focus,
                Focus::Results | Focus::Bookmarks { .. } | Focus::Location(_)
            )
        {
            discovery.checkpoint(app);
        }
        let palette = palette_key(key);
        let glob_field = if matches!(focus, Focus::Filters(_)) {
            Field::Filters
        } else {
            Field::Exclusions
        };
        match &mut focus {
            Focus::NewChoice => match key.code {
                K::Char('f' | 'd') => {
                    {
                        let e = &mut editor;
                        e.reset_insert(Field::FileName);
                    }
                    focus = Focus::Operation(crate::operations::Draft {
                        kind: crate::operations::EditKind::New(key.code == K::Char('d')),
                        cwd: app.cwd.clone(),
                        text: String::new(),
                    });
                }
                K::Esc => focus = Focus::Results,
                _ => {}
            },
            Focus::ScopeChoice {
                current,
                selected,
                action,
            } => {
                let paths = match key.code {
                    K::Char('c') => vec![current.clone()],
                    K::Char('s') => selected.clone(),
                    K::Char('a') => selected.iter().chain([&*current]).cloned().collect(),
                    K::Esc => {
                        focus = Focus::Results;
                        continue;
                    }
                    _ => continue,
                };
                focus = match action {
                    ScopedAction::Extract => start_extract(app, &mut editor, paths),
                    ScopedAction::Delete => delete_confirmation(paths, &mut app.status),
                    ScopedAction::Run(exit) => start_run(&mut editor, paths, *exit),
                    ScopedAction::Edit => {
                        focus = Focus::Results;
                        edit_files(&mut terminal, &mut input, &wake, app, &mut discovery, paths)?;
                        continue;
                    }
                    ScopedAction::Open => {
                        focus = Focus::Results;
                        open_here(&mut terminal, &mut input, &wake, app, &mut discovery, paths)?;
                        continue;
                    }
                };
            }
            Focus::TransferConfirm(plan) => match key.code {
                K::Enter => {
                    let request = plan.request();
                    let marks = plan.dropped.then(|| app.marks.clone());
                    focus = Focus::Results;
                    match request {
                        Ok(r) => perform_operation(app, &mut discovery, r),
                        Err(e) => app.status = e,
                    }
                    if let Some(marks) = marks {
                        app.marks = marks;
                    }
                }
                K::Esc => {
                    app.status = match plan.kind {
                        crate::operations::Transfer::Move => "Move cancelled; nothing changed",
                        crate::operations::Transfer::Copy => "Copy cancelled; nothing changed",
                        crate::operations::Transfer::Link => "Link cancelled; nothing changed",
                    }
                    .into();
                    focus = Focus::Results;
                }
                _ => {}
            },
            Focus::DeleteConfirm(plan) => match key.code {
                K::Enter => {
                    let request = plan.request();
                    focus = Focus::Results;
                    match request {
                        Ok(r) => perform_operation(app, &mut discovery, r),
                        Err(e) => app.status = e,
                    }
                }
                K::Esc => focus = Focus::Results,
                _ => {}
            },
            Focus::Operation(draft) => {
                // Neovim rename lists were routed above and only their written
                // close commits. Single-line buffers reach here outside
                // Neovim's Insert/Visual Escape and command-line Enter.
                if key.code == K::Esc {
                    focus = Focus::Results;
                } else if key.code == K::Enter && !draft.multiline() {
                    // Validation errors keep the buffer open for correction.
                    match draft.request() {
                        Ok(r) => {
                            focus = Focus::Results;
                            perform_operation(app, &mut discovery, r);
                        }
                        Err(e) => app.status = e,
                    }
                } else if ctrl && key.code == K::Char('s') {
                    let request = draft.request();
                    focus = Focus::Results;
                    match request {
                        Ok(r) => perform_operation(app, &mut discovery, r),
                        Err(e) => app.status = e,
                    }
                } else {
                    let (field, multiline) = (draft.field(), draft.multiline());
                    edit(
                        &mut draft.text,
                        key,
                        multiline,
                        field,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                }
            }
            Focus::Palettes {
                query,
                index,
                original,
            } => {
                let matches = app.matching_palettes(query);
                let last = matches.len().saturating_sub(1);
                let moved = match key.code {
                    _ if palette == Some(PaletteKey::Reload) => {
                        // Drop the preview first so a failed reload keeps the
                        // colors that were actually applied.
                        app.palette = *original;
                        focus = Focus::Results;
                        reload_palette(app, &palettes);
                        continue;
                    }
                    _ if key.code == K::Esc || palette == Some(PaletteKey::Choose) => {
                        app.palette = *original;
                        focus = Focus::Results;
                        continue;
                    }
                    K::Enter => {
                        if let Some(&i) = matches.get(*index) {
                            let (name, chosen) = app.palettes[i].clone();
                            palettes.choose(&name);
                            app.palette = chosen;
                            app.status = format!("Palette {name} applied for this session");
                            app.palette_name = name;
                            focus = Focus::Results;
                        }
                        continue;
                    }
                    K::Down | K::Char('j') if key.code == K::Down || alt => {
                        cycle(*index, matches.len(), true)
                    }
                    K::Up | K::Char('k') if key.code == K::Up || alt => {
                        cycle(*index, matches.len(), false)
                    }
                    K::PageDown => (*index + page).min(last),
                    K::PageUp => index.saturating_sub(page),
                    _ => {
                        let before = query.clone();
                        edit(
                            query,
                            key,
                            false,
                            Field::Palettes,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        if *query == before { *index } else { 0 }
                    }
                };
                *index = moved;
            }
            Focus::Bookmarks { query, index } => {
                let matches = app.matching_bookmarks(query);
                match key.code {
                    K::Esc => focus = Focus::Results,
                    K::Char('b') if alt => focus = Focus::Results,
                    // Up/Down reorder above; Alt+j/k (or Normal j/k) move.
                    K::Char('j') if alt => *index = cycle(*index, matches.len(), true),
                    K::Char('k') if alt => *index = cycle(*index, matches.len(), false),
                    K::PageDown => *index = (*index + page).min(matches.len().saturating_sub(1)),
                    K::PageUp => *index = index.saturating_sub(page),
                    K::Enter => {
                        if let Some(&target) = matches.get(*index)
                            && app.open_bookmark(target)
                        {
                            discovery.start(app, true);
                            focus = Focus::Results;
                        }
                    }
                    _ => {
                        let before = query.clone();
                        edit(
                            query,
                            key,
                            false,
                            Field::Bookmarks,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        if *query != before {
                            *index = 0;
                        }
                    }
                }
            }
            Focus::BookmarkEdit(draft) => {
                // Neovim's Insert/Visual Escape and command-line Enter were
                // captured above, so these keys act on the form itself.
                if key.code == K::Esc {
                    focus = Focus::Bookmarks {
                        query: draft.query.clone(),
                        index: draft.index,
                    };
                } else if key.code == K::Enter {
                    save_bookmark(app, &mut focus);
                } else if matches!(key.code, K::Tab | K::BackTab | K::Up | K::Down) {
                    draft.field = draft.field.next(
                        matches!(key.code, K::BackTab | K::Up) || key.modifiers.contains(M::SHIFT),
                    );
                } else if draft.field == BookmarkField::Icon && key.code == K::Char(' ') {
                    let index = crate::icons::BOOKMARK_ICONS
                        .iter()
                        .position(|(_, glyph)| glyph.map(|g| g.to_string()) == draft.icon)
                        .unwrap_or(0);
                    {
                        let e = &mut editor;
                        e.reset(Field::BookmarkIcons);
                    }
                    focus = Focus::BookmarkIcons {
                        draft: draft.clone(),
                        query: String::new(),
                        index,
                    };
                } else if let Some(field) = draft.field.editor() {
                    let text = if draft.field == BookmarkField::Name {
                        &mut draft.name
                    } else {
                        &mut draft.path
                    };
                    edit(
                        text,
                        key,
                        false,
                        field,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                }
            }
            Focus::BookmarkIcons {
                draft,
                query,
                index,
            } => {
                let matches = crate::icons::matching_bookmark_icons(query);
                match key.code {
                    K::Esc => focus = Focus::BookmarkEdit(draft.clone()),
                    K::Enter => {
                        if let Some(&i) = matches.get(*index) {
                            draft.icon = crate::icons::BOOKMARK_ICONS[i].1.map(|g| g.to_string());
                            focus = Focus::BookmarkEdit(draft.clone());
                        }
                    }
                    K::Down | K::Char('j') if key.code == K::Down || alt => {
                        *index = cycle(*index, matches.len(), true)
                    }
                    K::Up | K::Char('k') if key.code == K::Up || alt => {
                        *index = cycle(*index, matches.len(), false)
                    }
                    K::PageDown => *index = (*index + page).min(matches.len().saturating_sub(1)),
                    K::PageUp => *index = index.saturating_sub(page),
                    _ => {
                        let before = query.clone();
                        edit(
                            query,
                            key,
                            false,
                            Field::BookmarkIcons,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        if *query != before {
                            *index = 0;
                        }
                    }
                }
            }
            Focus::BookmarkDelete {
                target,
                query,
                index,
            } => {
                if key.code == K::Esc {
                    focus = Focus::Bookmarks {
                        query: query.clone(),
                        index: *index,
                    };
                } else if key.code == K::Enter {
                    match app
                        .bookmark_store
                        .commit(&app.bookmarks, &BookmarkChange::Remove(*target))
                    {
                        Ok(bookmarks) => {
                            app.bookmarks = bookmarks;
                            app.status = bookmark_saved(app, "Bookmark deleted");
                            focus = Focus::Bookmarks {
                                query: String::new(),
                                index: (*target).min(app.bookmarks.len().saturating_sub(1)),
                            };
                        }
                        Err(error) => app.status = error,
                    }
                }
            }
            Focus::Info { offset, keys } => match key.code {
                K::Esc | K::F(1) | K::Char('q') => focus = Focus::Results,
                K::Tab | K::BackTab | K::Left | K::Right => {
                    *keys = !*keys;
                    *offset = 0;
                }
                K::Down | K::Char('j') => *offset = offset.saturating_add(1),
                K::Up | K::Char('k') => *offset = offset.saturating_sub(1),
                K::PageDown => *offset = offset.saturating_add(page),
                K::PageUp => *offset = offset.saturating_sub(page),
                _ => {}
            },
            Focus::Exclusions(text) | Focus::Filters(text) => {
                if key.code == K::Esc {
                    focus = Focus::Results;
                } else {
                    edit(
                        text,
                        key,
                        true,
                        glob_field,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                }
            }
            Focus::Command(prompt) => {
                if key.code == K::Enter {
                    // An unknown command or bad quoting aborts with the reason.
                    match prompt.plan(&app.cwd, app.home.as_deref()) {
                        Ok(plan) => focus = Focus::RunConfirm(plan),
                        Err(error) => {
                            app.status = error;
                            focus = Focus::Results;
                        }
                    }
                } else if key.code == K::Esc {
                    focus = Focus::Results;
                } else if key.code == K::Tab {
                    prompt.complete(&app.cwd, app.home.as_deref());
                    // Hand the completed line back to Neovim, still typing.
                    {
                        let e = &mut editor;
                        e.reset_preserving_mode(Field::Command);
                    }
                } else {
                    edit(
                        &mut prompt.text,
                        key,
                        false,
                        Field::Command,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                }
            }
            Focus::RunConfirm(plan) => match key.code {
                K::Enter if plan.exit => return Ok(Finish::Exec(plan.clone())),
                K::Enter => {
                    let plan = plan.clone();
                    focus = Focus::Results;
                    run_here(&mut terminal, &mut input, &wake, app, &mut discovery, &plan)?;
                }
                K::Esc => {
                    app.status = "Run cancelled; nothing ran".into();
                    focus = Focus::Results;
                }
                _ => {}
            },
            Focus::Location(location) => {
                if key.code == K::Enter {
                    match location
                        .target(&app.cwd, app.home.as_deref())
                        .and_then(|p| p.canonicalize().map_err(|e| e.to_string()))
                    {
                        Ok(path) if path.is_dir() => {
                            app.navigate(path, false);
                            discovery.start(app, true);
                            focus = Focus::Results;
                        }
                        Ok(_) => app.status = "Location must be an accessible directory".into(),
                        Err(error) => app.status = error,
                    }
                } else if key.code == K::Esc {
                    focus = Focus::Results;
                } else if matches!(key.code, K::Up | K::Down) {
                    location.choose(key.code == K::Down);
                } else if key.code == K::Tab {
                    location.complete(&app.cwd, app.home.as_deref());
                    {
                        let e = &mut editor;
                        e.reset_preserving_mode(Field::Location);
                    }
                } else {
                    let before = location.text.clone();
                    edit(
                        &mut location.text,
                        key,
                        false,
                        Field::Location,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                    if location.text != before {
                        location.selected = None;
                    }
                }
            }
            Focus::Marks { rows, index } => {
                if key.code == K::Esc || (alt && key.code == K::Char(' ')) {
                    focus = Focus::Results;
                } else if key.code == K::Tab {
                    if let Some(p) = rows.get(*index) {
                        app.toggle(p.clone());
                    }
                } else if key.code == K::Down || (alt && key.code == K::Char('j')) {
                    *index = cycle(*index, rows.len(), true);
                } else if key.code == K::Up || (alt && key.code == K::Char('k')) {
                    *index = cycle(*index, rows.len(), false);
                } else if ctrl
                    && key.code == K::Char('w')
                    && let Some(p) = app.confirm(None)
                {
                    return Ok(Finish::Paths(p));
                } else if key.code == K::Enter
                    && let Some(mark) = rows.get(*index)
                {
                    // Like Normal-mode Enter in the results: exit into its folder.
                    let folder = if mark.is_dir() {
                        mark.clone()
                    } else {
                        mark.parent().unwrap_or(&app.cwd).to_owned()
                    };
                    return Ok(Finish::Folder(folder));
                }
            }
            Focus::Results => match (key.code, ctrl, alt) {
                (K::F(1), _, _) => {
                    focus = Focus::Info {
                        offset: 0,
                        keys: true,
                    }
                }
                (K::Char('w'), true, _) => {
                    if let Some(p) = app.confirm(None) {
                        return Ok(Finish::Paths(p));
                    }
                }
                // Ctrl+i needs a terminal that tells it apart from Tab (Kitty
                // keyboard protocol); elsewhere it arrives as Tab and marks.
                (K::Char('o' | 'i'), true, false) if app.can_navigate() => {
                    if app.jump(key.code == K::Char('o')) {
                        discovery.start(app, true);
                    } else {
                        app.status = if key.code == K::Char('o') {
                            "No earlier directory to go back to"
                        } else {
                            "No later directory to go forward to"
                        }
                        .into();
                    }
                }
                // Tabs keep their own directory and history; marks are shared.
                (K::Up | K::Down, false, true) => {
                    // Up counts up to the next tab.
                    if app.can_navigate() && app.switch_tab(key.code == K::Up) {
                        discovery.start(app, true);
                    }
                }
                (K::Char(' '), _, true) => {
                    focus = Focus::Marks {
                        rows: app.marks.to_vec(),
                        index: 0,
                    }
                }
                (K::Char('e'), _, true) => focus = Focus::Exclusions(app.policy.rules.join("\n")),
                (K::Char('f'), _, true) => focus = Focus::Filters(app.policy.filters.join("\n")),
                (K::Char('g' | 'b' | 'h'), _, true) if !app.can_navigate() => {}
                (K::Char('g'), _, true) => {
                    let bookmarks = app
                        .bookmarks
                        .iter()
                        .map(|b| (b.name.clone(), b.path.clone()))
                        .collect::<Vec<_>>();
                    let frequent = crate::location::zoxide(&app.cwd);
                    // It exists for typing, like the run prompt.
                    {
                        let e = &mut editor;
                        e.reset_insert(Field::Location);
                    }
                    focus = Focus::Location(crate::location::Location::new(
                        &bookmarks, frequent, &app.cwd,
                    ));
                }
                (K::Char('b'), _, true) => {
                    focus = Focus::Bookmarks {
                        query: String::new(),
                        index: 0,
                    }
                }
                (K::Char('l'), _, true) => activate_highlighted(app, &mut discovery),
                (K::Char('p' | 'P'), _, true) if palette == Some(PaletteKey::Reload) => {
                    reload_palette(app, &palettes)
                }
                (K::Char('p'), _, true) => {
                    let index = app
                        .palettes
                        .iter()
                        .position(|(name, _)| *name == app.palette_name)
                        .or_else(|| app.palettes.iter().position(|(_, p)| *p == app.palette))
                        .unwrap_or(0);
                    focus = Focus::Palettes {
                        query: String::new(),
                        index,
                        original: app.palette,
                    };
                }
                (K::F(5), _, _) => discovery.force_refresh(app),
                (K::Char('h'), _, true) => {
                    if let Some(p) = app.cwd.parent().map(PathBuf::from) {
                        app.navigate(p, true);
                        discovery.start(app, true);
                    }
                }
                (K::Char('j'), _, true) | (K::Down, false, false) => app.step(true),
                (K::Up, false, false) | (K::Char('k'), _, true) => app.step(false),
                (K::PageDown, _, _) => {
                    app.move_by(page as isize);
                    app.offset = app
                        .offset
                        .saturating_add(page)
                        .min(app.rows.len().saturating_sub(1));
                }
                (K::PageUp, _, _) => {
                    app.move_by(-(page as isize));
                    app.offset = app.offset.saturating_sub(page);
                }
                (K::Right, _, true) => {
                    app.depth = app.depth.saturating_add(1);
                    discovery.start(app, false);
                }
                (K::Left, _, true) => {
                    app.depth = app.depth.saturating_sub(1).max(1);
                    discovery.start(app, false);
                }
                (K::Char('.'), _, true) => {
                    app.policy.hidden = !app.policy.hidden;
                    discovery.start(app, false);
                }
                (K::Char('H'), _, true) => {
                    app.policy.ignore = !app.policy.ignore;
                    discovery.start(app, false);
                }
                (K::Esc, _, _) => {
                    app.query.clear();
                    app.offset = 0;
                    app.refresh();
                }
                (K::Tab, _, _) => {
                    if let Some(p) = app.highlight.clone() {
                        app.toggle(p);
                        app.move_by(1);
                    }
                }
                (K::Enter, _, _) => {
                    let target = app.highlighted_row().map(|c| crate::Candidate::clone(c));
                    // Normal mode: exit into the entry's folder (a directory
                    // itself), like l then !, but for deep results too.
                    if app.edit_view.as_ref().is_some_and(|v| v.normal()) {
                        let folder = match &target {
                            Some(c) if c.directory => c.path.clone(),
                            Some(c) => c.path.parent().unwrap_or(&app.cwd).to_owned(),
                            None => app.cwd.clone(),
                        };
                        return Ok(Finish::Folder(folder));
                    }
                    match target {
                        // Typing: a file is revealed in its folder, unless
                        // fff is picking paths for a script.
                        Some(file) if !file.directory && !app.output_paths => {
                            if app.reveal(file.path) {
                                discovery.start(app, true);
                            }
                        }
                        target => match app.enter(target) {
                            EnterOutcome::Selected(paths) => return Ok(Finish::Paths(paths)),
                            EnterOutcome::Navigated => discovery.start(app, true),
                            EnterOutcome::Stay => {}
                        },
                    }
                }
                _ => {
                    let before = app.query.clone();
                    edit(
                        &mut app.query,
                        key,
                        false,
                        Field::Query,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                    if app.query != before {
                        app.offset = 0;
                        app.refresh();
                    }
                }
            },
        }
    }
}
