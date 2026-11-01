use crate::{
    App, EnterOutcome,
    discovery::Discovery,
    editor::{EditView, Field, Nvim},
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
    Info {
        offset: usize,
    },
    Exclusions(String),
    Location(String),
    Bookmarks {
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

pub fn run(
    mut app: App,
    keys: crate::keys::KeyMap,
    cache: crate::config::CacheConfig,
    palettes: crate::reload::PaletteWatcher,
    backend: crate::config::EditorBackend,
) -> io::Result<Option<Vec<PathBuf>>> {
    let mut terminal = Terminal::enter()?;
    let mut discovery = Discovery::new(cache);
    discovery.start(&mut app, true);
    let mut editor = match backend {
        crate::config::EditorBackend::Basic => None,
        crate::config::EditorBackend::Nvim => match Nvim::start() {
            Ok(editor) => Some(editor),
            Err(error) => {
                app.status = format!("Using basic editing: {error}");
                None
            }
        },
    };
    let mut editor_directory = app.cwd.clone();
    let mut cursor_shape = SetCursorStyle::DefaultUserShape;
    let mut focus = Focus::Results;
    loop {
        for update in palettes.updates() {
            match update {
                Ok(state) => palette_reloaded(&mut app, &mut focus, state),
                Err(error) => {
                    app.status = format!("Palette reload failed; keeping current colors: {error}")
                }
            }
        }
        discovery.drain(&mut app);
        if editor_directory != app.cwd {
            editor_directory = app.cwd.clone();
            if let Some(e) = &mut editor {
                e.reset(Field::Query);
            }
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
        let page = completed.page.max(1);
        if !event::poll(Duration::from_millis(30))? {
            continue;
        }
        let event = event::read()?;
        if let Event::Paste(text) = event {
            let field = text_field(&app, &focus).map(|(field, _)| field);
            if let Some(field) = field {
                let text = if field == Field::Exclusions {
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
                    Focus::Exclusions(value) | Focus::Location(value) => paste(
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
                    Focus::Palettes {
                        query,
                        index,
                        original,
                    } => {
                        paste(
                            query,
                            &text,
                            field,
                            &mut editor,
                            &mut app.edit_view,
                            &mut app.status,
                        );
                        *index = 0;
                        preview_palette(&mut app, query, *index, *original);
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
        let Some(key) = keys.translate(key) else {
            continue;
        };
        if key.modifiers.contains(M::CONTROL) && key.code == K::Char('c') {
            return Ok(None);
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
                Focus::Palettes {
                    query,
                    index,
                    original,
                } => {
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
                        preview_palette(&mut app, query, *index, *original);
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
            let target = app.numbered(digit);
            if app.ready_path(&target) {
                return Ok(Some(vec![target]));
            }
            continue;
        }
        if matches!(focus, Focus::Results)
            && let Some(digit) = alt_numbered_key(key)
        {
            discovery.checkpoint(&app);
            match app.enter_numbered(digit) {
                EnterOutcome::Selected(paths) => return Ok(Some(paths)),
                EnterOutcome::Navigated => discovery.start(&mut app, true),
                EnterOutcome::Stay => {}
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
        match &mut focus {
            Focus::Palettes {
                query,
                index,
                original,
            } => {
                let matches = app.matching_palettes(query);
                let last = matches.len().saturating_sub(1);
                let moved = match key.code {
                    _ if palette == Some(PaletteKey::Reload) => {
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
                    K::Down | K::Char('j') if key.code == K::Down || alt => (*index + 1).min(last),
                    K::Up | K::Char('k') if key.code == K::Up || alt => index.saturating_sub(1),
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
                preview_palette(&mut app, query, *index, *original);
            }
            Focus::Bookmarks { query, index } => {
                let matches = app.matching_bookmarks(query);
                match key.code {
                    K::Esc => focus = Focus::Results,
                    K::Char('b') if alt => focus = Focus::Results,
                    K::Down | K::Char('j') if key.code == K::Down || alt => {
                        *index = (*index + 1).min(matches.len().saturating_sub(1))
                    }
                    K::Up | K::Char('k') if key.code == K::Up || alt => {
                        *index = index.saturating_sub(1)
                    }
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
            Focus::Info { offset } => match key.code {
                K::Esc | K::F(1) => focus = Focus::Results,
                K::Down => *offset = offset.saturating_add(1),
                K::Up => *offset = offset.saturating_sub(1),
                K::PageDown => *offset = offset.saturating_add(page),
                K::PageUp => *offset = offset.saturating_sub(page),
                _ => {}
            },
            Focus::Exclusions(text) => {
                if ctrl && key.code == K::Char('w') {
                    let rules = text
                        .lines()
                        .filter(|s| !s.is_empty())
                        .map(str::to_owned)
                        .collect();
                    match app.policy.set_rules(rules) {
                        Ok(()) => {
                            focus = Focus::Results;
                            if app.policy.ignore {
                                discovery.start(&mut app, false);
                            }
                        }
                        Err(e) => app.status = e.to_string(),
                    }
                } else if key.code == K::Esc {
                    focus = Focus::Results;
                } else {
                    edit(
                        text,
                        key,
                        true,
                        Field::Exclusions,
                        &mut editor,
                        &mut app.edit_view,
                        &mut app.status,
                    );
                }
            }
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
                    *index = (*index + 1).min(rows.len().saturating_sub(1));
                } else if key.code == K::Up || (alt && key.code == K::Char('k')) {
                    *index = index.saturating_sub(1);
                } else if ctrl
                    && key.code == K::Char('w')
                    && let Some(p) = app.confirm(None)
                {
                    return Ok(Some(p));
                }
            }
            Focus::Results => match (key.code, ctrl, alt) {
                (K::F(1), _, _) => focus = Focus::Info { offset: 0 },
                (K::Char('w'), true, _) => {
                    if let Some(p) = app.confirm(None) {
                        return Ok(Some(p));
                    }
                }
                (K::Char(' '), _, true) => {
                    focus = Focus::Marks {
                        rows: app.marks.clone(),
                        index: 0,
                    }
                }
                (K::Char('e'), _, true) => focus = Focus::Exclusions(app.policy.rules.join("\n")),
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
                (K::Char('j'), _, true) | (K::Down, false, false) => app.move_by(1),
                (K::Up, false, false) | (K::Char('k'), _, true) => app.move_by(-1),
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
                        EnterOutcome::Selected(paths) => return Ok(Some(paths)),
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
/// Show the highlighted chooser entry, or the pre-chooser colors with no match.
fn preview_palette(app: &mut App, query: &str, index: usize, original: crate::config::Palette) {
    app.palette = app
        .matching_palettes(query)
        .get(index)
        .map(|&i| app.palettes[i].1)
        .unwrap_or(original);
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
        preview_palette(app, query, *index, *original);
    } else {
        app.apply_palette(state);
    }
}
fn text_field(app: &App, focus: &Focus) -> Option<(Field, String)> {
    match focus {
        Focus::Results => Some((Field::Query, app.query.clone())),
        Focus::Location(text) => Some((Field::Location, text.clone())),
        Focus::Exclusions(text) => Some((Field::Exclusions, text.clone())),
        Focus::Bookmarks { query, .. } => Some((Field::Bookmarks, query.clone())),
        Focus::Palettes { query, .. } => Some((Field::Palettes, query.clone())),
        _ => None,
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
