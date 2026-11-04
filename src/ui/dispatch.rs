//! Routing one input event: a running operation hears only `cancel`, pastes
//! and the mouse become keys or actions, and keys in the results and the
//! marked list resolve through the keymap to a [`Function`], which runs as
//! a session action or as the default key the focus's own handler in
//! `focus.rs` takes. Elsewhere keys are fixed. `Ok(None)` goes on with the
//! session; `Ok(Some(_))` ends it.
use super::*;

/// What a mouse event comes to.
enum Click {
    /// It stands for a key; `on_row` when it is Enter on a row pointed at.
    Key {
        key: KeyEvent,
        on_row: bool,
    },
    Handled,
    Finish(Finish),
    /// The Select button: the marks where they are the answer, else
    /// Normal-mode Enter on the highlighted row.
    Select,
}

impl Session {
    pub(super) fn handle(&mut self, app: &mut App, event: Event) -> io::Result<Option<Finish>> {
        self.dirty = true;
        self.answering = true;
        // While an operation runs, only cancel (stop it) is heard.
        if let Focus::Busy(job) = &self.focus {
            if let Event::Key(key) = event
                && key.kind != KeyEventKind::Release
                && cancels(app, key)
                && job.stop()
            {
                // Only checking: nothing changed, so it is dropped at once.
                app.status = format!("Stopped {}; nothing changed", lowercase_first(&job.title));
                self.focus = Focus::Results;
            }
            return Ok(None);
        }
        // Typing a query may run ahead of rows being ranked on another
        // thread; anything else may act on rows, so it waits for them,
        // except cancel, which leaves without them.
        let cancel = matches!(&event, Event::Key(key) if cancels(app, *key));
        if app.ranking_pending() && !cancel && !types_query(app, &self.focus, &event) {
            app.finish_ranking();
        }
        match event {
            Event::Paste(text) => {
                self.paste(app, &text);
                Ok(None)
            }
            // The mouse's keys are fixed: Enter and Escape as they are by
            // default, past the keymap.
            Event::Mouse(event) => match self.click(app, event) {
                Click::Key { key, on_row } => {
                    self.escape_armed = false;
                    self.status.before_key(app);
                    self.act(app, key, on_row)
                }
                Click::Handled => Ok(None),
                Click::Finish(finish) => Ok(Some(finish)),
                Click::Select if !Button::Select.available(app) => Ok(None),
                Click::Select if select_returns_marks(app) => Ok(leave_with_marks(app)),
                Click::Select => {
                    self.status.before_key(app);
                    self.enter(app, keymap::Mode::Normal, Row::default())
                }
            },
            Event::Key(key) => self.key(app, key),
            _ => Ok(None),
        }
    }
    fn paste(&mut self, app: &mut App, text: &str) {
        // A held-back sequence prefix reaches Neovim before the paste.
        for pending in std::mem::take(&mut app.pending) {
            deliver(app, &mut self.focus, &mut self.editor, pending);
        }
        // Terminals paste the paths of files dropped onto them. While
        // browsing (Normal mode), existing paths are copied here after
        // confirmation; while typing, a paste is only text.
        if matches!(self.focus, Focus::Results)
            && app.program().navigates()
            && app.edit_view.as_ref().is_some_and(|v| v.normal())
            && let Some(paths) = dropped_paths(text, app.home.as_deref())
        {
            let copy = crate::operations::Transfer::Copy;
            transfer(app, &mut self.discovery, &mut self.focus, copy, paths, true);
            return;
        }
        paste_text(app, &mut self.focus, &mut self.editor, text);
    }
    /// Tabs, buttons, the lists and the results under the pointer. A button
    /// that stands for a key goes on as that key.
    fn click(&mut self, app: &mut App, event: MouseEvent) -> Click {
        app.mouse_seen = Some(std::time::Instant::now());
        if !app.pending.is_empty() {
            return Click::Handled;
        }
        let enter = KeyEvent::new(K::Enter, M::NONE);
        // A click on a tab's number switches to it, keeping the marks.
        let position = ratatui::layout::Position::new(event.column, event.row);
        let left = event.kind == MouseEventKind::Down(MouseButton::Left);
        if left
            && matches!(self.focus, Focus::Results)
            && let Some(&(_, tab)) = self
                .drawn
                .tabs
                .iter()
                .find(|(area, _)| area.contains(position))
        {
            if tab != app.tab && app.can_navigate() && app.select_tab(tab) {
                self.discovery.start(app, true);
            }
            return Click::Handled;
        }
        if left
            && let Some(&(_, button)) = self
                .drawn
                .buttons
                .iter()
                .find(|(area, _)| area.contains(position))
        {
            if button == Button::Select {
                return Click::Select;
            }
            return match button.key() {
                Some(key) => Click::Key { key, on_row: false },
                None => match press(app, &mut self.discovery, button) {
                    Some(finish) => Click::Finish(finish),
                    None => Click::Handled,
                },
            };
        }
        match mouse_lists(
            app,
            &mut self.focus,
            event,
            &self.drawn.lists,
            &mut self.mouse,
        ) {
            ListMouse::Enter => Click::Key {
                key: enter,
                on_row: false,
            },
            ListMouse::Open(_) if app.program().dialog() => Click::Key {
                key: enter,
                on_row: false,
            },
            ListMouse::Open(mark) => {
                // Like `l` on it: a folder is entered, a file opens.
                if mark.is_dir() {
                    if app.can_navigate() {
                        self.discovery.checkpoint(app);
                        app.navigate(mark, false);
                        app.wanted = None;
                        self.discovery.start(app, true);
                        self.focus = Focus::Results;
                    }
                } else {
                    open_in_background(app, &mark);
                }
                Click::Handled
            }
            ListMouse::Done => Click::Handled,
            // In a dialog a double click is Enter on the row.
            ListMouse::Pass => {
                if matches!(self.focus, Focus::Results)
                    && mouse_results(
                        app,
                        &mut self.discovery,
                        event,
                        self.drawn.results,
                        &mut self.mouse,
                    )
                {
                    Click::Key {
                        key: enter,
                        on_row: true,
                    }
                } else {
                    Click::Handled
                }
            }
        }
    }
    fn key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<Option<Finish>> {
        if key.kind == KeyEventKind::Release {
            return Ok(None);
        }
        self.status.before_key(app);
        let list = matches!(self.focus, Focus::Results | Focus::Marks { .. });
        if !list {
            self.escape_armed = false;
            if cancels(app, key) {
                return Ok(Some(Finish::Cancelled));
            }
        }
        // Multiline buffers keep Neovim's write/close lifecycle; single-line
        // names and destinations take Enter/Escape like the bookmark form.
        let buffer_input = match &self.focus {
            Focus::Exclusions(_) | Focus::Filters(_) => true,
            Focus::Operation(draft) => draft.multiline(),
            _ => false,
        };
        if buffer_input {
            deliver(app, &mut self.focus, &mut self.editor, key);
            return Ok(None);
        }
        if list {
            return self.list_key(app, key);
        }
        match self.focus {
            Focus::Info { .. } => {
                self.info_key(app, key);
                return Ok(None);
            }
            Focus::Keys(_) => return self.keys_key(app, key),
            Focus::Commands { .. } => return self.commands_key(app, key),
            _ => {}
        }
        match chooser_key(app, &mut self.focus, &mut self.editor, key) {
            ChooserKey::Pass(key) => self.act(app, key, false),
            ChooserKey::Consumed => Ok(None),
            ChooserKey::Quit(current) => Ok(self.quit(app, current)),
        }
    }
    /// A key in the results or the marked list: the keymap says which
    /// function it is here.
    fn list_key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<Option<Finish>> {
        let (mode, cell) = self.here(app);
        // `:` in Normal mode: the functions that work here, by name.
        if mode == keymap::Mode::Normal
            && app.pending.is_empty()
            && key.code == K::Char(':')
            && !key.modifiers.intersects(M::ALT | M::CONTROL)
        {
            self.escape_armed = false;
            self.editor.reset_insert(Field::Commands);
            let back = Box::new(std::mem::take(&mut self.focus));
            self.focus = Focus::Commands {
                query: String::new(),
                index: 0,
                here: cell,
                back,
            };
            return Ok(None);
        }
        // Leaving Insert mode and Neovim's command line are Neovim's.
        if cell.place == keymap::Place::Results
            && app.pending.is_empty()
            && let Some(view) = &app.edit_view
            && ((key.code == K::Esc && !view.normal())
                || (view.command() && !key.modifiers.intersects(M::ALT | M::CONTROL)))
        {
            self.escape_armed = false;
            deliver(app, &mut self.focus, &mut self.editor, key);
            return Ok(None);
        }
        loop {
            match app.keymap.feed(&mut app.pending, mode, cell, key) {
                Step::Pending => return Ok(None),
                // Earlier keys go to Neovim unchanged; then retry this one.
                Step::Flush(keys) => {
                    for pending in keys {
                        deliver(app, &mut self.focus, &mut self.editor, pending);
                    }
                }
                Step::Run(function, digit) => {
                    // Clearing twice in a row clears the marks.
                    if function != Function::ClearQuery {
                        self.escape_armed = false;
                    }
                    return self.run_function(app, function, digit, Row::default());
                }
                Step::Pass => {
                    self.escape_armed = false;
                    self.unbound(app, mode, cell, key);
                    return Ok(None);
                }
            }
        }
    }
    /// The mode keys are read in and where they apply, for the list in focus.
    pub(super) fn here(&self, app: &App) -> (keymap::Mode, keymap::Cell) {
        match &self.focus {
            Focus::Marks { rows, index } => {
                let highlight = match rows.get(*index) {
                    Some(mark) if mark.is_dir() => keymap::Highlight::Dir,
                    Some(_) => keymap::Highlight::File,
                    None => keymap::Highlight::None,
                };
                let cell = keymap::Cell {
                    program: app.program(),
                    place: keymap::Place::Marks,
                    highlight,
                };
                (keymap::Mode::Normal, cell)
            }
            _ => (app.mode(), app.cell()),
        }
    }
    /// What `function` does, from a key, `:` or the Keys screen: the
    /// results' and the marked list's functions each run their own code
    /// here, where the scopes in `keymap` let them.
    /// `row` is how Enter reached the list (`enter`), for Enter's functions.
    pub(super) fn run_function(
        &mut self,
        app: &mut App,
        function: Function,
        digit: Option<u8>,
        row: Row,
    ) -> io::Result<Option<Finish>> {
        use Function::*;
        let page = self.drawn.page;
        match function {
            HighlightPrevious | HighlightNext => {
                let forward = function == HighlightNext;
                match &mut self.focus {
                    Focus::Marks { rows, index } => *index = cycle(*index, rows.len(), forward),
                    _ => app.step(forward),
                }
            }
            HighlightPreviousPage => {
                app.move_by(-(page as isize));
                app.offset = app.offset.saturating_sub(page);
            }
            HighlightNextPage => {
                app.move_by(page as isize);
                app.offset = app
                    .offset
                    .saturating_add(page)
                    .min(app.shown().len().saturating_sub(page));
            }
            HighlightFirst | HighlightLast => {
                jump_to_end(app, &mut self.focus, function == HighlightLast)
            }
            GoToParent => {
                if app.can_navigate()
                    && let Some(parent) = app.cwd.parent().map(PathBuf::from)
                {
                    self.discovery.checkpoint(app);
                    app.navigate(parent, true);
                    self.discovery.start(app, true);
                }
            }
            GoToHighlighted => self.go_into_folder(app),
            GoToPath => {
                if app.can_navigate() {
                    self.open_location(app);
                }
            }
            GoToTrash => {
                if app.can_navigate() {
                    self.open_trash(app);
                }
            }
            GoBack | GoForward => {
                if !app.can_navigate() {
                } else if app.jump(function == GoBack) {
                    self.discovery.start(app, true);
                } else {
                    app.status = if function == GoBack {
                        "No earlier directory to go back to"
                    } else {
                        "No later directory to go forward to"
                    }
                    .into();
                }
            }
            // Tabs keep their own directory and history; marks are shared.
            GoToNextTab | GoToPreviousTab => {
                if app.can_navigate() && app.switch_tab(function == GoToNextTab) {
                    self.discovery.start(app, true);
                }
            }
            OpenInBackground => {
                self.discovery.checkpoint(app);
                activate_highlighted(app, &mut self.discovery);
            }
            OpenRow => self.open_numbered(app, u32::from(digit.unwrap_or(0))),
            ConfirmRow => return self.confirm_row(app, u32::from(digit.unwrap_or(0))),
            SaveOver
            | PrintHighlighted
            | PrintHighlightedAndMarks
            | QuitIntoHighlighted
            | OpenAndQuit => return Ok(self.finish_with(app, function, row)),
            SaveHere => return Ok(save_here(app, &mut self.focus)),
            OpenInTerminal | EditFiles | MoveMarked | CopyMarked | LinkMarked | Trash | Delete
            | Rename | Extract | Create | Undo | Restore => self.operation(app, function)?,
            Run | RunAndQuit => self.scoped(app, ScopedAction::Run(function == RunAndQuit))?,
            ToggleMark => match &self.focus {
                Focus::Marks { rows, index } => {
                    if let Some(path) = rows.get(*index) {
                        app.toggle(path.clone());
                    }
                }
                _ => {
                    if let Some(path) = app.highlight.clone() {
                        app.toggle(path.clone());
                        // Past the file's other matches, which share its mark.
                        app.move_past(&path);
                    }
                }
            },
            MarkAll | InvertMarks => {
                let rows: Vec<PathBuf> = match &self.focus {
                    Focus::Marks { rows, .. } => rows.clone(),
                    _ => app.shown_paths(),
                };
                app.mark_rows(&rows, function == InvertMarks);
            }
            ToggleMarkedList => {
                self.focus = match self.focus {
                    Focus::Marks { .. } => Focus::Results,
                    _ => Focus::Marks {
                        rows: app.marks.to_vec(),
                        index: 0,
                    },
                }
            }
            PrintMarks => return Ok(leave_with_marks(app)),
            ClearQuery => self.clear_query(app),
            SwitchQuery => {
                if app.program().searches_contents() {
                    app.grep.editing = !app.grep.editing;
                }
            }
            SearchShallower | SearchDeeper => {
                app.depth = if function == SearchDeeper {
                    app.depth.saturating_add(1)
                } else {
                    app.depth.saturating_sub(1).max(1)
                };
                self.discovery.start(app, false);
            }
            SearchToDepth => {
                let depth = digit.unwrap_or(1) as usize;
                if app.depth != depth {
                    app.depth = depth;
                    self.discovery.start(app, false);
                }
            }
            ToggleHidden => {
                app.policy.hidden = !app.policy.hidden;
                self.discovery.start(app, false);
            }
            ToggleDirectories | ToggleFiles => {
                let only = if function == ToggleDirectories {
                    crate::Kinds::Files
                } else {
                    crate::Kinds::Directories
                };
                app.kinds = app.kinds.toggled(only);
                app.offset = 0;
                app.refresh();
            }
            EditExclusions => self.focus = Focus::Exclusions(app.policy.rules.join("\n")),
            ToggleExclusions => {
                app.policy.ignore = !app.policy.ignore;
                self.discovery.start(app, false);
            }
            EditFilters => self.focus = Focus::Filters(app.policy.filters.join("\n")),
            // Filters only choose among discovered files: no rescan.
            ToggleFilters => {
                if app.policy.filters.is_empty() {
                    app.status = match app.bound(Function::EditFilters) {
                        Some(key) => format!("No file filters to turn on; {key} adds some"),
                        None => "No file filters to turn on".into(),
                    };
                } else {
                    app.policy.filters_off = !app.policy.filters_off;
                    app.offset = 0;
                    app.refresh();
                }
            }
            Rescan => {
                app.details.refresh_sizes();
                self.discovery.force_refresh(app);
            }
            TogglePreview => app.preview.shown = !app.preview.shown,
            ToggleLabels => app.toggle_labels(),
            TogglePreviewWrap => {
                app.preview.wrap = !app.preview.wrap;
                app.status = if app.preview.wrap {
                    "The preview wraps long lines".into()
                } else {
                    "The preview cuts long lines".into()
                };
            }
            ScrollPreviewDown | ScrollPreviewUp => {
                scroll_preview(app, function == ScrollPreviewDown)
            }
            ShowPalettes => self.open_palettes(app),
            ReloadPalette => reload_palette(app, &self.palettes),
            ShowKeys => {
                let (_, here) = self.here(app);
                self.open_keys(here);
            }
            ShowSession => self.focus = Focus::Info { offset: 0 },
            ShowBookmarks => {
                if app.can_navigate() {
                    self.focus = Focus::Bookmarks {
                        query: String::new(),
                        index: 0,
                    }
                }
            }
            EditConfig => self.edit_config(app)?,
            SwitchProgram => self.focus = Focus::ProgramChoice,
            Quit | QuitHere => return Ok(self.quit(app, function == QuitHere)),
            Cancel => return Ok(Some(Finish::Cancelled)),
        }
        Ok(None)
    }
    /// A key in the command picker: Enter runs the highlighted function
    /// where `:` was pressed, Escape goes back there; the arrows, Alt+j and
    /// Alt+k move (and j and k in Normal mode); other keys type.
    fn commands_key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<Option<Finish>> {
        let page = self.drawn.page.max(1);
        let normal = app.edit_view.as_ref().is_some_and(|v| v.normal());
        let Focus::Commands {
            query, index, here, ..
        } = &mut self.focus
        else {
            return Ok(None);
        };
        let found = crate::ui::commands(*here, query);
        let last = found.len().saturating_sub(1);
        let alt = key.modifiers == M::ALT;
        let plain = key.modifiers.is_empty();
        match key.code {
            K::Esc => {
                if let Focus::Commands { back, .. } = std::mem::take(&mut self.focus) {
                    self.focus = *back;
                }
            }
            K::Enter => {
                let Some(&function) = found.get(*index) else {
                    return Ok(None);
                };
                if let Focus::Commands { back, .. } = std::mem::take(&mut self.focus) {
                    self.focus = *back;
                }
                // Back in Normal mode where `:` was pressed, as its key would
                // have run it.
                self.sync_editor(app)?;
                return self.run_function(app, function, None, Row::default());
            }
            K::Down => *index = (*index + 1).min(last),
            K::Up => *index = index.saturating_sub(1),
            K::Char('j') if alt || (plain && normal) => *index = (*index + 1).min(last),
            K::Char('k') if alt || (plain && normal) => *index = index.saturating_sub(1),
            K::Char('n') if key.modifiers == M::CONTROL => *index = (*index + 1).min(last),
            K::Char('p') if key.modifiers == M::CONTROL => *index = index.saturating_sub(1),
            K::PageDown => *index = (*index + page).min(last),
            K::PageUp => *index = index.saturating_sub(page),
            _ => deliver(app, &mut self.focus, &mut self.editor, key),
        }
        Ok(None)
    }
    /// A key no binding has here. One that some binding has elsewhere says
    /// where it works; Enter, bound somewhere in this mode, says why it did
    /// nothing; other keys go to Neovim.
    fn unbound(&mut self, app: &mut App, mode: keymap::Mode, cell: keymap::Cell, key: KeyEvent) {
        let press = keymap::Key::press(key);
        let starts = |b: &&keymap::Binding| b.keys.len() == 1 && b.keys[0] == press;
        let special = key.modifiers.intersects(M::ALT | M::CONTROL) || matches!(key.code, K::F(_));
        let name = press.display();
        if special {
            let bindings = app.keymap.bindings();
            // Here, in the other mode.
            if let Some(other) = bindings
                .iter()
                .filter(starts)
                .find(|b| b.mode != mode && b.applies(cell))
            {
                let instead = app
                    .keymap
                    .first(other.function, mode, Some(cell))
                    .filter(|b| b.applies(cell))
                    .map(|b| b.display());
                app.status = match (mode, instead) {
                    (keymap::Mode::Normal, Some(instead)) => {
                        format!("{name} works in Insert mode; in Normal mode, {instead}")
                    }
                    (keymap::Mode::Normal, None) => format!("{name} works in Insert mode"),
                    (keymap::Mode::Insert, Some(instead)) => {
                        format!("{name} works in Normal mode; in Insert mode, {instead}")
                    }
                    (keymap::Mode::Insert, None) => format!("{name} works in Normal mode"),
                };
                return;
            }
            // Somewhere else, in either mode.
            if let Some(elsewhere) = bindings
                .iter()
                .filter(starts)
                .min_by_key(|b| b.mode != mode)
            {
                let function = elsewhere.function;
                let in_program = function
                    .available(mode)
                    .iter()
                    .any(|s| s.programs & keymap::Dimension::bit(cell.program) != 0);
                app.status = if in_program {
                    format!("{name} ({}) does nothing here", function.name())
                } else {
                    format!(
                        "{name} ({}) does not work in fff {}",
                        function.name(),
                        cell.program.title()
                    )
                };
                return;
            }
        }
        if key.code == K::Enter
            && app
                .keymap
                .bindings()
                .iter()
                .filter(starts)
                .any(|b| b.mode == mode)
        {
            app.nothing_to_enter();
            return;
        }
        deliver(app, &mut self.focus, &mut self.editor, key);
    }
    /// A key as the focus's handlers take it: a fixed key, or the mouse's.
    /// `on_row`: Enter on a row pointed at (a double click), rather than the
    /// Enter key itself.
    fn act(&mut self, app: &mut App, key: KeyEvent, on_row: bool) -> io::Result<Option<Finish>> {
        let capture = app.edit_view.as_ref().is_some_and(|v| {
            (key.code == K::Esc && !v.normal())
                || (v.command() && !key.modifiers.intersects(M::ALT | M::CONTROL))
        });
        if capture {
            deliver(app, &mut self.focus, &mut self.editor, key);
            return Ok(None);
        }
        let alt = key.modifiers.contains(M::ALT);
        if matches!(self.focus, Focus::Bookmarks { .. }) {
            if let Some(digit) = alt_numbered_key(key) {
                self.open_numbered_bookmark(app, digit);
                return Ok(None);
            }
            if self.bookmark_command(app, key, alt) {
                return Ok(None);
            }
        }
        let navigation =
            key.code == K::Enter || (alt && matches!(key.code, K::Char('h' | 'l' | '0'..='9')));
        if navigation && matches!(self.focus, Focus::Bookmarks { .. } | Focus::Location(_)) {
            self.discovery.checkpoint(app);
        }
        self.focus_key(
            app,
            key,
            Row {
                on_row,
                unnumbered: false,
            },
        )
    }
    /// q, Q and ! in Normal mode; `current` leaves in the browsing directory.
    pub(super) fn quit(&mut self, app: &mut App, current: bool) -> Option<Finish> {
        if app.program().dialog() && !current {
            return Some(Finish::Cancelled);
        }
        if app.program() == Program::ExplorerSave {
            return save_here(app, &mut self.focus);
        }
        // fff seek, and q in fff pick, leave without choosing anything.
        if !app.program().navigates() || (app.program().prints_paths() && !current) {
            return Some(Finish::Paths(Vec::new()));
        }
        // With a window of its own, ! leaves into a shell there.
        if app.program() == Program::Window && current {
            return match crate::run::shell_plan(&app.cwd) {
                Ok(plan) => Some(Finish::Exec(plan)),
                Err(error) => {
                    app.status = error;
                    None
                }
            };
        }
        Some(Finish::Paths(vec![if current {
            app.cwd.clone()
        } else {
            self.invocation.clone()
        }]))
    }
    /// A file operation, from Normal mode in the results or the marked list.
    fn operation(&mut self, app: &mut App, function: Function) -> io::Result<()> {
        use Function::*;
        match function {
            MoveMarked | CopyMarked | LinkMarked => {
                let sources = app.marks.to_vec();
                if sources.is_empty() {
                    app.status = "No files selected".into();
                } else {
                    use crate::operations::Transfer;
                    // v/p/s are common Vim keys: every transfer is confirmed.
                    let kind = match function {
                        MoveMarked => Transfer::Move,
                        CopyMarked => Transfer::Copy,
                        _ => Transfer::Link,
                    };
                    transfer(
                        app,
                        &mut self.discovery,
                        &mut self.focus,
                        kind,
                        sources,
                        false,
                    );
                }
            }
            Trash | Delete => self.scoped(app, ScopedAction::Delete(function == Delete))?,
            Undo => undo(app, &mut self.discovery, &mut self.focus),
            // Only in the trash: elsewhere, nothing has a place to go back to.
            Restore if !app.in_trash() => {
                let restore = app.bound_or_none(Function::Restore);
                app.status = match app.bound(Function::GoToTrash) {
                    Some(trash) => {
                        format!("{restore} restores entries in the trash; {trash} opens it")
                    }
                    None => format!("{restore} restores entries in the trash"),
                };
            }
            Restore => self.scoped(app, ScopedAction::Restore)?,
            Create => self.focus = Focus::NewChoice,
            // $EDITOR on the current or marked paths: no review, it only edits.
            EditFiles => self.scoped(app, ScopedAction::Edit)?,
            // The opener takes over this terminal until it ends.
            OpenInTerminal => self.scoped(app, ScopedAction::Open)?,
            Rename => {
                let sources = app.shown_paths();
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
                            self.editor.reset_at_row(Field::Operation, row);
                            self.focus = Focus::Operation(crate::operations::Draft {
                                kind: crate::operations::EditKind::Rename(sources),
                                cwd: app.cwd.clone(),
                                text,
                            });
                        }
                        Err(error) => app.status = bookmark_error(app, error),
                    }
                }
            }
            Extract => self.scoped(app, ScopedAction::Extract)?,
            _ => {}
        }
        Ok(())
    }
    /// `action` on the current entry or the marks, asking which when the
    /// current entry is not marked.
    fn scoped(&mut self, app: &mut App, action: ScopedAction) -> io::Result<()> {
        match scope(&app.marks, self.current(app)) {
            Scope::Ready(paths) => self.act_on(app, action, paths)?,
            Scope::Ask(current, selected) => {
                self.focus = Focus::ScopeChoice {
                    current,
                    selected,
                    action,
                }
            }
            Scope::Nothing => match action.nothing() {
                Some(message) => app.status = message.into(),
                None => self.act_on(app, action, vec![])?,
            },
        }
        Ok(())
    }
    /// `action` on `paths`, chosen directly or on the scope sheet.
    pub(super) fn act_on(
        &mut self,
        app: &mut App,
        action: ScopedAction,
        paths: Vec<PathBuf>,
    ) -> io::Result<()> {
        match action {
            ScopedAction::Extract => self.focus = start_extract(app, &mut self.editor, paths),
            ScopedAction::Delete(permanently) => delete(
                app,
                &mut self.discovery,
                &mut self.focus,
                paths,
                permanently,
            ),
            ScopedAction::Run(exit) => self.focus = start_run(&mut self.editor, paths, exit),
            ScopedAction::Edit | ScopedAction::Open => {
                // Leave the scope sheet before the program takes the terminal.
                if matches!(self.focus, Focus::ScopeChoice { .. }) {
                    self.focus = Focus::Results;
                }
                if action == ScopedAction::Edit {
                    self.edit_files(app, paths)?;
                } else {
                    self.open_here(app, paths)?;
                }
            }
            ScopedAction::Restore => restore(app, &mut self.discovery, &mut self.focus, paths),
        }
        Ok(())
    }
    /// $EDITOR (E) on `paths`, in this terminal.
    pub(super) fn edit_files(&mut self, app: &mut App, paths: Vec<PathBuf>) -> io::Result<()> {
        edit_files(
            &mut self.terminal,
            &mut self.input,
            &self.wake,
            app,
            &mut self.discovery,
            paths,
        )
    }
    /// Each path's opener (o), in this terminal.
    pub(super) fn open_here(&mut self, app: &mut App, paths: Vec<PathBuf>) -> io::Result<()> {
        open_here(
            &mut self.terminal,
            &mut self.input,
            &self.wake,
            app,
            &mut self.discovery,
            paths,
        )
    }
    /// Alt+number in the results: like `l` on that row, entering a directory
    /// or opening a file in the background.
    fn open_numbered(&mut self, app: &mut App, digit: u32) {
        self.discovery.checkpoint(app);
        if app.rendered.is_empty() {
            return;
        }
        // That row, rather than the first of its path: a contents query
        // shows a file once per match, and opens it at the row's.
        app.highlight_numbered(digit);
        let Some(target) = app.highlighted_row().cloned() else {
            return;
        };
        app.highlight_top = false;
        app.wanted = None;
        if target.directory {
            if app.ready_path(&target.path) && app.can_navigate() {
                app.navigate(target.path.clone(), false);
                self.discovery.start(app, true);
            }
        } else {
            open_in_background(app, &target.path);
        }
    }
    /// Alt+number among the bookmarks opens that one; 0 is the last.
    fn open_numbered_bookmark(&mut self, app: &mut App, digit: u32) {
        let index = if digit == 0 {
            usize::MAX
        } else {
            digit as usize - 1
        };
        let targets = &self.drawn.bookmark_targets;
        if let Some(&target) = targets.get(index.min(targets.len().saturating_sub(1))) {
            self.discovery.checkpoint(app);
            if app.open_bookmark(target) {
                self.discovery.start(app, true);
                self.focus = Focus::Results;
            }
        }
    }
    /// The bookmark chooser's commands: new, edit, delete, reorder and
    /// reload. Plain letters act while the filter is in Normal mode, like the
    /// results' file operations; Alt keeps them reachable while typing. False
    /// leaves the key to the chooser.
    fn bookmark_command(&mut self, app: &mut App, key: KeyEvent, alt: bool) -> bool {
        let Focus::Bookmarks { query, index } = &self.focus else {
            return false;
        };
        let matches = app.matching_bookmarks(query);
        let target = matches.get(*index).copied();
        let normal = key.modifiers.is_empty() && app.edit_view.as_ref().is_some_and(|v| v.normal());
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
                    // The form is for typing: its fields start in Insert mode.
                    self.editor.reset_insert(Field::BookmarkName);
                    self.editor.reset_insert(Field::BookmarkPath);
                    self.focus = Focus::BookmarkEdit(draft);
                }
                Err(error) => app.status = error,
            }
            return true;
        }
        if command('d') {
            if let Some(target) = target {
                self.focus = Focus::BookmarkDelete {
                    target,
                    query: query.clone(),
                    index: *index,
                };
            }
            return true;
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
                        self.focus = Focus::Bookmarks {
                            query: query.clone(),
                            index: neighbor,
                        };
                    }
                    Err(error) => app.status = error,
                }
            }
            return true;
        }
        if command('r') {
            match app.bookmark_store.reload() {
                Ok(bookmarks) => {
                    app.bookmarks = bookmarks;
                    self.editor.reset(Field::Bookmarks);
                    self.focus = Focus::Bookmarks {
                        query: String::new(),
                        index: 0,
                    };
                    app.status = "Bookmarks reloaded".into();
                }
                Err(error) => app.status = error,
            }
            return true;
        }
        false
    }
}

/// Whether `key` cancels: a lone key `cancel` has in either mode.
pub(super) fn cancels(app: &App, key: KeyEvent) -> bool {
    keymap::Mode::ALL
        .into_iter()
        .any(|mode| app.keymap.cancels(mode, key))
}
