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
        // Leaving waits for an answer, then for the operations to end.
        if matches!(self.focus, Focus::QuitConfirm(_) | Focus::Leaving(_))
            && !matches!(event, Event::Key(_))
        {
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
            ListMouse::Open(_) if app.program().clicks_enter() => Click::Key {
                key: enter,
                on_row: false,
            },
            ListMouse::Open(mark) => {
                // Like `l` on it: a folder is entered, a file opens.
                if mark.is_dir() {
                    if app.can_navigate() && !app.leaving(&mark) {
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
        match &self.focus {
            Focus::QuitConfirm(_) => {
                self.quit_confirm_key(app, key);
                return Ok(None);
            }
            Focus::Leaving(_) => return Ok(None),
            _ => {}
        }
        let list = matches!(self.focus, Focus::Results | Focus::Marks { .. });
        if !list && cancels(app, key) {
            return Ok(Some(Finish::Cancelled));
        }
        // Multiline buffers keep Neovim's write/close lifecycle; single-line
        // names and destinations take Enter/Escape like the workspace form.
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
            Focus::Jobs { .. } => {
                self.jobs_key(app, key);
                return Ok(None);
            }
            _ => {}
        }
        match chooser_key(app, &mut self.focus, &mut self.editor, key) {
            ChooserKey::Pass(key) => self.act(app, key, false),
            ChooserKey::Consumed => Ok(None),
            ChooserKey::Quit(false) => Ok(self.quit(app)),
            ChooserKey::Quit(true) => Ok(self.quit_here(app)),
        }
    }
    /// A key in the results or the marked list: the keymap says which
    /// function it is here.
    fn list_key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<Option<Finish>> {
        let (mode, cell) = self.here(app);
        // `:` in Normal mode: the functions that work here, by name. With
        // the focus on the query it is Neovim's command line (`:s`).
        if mode == keymap::Mode::Normal
            && cell.focus != keymap::Focus::Query
            && app.pending.is_empty()
            && key.code == K::Char(':')
            && !key.modifiers.intersects(M::ALT | M::CONTROL)
        {
            self.open_picker(Picker::Commands(cell), 0);
            return Ok(None);
        }
        match self.query_key(app, mode, cell, key) {
            Typed::Waiting | Typed::Neovim => {}
            Typed::Run(function, digit) => {
                return self.run_function(app, function, digit, Row::default());
            }
            Typed::Unbound(cell) => self.unbound(app, mode, cell, key),
        }
        Ok(None)
    }
    /// A key on a query and the list below it, the results' or the Keys
    /// screen's alike: Escape out of Insert mode and Neovim's command line
    /// are Neovim's, other keys are your own at `cell`, and the keys of a
    /// sequence that did not come go to Neovim. Insert mode, and a key
    /// typed into the query, give the query the focus.
    pub(super) fn query_key(
        &mut self,
        app: &mut App,
        mode: keymap::Mode,
        mut cell: keymap::Cell,
        key: KeyEvent,
    ) -> Typed {
        let list = cell.focus == keymap::Focus::Marks;
        // Typing: Insert, Replace, Visual or Select mode (`n…` are Normal
        // and its pending operators).
        if !list
            && app
                .edit_view
                .as_ref()
                .is_some_and(|v| !v.mode.starts_with('n'))
        {
            *query_focus(&mut self.focus, app) = true;
        }
        if !list
            && app.pending.is_empty()
            && let Some(view) = &app.edit_view
            && ((key.code == K::Esc && !view.normal())
                || (view.command() && !key.modifiers.intersects(M::ALT | M::CONTROL)))
        {
            deliver(app, &mut self.focus, &mut self.editor, key);
            return Typed::Neovim;
        }
        loop {
            match app.keymap.feed(&mut app.pending, mode, cell, key) {
                Step::Pending => return Typed::Waiting,
                // Earlier keys go to Neovim unchanged, which gives it the
                // query; then this one is tried again there.
                Step::Flush(keys) => {
                    for pending in keys {
                        self.type_key(app, pending);
                    }
                    if !list {
                        cell.focus = keymap::Focus::Query;
                    }
                }
                Step::Run(function, digit) => return Typed::Run(function, digit),
                Step::Pass => return Typed::Unbound(cell),
            }
        }
    }
    /// Hands `key` to Neovim in the field in focus. Anything but Escape
    /// typed into a query gives it the focus: `i`, `a`, `cw` and every
    /// other key no binding has.
    pub(super) fn type_key(&mut self, app: &mut App, key: KeyEvent) {
        deliver(app, &mut self.focus, &mut self.editor, key);
        if key.code != K::Esc && !matches!(self.focus, Focus::Marks { .. }) {
            *query_focus(&mut self.focus, app) = true;
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
                    focus: keymap::Focus::Marks,
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
                    .min(app.shown_len().saturating_sub(page));
            }
            HighlightFirst | HighlightLast => {
                jump_to_end(app, &mut self.focus, function == HighlightLast);
                // From the query in Normal mode, j and k go down into the
                // results.
                if app.mode() == keymap::Mode::Normal && matches!(self.focus, Focus::Results) {
                    app.query_focused = false;
                }
            }
            GoToParent => {
                if app.can_navigate()
                    && let Some(parent) = app.cwd.parent().map(PathBuf::from)
                {
                    self.discovery.checkpoint(app);
                    if app.navigate(parent, true) {
                        self.discovery.start(app, true);
                    }
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
            GoToOlderJump | GoToNewerJump => {
                if !app.can_navigate() {
                } else if app.jump(function == GoToOlderJump) {
                    self.discovery.start(app, true);
                } else {
                    app.status = if function == GoToOlderJump {
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
            HighlightRow => self.highlight_row(app, u32::from(digit.unwrap_or(0))),
            HighlightNextInRow => {
                return self.on_row(app, u32::from(digit.unwrap_or(0)), HighlightNext);
            }
            OpenRow => return self.on_row(app, u32::from(digit.unwrap_or(0)), OpenAndQuit),
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
            ToggleJobList if app.jobs.is_empty() => {
                app.status = "No file operation is running".into()
            }
            ToggleJobList => {
                self.focus = Focus::Jobs {
                    index: app.jobs.len() - 1,
                }
            }
            PrintMarks => return Ok(leave_with_marks(app)),
            ClearQuery => self.clear_query(app),
            ClearMarks => {
                let count = app.marks.len();
                app.marks.clear();
                app.status = format!("Cleared {count} mark{}", if count == 1 { "" } else { "s" });
                app.status_success = app.status.clone();
            }
            ToggleQueryFocus => app.query_focused = !app.query_focused,
            SwitchQuery => {
                app.grep.editing = !app.grep.editing;
                // The other query is typed into in the mode Tab was pressed in.
                let field = if app.grep.editing {
                    Field::Contents
                } else {
                    Field::Query
                };
                self.editor.carry_mode(field);
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
            ShowWorkspaces => {
                if app.can_navigate() {
                    self.focus = Focus::Workspaces {
                        query: String::new(),
                        index: 0,
                    }
                }
            }
            EditConfig => self.edit_config(app)?,
            SwitchProgram => self.focus = Focus::ProgramChoice,
            ShowSorting => self.show_sorting(app),
            Quit => return Ok(self.quit(app)),
            QuitHere | PrintHere => return Ok(self.quit_here(app)),
            Cancel => return Ok(Some(Finish::Cancelled)),
        }
        Ok(None)
    }
    /// Open `kind`'s picker over the current focus, at choice `index`.
    fn open_picker(&mut self, kind: Picker, index: usize) {
        self.editor.reset_insert(Field::Picker);
        let back = Box::new(std::mem::take(&mut self.focus));
        self.focus = Focus::Picker {
            query: String::new(),
            index,
            kind,
            back,
        };
    }
    /// `show_sorting`: the orders, at the current one, in its direction.
    fn show_sorting(&mut self, app: &App) {
        let current = Choice::Sort(app.sort.map(|s| s.by));
        let kind = Picker::Sort {
            descending: app.sort.is_none_or(|s| s.descending),
        };
        let index = kind.choices("").iter().position(|&c| c == current);
        self.open_picker(kind, index.unwrap_or(0));
    }
    /// A key in a picker: Enter takes the highlighted choice where the
    /// picker was opened (runs the function, sorts by the criterion),
    /// Escape goes back there from Normal mode (while typing it is Neovim's,
    /// as in the choosers); the arrows, Alt+j and Alt+k move (and j and k in
    /// Normal mode); in the sorting picker, Tab switches between ascending
    /// and descending; other keys type.
    pub(super) fn picker_key(
        &mut self,
        app: &mut App,
        key: KeyEvent,
    ) -> io::Result<Option<Finish>> {
        let page = self.drawn.page.max(1);
        let Focus::Picker {
            query, index, kind, ..
        } = &mut self.focus
        else {
            return Ok(None);
        };
        let found = kind.choices(query);
        let last = found.len().saturating_sub(1);
        let alt = key.modifiers == M::ALT;
        let plain = key.modifiers.is_empty();
        match key.code {
            K::Esc => {
                if let Focus::Picker { back, .. } = std::mem::take(&mut self.focus) {
                    self.focus = *back;
                }
            }
            K::Tab if plain && let Picker::Sort { descending } = kind => {
                *descending = !*descending;
            }
            K::Enter => {
                let Some(&choice) = found.get(*index) else {
                    return Ok(None);
                };
                let kind = *kind;
                if let Focus::Picker { back, .. } = std::mem::take(&mut self.focus) {
                    self.focus = *back;
                }
                // Back in Normal mode where the picker was opened, as a key
                // there would have left it.
                self.sync_editor(app)?;
                match (choice, kind) {
                    (Choice::Function(function), _) => {
                        return self.run_function(app, function, None, Row::default());
                    }
                    (Choice::Sort(by), Picker::Sort { descending }) => {
                        app.set_sort(by.map(|by| crate::rows::Sort { by, descending }));
                    }
                    (Choice::Sort(_), Picker::Commands(_)) => {}
                }
            }
            K::Down => *index = (*index + 1).min(last),
            K::Up => *index = index.saturating_sub(1),
            K::Char('j') if alt => *index = (*index + 1).min(last),
            K::Char('k') if alt => *index = index.saturating_sub(1),
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
        let starts = |b: &&keymap::Binding| b.keys.len() == 1 && b.keys[0].takes(press);
        let special = key.modifiers.intersects(M::ALT | M::CONTROL) || matches!(key.code, K::F(_));
        let name = press.display();
        // A file's open key in a dialog, which chooses files instead.
        let opens = app.keymap.bindings().iter().filter(starts).any(|b| {
            b.mode == mode && b.function == Function::OpenInBackground && b.scope.contains(cell)
        });
        if opens
            && cell.highlight == keymap::Highlight::File
            && let Some(hint) = app.running.dialog().and_then(|d| d.file_hint(app))
        {
            app.status = hint;
            return;
        }
        if special {
            let bindings = app.keymap.bindings();
            // Here, in the other mode: outside the marked list, where keys
            // in that mode apply.
            let there = |m: keymap::Mode| match cell.focus {
                keymap::Focus::Marks => cell,
                _ => app.cell_in(m),
            };
            if let Some(other) = bindings
                .iter()
                .filter(starts)
                .find(|b| b.mode != mode && b.applies(there(b.mode)))
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
                        "{name} ({}) does not work in {}",
                        function.name(),
                        cell.program.described()
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
        self.type_key(app, key);
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
        // Alt+number on a chooser's or picker's row is Enter there.
        let key = match alt_numbered_key(key) {
            Some(digit) if self.highlight_numbered_choice(app, digit) => {
                KeyEvent::new(K::Enter, M::NONE)
            }
            _ => key,
        };
        let alt = key.modifiers.contains(M::ALT);
        if matches!(self.focus, Focus::Workspaces { .. }) && self.workspace_command(app, key, alt) {
            return Ok(None);
        }
        let navigation =
            key.code == K::Enter || (alt && matches!(key.code, K::Char('h' | 'l' | '0'..='9')));
        if navigation && matches!(self.focus, Focus::Workspaces { .. } | Focus::Location(_)) {
            self.discovery.checkpoint(app);
        }
        self.focus_key(app, key, Row { on_row })
    }
    /// q, Q and ! in Normal mode; `current` leaves in the browsing directory.
    /// The jobs list's keys, fixed: j and k (or the arrows) move, x stops
    /// the highlighted operation and undoes what it did, u does the same and
    /// marks its entries again, Esc (or the key that opened it) goes back,
    /// and the marked list's key goes to that list.
    fn jobs_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::Jobs { index } = &mut self.focus else {
            return;
        };
        let count = app.jobs.len();
        *index = (*index).min(count.saturating_sub(1));
        let bound = |function| {
            app.keymap
                .of(function, keymap::Mode::Normal)
                .iter()
                .any(|b| b.keys.len() == 1 && b.keys[0].takes(keymap::Key::press(key)))
        };
        match key.code {
            _ if bound(Function::ToggleJobList) => self.focus = Focus::Results,
            // The marked list's key goes there, as from the results.
            _ if bound(Function::ToggleMarkedList) && key.code != K::Esc => {
                self.focus = Focus::Marks {
                    rows: app.marks.to_vec(),
                    index: 0,
                }
            }
            K::Esc => self.focus = Focus::Results,
            K::Char('j') | K::Down => *index = cycle(*index, count, true),
            K::Char('k') | K::Up => *index = cycle(*index, count, false),
            K::Char(c @ ('x' | 'u')) if !key.modifiers.intersects(M::ALT | M::CONTROL) => {
                stop_job(app, *index, c == 'u')
            }
            _ => {}
        }
    }
    /// Leaving while operations run: Enter stops them and leaves once they
    /// have ended; Esc stays.
    fn quit_confirm_key(&mut self, app: &mut App, key: KeyEvent) {
        match key.code {
            K::Enter => {
                let Focus::QuitConfirm(finish) = std::mem::take(&mut self.focus) else {
                    return;
                };
                stop_all(app);
                self.focus = Focus::Leaving(finish);
            }
            K::Esc => self.focus = Focus::Results,
            _ => {}
        }
    }
    /// `quit`: leave without an answer, back where fff started.
    pub(super) fn quit(&self, app: &App) -> Option<Finish> {
        Some(if app.program().dialog() {
            Finish::Cancelled
        } else if app.program().prints_paths() || !app.program().navigates() {
            Finish::Paths(Vec::new())
        } else {
            Finish::Paths(vec![self.invocation.clone()])
        })
    }
    /// `quit_here` and `print_here`, which never work in one program, and
    /// `!` in the choosers: leave with the current directory, saving there
    /// in a save dialog and leaving a shell there in a window of its own.
    /// Where the current directory is no answer, `!` quits.
    pub(super) fn quit_here(&mut self, app: &mut App) -> Option<Finish> {
        match app.program() {
            Program::ExplorerSave => save_here(app, &mut self.focus),
            Program::Window => match crate::run::shell_plan(&app.cwd) {
                Ok(plan) => Some(Finish::Exec(plan)),
                Err(error) => {
                    app.status = error;
                    None
                }
            },
            Program::Roam
            | Program::Pick
            | Program::ExplorerFolder
            | Program::ExplorerFolderMultiple => Some(Finish::Paths(vec![app.cwd.clone()])),
            Program::Seek | Program::ExplorerOpen | Program::ExplorerMultiple | Program::Piped => {
                self.quit(app)
            }
        }
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
            Undo => undo(app, &mut self.discovery),
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
                        Err(error) => app.status = workspace_error(app, error),
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
            ScopedAction::Extract => self.focus = start_extract(app, &mut self.discovery, paths),
            ScopedAction::Delete(permanently) => delete(
                app,
                &mut self.discovery,
                &mut self.focus,
                paths,
                permanently,
            ),
            ScopedAction::Run(_) if app.in_use(&paths) => self.focus = Focus::Results,
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
    /// The `_row` functions: highlight row `digit`, then do `function` there
    /// as its key would. With no rows shown, nothing.
    fn on_row(
        &mut self,
        app: &mut App,
        digit: u32,
        function: Function,
    ) -> io::Result<Option<Finish>> {
        if app.rendered.is_empty() {
            return Ok(None);
        }
        self.discovery.checkpoint(app);
        // That row, rather than the first of its path: a contents query
        // shows a file once per match, and opens it at the row's.
        self.highlight_row(app, digit);
        let (mode, cell) = self.here(app);
        if !function.works(mode, cell) {
            app.status = format!("{} does nothing on that row", function.name());
            return Ok(None);
        }
        self.run_function(app, function, None, Row { on_row: true })
    }
    /// Alt+number in a chooser or picker highlights that row as drawn, as
    /// `App::highlight_numbered` does in the results. False elsewhere, or
    /// with no rows.
    fn highlight_numbered_choice(&mut self, app: &App, digit: u32) -> bool {
        let rows = &self.drawn.lists.choices;
        let Some(&(_, target)) = rows.get(crate::numbered_row(digit, rows.len())) else {
            return false;
        };
        match chooser_rows(app, &mut self.focus) {
            Some((index, _)) => {
                *index = target;
                true
            }
            None => false,
        }
    }
    /// The workspace chooser's commands: new, edit, delete, reorder and
    /// reload. Plain letters act while the filter is in Normal mode, like the
    /// results' file operations; Alt keeps them reachable while typing. False
    /// leaves the key to the chooser.
    fn workspace_command(&mut self, app: &mut App, key: KeyEvent, alt: bool) -> bool {
        let Focus::Workspaces { query, index } = &self.focus else {
            return false;
        };
        let matches = app.matching_workspaces(query);
        let target = matches.get(*index).copied();
        let normal = key.modifiers.is_empty() && app.edit_view.as_ref().is_some_and(|v| v.normal());
        let command = |c| key.code == K::Char(c) && (alt || normal);
        let next = if command('n') {
            Some(WorkspaceDraft::new(
                &app.cwd,
                None,
                app.snapshot(),
                query.clone(),
                *index,
            ))
        } else if command('e') {
            target.map(|i| {
                WorkspaceDraft::new(
                    &app.cwd,
                    Some((i, &app.workspaces[i])),
                    app.snapshot(),
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
                    self.editor.reset_insert(Field::WorkspaceName);
                    self.editor.reset_insert(Field::WorkspacePath);
                    self.focus = Focus::WorkspaceEdit(draft);
                }
                Err(error) => app.status = error,
            }
            return true;
        }
        if command('d') {
            if let Some(target) = target {
                self.focus = Focus::WorkspaceDelete {
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
                    .workspace_store
                    .commit(&app.workspaces, &WorkspaceChange::Swap(target, other))
                {
                    Ok(workspaces) => {
                        app.workspaces = workspaces;
                        self.focus = Focus::Workspaces {
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
            match app.workspace_store.reload() {
                Ok(workspaces) => {
                    app.workspaces = workspaces;
                    self.editor.reset(Field::Workspaces);
                    self.focus = Focus::Workspaces {
                        query: String::new(),
                        index: 0,
                    };
                    app.status = "Workspaces reloaded".into();
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
/// What a key on a query and the list below it came to.
pub(super) enum Typed {
    /// It begins a sequence.
    Waiting,
    /// Neovim's: it leaves Insert mode or edits its command line.
    Neovim,
    Run(Function, Option<u8>),
    /// No binding has it at this cell (the query, once keys held back
    /// went to it).
    Unbound(keymap::Cell),
}
/// Whether the query on screen has the focus: the Keys screen's, or the
/// results'.
pub(super) fn query_focus<'a>(focus: &'a mut Focus, app: &'a mut App) -> &'a mut bool {
    match focus {
        Focus::Keys(screen) => &mut screen.query_focused,
        _ => &mut app.query_focused,
    }
}
