//! Routing one input event: a running operation hears only Ctrl+c, pastes
//! and the mouse become keys or actions, and keys pass the session-wide
//! bindings (Normal-mode list keys, operations, numbered rows) before the
//! focus's own handler in `focus.rs`. `Ok(None)` goes on with the session;
//! `Ok(Some(_))` ends it.
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
}

impl Session {
    pub(super) fn handle(&mut self, app: &mut App, event: Event) -> io::Result<Option<Finish>> {
        self.dirty = true;
        self.answering = true;
        // While an operation runs, only Ctrl+c (stop it) is heard.
        if let Focus::Busy(job) = &self.focus {
            if let Event::Key(key) = event
                && key.kind != KeyEventKind::Release
                && let Some(key) = self.keys.translate(key)
                && key.modifiers.contains(M::CONTROL)
                && key.code == K::Char('c')
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
        // except Ctrl+c, which leaves without them.
        let cancels = matches!(&event, Event::Key(key) if self.keys.translate(*key).is_some_and(|k| {
            k.modifiers.contains(M::CONTROL) && k.code == K::Char('c')
        }));
        if app.ranking_pending() && !cancels && !types_query(app, &self.focus, &event) {
            app.finish_ranking();
        }
        let (key, on_row) = match event {
            Event::Paste(text) => {
                self.paste(app, &text);
                return Ok(None);
            }
            Event::Mouse(event) => match self.click(app, event) {
                Click::Key { key, on_row } => (key, on_row),
                Click::Handled => return Ok(None),
                Click::Finish(finish) => return Ok(Some(finish)),
            },
            Event::Key(key) => (key, false),
            _ => return Ok(None),
        };
        // Escape twice in a row clears the marks; any other key disarms it.
        self.escape_armed &= key.code == K::Esc;
        self.key(app, key, on_row)
    }
    fn paste(&mut self, app: &mut App, text: &str) {
        // A held-back Normal-mode prefix reaches Neovim before the paste.
        for pending in app.normal_keys.take_pending() {
            deliver(app, &mut self.focus, &mut self.editor, pending);
        }
        // Terminals paste the paths of files dropped onto them. While
        // browsing (Normal mode), existing paths are copied here after
        // confirmation; while typing, a paste is only text.
        if matches!(self.focus, Focus::Results)
            && !app.no_dir
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
        if app.normal_keys.pending() {
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
            return match button.key(app) {
                Some(key) => Click::Key {
                    key,
                    on_row: button == Button::Select,
                },
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
            ListMouse::Open(_) if app.dialog.is_some() => Click::Key {
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
    /// `on_row`: Enter on a row the user pointed at (a double click,
    /// Ctrl+number), rather than the Enter key itself.
    fn key(
        &mut self,
        app: &mut App,
        key: KeyEvent,
        mut on_row: bool,
    ) -> io::Result<Option<Finish>> {
        if key.kind == KeyEventKind::Release {
            return Ok(None);
        }
        self.status.before_key(app);
        let pressed_alt = key.modifiers.contains(M::ALT);
        let Some(key) = self.keys.translate(key) else {
            return Ok(None);
        };
        if key.modifiers.contains(M::CONTROL) && key.code == K::Char('c') {
            return Ok(Some(Finish::Cancelled));
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
        // Alt chords for features are for typing; Normal-mode results have
        // their sequences, which the message names. Another chord mapped to
        // a feature works in every mode.
        if pressed_alt
            && matches!(self.focus, Focus::Results)
            && !app.normal_keys.pending()
            && app.edit_view.as_ref().is_some_and(|v| v.normal())
            && let Some(action) = ListAction::typing_chord(key)
            // fff seek has no d and D to name, and says why instead.
            && !(app.no_dir && matches!(action, ListAction::Directories | ListAction::Files))
        {
            let chord = crate::keys::display(key);
            app.status = match app.sequence(action) {
                Some(sequence) => format!("{chord} works while typing; in Normal mode, {sequence}"),
                None => format!("{chord} works while typing"),
            };
            return Ok(None);
        }
        let key = match normal_list_key(app, &mut self.focus, &mut self.editor, key) {
            NormalKey::Pass(key) => key,
            // Configured sequences resolve to the operation's default letter.
            NormalKey::Operation(letter) => {
                self.operation(app, letter)?;
                return Ok(None);
            }
            NormalKey::Consumed => return Ok(None),
            NormalKey::EditConfig => {
                self.edit_config(app)?;
                return Ok(None);
            }
            NormalKey::Depth(depth) => {
                if app.depth != depth {
                    app.depth = depth;
                    self.discovery.start(app, false);
                }
                return Ok(None);
            }
            NormalKey::Quit(current) => return Ok(self.quit(app, current)),
            NormalKey::Run(exit) => {
                self.focus = match scope(&app.marks, self.current(app)) {
                    Scope::Ready(paths) => start_run(&mut self.editor, paths, exit),
                    Scope::Ask(current, selected) => Focus::ScopeChoice {
                        current,
                        selected,
                        action: ScopedAction::Run(exit),
                    },
                    // A program may run on no files, in the browsing directory.
                    Scope::Nothing => start_run(&mut self.editor, vec![], exit),
                };
                return Ok(None);
            }
        };
        let capture = app.edit_view.as_ref().is_some_and(|v| {
            (key.code == K::Esc && !v.normal())
                || (v.command() && !key.modifiers.intersects(M::ALT | M::CONTROL))
        });
        if capture {
            deliver(app, &mut self.focus, &mut self.editor, key);
            return Ok(None);
        }
        // Ctrl+number: Enter on that row, as if it were highlighted first
        // (with no visible rows, Enter with nothing highlighted).
        let mut unnumbered = false;
        let key = if matches!(self.focus, Focus::Results)
            && let Some(digit) = numbered_key(key)
        {
            if !app.rendered.is_empty() {
                app.highlight = Some(app.numbered(digit));
                app.highlight_top = false;
                app.wanted = None;
                on_row = true;
            } else if app.no_dir {
                // Static (fff seek) sessions never fall back to the current
                // directory.
                app.nothing_to_enter();
                return Ok(None);
            } else {
                unnumbered = true;
            }
            KeyEvent::new(K::Enter, M::NONE)
        } else {
            key
        };
        let alt = key.modifiers.contains(M::ALT);
        if let Some(digit) = alt_numbered_key(key) {
            match self.focus {
                Focus::Results => {
                    self.open_numbered(app, digit);
                    return Ok(None);
                }
                Focus::Bookmarks { .. } => {
                    self.open_numbered_bookmark(app, digit);
                    return Ok(None);
                }
                _ => {}
            }
        }
        if matches!(self.focus, Focus::Bookmarks { .. }) && self.bookmark_command(app, key, alt) {
            return Ok(None);
        }
        let navigation =
            key.code == K::Enter || (alt && matches!(key.code, K::Char('h' | 'l' | '0'..='9')));
        if navigation
            && matches!(
                self.focus,
                Focus::Results | Focus::Bookmarks { .. } | Focus::Location(_)
            )
        {
            self.discovery.checkpoint(app);
        }
        self.focus_key(app, key, Row { on_row, unnumbered })
    }
    /// q, Q and ! in Normal mode; `current` leaves in the browsing directory.
    fn quit(&mut self, app: &mut App, current: bool) -> Option<Finish> {
        if app.dialog.is_some() && !current {
            return Some(Finish::Cancelled);
        }
        if app.dialog.as_ref().is_some_and(|d| d.save.is_some()) {
            return save_here(app, &mut self.focus);
        }
        // fff seek, and q in fff pick, leave without choosing anything.
        if app.no_dir || (app.output_paths && !current) {
            return Some(Finish::Paths(Vec::new()));
        }
        // With a window of its own, ! leaves into a shell there.
        if app.window && current {
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
    /// A file operation's default letter, from Normal mode in the results or
    /// the marked list.
    fn operation(&mut self, app: &mut App, letter: char) -> io::Result<()> {
        let current = self.current(app);
        match letter {
            'v' | 'p' | 's' => {
                let sources = app.marks.to_vec();
                if sources.is_empty() {
                    app.status = "No files selected".into();
                } else {
                    use crate::operations::Transfer;
                    // v/p/s are common Vim keys: every transfer is confirmed.
                    let kind = match letter {
                        'v' => Transfer::Move,
                        'p' => Transfer::Copy,
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
            'x' | 'X' => match scope(&app.marks, current) {
                Scope::Ready(paths) => delete(
                    app,
                    &mut self.discovery,
                    &mut self.focus,
                    paths,
                    letter == 'X',
                ),
                Scope::Ask(current, selected) => {
                    self.focus = Focus::ScopeChoice {
                        current,
                        selected,
                        action: ScopedAction::Delete(letter == 'X'),
                    }
                }
                Scope::Nothing => app.status = "Nothing to delete".into(),
            },
            'u' => undo(app, &mut self.discovery, &mut self.focus),
            // Only in the trash: elsewhere, nothing has a place to go back to.
            'R' if !app.in_trash() => {
                let restore = app.sequence(ListAction::Restore).unwrap_or_default();
                app.status = match app.either("alt+t", ListAction::Trash) {
                    Some(trash) => {
                        format!("{restore} restores entries in the trash; {trash} opens it")
                    }
                    None => format!("{restore} restores entries in the trash"),
                };
            }
            'R' => match scope(&app.marks, current) {
                Scope::Ready(paths) => restore(app, &mut self.discovery, &mut self.focus, paths),
                Scope::Ask(current, selected) => {
                    self.focus = Focus::ScopeChoice {
                        current,
                        selected,
                        action: ScopedAction::Restore,
                    }
                }
                Scope::Nothing => app.status = "Nothing to restore".into(),
            },
            'n' => self.focus = Focus::NewChoice,
            // $EDITOR on the current or marked paths: no review, it only edits.
            'E' => match scope(&app.marks, current) {
                Scope::Ask(current, selected) => {
                    self.focus = Focus::ScopeChoice {
                        current,
                        selected,
                        action: ScopedAction::Edit,
                    }
                }
                Scope::Ready(paths) => self.edit_files(app, paths)?,
                Scope::Nothing => app.status = "Nothing to edit".into(),
            },
            // The opener takes over this terminal until it ends.
            'o' => match scope(&app.marks, current) {
                Scope::Ask(current, selected) => {
                    self.focus = Focus::ScopeChoice {
                        current,
                        selected,
                        action: ScopedAction::Open,
                    }
                }
                Scope::Ready(paths) => self.open_here(app, paths)?,
                Scope::Nothing => app.status = "Nothing to open".into(),
            },
            'r' => {
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
            'e' => match scope(&app.marks, current) {
                Scope::Ready(paths) => self.focus = start_extract(app, &mut self.editor, paths),
                Scope::Ask(current, selected) => {
                    self.focus = Focus::ScopeChoice {
                        current,
                        selected,
                        action: ScopedAction::Extract,
                    }
                }
                Scope::Nothing => app.status = "Select an extractable archive first".into(),
            },
            _ => {}
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
        let path = app.numbered(digit);
        let Some(target) = app.rows.iter().find(|c| c.path == path).cloned() else {
            return;
        };
        app.highlight = Some(target.path.clone());
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
