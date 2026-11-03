//! Each focus's own keys, once `dispatch.rs` has routed the session-wide
//! ones. A key a handler does not use goes to Neovim when the focus has a
//! text field (`deliver`).
use super::*;

/// How Enter reached the results.
#[derive(Clone, Copy)]
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
            Focus::Results => return Ok(self.results_key(app, key, row)),
            Focus::Marks { .. } => return Ok(self.marks_key(app, key)),
            Focus::Replace(_) => return Ok(self.replace_key(app, key)),
            Focus::RunConfirm(_) => return self.run_confirm_key(app, key),
            Focus::NewChoice => self.new_choice_key(app, key),
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
            Focus::Info { .. } => {}
            Focus::Command(_) => self.command_key(app, key),
            Focus::Location(_) => self.location_key(app, key),
            // Keys never reach here while an operation runs, and the glob
            // editors' keys all went to Neovim (`buffer_input`).
            Focus::Busy(_) | Focus::Exclusions(_) | Focus::Filters(_) => {}
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
                perform_operation(app, &mut self.discovery, &mut self.focus, request, None);
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
        match *action {
            ScopedAction::Extract => self.focus = start_extract(app, &mut self.editor, paths),
            ScopedAction::Delete(permanently) => delete(
                app,
                &mut self.discovery,
                &mut self.focus,
                paths,
                permanently,
            ),
            ScopedAction::Run(exit) => self.focus = start_run(&mut self.editor, paths, exit),
            ScopedAction::Edit => {
                self.focus = Focus::Results;
                self.edit_files(app, paths)?;
            }
            ScopedAction::Open => {
                self.focus = Focus::Results;
                self.open_here(app, paths)?;
            }
            ScopedAction::Restore => restore(app, &mut self.discovery, &mut self.focus, paths),
        }
        Ok(())
    }
    fn transfer_confirm_key(&mut self, app: &mut App, key: KeyEvent) {
        let Focus::TransferConfirm(plan) = &self.focus else {
            return;
        };
        match key.code {
            K::Enter => {
                let request = plan.request();
                let marks = plan.dropped.then(|| app.marks.clone());
                self.focus = Focus::Results;
                match request {
                    Ok(r) => perform_operation(app, &mut self.discovery, &mut self.focus, r, marks),
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
                    Ok(r) => perform_operation(app, &mut self.discovery, &mut self.focus, r, None),
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
                perform_operation(app, &mut self.discovery, &mut self.focus, request, None);
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
                    perform_operation(app, &mut self.discovery, &mut self.focus, request, None);
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
                Ok(r) => perform_operation(app, &mut self.discovery, &mut self.focus, r, None),
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
                    app.navigate(path, false);
                    self.discovery.start(app, true);
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
    /// Alt+Space: the marked list.
    fn marks_key(&mut self, app: &mut App, key: KeyEvent) -> Option<Finish> {
        let ctrl = key.modifiers.contains(M::CONTROL);
        let alt = key.modifiers.contains(M::ALT);
        let Focus::Marks { rows, index } = &mut self.focus else {
            return None;
        };
        if key.code == K::Esc || (alt && key.code == K::Char(' ')) {
            self.focus = Focus::Results;
        } else if key.code == K::Char(' ') && key.modifiers.contains(M::SHIFT) {
            if let Some(p) = rows.get(*index) {
                app.toggle(p.clone());
            }
        } else if key.code == K::Down || (alt && key.code == K::Char('j')) {
            *index = cycle(*index, rows.len(), true);
        } else if key.code == K::Up || (alt && key.code == K::Char('k')) {
            *index = cycle(*index, rows.len(), false);
        } else if ctrl && key.code == K::Char('w') {
            return leave_with_marks(app);
        } else if key.code == K::Enter
            && let Some(mark) = rows.get(*index).cloned()
        {
            return self.enter_mark(app, mark);
        }
        None
    }
    /// Enter on a mark: like Normal-mode Enter in the results.
    fn enter_mark(&mut self, app: &mut App, mark: PathBuf) -> Option<Finish> {
        if std::fs::symlink_metadata(&mark).is_err() {
            app.status = "This mark no longer exists; Space unmarks it".into();
            return None;
        }
        if app.dialog.as_ref().is_some_and(|d| d.save.is_some()) && !mark.is_dir() {
            return save_as(app, &mut self.focus, mark);
        }
        if app.dialog.is_some() {
            return Some(Finish::Paths(vec![mark]));
        }
        // A file opens and fff leaves (or is printed), a directory is left
        // into.
        if app.output_paths && !mark.is_dir() {
            return Some(Finish::Paths(vec![mark]));
        }
        if !app.output_paths && !app.no_dir && !mark.is_dir() {
            return match open_and_leave(app, std::slice::from_ref(&mark)) {
                Ok(finish) => Some(finish),
                Err(error) => {
                    app.status = error;
                    None
                }
            };
        }
        // With a window of its own, a directory is entered.
        if app.window && mark.is_dir() {
            if app.can_navigate() {
                app.navigate(mark, false);
                app.wanted = None;
                self.discovery.start(app, true);
                self.focus = Focus::Results;
            }
            return None;
        }
        let folder = if mark.is_dir() {
            mark
        } else {
            mark.parent().unwrap_or(&app.cwd).to_owned()
        };
        Some(Finish::Folder(folder))
    }
    fn results_key(&mut self, app: &mut App, key: KeyEvent, row: Row) -> Option<Finish> {
        let ctrl = key.modifiers.contains(M::CONTROL);
        let alt = key.modifiers.contains(M::ALT);
        let page = self.drawn.page;
        match (key.code, ctrl, alt) {
            (K::F(1) | K::F(2), _, _) => {
                self.focus = Focus::Info {
                    offset: 0,
                    keys: key.code == K::F(1),
                    screen: Box::new(KeysScreen::new(app.cell())),
                }
            }
            (K::Char('w'), true, _) => return leave_with_marks(app),
            // Dialogs: save here, or choose this folder, while typing too.
            (K::Char('s'), true, _) if app.dialog.is_some() => {
                if app.dialog.as_ref().is_some_and(|d| d.save.is_some()) {
                    return save_here(app, &mut self.focus);
                } else if app.dialog.as_ref().is_some_and(|d| d.directory) {
                    return Some(Finish::Paths(vec![app.cwd.clone()]));
                }
            }
            // Ctrl+i needs a terminal that tells it apart from Tab (Kitty
            // keyboard protocol); elsewhere it arrives as Tab, which is unbound.
            (K::Char('o' | 'i'), true, false) if !app.can_navigate() => {}
            (K::Char('o' | 'i'), true, false) => {
                if app.jump(key.code == K::Char('o')) {
                    self.discovery.start(app, true);
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
                    self.discovery.start(app, true);
                }
            }
            (K::Char(' '), _, true) => {
                self.focus = Focus::Marks {
                    rows: app.marks.to_vec(),
                    index: 0,
                }
            }
            (K::Char('e'), _, true) => self.focus = Focus::Exclusions(app.policy.rules.join("\n")),
            (K::Char('f'), _, true) => self.focus = Focus::Filters(app.policy.filters.join("\n")),
            (K::Char('g' | 'b' | 'h'), _, true) if !app.can_navigate() => {}
            (K::Char('g'), _, true) => {
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
            (K::Char('b'), _, true) => {
                self.focus = Focus::Bookmarks {
                    query: String::new(),
                    index: 0,
                }
            }
            (K::Char('l'), _, true) => activate_highlighted(app, &mut self.discovery),
            (K::Char('p' | 'P'), _, true) if palette_key(key) == Some(PaletteKey::Reload) => {
                reload_palette(app, &self.palettes)
            }
            (K::Char('p'), _, true) => {
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
            (K::Char('t'), _, true) if app.can_navigate() => self.open_trash(app),
            (K::F(5), _, _) => self.discovery.force_refresh(app),
            (K::Char('h'), _, true) => {
                if let Some(p) = app.cwd.parent().map(PathBuf::from) {
                    app.navigate(p, true);
                    self.discovery.start(app, true);
                }
            }
            (K::Char(c @ ('j' | 'k')), true, false) => scroll_preview(app, c == 'j'),
            (K::Char('j'), _, true) | (K::Down, false, false) => app.step(true),
            (K::Up, false, false) | (K::Char('k'), _, true) => app.step(false),
            (K::PageDown, _, _) => {
                app.move_by(page as isize);
                app.offset = app
                    .offset
                    .saturating_add(page)
                    .min(app.shown().len().saturating_sub(page));
            }
            (K::PageUp, _, _) => {
                app.move_by(-(page as isize));
                app.offset = app.offset.saturating_sub(page);
            }
            (K::Right, _, true) => {
                app.depth = app.depth.saturating_add(1);
                self.discovery.start(app, false);
            }
            (K::Left, _, true) => {
                app.depth = app.depth.saturating_sub(1).max(1);
                self.discovery.start(app, false);
            }
            (K::Char('.'), _, true) => {
                app.policy.hidden = !app.policy.hidden;
                self.discovery.start(app, false);
            }
            (K::Char('v'), _, true) => app.preview.shown = !app.preview.shown,
            (K::Char('d' | 'D'), _, true) if app.no_dir => {
                let program = if app.grep.is_some() { "grep" } else { "seek" };
                app.status = format!("fff {program} lists only files");
            }
            // fff grep: type into the other query.
            (K::Char('/'), _, true) if app.grep.is_some() => {
                if let Some(grep) = &mut app.grep {
                    grep.editing = !grep.editing;
                }
            }
            (K::Char(c @ ('d' | 'D')), _, true) => {
                let only = if c == 'd' {
                    crate::Kinds::Files
                } else {
                    crate::Kinds::Directories
                };
                app.kinds = app.kinds.toggled(only);
                app.offset = 0;
                app.refresh();
            }
            (K::Char('H'), _, true) => {
                app.policy.ignore = !app.policy.ignore;
                self.discovery.start(app, false);
            }
            // Filters only choose among discovered files: no rescan.
            (K::Char('F'), _, true) => {
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
            // fff grep: Escape clears the query being typed into, else the
            // other one.
            (K::Esc, _, _)
                if app
                    .grep
                    .as_ref()
                    .is_some_and(|g| !g.text.is_empty() && (g.editing || app.query.is_empty())) =>
            {
                app.set_contents(String::new());
            }
            (K::Esc, _, _)
                if app.grep.as_ref().is_some_and(|g| g.editing) && !app.query.is_empty() =>
            {
                app.query.clear();
                app.offset = 0;
                app.refresh();
            }
            // Escape clears the query; pressed again with none, the marks.
            (K::Esc, _, _) if app.query.is_empty() => {
                if std::mem::replace(&mut self.escape_armed, true) && !app.marks.is_empty() {
                    let count = app.marks.len();
                    app.marks.clear();
                    app.status =
                        format!("Cleared {count} mark{}", if count == 1 { "" } else { "s" });
                    app.status_success = app.status.clone();
                    self.escape_armed = false;
                }
            }
            (K::Esc, _, _) => {
                app.query.clear();
                app.offset = 0;
                app.refresh();
            }
            // Shift+Space marks in every mode; Tab only while typing, for
            // terminals that cannot tell Shift+Space from Space.
            (K::Char(' '), _, _) | (K::Tab, _, _)
                if (key.code == K::Tab && !app.edit_view.as_ref().is_some_and(|v| v.normal()))
                    || (key.code == K::Char(' ') && key.modifiers.contains(M::SHIFT)) =>
            {
                if let Some(p) = app.highlight.clone() {
                    app.toggle(p);
                    app.move_by(1);
                }
            }
            (K::Tab, _, _) => {}
            (K::Enter, _, _) => return self.results_enter(app, row),
            _ => deliver(app, &mut self.focus, &mut self.editor, key),
        }
        None
    }
    /// Alt+t: the trash, a folder like any other; again, the next one (each
    /// filesystem has its own).
    fn open_trash(&mut self, app: &mut App) {
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
        app.navigate(next, false);
        self.discovery.start(app, true);
    }
    /// Enter in the results: what it does depends on the mode (typing or
    /// Normal), the session kind and the highlighted entry.
    fn results_enter(&mut self, app: &mut App, row: Row) -> Option<Finish> {
        let target = app
            .highlighted_row()
            .filter(|_| !row.unnumbered)
            .map(|c| crate::Candidate::clone(c));
        // The row pointed at is gone (a ranking that was pending landed):
        // never substitute the browsing directory.
        if row.on_row && target.is_none() {
            app.status = "That row is no longer listed".into();
            return None;
        }
        let normal = app.edit_view.as_ref().is_some_and(|v| v.normal());
        let saving = app.dialog.as_ref().is_some_and(|d| d.save.is_some());
        // Typing a name to save under: Enter saves it, as in any name field.
        // Ctrl+number and a double click still act on their row.
        if !normal && !row.on_row && saving {
            return save_here(app, &mut self.focus);
        }
        // Normal mode: finish with the entry. In the default mode a file
        // opens and fff leaves; otherwise fff exits into the entry's folder
        // (a directory itself), also for deep results. A dialog chooses files
        // and enters folders instead.
        if app.dialog.is_none() && normal {
            return finish_with(app, &mut self.discovery, target.as_ref());
        }
        match target {
            // fff seek never navigates, so there is nothing to reveal:
            // typing, Enter chooses the file too.
            Some(file) if !file.directory && app.no_dir && app.dialog.is_none() => {
                finish_with(app, &mut self.discovery, Some(&file))
            }
            // Typing: a file is revealed in its folder, unless fff is picking
            // paths for a script.
            Some(file) if !file.directory && !app.output_paths => {
                if app.reveal(file.path) {
                    self.discovery.start(app, true);
                } else {
                    // The query was cleared here, not by moving: keep
                    // typing, as after a move.
                    self.editor.reset_preserving_mode(Field::Query);
                }
                None
            }
            // Saving over a file asks first; with nothing listed (an empty
            // folder) Enter saves.
            Some(file) if !file.directory && saving && app.ready_path(&file.path) => {
                save_as(app, &mut self.focus, file.path)
            }
            None if saving => save_here(app, &mut self.focus),
            // A dialog with one answer takes this file, whatever is marked.
            Some(file)
                if !file.directory
                    && app.dialog.as_ref().is_some_and(|d| !d.multiple)
                    && app.ready_path(&file.path) =>
            {
                Some(Finish::Paths(vec![file.path]))
            }
            target => match app.enter(target) {
                EnterOutcome::Selected(paths) => Some(Finish::Paths(paths)),
                EnterOutcome::Navigated => {
                    self.discovery.start(app, true);
                    None
                }
                EnterOutcome::Stay => None,
            },
        }
    }
}
/// Ctrl+j / Ctrl+k: the preview pane's content scrolls, the highlight stays.
fn scroll_preview(app: &mut App, down: bool) {
    if !app.preview.shown {
        app.status = match app.bound(Function::Preview) {
            Some(key) => format!("No preview pane to scroll; {key} opens it"),
            None => "No preview pane to scroll".into(),
        };
    } else if app.preview.current().is_some() {
        app.preview.scroll(down);
    }
}
