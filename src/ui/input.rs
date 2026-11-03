use super::*;

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
pub(super) enum PaletteKey {
    Choose,
    Reload,
}
/// Alt+p opens the chooser; Alt+P (Alt+Shift+p in either terminal encoding)
/// re-reads the configured palette.
pub(super) fn palette_key(key: KeyEvent) -> Option<PaletteKey> {
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
pub(super) struct PalettePreview {
    target: Option<(Option<String>, crate::config::Palette)>,
    since: Option<std::time::Instant>,
    applied: bool,
}
impl PalettePreview {
    pub(super) fn tick(&mut self, app: &mut App, focus: &Focus, now: std::time::Instant) -> bool {
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
    pub(super) fn remaining(&self, now: std::time::Instant) -> Option<Duration> {
        if self.applied {
            return None;
        }
        self.since.map(|since| {
            Duration::from_millis(100).saturating_sub(now.saturating_duration_since(since))
        })
    }
}
/// Opt-in hook for Normal-mode list keys: the actions a list view supports.
/// Unsupported actions behave as unbound, so their keys still reach Neovim.
/// List views with rows also have an arm in `jump_to_end`, and those with a
/// text field one in `focused_text`;
/// actions with an `alt_key` are replayed as the view's own chord handling.
pub(super) fn list_actions(focus: &Focus) -> Option<&'static [ListAction]> {
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
            MarkAll,
            InvertMarks,
            Quit,
            QuitCurrent,
            Run,
            RunExit,
            Move,
            Copy,
            Link,
            Delete,
            DeletePermanently,
            Rename,
            Extract,
            Edit,
            New,
            Open,
            Undo,
            Restore,
            Trash,
            Shallower,
            Deeper,
            Bookmarks,
            Hidden,
            Directories,
            Files,
            Preview,
            GoTo,
            PrevTab,
            NextTab,
            EditConfig,
            Exclusions,
            EnforceExclusions,
            Filters,
        ]),
        Focus::Marks { .. } => Some(&[
            Down,
            Up,
            First,
            Last,
            Mark,
            MarkAll,
            InvertMarks,
            Quit,
            QuitCurrent,
            Run,
            RunExit,
            Move,
            Copy,
            Link,
            Delete,
            DeletePermanently,
            Rename,
            Extract,
            Edit,
            New,
            Open,
            Undo,
            Restore,
            EditConfig,
        ]),
        Focus::Bookmarks { .. } | Focus::Palettes { .. } | Focus::BookmarkIcons { .. } => {
            Some(&[Down, Up, First, Last, Quit, QuitCurrent])
        }
        Focus::Exclusions(_) | Focus::Filters(_) | Focus::BookmarkEdit(_) => None,
        Focus::Location(_) => Some(&[Quit, QuitCurrent]),
        _ => None,
    }
}
pub(super) fn jump_to_end(app: &mut App, focus: &mut Focus, last: bool) {
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
pub(super) enum NormalKey {
    Pass(KeyEvent),
    Consumed,
    Quit(bool),
    /// gx (false) or gX (true).
    Run(bool),
    /// A file operation, by its default letter.
    Operation(char),
    /// A plain digit in Normal-mode results: search to this depth.
    Depth(usize),
    /// gc: edit the configuration, then apply it.
    EditConfig,
}
/// Dispatch configured Normal-mode actions in their supported list views.
/// Marked lists accept their actions without an editing mode.
pub(super) fn normal_list_key(
    app: &mut App,
    focus: &mut Focus,
    editor: &mut Nvim,
    key: KeyEvent,
) -> NormalKey {
    let Some(actions) = list_actions(focus) else {
        return NormalKey::Pass(key);
    };
    loop {
        let normal = matches!(focus, Focus::Marks { .. })
            || app.edit_view.as_ref().is_some_and(|v| v.normal());
        // Shift+Space is its own chord (mark), never a configured `<space>`.
        let shift_space = key.code == K::Char(' ') && key.modifiers.contains(M::SHIFT);
        if (!normal || shift_space) && !app.normal_keys.pending() {
            return NormalKey::Pass(key);
        }
        if matches!(focus, Focus::Results)
            && normal
            && app.normal_keys.depth_digits
            && !app.normal_keys.pending()
            && key.modifiers.is_empty()
            && let K::Char(c @ '1'..='9') = key.code
        {
            return NormalKey::Depth(c as usize - '0' as usize);
        }
        let can_quit = app.normal_keys.pending()
            || matches!(focus, Focus::Marks { .. })
            || app.edit_view.as_ref().is_some_and(|v| v.normal());
        // fff seek cannot navigate and lists only files: it has no tabs and
        // nothing to hide, so those keys act as unbound (Up and Down move the
        // highlight, d and D reach Neovim).
        let navigation = !app.no_dir;
        match app.normal_keys.feed(key, |a| {
            actions.contains(&a)
                && (can_quit || !matches!(a, ListAction::Quit | ListAction::QuitCurrent))
                && (navigation
                    || !matches!(
                        a,
                        ListAction::PrevTab
                            | ListAction::NextTab
                            | ListAction::Directories
                            | ListAction::Files
                    ))
        }) {
            Step::Pass => return NormalKey::Pass(key),
            Step::Pending => return NormalKey::Consumed,
            Step::Action(ListAction::Quit) => return NormalKey::Quit(false),
            Step::Action(ListAction::QuitCurrent) => return NormalKey::Quit(true),
            Step::Action(ListAction::Run) => return NormalKey::Run(false),
            Step::Action(ListAction::RunExit) => return NormalKey::Run(true),
            Step::Action(ListAction::EditConfig) => return NormalKey::EditConfig,
            Step::Action(action @ (ListAction::MarkAll | ListAction::InvertMarks)) => {
                let rows: Vec<PathBuf> = match focus {
                    Focus::Marks { rows, .. } => rows.clone(),
                    _ => app.rows.iter().map(|c| c.path.clone()).collect(),
                };
                app.mark_rows(&rows, action == ListAction::InvertMarks);
                return NormalKey::Consumed;
            }
            Step::Action(action) if action.operation_key().is_some() => {
                return NormalKey::Operation(action.operation_key().unwrap());
            }
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
/// The text `focus` edits in Neovim, and its field; `query` is the results'
/// query, which lives outside the focus. `None` where nothing is typed.
fn focused_text<'a>(
    focus: &'a mut Focus,
    query: &'a mut String,
) -> Option<(&'a mut String, Field)> {
    Some(match focus {
        Focus::Results => (query, Field::Query),
        Focus::Location(location) => (&mut location.text, Field::Location),
        Focus::Command(prompt) => (&mut prompt.text, Field::Command),
        Focus::Exclusions(text) => (text, Field::Exclusions),
        Focus::Filters(text) => (text, Field::Filters),
        Focus::Operation(draft) => {
            let field = draft.field();
            (&mut draft.text, field)
        }
        Focus::Bookmarks { query, .. } => (query, Field::Bookmarks),
        Focus::BookmarkIcons { query, .. } => (query, Field::BookmarkIcons),
        Focus::BookmarkEdit(draft) => match draft.field {
            BookmarkField::Name => (&mut draft.name, Field::BookmarkName),
            BookmarkField::Path => (&mut draft.path, Field::BookmarkPath),
            BookmarkField::Icon => return None,
        },
        Focus::Palettes { query, .. } => (query, Field::Palettes),
        _ => return None,
    })
}
/// The focused field and a copy of its text, for activating it in Neovim.
pub(super) fn text_field(app: &mut App, focus: &mut Focus) -> Option<(Field, String)> {
    focused_text(focus, &mut app.query).map(|(text, field)| (field, text.clone()))
}
/// The focused text changed: filtered lists go back to their first row, the
/// go-to prompt's chosen suggestion is dropped, and the results follow the query.
fn text_changed(app: &mut App, focus: &mut Focus) {
    match focus {
        Focus::Results => {
            app.offset = 0;
            app.refresh();
        }
        Focus::Bookmarks { index, .. }
        | Focus::BookmarkIcons { index, .. }
        | Focus::Palettes { index, .. } => *index = 0,
        Focus::Location(location) => location.selected = None,
        _ => {}
    }
}
/// Type `key` into the focused text field (also to replay an unmatched
/// sequence prefix there).
pub(super) fn deliver(app: &mut App, focus: &mut Focus, editor: &mut Nvim, key: KeyEvent) {
    let Some((text, field)) = focused_text(focus, &mut app.query) else {
        return;
    };
    let before = text.clone();
    match editor.input(field, key) {
        Ok((updated, state)) => {
            *text = updated;
            app.edit_view = Some(state);
        }
        Err(error) => editor.break_with(error),
    }
    if *text != before {
        text_changed(app, focus);
    }
}
/// Paste into the focused text field. Single-line fields take line breaks
/// as spaces.
pub(super) fn paste_text(app: &mut App, focus: &mut Focus, editor: &mut Nvim, pasted: &str) {
    let Some((text, field)) = focused_text(focus, &mut app.query) else {
        return;
    };
    let pasted = if matches!(field, Field::Exclusions | Field::Filters | Field::Operation) {
        pasted.to_owned()
    } else {
        pasted.replace(['\r', '\n'], " ")
    };
    let before = text.clone();
    match editor.paste(field, &pasted) {
        Ok((updated, state)) => {
            *text = updated;
            app.edit_view = Some(state);
        }
        Err(error) => editor.break_with(error),
    }
    if *text != before {
        text_changed(app, focus);
    }
}
/// Starts the embedded Neovim every text field uses; fff needs it.
pub fn start_editor(program: &str) -> io::Result<Nvim> {
    Nvim::start_program(program).map_err(|error| {
        io::Error::other(format!(
            "fff needs Neovim ({program}) for its text fields: {error}"
        ))
    })
}
