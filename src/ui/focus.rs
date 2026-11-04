//! Each focus's own keys, once `dispatch.rs` has routed the session-wide
//! ones. A key a handler does not use goes to Neovim when the focus has a
//! text field (`deliver`).
use super::*;

/// How Enter reached the results.
#[derive(Clone, Copy, Default)]
pub(super) struct Row {
    /// Enter on a row pointed at (Ctrl+number, a double click): it must
    /// still be listed.
    pub on_row: bool,
    /// Ctrl+number with no rows shown: Enter with nothing highlighted.
    pub unnumbered: bool,
}

impl Session {
    pub(super) fn focus_key(
        &mut self,
        app: &mut App,
        key: KeyEvent,
        row: Row,
    ) -> io::Result<Option<Finish>> {
        match self.focus {
            // The mouse's Enter (a double click, the button bar); the lists'
            // other keys are functions (`run_function`).
            Focus::Results | Focus::Marks { .. } if key.code == K::Enter => {
                let (mode, _) = self.here(app);
                return self.enter(app, mode, row);
            }
            Focus::Results => deliver(app, &mut self.focus, &mut self.editor, key),
            Focus::Marks { .. } => {}
            Focus::Replace(_) => return Ok(self.replace_key(app, key)),
            Focus::RunConfirm(_) => return self.run_confirm_key(app, key),
            Focus::NewChoice => self.new_choice_key(app, key),
            Focus::ProgramChoice => self.program_choice_key(app, key),
            Focus::ScopeChoice { .. } => self.scope_choice_key(app, key)?,
            Focus::TransferConfirm(_) => self.transfer_confirm_key(app, key),
            Focus::DeleteConfirm(_) => self.delete_confirm_key(app, key),
            Focus::TrashConfirm(_) => self.trash_confirm_key(app, key),
            Focus::Operation(_) => self.draft_key(app, key),
            Focus::Palettes { .. } => self.palettes_key(app, key),
            Focus::Bookmarks { .. } => self.bookmarks_key(app, key),
            Focus::BookmarkEdit(_) => self.bookmark_edit_key(app, key),
            Focus::BookmarkIcons { .. } => self.bookmark_icons_key(app, key),
            Focus::BookmarkDelete { .. } => self.bookmark_delete_key(app, key),
            Focus::Info { .. } | Focus::Keys(_) | Focus::Commands { .. } => {}
            Focus::Command(_) => self.command_key(app, key),
            Focus::Location(_) => self.location_key(app, key),
            // Keys never reach here while checks run, in the jobs list or
            // while leaving, and the glob editors' keys all went to Neovim
            // (`buffer_input`).
            Focus::Busy(_)
            | Focus::Jobs { .. }
            | Focus::Extracting(_)
            | Focus::QuitConfirm(_)
            | Focus::Leaving(_)
            | Focus::Exclusions(_)
            | Focus::Filters(_) => {}
        }
        Ok(None)
    }
    /// A name that is taken: Enter replaces what is there, k keeps both
    /// (under the free name beside it), Esc goes back.
    fn replace_key(&mut self, app: &mut App, key: KeyEvent) -> Option<Finish> {
        use crate::operations::Taken;
        let Focus::Replace(replace) = &self.focus else {
            return None;
        };
        let taken = match key.code {
            K::Enter => Taken::Replace,
            K::Char('k') => Taken::KeepBoth,
            K::Esc => {
                if matches!(replace.then, Replacing::Restore(_)) {
                    app.status = "Restore cancelled; nothing changed".into();
                }
                self.focus = Focus::Results;
                return None;
            }
            _ => return None,
        };
        match &replace.then {
            Replacing::Save => {
                let (target, both) = &replace.taken[0];
                Some(Finish::Paths(vec![if taken == Taken::Replace {
                    target.clone()
                } else {
                    both.clone()
                }]))
            }
            Replacing::Restore(plan) => {
                let request = plan.request(taken);
                perform_operation(app, &mut self.discovery, &mut self.focus, request, false);
                None
            }
        }
    }
    fn new_choice_key(&mut self, app: &App, key: KeyEvent) {
        match key.code {
            K::Char('f' | 'd') => {
                self.editor.reset_insert(Field::FileName);
                self.focus = Focus::Operation(crate::operations::Draft {
                    kind: crate::operations::EditKind::New(key.code == K::Char('d')),
                    cwd: app.cwd.clone(),
                    text: String::new(),
                });
            }
            K::Esc => self.focus = Focus::Results,
            _ => {}
        }
    }
    /// A program's letter switches to it; Esc stays.
    fn program_choice_key(&mut self, app: &mut App, key: KeyEvent) {
        let K::Char(letter) = key.code else {
            if key.code == K::Esc {
                self.focus = Focus::Results;
            }
            return;
        };
        let Some(&(program, _)) = crate::program::Program::SWITCHABLE
            .iter()
            .find(|&&(p, l)| l == letter && p != app.program())
        else {
            return;
        };
        self.focus = Focus::Results;
        let scan = app.switch_program(program);
        // The query typed into next keeps the mode gm was pressed in.
        let field = if app.typing_contents() {
            Field::Contents
        } else {
            Field::Query
        };
        self.editor.reset_preserving_mode(field);
        if scan {
            // Starting a scan clears the status; keep saying what changed.
            let status = std::mem::take(&mut app.status);
            self.discovery.start(app, false);
            app.status = status;
        }
    }
    fn scope_choice_key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<()> {
        let Focus::ScopeChoice {
            current,
            selected,
            action,
        } = &self.focus
        else {
            return Ok(());
        };
        let paths = match key.code {
            K::Char('c') => vec![current.clone()],
            K::Char('s') => selected.clone(),
            K::Char('a') => selected.iter().chain([current]).cloned().collect(),
            K::Esc => {
                self.focus = Focus::Results;
                return Ok(());
            }
            _ => return Ok(()),
        };
        let action = *action;
        self.act_on(app, action, paths)
    }
    fn transfer_confirm_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::TransferConfirm(plan) = &self.focus else {
            return;
        };
        match key.code {
            K::Enter => {
                let request = plan.request();
                let dropped = plan.dropped;
                self.focus = Focus::Results;
                match request {
                    Ok(r) => perform_operation(app, &mut self.discovery, &mut self.focus, r, dropped),
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
                self.focus = Focus::Results;
            }
            _ => {}
        }
    }
    fn delete_confirm_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::DeleteConfirm(plan) = &self.focus else {
            return;
        };
        match key.code {
            K::Enter => {
                let request = plan.request();
                self.focus = Focus::Results;
                match request {
                    Ok(r) => perform_operation(app, &mut self.discovery, &mut self.focus, r, false),
                    Err(e) => app.status = e,
                }
            }
            K::Esc => self.focus = Focus::Results,
            _ => {}
        }
    }
    fn trash_confirm_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::TrashConfirm(plan) = &self.focus else {
            return;
        };
        match key.code {
            // d deletes the listed entries permanently, t trashes the
            // large ones anyway; the others go to the trash either way.
            K::Char('d') => {
                let request = plan.trash_request(false);
                perform_operation(app, &mut self.discovery, &mut self.focus, request, false);
            }
            K::Char('t') => {
                if plan
                    .fates
                    .iter()
                    .any(|f| matches!(f, crate::operations::Fate::Unavailable(_)))
                {
                    app.status = "Some of these entries cannot go to the trash; \
                                  d deletes them permanently"
                        .into();
                } else {
                    let request = plan.trash_request(true);
                    perform_operation(app, &mut self.discovery, &mut self.focus, request, false);
                }
            }
            K::Esc => {
                app.status = "Deletion cancelled; nothing changed".into();
                self.focus = Focus::Results;
            }
            _ => {}
        }
    }
    /// A single-line operation buffer (new entry, extraction destination).
    /// Neovim rename lists were routed before, and only their written close
    /// commits; these keys arrive outside Neovim's Insert/Visual Escape and
    /// command-line Enter.
    fn draft_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::Operation(draft) = &self.focus else {
            return;
        };
        let ctrl = key.modifiers.contains(M::CONTROL);
        if key.code == K::Esc {
            self.focus = Focus::Results;
        } else if key.code == K::Enter || (ctrl && key.code == K::Char('s')) {
            // Validation errors keep the buffer open for correction.
            match draft.request() {
                Ok(r) => perform_operation(app, &mut self.discovery, &mut self.focus, r, false),
                Err(e) => app.status = e,
            }
        } else {
            deliver(app, &mut self.focus, &mut self.editor, key);
        }
    }
    fn palettes_key(&mut self, app: &mut App, key: KeyEvent) {
        let page = self.drawn.page;
        let alt = key.modifiers.contains(M::ALT);
        let palette = palette_key(key);
        let Focus::Palettes {
            query,
            index,
            original,
        } = &mut self.focus
        else {
            return;
        };
        let matches = app.matching_palettes(query);
        let last = matches.len().saturating_sub(1);
        let moved = match key.code {
            _ if palette == Some(PaletteKey::Reload) => {
                // Drop the preview first so a failed reload keeps the
                // colors that were actually applied.
                app.palette = *original;
                self.focus = Focus::Results;
                reload_palette(app, &self.palettes);
                return;
            }
            _ if key.code == K::Esc || palette == Some(PaletteKey::Choose) => {
                app.palette = *original;
                self.focus = Focus::Results;
                return;
            }
            K::Enter => {
                if let Some(&i) = matches.get(*index) {
                    let (name, chosen) = app.palettes[i].clone();
                    self.palettes.choose(&name);
                    app.palette = chosen;
                    app.status = format!("Palette {name} applied for this session");
                    app.palette_name = name;
                    self.focus = Focus::Results;
                }
                return;
            }
            K::Down | K::Char('j') if key.code == K::Down || alt => {
                cycle(*index, matches.len(), true)
            }
            K::Up | K::Char('k') if key.code == K::Up || alt => cycle(*index, matches.len(), false),
            K::PageDown => (*index + page).min(last),
            K::PageUp => index.saturating_sub(page),
            _ => {
                deliver(app, &mut self.focus, &mut self.editor, key);
                return;
            }
        };
        *index = moved;
    }
    /// The bookmark chooser's movement and Enter; its commands were handled
    /// in `bookmark_command`.
    fn bookmarks_key(&mut self, app: &mut App, key: KeyEvent) {
        let page = self.drawn.page;
        let alt = key.modifiers.contains(M::ALT);
        let Focus::Bookmarks { query, index } = &mut self.focus else {
            return;
        };
        let matches = app.matching_bookmarks(query);
        match key.code {
            K::Esc => self.focus = Focus::Results,
            K::Char('b') if alt => self.focus = Focus::Results,
            // Up/Down reorder; Alt+j/k (or Normal j/k) move.
            K::Char('j') if alt => *index = cycle(*index, matches.len(), true),
            K::Char('k') if alt => *index = cycle(*index, matches.len(), false),
            K::PageDown => *index = (*index + page).min(matches.len().saturating_sub(1)),
            K::PageUp => *index = index.saturating_sub(page),
            K::Enter => {
                if let Some(&target) = matches.get(*index)
                    && app.open_bookmark(target)
                {
                    self.discovery.start(app, true);
                    self.focus = Focus::Results;
                }
            }
            _ => deliver(app, &mut self.focus, &mut self.editor, key),
        }
    }
    fn bookmark_edit_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::BookmarkEdit(draft) = &mut self.focus else {
            return;
        };
        // Neovim's Insert/Visual Escape and command-line Enter were
        // captured before, so these keys act on the form itself.
        if key.code == K::Esc {
            self.focus = Focus::Bookmarks {
                query: draft.query.clone(),
                index: draft.index,
            };
        } else if key.code == K::Enter {
            save_bookmark(app, &mut self.focus);
        } else if matches!(key.code, K::Tab | K::BackTab | K::Up | K::Down) {
            draft.field = draft
                .field
                .next(matches!(key.code, K::BackTab | K::Up) || key.modifiers.contains(M::SHIFT));
        } else if draft.field == BookmarkField::Icon && key.code == K::Char(' ') {
            let index = crate::icons::BOOKMARK_ICONS
                .iter()
                .position(|(_, glyph)| glyph.map(|g| g.to_string()) == draft.icon)
                .unwrap_or(0);
            self.editor.reset(Field::BookmarkIcons);
            self.focus = Focus::BookmarkIcons {
                draft: draft.clone(),
                query: String::new(),
                index,
            };
        } else {
            deliver(app, &mut self.focus, &mut self.editor, key);
        }
    }
    fn bookmark_icons_key(&mut self, app: &mut App, key: KeyEvent) {
        let page = self.drawn.page;
        let alt = key.modifiers.contains(M::ALT);
        let Focus::BookmarkIcons {
            draft,
            query,
            index,
        } = &mut self.focus
        else {
            return;
        };
        let matches = crate::icons::matching_bookmark_icons(query);
        match key.code {
            K::Esc => self.focus = Focus::BookmarkEdit(draft.clone()),
            K::Enter => {
                if let Some(&i) = matches.get(*index) {
                    draft.icon = crate::icons::BOOKMARK_ICONS[i].1.map(|g| g.to_string());
                    self.focus = Focus::BookmarkEdit(draft.clone());
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
            _ => deliver(app, &mut self.focus, &mut self.editor, key),
        }
    }
    fn bookmark_delete_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::BookmarkDelete {
            target,
            query,
            index,
        } = &self.focus
        else {
            return;
        };
        if key.code == K::Esc {
            self.focus = Focus::Bookmarks {
                query: query.clone(),
                index: *index,
            };
        } else if key.code == K::Enter {
            let target = *target;
            match app
                .bookmark_store
                .commit(&app.bookmarks, &BookmarkChange::Remove(target))
            {
                Ok(bookmarks) => {
                    app.bookmarks = bookmarks;
                    app.status = bookmark_saved(app, "Bookmark deleted");
                    self.focus = Focus::Bookmarks {
                        query: String::new(),
                        index: target.min(app.bookmarks.len().saturating_sub(1)),
                    };
                }
                Err(error) => app.status = bookmark_error(app, error),
            }
        }
    }
    /// F2's Session tab: the session key or Escape closes it, the guide key
    /// goes to the Keys screen, the arrows scroll.
    pub(super) fn info_key(&mut self, app: &App, key: KeyEvent) {
        let press = crate::keymap::Key::press(key);
        let is = |f: Function| {
            keymap::Mode::ALL.into_iter().any(|m| {
                app.keymap
                    .of(f, m)
                    .iter()
                    .any(|b| b.keys.len() == 1 && b.keys[0] == press)
            })
        };
        let page = self.drawn.page;
        let Focus::Info { offset } = &mut self.focus else {
            return;
        };
        match key.code {
            K::Esc | K::Char('q') => self.focus = Focus::Results,
            _ if is(Function::ShowSession) => self.focus = Focus::Results,
            _ if is(Function::ShowKeys) => self.open_keys(app.cell()),
            K::Down | K::Char('j') => *offset = offset.saturating_add(1),
            K::Up | K::Char('k') => *offset = offset.saturating_sub(1),
            K::PageDown => *offset = offset.saturating_add(page),
            K::PageUp => *offset = offset.saturating_sub(page),
            _ => {}
        }
    }
    /// gx / gX: the command line.
    fn command_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::Command(prompt) = &mut self.focus else {
            return;
        };
        if key.code == K::Enter {
            // An unknown command or bad quoting aborts with the reason.
            match prompt.plan(&app.cwd, app.home.as_deref()) {
                Ok(plan) => self.focus = Focus::RunConfirm(plan),
                Err(error) => {
                    app.status = error;
                    self.focus = Focus::Results;
                }
            }
        } else if key.code == K::Esc {
            self.focus = Focus::Results;
        } else if key.code == K::Tab {
            prompt.complete(&app.cwd, app.home.as_deref());
            // Hand the completed line back to Neovim, still typing.
            self.editor.reset_preserving_mode(Field::Command);
        } else {
            deliver(app, &mut self.focus, &mut self.editor, key);
        }
    }
    /// The review of a gx / gX command before it runs.
    fn run_confirm_key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<Option<Finish>> {
        let Focus::RunConfirm(plan) = &self.focus else {
            return Ok(None);
        };
        match key.code {
            K::Enter if plan.exit => return Ok(Some(Finish::Exec(plan.clone()))),
            K::Enter => {
                let plan = plan.clone();
                self.focus = Focus::Results;
                run_here(
                    &mut self.terminal,
                    &mut self.input,
                    &self.wake,
                    app,
                    &mut self.discovery,
                    &plan,
                )?;
            }
            K::Esc => {
                app.status = "Run cancelled; nothing ran".into();
                self.focus = Focus::Results;
            }
            _ => {}
        }
        Ok(None)
    }
    /// Alt+g: go to a typed or suggested location.
    fn location_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::Location(location) = &mut self.focus else {
            return;
        };
        if key.code == K::Enter {
            match location
                .target(&app.cwd, app.home.as_deref())
                .and_then(|p| p.canonicalize().map_err(|e| e.to_string()))
            {
                Ok(path) if path.is_dir() => {
                    if app.navigate(path, false) {
                        self.discovery.start(app, true);
                    }
                    self.focus = Focus::Results;
                }
                Ok(_) => app.status = "Location must be an accessible directory".into(),
                Err(error) => app.status = error,
            }
        } else if key.code == K::Esc {
            self.focus = Focus::Results;
        } else if matches!(key.code, K::Up | K::Down) {
            location.choose(key.code == K::Down);
        } else if key.code == K::Tab {
            location.complete(&app.cwd, app.home.as_deref());
            self.editor.reset_preserving_mode(Field::Location);
        } else {
            deliver(app, &mut self.focus, &mut self.editor, key);
        }
    }
    /// Enter pressed by the mouse, or on a row pointed at (`row`): what
    /// Enter does there by default, in `mode`. Saving under the typed name
    /// ignores the rows, so a row pointed at while typing one takes the row's
    /// Normal-mode Enter instead.
    pub(super) fn enter(
        &mut self,
        app: &mut App,
        mode: keymap::Mode,
        row: Row,
    ) -> io::Result<Option<Finish>> {
        let (_, mut cell) = self.here(app);
        if row.unnumbered {
            cell.highlight = keymap::Highlight::None;
        }
        let enter = keymap::Key::press(KeyEvent::new(K::Enter, M::NONE));
        let function = match Function::default_of(enter, mode, cell) {
            Some(Function::SaveHere) if row.on_row => {
                Function::default_of(enter, keymap::Mode::Normal, cell)
            }
            function => function,
        };
        match function {
            Some(function) => self.run_function(app, function, None, row),
            None => {
                app.nothing_to_enter();
                Ok(None)
            }
        }
    }
    /// Ctrl+number: Enter on that row, as if it were highlighted first (with
    /// no visible rows, Enter with nothing highlighted).
    pub(super) fn confirm_row(&mut self, app: &mut App, digit: u32) -> io::Result<Option<Finish>> {
        let mut row = Row::default();
        if !app.rendered.is_empty() {
            app.highlight_numbered(digit);
            app.highlight_top = false;
            app.wanted = None;
            row.on_row = true;
        } else if !app.program().navigates() {
            // Static (fff seek) sessions never fall back to the current
            // directory.
            app.nothing_to_enter();
            return Ok(None);
        } else {
            row.unnumbered = true;
        }
        self.enter(app, app.mode(), row)
    }
    /// Enter's functions that finish with the highlighted entry
    /// (`finish_with_entry`).
    pub(super) fn finish_with(
        &mut self,
        app: &mut App,
        function: Function,
        row: Row,
    ) -> Option<Finish> {
        let (mode, _) = self.here(app);
        let chosen = chosen(app, &self.focus, &mut self.discovery, row).ok()?;
        finish_with_entry(app, &mut self.focus, function, chosen, mode)
    }
    /// `go_to_highlighted`: into the highlighted directory, or to the
    /// highlighted file's folder with the file highlighted.
    pub(super) fn go_into_folder(&mut self, app: &mut App) {
        let Some(path) = self.current(app) else {
            return;
        };
        self.discovery.checkpoint(app);
        if path.is_dir() {
            if app.ready_path(&path) && app.can_navigate() {
                // Arriving, the cursor left there comes back.
                if app.navigate(path, false) {
                    self.discovery.start(app, true);
                }
            }
        } else if app.reveal(path) {
            self.discovery.start(app, true);
        } else {
            // The query was cleared here, not by moving: keep its mode, as
            // after a move.
            self.editor.reset_preserving_mode(Field::Query);
        }
        self.focus = Focus::Results;
    }
    /// `go_to_path`: the prompt, with bookmarks and zoxide's suggestions.
    pub(super) fn open_location(&mut self, app: &App) {
        let bookmarks = app
            .bookmarks
            .iter()
            .map(|b| (b.name.clone(), b.path.clone()))
            .collect::<Vec<_>>();
        let frequent = crate::location::zoxide(&app.cwd);
        // It exists for typing, like the run prompt.
        self.editor.reset_insert(Field::Location);
        self.focus = Focus::Location(crate::location::Location::new(
            &bookmarks, frequent, &app.cwd,
        ));
    }
    /// `show_palettes`: the chooser, on the palette in use.
    pub(super) fn open_palettes(&mut self, app: &App) {
        let index = app
            .palettes
            .iter()
            .position(|(name, _)| *name == app.palette_name)
            .or_else(|| app.palettes.iter().position(|(_, p)| *p == app.palette))
            .unwrap_or(0);
        self.focus = Focus::Palettes {
            query: String::new(),
            index,
            original: app.palette,
        };
    }
    /// `clear_query`: the query; pressed again with none, the marks. With
    /// a contents query, the query being typed into, else the other one.
    pub(super) fn clear_query(&mut self, app: &mut App) {
        if !app.grep.text.is_empty() && (app.grep.editing || app.query.is_empty())
        {
            app.set_contents(String::new());
        } else if !app.query.is_empty() {
            app.query.clear();
            app.offset = 0;
            app.refresh();
        } else if std::mem::replace(&mut self.escape_armed, true) && !app.marks.is_empty() {
            let count = app.marks.len();
            app.marks.clear();
            app.status = format!("Cleared {count} mark{}", if count == 1 { "" } else { "s" });
            app.status_success = app.status.clone();
            self.escape_armed = false;
        }
    }
    /// Alt+t: the trash, a folder like any other; again, the next one (each
    /// filesystem has its own).
    pub(super) fn open_trash(&mut self, app: &mut App) {
        let Some(trash) = &app.trash else {
            app.status = "The trash is off ([trash] enabled = false)".into();
            return;
        };
        let folders = trash.folders();
        app.refresh_trash();
        let next = match folders.iter().position(|f| *f == app.cwd) {
            Some(_) if folders.len() == 1 => {
                app.status = "This is the only trash".into();
                return;
            }
            Some(i) => folders[(i + 1) % folders.len()].clone(),
            None => match folders.first() {
                Some(first) => first.clone(),
                None => {
                    app.status = "The trash is empty".into();
                    return;
                }
            },
        };
        self.discovery.checkpoint(app);
        if app.navigate(next, false) {
            self.discovery.start(app, true);
        }
    }
}
/// Ctrl+j / Ctrl+k: the preview pane's content scrolls, the highlight stays.
pub(super) fn scroll_preview(app: &mut App, down: bool) {
    if !app.preview.shown {
        app.status = match app.bound(Function::TogglePreview) {
            Some(key) => format!("No preview pane to scroll; {key} opens it"),
            None => "No preview pane to scroll".into(),
        };
    } else if app.preview.current().is_some() || app.highlighted_hit().is_some() {
        app.preview.scroll(down);
    }
}
/// The entry Enter's functions act on: the highlighted row of the list in
/// focus (the one pointed at, `row`) as a path and whether it is a
/// directory, or `None` with nothing highlighted. `Err` stops, the status
/// saying why: a mark or a cached row that is gone, or a row pointed at that
/// is no longer listed.
pub(super) fn chosen(
    app: &mut App,
    focus: &Focus,
    discovery: &mut Discovery,
    row: Row,
) -> Result<Option<(PathBuf, bool)>, ()> {
    if let Focus::Marks { rows, index } = focus {
        let Some(mark) = rows.get(*index).cloned() else {
            return Ok(None);
        };
        if std::fs::symlink_metadata(&mark).is_err() {
            // The marked list reads keys in Normal mode, on a file.
            let cell = crate::keymap::Cell {
                program: app.program(),
                place: crate::keymap::Place::Marks,
                highlight: crate::keymap::Highlight::File,
            };
            let unmark = app
                .keymap
                .first(Function::ToggleMark, crate::keymap::Mode::Normal, Some(cell))
                .filter(|b| b.applies(cell))
                .map(|b| b.display());
            app.status = match unmark {
                Some(key) => format!("This mark no longer exists; {key} unmarks it"),
                None => "This mark no longer exists".into(),
            };
            return Err(());
        }
        let directory = mark.is_dir();
        return Ok(Some((mark, directory)));
    }
    if row.unnumbered {
        return Ok(None);
    }
    let Some(target) = app.highlighted_row().map(|c| crate::Candidate::clone(c)) else {
        if row.on_row {
            // A ranking that was pending landed: never substitute anything
            // for the row pointed at.
            app.status = "That row is no longer listed".into();
            return Err(());
        }
        return Ok(None);
    };
    if !app.ready_entry(&target) {
        return Err(());
    }
    discovery.checkpoint(app);
    app.highlight_top = false;
    app.wanted = None;
    Ok(Some((target.path, target.directory)))
}
/// What Enter's functions that finish do with the `chosen` entry, each its
/// one thing where its scopes let it run; `mode` is the one keys are read
/// in. `None` stays, the status saying why.
pub(super) fn finish_with_entry(
    app: &mut App,
    focus: &mut Focus,
    function: Function,
    chosen: Option<(PathBuf, bool)>,
    mode: keymap::Mode,
) -> Option<Finish> {
    use Function::*;
    match (function, chosen) {
        (PrintHighlighted, Some((path, _))) => Some(Finish::Paths(vec![path])),
        (PrintHighlightedAndMarks, Some((path, _))) => app.confirm(Some(path)).map(Finish::Paths),
        (OpenAndQuit, Some((path, _))) => match open_and_leave(app, std::slice::from_ref(&path)) {
            Ok(finish) => Some(finish),
            Err(error) => {
                app.status = error;
                None
            }
        },
        (SaveOver, Some((path, _))) => save_as(app, focus, path),
        // A directory itself, or a file's folder (fff seek).
        (QuitIntoHighlighted, Some((path, true))) => Some(Finish::Folder(path)),
        (QuitIntoHighlighted, Some((path, false))) => {
            Some(Finish::Folder(path.parent().unwrap_or(&app.cwd).to_owned()))
        }
        // Nothing highlighted: the current directory. While typing, only a
        // folder verified empty, so that a query matching nothing does not
        // leave.
        (QuitIntoHighlighted, None) if mode == keymap::Mode::Normal => {
            Some(Finish::Folder(app.cwd.clone()))
        }
        (QuitIntoHighlighted, None) => app.empty_folder().map(Finish::Paths),
        _ => {
            app.nothing_to_enter();
            None
        }
    }
}
