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
/// Normal-mode keys of the choosers and the go-to prompt, which are fixed:
/// j, k, gg and G move (choosers only), q and ! leave.
pub(super) enum ChooserKey {
    Pass(KeyEvent),
    Consumed,
    Quit(bool),
}
pub(super) fn chooser_key(
    app: &mut App,
    focus: &mut Focus,
    editor: &mut Nvim,
    key: KeyEvent,
) -> ChooserKey {
    let list = matches!(
        focus,
        Focus::Bookmarks { .. } | Focus::Palettes { .. } | Focus::BookmarkIcons { .. }
    );
    if !(list || matches!(focus, Focus::Location(_)))
        // Normal mode, also while a sequence begun there waits.
        || !app.edit_view.as_ref().is_some_and(|v| v.mode == "n")
    {
        return ChooserKey::Pass(key);
    }
    let fixed: &[(&str, u8)] = if list {
        &[("j", 0), ("k", 1), ("gg", 2), ("G", 3), ("q", 4), ("!", 5)]
    } else {
        &[("q", 4), ("!", 5)]
    };
    loop {
        let mut sequence: Vec<_> = app.pending.iter().map(|e| keymap::Key::press(*e)).collect();
        sequence.push(keymap::Key::press(key));
        let candidates: Vec<(Vec<keymap::Key>, u8)> = fixed
            .iter()
            .map(|(text, act)| (keymap::parse(text).unwrap(), *act))
            .filter(|(keys, _)| {
                keys.len() >= sequence.len() && keys[..sequence.len()] == sequence[..]
            })
            .collect();
        if let Some((_, act)) = candidates.iter().find(|(k, _)| k.len() == sequence.len()) {
            app.pending.clear();
            return match act {
                // As Alt+j and Alt+k: the bookmark chooser's arrows reorder.
                0 => ChooserKey::Pass(KeyEvent::new(K::Char('j'), M::ALT)),
                1 => ChooserKey::Pass(KeyEvent::new(K::Char('k'), M::ALT)),
                4 => ChooserKey::Quit(false),
                5 => ChooserKey::Quit(true),
                act => {
                    jump_to_end(app, focus, *act == 3);
                    ChooserKey::Consumed
                }
            };
        }
        if !candidates.is_empty() {
            app.pending.push(key);
            return ChooserKey::Consumed;
        }
        if app.pending.is_empty() {
            return ChooserKey::Pass(key);
        }
        // Earlier keys go to Neovim unchanged; then retry the current key.
        for pending in std::mem::take(&mut app.pending) {
            deliver(app, focus, editor, pending);
        }
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
/// The text `focus` edits in Neovim, and its field; `query` is the results'
/// query, which lives outside the focus. `None` where nothing is typed.
fn focused_text<'a>(
    focus: &'a mut Focus,
    query: &'a mut String,
    grep: Option<&'a mut crate::grep::Grep>,
) -> Option<(&'a mut String, Field)> {
    Some(match focus {
        Focus::Results => match grep {
            Some(grep) if grep.editing => (&mut grep.text, Field::Contents),
            _ => (query, Field::Query),
        },
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
        Focus::Keys(screen) if screen.editor.is_none() => (&mut screen.query, Field::Keys),
        Focus::Commands { query, .. } => (query, Field::Commands),
        _ => return None,
    })
}
/// The focused field and a copy of its text, for activating it in Neovim.
pub(super) fn text_field(app: &mut App, focus: &mut Focus) -> Option<(Field, String)> {
    focused_text(focus, &mut app.query, app.grep.as_mut())
        .map(|(text, field)| (field, text.clone()))
}
/// The focused text changed: filtered lists go back to their first row, the
/// go-to prompt's chosen suggestion is dropped, and the results follow the query.
fn text_changed(app: &mut App, focus: &mut Focus) {
    match focus {
        Focus::Results if app.grep.as_ref().is_some_and(|g| g.editing) => {
            let text = app
                .grep
                .as_ref()
                .map(|g| g.text.clone())
                .unwrap_or_default();
            app.set_contents(text);
        }
        Focus::Results => {
            app.offset = 0;
            app.refresh();
        }
        Focus::Bookmarks { index, .. }
        | Focus::BookmarkIcons { index, .. }
        | Focus::Palettes { index, .. } => *index = 0,
        Focus::Location(location) => location.selected = None,
        Focus::Keys(screen) => screen.index = 0,
        Focus::Commands { index, .. } => *index = 0,
        _ => {}
    }
}
/// Type `key` into the focused text field (also to replay an unmatched
/// sequence prefix there).
pub(super) fn deliver(app: &mut App, focus: &mut Focus, editor: &mut Nvim, key: KeyEvent) {
    let Some((text, field)) = focused_text(focus, &mut app.query, app.grep.as_mut()) else {
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
    let Some((text, field)) = focused_text(focus, &mut app.query, app.grep.as_mut()) else {
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
