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
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode as K, KeyEvent,
        KeyEventKind, KeyModifiers as M, KeyboardEnhancementFlags as Flags,
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
    Location(String),
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
struct Terminal(TuiTerminal<CrosstermBackend<std::fs::File>>);
impl Terminal {
    fn enter() -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let tui = TuiTerminal::new(CrosstermBackend::new(file))?;
        terminal::enable_raw_mode()?;
        let mut guard = Self(tui);
        execute!(
            guard.0.backend_mut(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(
                Flags::DISAMBIGUATE_ESCAPE_CODES | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            )
        )?;
        Ok(guard)
    }
}
impl Terminal {
    /// Hands the terminal to another program: fff's screen and modes are undone.
    fn suspend(&mut self) {
        let _ = execute!(
            self.0.backend_mut(),
            EndSynchronizedUpdate,
            PopKeyboardEnhancementFlags,
            Show,
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
    /// Takes the terminal back and redraws everything on the next frame.
    fn resume(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        execute!(
            self.0.backend_mut(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(
                Flags::DISAMBIGUATE_ESCAPE_CODES | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            ),
            terminal::Clear(terminal::ClearType::All)
        )?;
        // Forget both frames so the next draw sends every cell. ratatui's own
        // clear() would query the cursor position, which needs a reply.
        self.0.swap_buffers();
        self.0.swap_buffers();
        Ok(())
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = execute!(
            self.0.backend_mut(),
            EndSynchronizedUpdate,
            PopKeyboardEnhancementFlags,
            Show,
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

/// Crossterm has one reader. Input and discovery notify the same sleeping UI;
/// the bounded input queue preserves order and applies backpressure on pastes.
struct Input {
    events: std::sync::mpsc::Receiver<io::Result<Event>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Input {
    fn start(wake: std::sync::mpsc::SyncSender<()>) -> Self {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let (tx, events) = mpsc::sync_channel(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let event = match event::poll(Duration::from_millis(20)) {
                    Ok(false) => continue,
                    Ok(true) => event::read(),
                    Err(error) => Err(error),
                };
                let failed = event.is_err();
                if tx.send(event).is_err() {
                    break;
                }
                let _ = wake.try_send(());
                if failed {
                    break;
                }
            }
        });
        Self {
            events,
            cancel,
            thread: Some(thread),
        }
    }
}
impl Input {
    /// A stand-in that reads nothing, while another program owns the terminal.
    fn idle() -> Self {
        Self {
            events: std::sync::mpsc::sync_channel(1).1,
            cancel: Default::default(),
            thread: None,
        }
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // Disconnect before joining, in case the bounded queue is full.
        let (_, empty) = std::sync::mpsc::channel();
        drop(std::mem::replace(&mut self.events, empty));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// How a session ends.
pub enum Finish {
    /// Confirmed paths, or the directory of a quit key.
    Paths(Vec<PathBuf>),
    /// Ctrl+c.
    Cancelled,
    /// gX: the terminal is restored; the caller replaces fff with the program.
    Exec(crate::run::Plan),
}
pub fn run(
    mut app: App,
    keys: crate::keys::KeyMap,
    cache: crate::config::CacheConfig,
    palettes: crate::reload::PaletteWatcher,
    backend: Option<crate::config::EditorBackend>,
    performance: crate::config::PerformanceConfig,
) -> io::Result<Finish> {
    let invocation = std::env::current_dir()?.canonicalize()?;
    let mut terminal = Terminal::enter()?;
    let (wake, waking) = std::sync::mpsc::sync_channel(1);
    let mut input = Input::start(wake.clone());
    let mut discovery = Discovery::new(cache);
    discovery.set_waker(wake.clone());
    discovery.refresh_every = Duration::from_millis(performance.refresh_ms);
    discovery.start(&mut app, true);
    let mut editor = start_editor(backend, "nvim", &mut app.status);
    // A --no-dir session is a file choice: start ready to type the query.
    if app.no_dir
        && let Some(e) = &mut editor
    {
        e.reset_insert(Field::Query);
    }
    let mut editor_directory = app.cwd.clone();
    warn_stranded(&mut app);
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
    loop {
        dirty |= status.tick(&mut app);
        for update in palettes.updates() {
            dirty = true;
            match update {
                Ok(state) => palette_reloaded(&mut app, &mut focus, state),
                Err(error) => {
                    app.status = format!("Palette reload failed; keeping current colors: {error}")
                }
            }
        }
        dirty |= discovery.drain(&mut app);
        discovery.idle(
            &app,
            matches!(focus, Focus::Results),
            std::time::Instant::now(),
        );
        if editor_directory != app.cwd {
            editor_directory = app.cwd.clone();
            warn_stranded(&mut app);
            // Carry the initiating field's editing mode into the new location.
            if let Some(e) = &mut editor {
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
                        Ok(request) => perform_operation(&mut app, &mut discovery, request),
                        Err(error) => app.status = error,
                    }
                }
            } else if matches!(focus, Focus::Exclusions(_) | Focus::Filters(_)) {
                if view.saved {
                    apply_globs(&mut app, &mut focus, &mut discovery);
                } else {
                    focus = Focus::Results;
                }
            } else if let Focus::BookmarkEdit(draft) = &focus {
                // Enter and Escape are the form's keys; Neovim's own
                // ZZ/:wq and ZQ/:q! still save or cancel it.
                if view.saved {
                    save_bookmark(&mut app, &mut focus);
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
        if let Some((field, text)) = text_field(&app, &focus) {
            if let Some(e) = &mut editor {
                match e.activate(field, &text) {
                    Ok(view) => app.edit_view = Some(view),
                    Err(error) => {
                        app.status = format!("Using basic editing: {error}");
                        editor = None;
                        app.edit_view = None;
                    }
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
        dirty |= preview.tick(&mut app, &focus, std::time::Instant::now());
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
            execute!(terminal.0.backend_mut(), BeginSynchronizedUpdate)?;
            let drawn = terminal
                .0
                .draw(|frame| completed = Some(crate::view::render(frame, &mut app, &mut focus)))
                .map(|_| ());
            let ended = execute!(terminal.0.backend_mut(), EndSynchronizedUpdate);
            drawn?;
            ended?;
            let completed = completed.expect("terminal draw callback completes");
            // Only publish targets after Terminal::draw has flushed the frame.
            if matches!(focus, Focus::Results) {
                app.rendered = completed.targets;
            }
            bookmark_targets = completed.bookmark_targets;
            page = completed.page.max(1);
            dirty = false;
            drawn_at = std::time::Instant::now();
        }
        // Both input and discovery wake this wait immediately. The timeout only
        // services due re-sorts, status expiry, palette reload and idle prefetch.
        let wait = if discovery.busy(&app) || app.normal_keys.pending() {
            30
        } else {
            250
        };
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
                deliver(&mut app, &mut focus, &mut editor, pending);
            }
            let field = text_field(&app, &focus).map(|(field, _)| field);
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
                    Focus::Exclusions(value) | Focus::Filters(value) | Focus::Location(value) => {
                        paste(
                            value,
                            &text,
                            field,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        )
                    }
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
        let Event::Key(key) = event else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        status.before_key(&mut app);
        let Some(key) = keys.translate(key) else {
            continue;
        };
        if key.modifiers.contains(M::CONTROL) && key.code == K::Char('c') {
            return Ok(Finish::Cancelled);
        }
        // Multiline buffers keep Neovim's write/close lifecycle; single-line
        // names and destinations take Enter/Escape like the bookmark form.
        let buffer_input = editor.is_some()
            && match &focus {
                Focus::Exclusions(_) | Focus::Filters(_) => true,
                Focus::Operation(draft) => draft.multiline(),
                _ => false,
            };
        if buffer_input {
            deliver(&mut app, &mut focus, &mut editor, key);
            continue;
        }
        let key = match normal_list_key(&mut app, &mut focus, &mut editor, key) {
            NormalKey::Pass(key) => key,
            NormalKey::Consumed => continue,
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
        let file_normal = matches!(focus, Focus::Marks { .. })
            || (matches!(focus, Focus::Results)
                && app.edit_view.as_ref().is_some_and(|v| v.normal()));
        if file_normal
            && key.modifiers.is_empty()
            && matches!(key.code, K::Char('v' | 'p' | 'x' | 'r' | 'e' | 'n' | 's'))
        {
            let current = match &focus {
                Focus::Marks { rows, index } => rows.get(*index).cloned(),
                _ => app.highlight.clone(),
            };
            match key.code {
                K::Char('v' | 'p' | 's') => {
                    let sources = app.marks.clone();
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
                                if let Some(e) = &mut editor {
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
                    Scope::Ready(paths) => focus = start_extract(&mut app, &mut editor, paths),
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
                Focus::Location(value) => edit(
                    value,
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
            discovery.checkpoint(&app);
            match app.enter_numbered(digit) {
                EnterOutcome::Selected(paths) => return Ok(Finish::Paths(paths)),
                EnterOutcome::Navigated => discovery.start(&mut app, true),
                EnterOutcome::Stay => {}
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
                discovery.checkpoint(&app);
                if app.open_bookmark(target) {
                    discovery.start(&mut app, true);
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
                        if let Some(e) = &mut editor {
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
                        if let Some(e) = &mut editor {
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
        if editor.is_none()
            && matches!(
                focus,
                Focus::BookmarkEdit(_) | Focus::Exclusions(_) | Focus::Filters(_)
            )
            && ctrl
            && key.code == K::Char('s')
        {
            if matches!(focus, Focus::BookmarkEdit(_)) {
                save_bookmark(&mut app, &mut focus);
            } else {
                apply_globs(&mut app, &mut focus, &mut discovery);
            }
            continue;
        }
        let navigation =
            key.code == K::Enter || (alt && matches!(key.code, K::Char('h' | 'l' | '0'..='9')));
        if navigation
            && matches!(
                focus,
                Focus::Results | Focus::Bookmarks { .. } | Focus::Location(_)
            )
        {
            discovery.checkpoint(&app);
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
                    if let Some(e) = &mut editor {
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
                    ScopedAction::Extract => start_extract(&mut app, &mut editor, paths),
                    ScopedAction::Delete => delete_confirmation(paths, &mut app.status),
                    ScopedAction::Run(exit) => start_run(&mut editor, paths, *exit),
                };
            }
            Focus::TransferConfirm(plan) => match key.code {
                K::Enter => {
                    let request = plan.request();
                    focus = Focus::Results;
                    match request {
                        Ok(r) => perform_operation(&mut app, &mut discovery, r),
                        Err(e) => app.status = e,
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
                        Ok(r) => perform_operation(&mut app, &mut discovery, r),
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
                            perform_operation(&mut app, &mut discovery, r);
                        }
                        Err(e) => app.status = e,
                    }
                } else if ctrl && key.code == K::Char('s') {
                    let request = draft.request();
                    focus = Focus::Results;
                    match request {
                        Ok(r) => perform_operation(&mut app, &mut discovery, r),
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
                        reload_palette(&mut app, &palettes);
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
                            discovery.start(&mut app, true);
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
                    save_bookmark(&mut app, &mut focus);
                } else if matches!(key.code, K::Tab | K::BackTab | K::Up | K::Down) {
                    draft.field = draft.field.next(
                        matches!(key.code, K::BackTab | K::Up) || key.modifiers.contains(M::SHIFT),
                    );
                } else if draft.field == BookmarkField::Icon && key.code == K::Char(' ') {
                    let index = crate::icons::BOOKMARK_ICONS
                        .iter()
                        .position(|(_, glyph)| glyph.map(|g| g.to_string()) == draft.icon)
                        .unwrap_or(0);
                    if let Some(e) = &mut editor {
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
                            app.status = bookmark_saved(&app, "Bookmark deleted");
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
                    if let Some(e) = &mut editor {
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
                    // The program gets the terminal: stop reading input and
                    // leave fff's screen until it ends.
                    drop(std::mem::replace(&mut input, Input::idle()));
                    terminal.suspend();
                    let status = plan.wait();
                    if let Ok(status) = &status
                        && !status.success()
                    {
                        crate::run::pause_after_failure(&plan.line, *status);
                    }
                    terminal.resume()?;
                    input = Input::start(wake.clone());
                    app.status = match status {
                        Ok(status) if status.success() => {
                            let done = format!("{} finished", plan.name);
                            app.status_success = done.clone();
                            done
                        }
                        Ok(status) => format!("{} {}", plan.line, crate::run::describe(status)),
                        Err(error) => format!("Could not run {}: {error}", plan.name),
                    };
                    let message = std::mem::take(&mut app.status);
                    discovery.force_refresh(&mut app);
                    app.status = message;
                }
                K::Esc => {
                    app.status = "Run cancelled; nothing ran".into();
                    focus = Focus::Results;
                }
                _ => {}
            },
            Focus::Location(text) => {
                if key.code == K::Enter {
                    let path = PathBuf::from(&*text);
                    let path = if path.is_absolute() {
                        path
                    } else {
                        app.cwd.join(path)
                    };
                    match path.canonicalize() {
                        Ok(path) if path.is_dir() => {
                            app.navigate(path, false);
                            discovery.start(&mut app, true);
                            focus = Focus::Results;
                        }
                        _ => app.status = "Location must be an accessible directory".into(),
                    }
                } else if key.code == K::Esc {
                    focus = Focus::Results;
                } else {
                    edit(
                        text,
                        key,
                        false,
                        Field::Location,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
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
                (K::Char(' '), _, true) => {
                    focus = Focus::Marks {
                        rows: app.marks.clone(),
                        index: 0,
                    }
                }
                (K::Char('e'), _, true) => focus = Focus::Exclusions(app.policy.rules.join("\n")),
                (K::Char('f'), _, true) => focus = Focus::Filters(app.policy.filters.join("\n")),
                (K::Char('g' | 'b' | 'h'), _, true) if !app.can_navigate() => {}
                (K::Char('g'), _, true) => focus = Focus::Location(String::new()),
                (K::Char('b'), _, true) => {
                    focus = Focus::Bookmarks {
                        query: String::new(),
                        index: 0,
                    }
                }
                (K::Char('l'), _, true) => {
                    if app.navigate_highlighted() {
                        discovery.start(&mut app, true);
                    }
                }
                (K::Char('p' | 'P'), _, true) if palette == Some(PaletteKey::Reload) => {
                    reload_palette(&mut app, &palettes)
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
                (K::F(5), _, _) => discovery.force_refresh(&mut app),
                (K::Char('h'), _, true) => {
                    if let Some(p) = app.cwd.parent().map(PathBuf::from) {
                        app.navigate(p, true);
                        discovery.start(&mut app, true);
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
                    discovery.start(&mut app, false);
                }
                (K::Left, _, true) => {
                    app.depth = app.depth.saturating_sub(1).max(1);
                    discovery.start(&mut app, false);
                }
                (K::Char('.'), _, true) => {
                    app.policy.hidden = !app.policy.hidden;
                    discovery.start(&mut app, false);
                }
                (K::Char('H'), _, true) => {
                    app.policy.ignore = !app.policy.ignore;
                    discovery.start(&mut app, false);
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
                    let target = app
                        .rows
                        .iter()
                        .find(|c| Some(&c.path) == app.highlight.as_ref())
                        .cloned();
                    match app.enter(target) {
                        EnterOutcome::Selected(paths) => return Ok(Finish::Paths(paths)),
                        EnterOutcome::Navigated => discovery.start(&mut app, true),
                        EnterOutcome::Stay => {}
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

/// Clears status messages after `App::status_timeout`. A message set again by a
/// key, even with the same text, restarts the timer; one that is merely left in
/// place keeps its original deadline.
#[derive(Default)]
pub struct StatusTimer {
    shown: String,
    since: Option<std::time::Instant>,
    previous: Option<String>,
}
impl StatusTimer {
    /// Call before handling a key: the handler starts from an empty status.
    pub fn before_key(&mut self, app: &mut App) {
        self.previous = Some(std::mem::take(&mut app.status));
    }
    /// Call once per loop iteration, before drawing. Returns true when a
    /// message expired, so the frame needs redrawing.
    pub fn tick(&mut self, app: &mut App) -> bool {
        self.tick_at(app, std::time::Instant::now())
    }
    pub fn tick_at(&mut self, app: &mut App, now: std::time::Instant) -> bool {
        if let Some(previous) = self.previous.take() {
            if app.status.is_empty() {
                app.status = previous;
            } else {
                // Re-reported by the key just handled.
                self.shown.clear();
            }
        }
        if app.status != self.shown {
            self.shown = app.status.clone();
            self.since = Some(now);
        }
        if let (Some(timeout), Some(since)) = (app.status_timeout, self.since)
            && !app.status.is_empty()
            && now.duration_since(since) >= timeout
        {
            app.status.clear();
            self.shown.clear();
            return true;
        }
        false
    }
}

pub fn numbered_key(key: KeyEvent) -> Option<u32> {
    if key.modifiers == M::CONTROL
        && let K::Char(c) = key.code
    {
        return c.to_digit(10);
    }
    None
}
pub fn alt_numbered_key(key: KeyEvent) -> Option<u32> {
    if key.modifiers == M::ALT
        && let K::Char(c) = key.code
    {
        return c.to_digit(10);
    }
    None
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum PaletteKey {
    Choose,
    Reload,
}
/// Alt+p opens the chooser; Alt+P (Alt+Shift+p in either terminal encoding)
/// re-reads the configured palette.
fn palette_key(key: KeyEvent) -> Option<PaletteKey> {
    if !key.modifiers.contains(M::ALT) || key.modifiers.contains(M::CONTROL) {
        return None;
    }
    match key.code {
        K::Char('P') => Some(PaletteKey::Reload),
        K::Char('p') if key.modifiers.contains(M::SHIFT) => Some(PaletteKey::Reload),
        K::Char('p') => Some(PaletteKey::Choose),
        _ => None,
    }
}
/// Debounce chooser previews; only a stable highlight changes the canvas.
#[derive(Default)]
struct PalettePreview {
    target: Option<(Option<String>, crate::config::Palette)>,
    since: Option<std::time::Instant>,
    applied: bool,
}
impl PalettePreview {
    fn tick(&mut self, app: &mut App, focus: &Focus, now: std::time::Instant) -> bool {
        let Focus::Palettes {
            query,
            index,
            original,
        } = focus
        else {
            *self = Self::default();
            return false;
        };
        let target = app
            .matching_palettes(query)
            .get(*index)
            .map(|&i| (Some(app.palettes[i].0.clone()), app.palettes[i].1))
            .unwrap_or((None, *original));
        if self.target.as_ref() != Some(&target) {
            self.target = Some(target);
            self.since = Some(now);
            self.applied = false;
        }
        if self.remaining(now).is_some_and(|d| d.is_zero()) {
            self.applied = true;
            let palette = self.target.as_ref().unwrap().1;
            let changed = app.palette != palette;
            app.palette = palette;
            return changed;
        }
        false
    }
    fn remaining(&self, now: std::time::Instant) -> Option<Duration> {
        if self.applied {
            return None;
        }
        self.since.map(|since| {
            Duration::from_millis(100).saturating_sub(now.saturating_duration_since(since))
        })
    }
}
/// Single steps wrap around a list's ends; paging still stops at them.
fn cycle(index: usize, len: usize, forward: bool) -> usize {
    match (len, forward) {
        (0, _) => 0,
        (_, true) => (index + 1) % len,
        (_, false) => index.checked_sub(1).unwrap_or(len - 1).min(len - 1),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopedAction {
    Extract,
    Delete,
    /// gx (false) or gX (true).
    Run(bool),
}
enum Scope {
    Ready(Vec<PathBuf>),
    Ask(PathBuf, Vec<PathBuf>),
    Nothing,
}
/// The paths an action applies to. Unambiguous without marks (the current
/// entry) or when the current entry is marked (the marks); otherwise ask.
fn scope(marks: &[PathBuf], current: Option<PathBuf>) -> Scope {
    match current {
        None if marks.is_empty() => Scope::Nothing,
        None => Scope::Ready(marks.to_vec()),
        Some(current) if marks.is_empty() => Scope::Ready(vec![current]),
        Some(current) if marks.contains(&current) => Scope::Ready(marks.to_vec()),
        Some(current) => Scope::Ask(current, marks.to_vec()),
    }
}
/// Opens the destination prompt only when every chosen file is extractable;
/// otherwise nothing happens and each refused file is named with its reason.
fn start_extract(
    app: &mut App,
    editor: &mut Option<crate::editor::Nvim>,
    archives: Vec<PathBuf>,
) -> Focus {
    let refused = archives
        .iter()
        .filter_map(|p| {
            crate::operations::validate_archive(p).err().map(|e| {
                let name = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
                format!("{name} ({e})")
            })
        })
        .collect::<Vec<_>>();
    if !refused.is_empty() {
        app.status = if archives.len() == 1 {
            format!("Cannot extract {}", refused[0])
        } else {
            format!(
                "Nothing extracted; {} of {} cannot be extracted: {}",
                refused.len(),
                archives.len(),
                refused.join("; ")
            )
        };
        return Focus::Results;
    }
    if let Some(e) = editor {
        e.reset(Field::Extraction);
    }
    Focus::Operation(crate::operations::Draft {
        kind: crate::operations::EditKind::Extract(archives),
        cwd: app.cwd.clone(),
        text: String::new(),
    })
}
fn start_run(editor: &mut Option<Nvim>, paths: Vec<PathBuf>, exit: bool) -> Focus {
    if let Some(e) = editor {
        e.reset_insert(Field::Command);
    }
    Focus::Command(crate::run::Prompt::new(paths, exit))
}
fn delete_confirmation(sources: Vec<PathBuf>, status: &mut String) -> Focus {
    match crate::operations::DeletePlan::new(sources) {
        Ok(plan) => Focus::DeleteConfirm(plan),
        Err(error) => {
            *status = error;
            Focus::Results
        }
    }
}
fn perform_operation(
    app: &mut App,
    discovery: &mut Discovery,
    request: crate::operations::Request,
) {
    use crate::operations::Request;
    // Deletion only unmarks what it removed, so deleting the current path
    // keeps the remaining selection.
    let clear_selection = matches!(
        &request,
        Request::Move { .. } | Request::Copy { .. } | Request::Symlink { .. }
    );
    let destructive = match &request {
        Request::Move {
            sources, replace, ..
        } => sources.iter().chain(replace).collect(),
        // Overwritten entries are removed just like deleted ones.
        Request::Copy { replace, .. } => replace.iter().collect(),
        Request::Rename { sources, .. } | Request::Delete { sources, .. } => {
            sources.iter().collect()
        }
        _ => vec![],
    };
    if destructive.iter().any(|p| {
        let resolved = p
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| p.file_name().map(|name| parent.join(name)));
        // Compare resolved paths too: the browsing path may run through a symlink.
        let cwd = app.cwd.canonicalize().unwrap_or_else(|_| app.cwd.clone());
        [&app.cwd, &cwd].iter().any(|cwd| {
            cwd.starts_with(p) || resolved.as_ref().is_some_and(|path| cwd.starts_with(path))
        })
    }) {
        app.status = "Cannot change the current directory or its ancestors".into();
        return;
    }
    let summary = operation_summary(&request);
    match crate::operations::Engine::default().execute(request) {
        Ok(outcome) => {
            for marked in &mut app.marks {
                // Nested renames report final paths; the deepest match wins.
                if let Some((from, to)) = outcome
                    .moved
                    .iter()
                    .filter(|(from, _)| marked.starts_with(from))
                    .max_by_key(|(from, _)| from.components().count())
                {
                    let suffix = marked.strip_prefix(from).expect("prefix checked");
                    *marked = if suffix.as_os_str().is_empty() {
                        to.clone()
                    } else {
                        to.join(suffix)
                    };
                }
            }
            app.marks
                .retain(|p| !outcome.removed.iter().any(|r| p.starts_with(r)));
            if clear_selection {
                app.marks.clear();
            }
            let complete = outcome.warning.is_none();
            app.status = if let Some(warning) = outcome.warning {
                // It took effect, but not completely: marks above reflect what
                // happened; the message is shown as a problem.
                warning
            } else if !outcome.changed {
                "Nothing to rename; no names changed".into()
            } else {
                // Rename counts only the rows that changed.
                summary.unwrap_or_else(|| {
                    format!("Renamed {}", entries(outcome.moved.iter().map(|(p, _)| p)))
                })
            };
            if complete {
                app.status_success = app.status.clone();
            }
        }
        Err(error) => app.status = error,
    }
    let message = std::mem::take(&mut app.status);
    discovery.force_refresh(app);
    app.status = message;
}
/// An fff killed mid-operation can leave user data in hidden staging folders.
fn warn_stranded(app: &mut App) {
    let stranded = crate::operations::stranded(&app.cwd);
    if let Some(first) = stranded.first() {
        app.status = format!(
            "An interrupted operation left data in {}{}; move it back or delete it",
            first.file_name().unwrap_or_default().to_string_lossy(),
            match stranded.len() {
                1 => String::new(),
                n => format!(" and {} more", n - 1),
            }
        );
    }
}
/// "README.md" for one entry, "3 entries" for several.
fn entries<'a>(paths: impl ExactSizeIterator<Item = &'a PathBuf>) -> String {
    let count = paths.len();
    match paths.last() {
        Some(path) if count == 1 => path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        _ => format!("{count} entries"),
    }
}
/// What a successful request did; rename is summarized from its outcome.
fn operation_summary(request: &crate::operations::Request) -> Option<String> {
    use crate::operations::Request;
    Some(match request {
        Request::Move {
            sources, replace, ..
        }
        | Request::Copy {
            sources, replace, ..
        } => {
            let verb = if matches!(request, Request::Move { .. }) {
                "Moved"
            } else {
                "Copied"
            };
            let mut done = format!("{verb} {}", entries(sources.iter()));
            if !replace.is_empty() {
                done += &format!(", overwriting {}", entries(replace.iter()));
            }
            done
        }
        Request::Symlink { sources, .. } => format!("Linked {}", entries(sources.iter())),
        Request::Delete { sources, .. } => format!("Deleted {}", entries(sources.iter())),
        Request::Create { path, directory } => format!(
            "Created {} {}",
            if *directory { "directory" } else { "file" },
            entries(std::iter::once(path))
        ),
        Request::Extract {
            archives,
            destination: Some(destination),
        } => format!(
            "Extracted {} into {}",
            entries(archives.iter()),
            destination.display()
        ),
        Request::Extract { archives, .. } => format!(
            "Extracted {} beside {}",
            entries(archives.iter()),
            if archives.len() == 1 {
                "it"
            } else {
                "each archive"
            }
        ),
        Request::Rename { .. } => return None,
    })
}
fn bookmark_saved(app: &App, action: &str) -> String {
    match &app.bookmark_store.path {
        Some(path) => format!("{action} in {}", path.display()),
        None => format!("{action} for this session"),
    }
}
fn save_bookmark(app: &mut App, focus: &mut Focus) {
    let Focus::BookmarkEdit(draft) = focus else {
        return;
    };
    let back = Focus::Bookmarks {
        query: draft.query.clone(),
        index: draft.index,
    };
    let bookmark = draft.bookmark(&app.cwd, app.home.as_deref());
    // An unchanged existing bookmark leaves the configuration untouched.
    if let (Some(index), Ok(bookmark)) = (draft.editing, &bookmark)
        && app.bookmarks.get(index) == Some(bookmark)
    {
        *focus = back;
        return;
    }
    let result = bookmark.and_then(|bookmark| {
        app.bookmark_store.commit(
            &app.bookmarks,
            &BookmarkChange::Put {
                index: draft.editing,
                bookmark,
            },
        )
    });
    match result {
        Ok(bookmarks) => {
            let index = draft.editing.unwrap_or(app.bookmarks.len());
            app.bookmarks = bookmarks;
            app.status = bookmark_saved(app, "Bookmark saved");
            *focus = Focus::Bookmarks {
                query: String::new(),
                index,
            };
        }
        Err(error) => app.status = error,
    }
}
/// Apply the exclusion (Alt+e) or file filter (Alt+f) editor: one glob per line.
fn apply_globs(app: &mut App, focus: &mut Focus, discovery: &mut Discovery) {
    let (text, filters) = match focus {
        Focus::Exclusions(text) => (text, false),
        Focus::Filters(text) => (text, true),
        _ => return,
    };
    let current = if filters {
        &app.policy.filters
    } else {
        &app.policy.rules
    };
    // Compare final contents rather than Neovim's modified flag: :w clears
    // that flag, and editing then undoing should leave the policy untouched.
    let unchanged = *text == current.join("\n");
    let lines: Vec<_> = text
        .lines()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .collect();
    let rules: Vec<String> = lines.iter().map(|(_, s)| (*s).to_owned()).collect();
    let invalid = lines
        .iter()
        .find_map(|(line, rule)| crate::glob(rule).err().map(|error| (*line + 1, error)));
    let unchanged = unchanged || rules == *current;
    *focus = Focus::Results;
    if unchanged {
        return;
    }
    if let Some((line, error)) = invalid {
        let kind = if filters { "Filter" } else { "Exclusion" };
        app.status = format!("{kind} line {line}: {}", error.kind());
        return;
    }
    // Both setters are atomic: every glob compiles before the list is replaced.
    if filters {
        // Filters only choose among discovered files, so no rescan is needed.
        if app.policy.set_filters(rules).is_ok() {
            app.offset = 0;
            app.refresh();
        }
    } else if app.policy.set_rules(rules).is_ok() && app.policy.ignore {
        discovery.start(app, false);
    }
}
/// Alt+P: drop the session choice and apply the configuration's palette now.
fn reload_palette(app: &mut App, palettes: &crate::reload::PaletteWatcher) {
    match palettes.reload_now() {
        Ok(state) => {
            app.status = format!("Palette {} reloaded from configuration", state.name);
            app.apply_palette(state);
        }
        Err(error) => {
            app.status = format!("Palette reload failed; keeping current colors: {error}")
        }
    }
}
/// Background reload; while the chooser is open it updates the revert target.
fn palette_reloaded(app: &mut App, focus: &mut Focus, state: crate::reload::PaletteState) {
    let current = match focus {
        Focus::Palettes { original, .. } => *original,
        _ => app.palette,
    };
    if state.palette != current {
        app.status = "Palette reloaded".into();
    }
    if let Focus::Palettes {
        query,
        index,
        original,
    } = focus
    {
        *original = state.palette;
        app.palette_name = state.name;
        app.palettes = state.available;
        *index = (*index).min(app.matching_palettes(query).len().saturating_sub(1));
    } else {
        app.apply_palette(state);
    }
}
/// Opt-in hook for Normal-mode list keys: the actions a list view supports.
/// Unsupported actions behave as unbound, so their keys still reach Neovim.
/// Each list view has an arm here, in `jump_to_end` and in `deliver`;
/// Left/Down/Up/Right are replayed as the view's own Alt+h/j/k/l handling.
fn list_actions(focus: &Focus) -> Option<&'static [ListAction]> {
    use ListAction::*;
    match focus {
        Focus::Results => Some(&[
            Left,
            Down,
            Up,
            Right,
            First,
            Last,
            Mark,
            Quit,
            QuitCurrent,
            Run,
            RunExit,
        ]),
        Focus::Marks { .. } => {
            Some(&[Down, Up, First, Last, Mark, Quit, QuitCurrent, Run, RunExit])
        }
        Focus::Bookmarks { .. } | Focus::Palettes { .. } | Focus::BookmarkIcons { .. } => {
            Some(&[Down, Up, First, Last, Quit, QuitCurrent])
        }
        Focus::Exclusions(_) | Focus::Filters(_) | Focus::BookmarkEdit(_) => None,
        Focus::Location(_) => Some(&[Quit, QuitCurrent]),
        _ => None,
    }
}
fn jump_to_end(app: &mut App, focus: &mut Focus, last: bool) {
    let end = |len: usize| if last { len.saturating_sub(1) } else { 0 };
    match focus {
        Focus::Results => app.move_by(if last { isize::MAX } else { isize::MIN }),
        Focus::Marks { rows, index } => *index = end(rows.len()),
        Focus::Bookmarks { query, index } => *index = end(app.matching_bookmarks(query).len()),
        Focus::BookmarkIcons { query, index, .. } => {
            *index = end(crate::icons::matching_bookmark_icons(query).len())
        }
        Focus::Palettes { query, index, .. } => {
            *index = end(app.matching_palettes(query).len());
        }
        _ => {}
    }
}
enum NormalKey {
    Pass(KeyEvent),
    Consumed,
    Quit(bool),
    /// gx (false) or gX (true).
    Run(bool),
}
/// Dispatch configured Normal-mode actions in their supported list views.
/// Marked lists accept their actions without an editing mode.
fn normal_list_key(
    app: &mut App,
    focus: &mut Focus,
    editor: &mut Option<Nvim>,
    key: KeyEvent,
) -> NormalKey {
    let Some(actions) = list_actions(focus) else {
        return NormalKey::Pass(key);
    };
    loop {
        let normal = matches!(focus, Focus::Marks { .. })
            || app.edit_view.as_ref().is_some_and(|v| v.normal());
        if !normal && !app.normal_keys.pending() {
            return NormalKey::Pass(key);
        }
        let can_quit = app.normal_keys.pending()
            || matches!(focus, Focus::Marks { .. })
            || app.edit_view.as_ref().is_some_and(|v| v.normal());
        match app.normal_keys.feed(key, |a| {
            actions.contains(&a)
                && (can_quit || !matches!(a, ListAction::Quit | ListAction::QuitCurrent))
        }) {
            Step::Pass => return NormalKey::Pass(key),
            Step::Pending => return NormalKey::Consumed,
            Step::Action(ListAction::Quit) => return NormalKey::Quit(false),
            Step::Action(ListAction::QuitCurrent) => return NormalKey::Quit(true),
            Step::Action(ListAction::Run) => return NormalKey::Run(false),
            Step::Action(ListAction::RunExit) => return NormalKey::Run(true),
            Step::Action(action) => {
                return match action.alt_key() {
                    Some(alt) => NormalKey::Pass(alt),
                    None => {
                        jump_to_end(app, focus, action == ListAction::Last);
                        NormalKey::Consumed
                    }
                };
            }
            // Earlier keys go to Neovim unchanged; then retry the current key.
            Step::Flush(keys) => {
                for pending in keys {
                    deliver(app, focus, editor, pending);
                }
            }
        }
    }
}
/// Replay an unmatched sequence prefix into its original text field.
fn deliver(app: &mut App, focus: &mut Focus, editor: &mut Option<Nvim>, key: KeyEvent) {
    let (text, field) = match focus {
        Focus::Results => (&mut app.query, Field::Query),
        Focus::Bookmarks { query, .. } => (query, Field::Bookmarks),
        Focus::Palettes { query, .. } => (query, Field::Palettes),
        Focus::Exclusions(text) => (text, Field::Exclusions),
        Focus::Filters(text) => (text, Field::Filters),
        Focus::Operation(draft) => {
            let field = draft.field();
            (&mut draft.text, field)
        }
        Focus::BookmarkIcons { query, .. } => (query, Field::BookmarkIcons),
        Focus::BookmarkEdit(draft) => match draft.field {
            BookmarkField::Name => (&mut draft.name, Field::BookmarkName),
            BookmarkField::Path => (&mut draft.path, Field::BookmarkPath),
            BookmarkField::Icon => return,
        },
        _ => return,
    };
    let before = text.clone();
    edit(
        text,
        key,
        matches!(field, Field::Exclusions | Field::Filters | Field::Operation),
        field,
        editor,
        &mut app.edit_view,
        &mut app.status,
    );
    if *text != before {
        match focus {
            Focus::Bookmarks { index, .. } => *index = 0,
            Focus::Palettes { index, .. } => *index = 0,
            Focus::BookmarkIcons { index, .. } => *index = 0,
            Focus::BookmarkEdit(_)
            | Focus::Exclusions(_)
            | Focus::Filters(_)
            | Focus::Operation(_) => {}
            _ => {
                app.offset = 0;
                app.refresh();
            }
        }
    }
}
fn text_field(app: &App, focus: &Focus) -> Option<(Field, String)> {
    match focus {
        Focus::Results => Some((Field::Query, app.query.clone())),
        Focus::Location(text) => Some((Field::Location, text.clone())),
        Focus::Command(prompt) => Some((Field::Command, prompt.text.clone())),
        Focus::Exclusions(text) => Some((Field::Exclusions, text.clone())),
        Focus::Filters(text) => Some((Field::Filters, text.clone())),
        Focus::Operation(draft) => Some((draft.field(), draft.text.clone())),
        Focus::Bookmarks { query, .. } => Some((Field::Bookmarks, query.clone())),
        Focus::BookmarkIcons { query, .. } => Some((Field::BookmarkIcons, query.clone())),
        Focus::BookmarkEdit(draft) => match draft.field {
            BookmarkField::Name => Some((Field::BookmarkName, draft.name.clone())),
            BookmarkField::Path => Some((Field::BookmarkPath, draft.path.clone())),
            BookmarkField::Icon => None,
        },
        Focus::Palettes { query, .. } => Some((Field::Palettes, query.clone())),
        _ => None,
    }
}
/// Starts the text engine. `None` means the default (Neovim) was not explicitly
/// requested: then an unavailable or broken Neovim silently degrades to basic
/// editing. An explicit `nvim` request reports the fallback in the status line.
pub fn start_editor(
    backend: Option<crate::config::EditorBackend>,
    program: &str,
    status: &mut String,
) -> Option<Nvim> {
    use crate::config::EditorBackend;
    match backend.unwrap_or_default() {
        EditorBackend::Basic => None,
        EditorBackend::Nvim => match Nvim::start_program(program) {
            Ok(editor) => Some(editor),
            Err(error) => {
                if backend.is_some() {
                    *status = format!("Using basic editing: {error}");
                }
                None
            }
        },
    }
}
fn edit(
    text: &mut String,
    key: KeyEvent,
    multiline: bool,
    field: Field,
    editor: &mut Option<Nvim>,
    view: &mut Option<EditView>,
    status: &mut String,
) {
    if let Some(e) = editor {
        match e.input(field, key) {
            Ok((updated, state)) => {
                *text = updated;
                *view = Some(state);
            }
            Err(error) => {
                *status = format!("Using basic editing: {error}");
                *editor = None;
                *view = None;
            }
        }
    } else {
        match key.code {
            K::Backspace => {
                text.pop();
            }
            K::Enter if multiline => text.push('\n'),
            K::Char(c) if !key.modifiers.intersects(M::CONTROL | M::ALT) => text.push(c),
            _ => {}
        }
    }
}
fn paste(
    text: &mut String,
    value: &str,
    field: Field,
    editor: &mut Option<Nvim>,
    view: &mut Option<EditView>,
    status: &mut String,
) {
    if let Some(e) = editor {
        match e.paste(field, value) {
            Ok((updated, state)) => {
                *text = updated;
                *view = Some(state);
            }
            Err(error) => {
                *status = format!("Using basic editing: {error}");
                *editor = None;
                *view = None;
            }
        }
    } else {
        text.push_str(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Palette;
    use std::time::Instant;

    #[test]
    fn exclusion_exit_applies_valid_edits_and_rejects_invalid_edits_atomically() {
        let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
        app.policy.set_rules(vec!["build".into()]).unwrap();
        let mut discovery = Discovery::new(Default::default());
        for (text, expected, error) in [
            ("build", vec!["build"], false),
            ("build\n*.log", vec!["build", "*.log"], false),
            ("changed\n\n[", vec!["build", "*.log"], true),
            ("", vec![], false),
        ] {
            app.status.clear();
            let mut focus = Focus::Exclusions(text.into());
            apply_globs(&mut app, &mut focus, &mut discovery);
            assert!(matches!(focus, Focus::Results));
            assert_eq!(app.policy.rules, expected);
            assert_eq!(
                app.status.starts_with("Exclusion line 3:"),
                error,
                "{}",
                app.status
            );
        }
    }

    #[test]
    fn filter_exit_applies_globs_without_touching_exclusions() {
        let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
        app.policy.set_rules(vec!["build".into()]).unwrap();
        let mut discovery = Discovery::new(Default::default());
        let generation = app.generation;
        for (text, expected, error) in [
            ("*.pdf\n\n*.epub", vec!["*.pdf", "*.epub"], false),
            ("*.pdf\n[", vec!["*.pdf", "*.epub"], true),
            ("", vec![], false),
        ] {
            app.status.clear();
            let mut focus = Focus::Filters(text.into());
            apply_globs(&mut app, &mut focus, &mut discovery);
            assert!(matches!(focus, Focus::Results));
            assert_eq!(app.policy.filters, expected);
            assert_eq!(
                app.status.starts_with("Filter line 2:"),
                error,
                "{}",
                app.status
            );
        }
        assert_eq!(app.policy.rules, ["build"]);
        // Filters choose among discovered files; they never restart discovery.
        assert_eq!(app.generation, generation);
    }

    #[test]
    fn palette_preview_waits_a_full_dwell_and_restarts_on_highlight_changes() {
        let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
        let original = app.palette;
        let mut focus = Focus::Palettes {
            query: "nord".into(),
            index: 0,
            original,
        };
        let mut preview = PalettePreview::default();
        let now = Instant::now();
        assert!(!preview.tick(&mut app, &focus, now));
        assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(99)));
        assert_eq!(app.palette, original);
        assert!(preview.tick(&mut app, &focus, now + Duration::from_millis(100)));
        let nord = Palette::named("nord").unwrap();
        assert_eq!(app.palette, nord);
        if let Focus::Palettes { query, .. } = &mut focus {
            *query = "dracula".into();
        }
        assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(101)));
        if let Focus::Palettes { query, .. } = &mut focus {
            *query = "light".into();
        }
        assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(150)));
        assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(249)));
        assert_eq!(app.palette, nord);
        assert!(preview.tick(&mut app, &focus, now + Duration::from_millis(250)));
        assert_eq!(app.palette, Palette::named("light").unwrap());
        assert_eq!(preview.remaining(now + Duration::from_secs(1)), None);
    }

    #[test]
    fn leaving_palette_chooser_discards_pending_preview() {
        let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
        let original = app.palette;
        let focus = Focus::Palettes {
            query: "nord".into(),
            index: 0,
            original,
        };
        let now = Instant::now();
        let mut preview = PalettePreview::default();
        preview.tick(&mut app, &focus, now);
        assert!(!preview.tick(&mut app, &Focus::Results, now + Duration::from_millis(20)));
        assert!(!preview.tick(&mut app, &Focus::Results, now + Duration::from_secs(1)));
        assert_eq!(app.palette, original);
        assert_eq!(preview.remaining(now), None);
    }
}
