//! The Keys screen (F1): every function with its keys, searched by text or
//! by pressing keys, and edited in place. Edits apply at once and are saved
//! to `keys.toml`; one that would leave a key doing two things, or lock you
//! in, is refused. The screen's own keys are fixed, and Escape always backs
//! out of it.
use super::*;
use crate::keymap::{Binding, Cell, Dimension, Function, Key, Keymap, Mode, Scope};
use crossterm::event::KeyCode;

/// How the screen searches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Search {
    /// The query filters names, descriptions and key names.
    Text,
    /// Keys pressed are recorded, not typed.
    Capturing(Vec<KeyEvent>),
    /// Recording stopped: the keys filter, arrows move.
    Captured(Vec<KeyEvent>),
}
pub(crate) struct KeysScreen {
    /// Where F1 was pressed.
    pub here: Cell,
    /// Every function, not only those that work where F1 was pressed.
    pub everywhere: bool,
    pub query: String,
    pub search: Search,
    pub index: usize,
    pub editing: Option<Editing>,
    /// What the last edit came to; `true` for an error.
    pub message: Option<(String, bool)>,
}
/// A row of the screen's list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Listed {
    Function(Function),
    /// An entry of [`crate::keymap::FIXED`].
    Fixed(usize),
    /// A key search's match.
    Binding(Binding),
}
/// The function being looked at, and its highlighted binding.
pub(crate) struct Editing {
    pub function: Function,
    pub index: usize,
    pub step: EditStep,
}
pub(crate) enum EditStep {
    Browse,
    /// `a`: which mode the new key is for.
    ChooseMode,
    /// The keys of a new binding, as pressed.
    Capture {
        mode: Mode,
        keys: Vec<Key>,
    },
    /// Where a binding applies: one toggle per value of each dimension.
    Scope {
        draft: Binding,
        /// The binding it replaces, by its row.
        replaces: Option<usize>,
        cursor: usize,
    },
}
/// The toggles of the scope editor, in order.
pub(crate) fn toggles() -> Vec<(&'static str, &'static str, u16, usize)> {
    let mut all = Vec::new();
    for (i, &p) in crate::keymap::Program::ALL.iter().enumerate() {
        all.push(("Program", p.name(), p.bit(), i));
    }
    for &p in crate::keymap::Place::ALL {
        all.push(("Place", p.name(), p.bit(), 0));
    }
    for &h in crate::keymap::Highlight::ALL {
        all.push(("Highlight", h.name(), h.bit(), 0));
    }
    all
}
fn dimension<'a>(scope: &'a mut Scope, name: &str) -> &'a mut u16 {
    match name {
        "Program" => &mut scope.programs,
        "Place" => &mut scope.places,
        _ => &mut scope.highlights,
    }
}
pub(crate) fn dimension_of(scope: Scope, name: &str) -> u16 {
    match name {
        "Program" => scope.programs,
        "Place" => scope.places,
        _ => scope.highlights,
    }
}

impl KeysScreen {
    pub fn new(here: Cell) -> Self {
        Self {
            here,
            everywhere: false,
            query: String::new(),
            search: Search::Text,
            index: 0,
            editing: None,
            message: None,
        }
    }
    /// Whether a box reaches where F1 was pressed, whatever is highlighted.
    pub fn reaches(&self, scope: Scope) -> bool {
        scope.programs & self.here.program.bit() != 0 && scope.places & self.here.place.bit() != 0
    }
    pub fn works_here(&self, function: Function) -> bool {
        Mode::ALL
            .into_iter()
            .any(|m| function.available(m).into_iter().any(|s| self.reaches(s)))
    }
    pub fn applies_here(&self, binding: &Binding) -> bool {
        binding.reach().into_iter().any(|s| self.reaches(s))
    }
    /// The list's rows: functions (and fixed keys) matching the query, or
    /// the bindings whose keys start with those pressed.
    pub fn listed(&self, keymap: &Keymap) -> Vec<Listed> {
        if let Search::Capturing(keys) | Search::Captured(keys) = &self.search {
            let pressed: Vec<Key> = keys.iter().map(|k| Key::press(*k)).collect();
            let mut found: Vec<&Binding> = keymap
                .bindings()
                .iter()
                .filter(|b| {
                    b.keys.len() >= pressed.len()
                        && b.keys.iter().zip(&pressed).all(|(k, p)| k.takes(*p))
                })
                .collect();
            found.sort_by_key(|b| (b.mode, b.display(), b.function));
            return found.into_iter().cloned().map(Listed::Binding).collect();
        }
        let words: Vec<String> = self
            .query
            .split_whitespace()
            .map(|w| w.to_lowercase())
            .collect();
        let labels = |f: Function| {
            keymap
                .bindings()
                .iter()
                .filter(|b| b.function == f)
                .map(|b| b.display().to_lowercase())
                .collect::<Vec<_>>()
                .join(" ")
        };
        let mut rows: Vec<(i64, Listed)> = Vec::new();
        for (i, &f) in Function::ALL.iter().enumerate() {
            if !self.everywhere && !self.works_here(f) {
                continue;
            }
            if words.is_empty() {
                rows.push((-(i as i64), Listed::Function(f)));
                continue;
            }
            let name = crate::score(f.name(), &self.query.replace(' ', "")).map(|s| s + 100_000);
            let text = format!(
                "{} {} {}",
                f.name(),
                f.description().to_lowercase(),
                labels(f)
            );
            let words_in = words
                .iter()
                .all(|w| text.contains(w.as_str()))
                .then_some(-(i as i64));
            if let Some(score) = name.or(words_in) {
                rows.push((score, Listed::Function(f)));
            }
        }
        for (i, (keys, place, what)) in crate::keymap::FIXED.iter().enumerate() {
            let text = format!("{keys} {place} {what}").to_lowercase();
            if words.iter().all(|w| text.contains(w.as_str())) {
                rows.push((-10_000 - i as i64, Listed::Fixed(i)));
            }
        }
        rows.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        rows.into_iter().map(|(_, row)| row).collect()
    }
}
/// The rows of a function's view: its bindings, Insert mode first.
pub(crate) fn binding_rows(keymap: &Keymap, function: Function) -> Vec<Binding> {
    Mode::ALL
        .into_iter()
        .flat_map(|m| keymap.of(function, m).into_iter().cloned())
        .collect()
}
/// Functions whose key is a digit family: a digit pressed for them stands
/// for all digits.
fn takes_digits(function: Function) -> bool {
    matches!(
        function,
        Function::Depth | Function::OpenRow | Function::EnterRow
    )
}

impl Session {
    /// F1 / F2 overlay. The guide and session keys switch tabs or close
    /// them; Escape backs out step by step.
    pub(super) fn info_key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<Option<Finish>> {
        let press = Key::press(key);
        let is = |f: Function| {
            Mode::ALL.into_iter().any(|m| {
                app.keymap
                    .of(f, m)
                    .iter()
                    .any(|b| b.keys.len() == 1 && b.keys[0] == press)
            })
        };
        let (guide, session) = (is(Function::Guide), is(Function::Session));
        let page = self.drawn.page;
        let Focus::Info {
            offset,
            keys,
            screen,
        } = &mut self.focus
        else {
            return Ok(None);
        };
        if !*keys {
            match key.code {
                K::Esc | K::Char('q') => self.focus = Focus::Results,
                _ if session => self.focus = Focus::Results,
                K::Tab | K::BackTab | K::Left | K::Right => *keys = true,
                _ if guide => *keys = true,
                K::Down | K::Char('j') => *offset = offset.saturating_add(1),
                K::Up | K::Char('k') => *offset = offset.saturating_sub(1),
                K::PageDown => *offset = offset.saturating_add(page),
                K::PageUp => *offset = offset.saturating_sub(page),
                _ => {}
            }
            return Ok(None);
        }
        // While keys are recorded, they are the search, not commands.
        let recording = matches!(screen.search, Search::Capturing(_))
            || screen
                .editing
                .as_ref()
                .is_some_and(|e| matches!(e.step, EditStep::Capture { .. }));
        if !recording && screen.editing.is_none() {
            if guide && !matches!(key.code, K::Char(_)) {
                self.focus = Focus::Results;
                return Ok(None);
            }
            if session && !matches!(key.code, K::Char(_)) {
                *keys = false;
                *offset = 0;
                return Ok(None);
            }
        }
        let close = keys_screen_key(app, screen, key, page);
        if close {
            self.focus = Focus::Results;
        }
        Ok(None)
    }
}
/// A key on the Keys tab; `true` closes the overlay.
fn keys_screen_key(app: &mut App, screen: &mut KeysScreen, key: KeyEvent, page: usize) -> bool {
    let ctrl = key.modifiers.contains(M::CONTROL);
    if screen.editing.is_some() {
        editing_key(app, screen, key);
        return false;
    }
    let rows = screen.listed(&app.keymap);
    let last = rows.len().saturating_sub(1);
    match (&mut screen.search, key.code) {
        (Search::Capturing(keys), K::Tab) => screen.search = Search::Captured(std::mem::take(keys)),
        (Search::Capturing(_) | Search::Captured(_), K::Esc) => {
            screen.search = Search::Text;
            screen.index = 0;
        }
        (Search::Capturing(keys), K::Backspace) => {
            keys.pop();
            screen.index = 0;
        }
        (Search::Capturing(keys), _) => {
            keys.push(key);
            screen.index = 0;
        }
        (Search::Captured(_), K::Tab) => {
            screen.search = Search::Text;
            screen.index = 0;
        }
        (Search::Text, K::Tab) => {
            screen.search = Search::Capturing(Vec::new());
            screen.index = 0;
        }
        (_, K::Esc) if !screen.query.is_empty() => {
            screen.query.clear();
            screen.index = 0;
        }
        (_, K::Esc) => return true,
        (_, K::Up) => screen.index = screen.index.saturating_sub(1),
        (_, K::Down) => screen.index = (screen.index + 1).min(last),
        (_, K::PageUp) => screen.index = screen.index.saturating_sub(page),
        (_, K::PageDown) => screen.index = (screen.index + page).min(last),
        (_, K::Enter) => match rows.get(screen.index) {
            Some(Listed::Function(f)) => open(screen, *f, None),
            Some(Listed::Binding(b)) => {
                let row = binding_rows(&app.keymap, b.function)
                    .iter()
                    .position(|r| r == b);
                open(screen, b.function, row);
            }
            _ => {}
        },
        (Search::Text, K::Char('a')) if ctrl => {
            screen.everywhere = !screen.everywhere;
            screen.index = 0;
        }
        (Search::Text, K::Char('u')) if ctrl => {
            screen.query.clear();
            screen.index = 0;
        }
        (Search::Text, K::Backspace) => {
            screen.query.pop();
            screen.index = 0;
        }
        (Search::Text, K::Char(c)) if !key.modifiers.intersects(M::CONTROL | M::ALT) => {
            screen.query.push(c);
            screen.index = 0;
        }
        _ => {}
    }
    false
}
fn open(screen: &mut KeysScreen, function: Function, row: Option<usize>) {
    screen.message = None;
    screen.editing = Some(Editing {
        function,
        index: row.unwrap_or(0),
        step: EditStep::Browse,
    });
}
/// A key in a function's view.
fn editing_key(app: &mut App, screen: &mut KeysScreen, key: KeyEvent) {
    let Some(editing) = screen.editing.as_mut() else {
        return;
    };
    let function = editing.function;
    let rows = binding_rows(&app.keymap, function);
    match &mut editing.step {
        EditStep::Browse => match key.code {
            K::Esc | K::Backspace | K::Char('q') => screen.editing = None,
            K::Up | K::Char('k') => editing.index = editing.index.saturating_sub(1),
            K::Down | K::Char('j') => {
                editing.index = (editing.index + 1).min(rows.len().saturating_sub(1))
            }
            K::Char('a') => {
                screen.message = None;
                let modes: Vec<Mode> = Mode::ALL
                    .into_iter()
                    .filter(|m| !function.available(*m).is_empty())
                    .collect();
                editing.step = match modes[..] {
                    [mode] => EditStep::Capture {
                        mode,
                        keys: Vec::new(),
                    },
                    _ => EditStep::ChooseMode,
                };
            }
            K::Char('s') => {
                if let Some(binding) = rows.get(editing.index) {
                    screen.message = None;
                    editing.step = EditStep::Scope {
                        draft: binding.clone(),
                        replaces: Some(editing.index),
                        cursor: 0,
                    };
                }
            }
            K::Char('d') => {
                if let Some(binding) = rows.get(editing.index).cloned() {
                    let mut list: Vec<Binding> = app
                        .keymap
                        .of(function, binding.mode)
                        .into_iter()
                        .cloned()
                        .collect();
                    list.retain(|b| *b != binding);
                    let next = app.keymap.with(function, binding.mode, list);
                    let done = commit(app, screen, next, "Removed");
                    if done && let Some(editing) = screen.editing.as_mut() {
                        editing.index = editing.index.saturating_sub(1);
                    }
                }
            }
            K::Char('R') => {
                let next = app.keymap.reset(function);
                commit(app, screen, next, "Back to the default keys");
            }
            _ => {}
        },
        EditStep::ChooseMode => match key.code {
            K::Char('i') => {
                editing.step = EditStep::Capture {
                    mode: Mode::Insert,
                    keys: Vec::new(),
                }
            }
            K::Char('n') => {
                editing.step = EditStep::Capture {
                    mode: Mode::Normal,
                    keys: Vec::new(),
                }
            }
            K::Esc => editing.step = EditStep::Browse,
            _ => {}
        },
        // The first key may be any key; after it, Enter finishes,
        // Backspace takes the last back and Escape gives up.
        EditStep::Capture { mode, keys } => {
            if keys.is_empty() {
                let mut pressed = Key::press(key);
                if takes_digits(function)
                    && let Key::Press(KeyCode::Char(c), m) = pressed
                    && c.is_ascii_digit()
                {
                    pressed = Key::Digits(m);
                }
                keys.push(pressed);
                return;
            }
            match key.code {
                K::Esc => editing.step = EditStep::Browse,
                K::Backspace => {
                    keys.pop();
                }
                K::Enter => {
                    let draft = Binding {
                        function,
                        mode: *mode,
                        keys: std::mem::take(keys),
                        scope: Scope::ALL,
                    };
                    add(app, screen, draft, None);
                }
                _ if matches!(keys[0], Key::Digits(_)) => {
                    screen.message = Some(("A digit family is a key on its own".into(), true));
                }
                _ => keys.push(Key::press(key)),
            }
        }
        EditStep::Scope {
            draft,
            replaces,
            cursor,
        } => {
            let all = toggles();
            match key.code {
                K::Esc => editing.step = EditStep::Browse,
                K::Up | K::Char('k') => *cursor = cursor.saturating_sub(1),
                K::Down | K::Char('j') => *cursor = (*cursor + 1).min(all.len() - 1),
                K::Char(' ') => {
                    let (dim, _, bit, _) = all[*cursor];
                    let set = dimension(&mut draft.scope, dim);
                    // A dimension keeps at least one value.
                    if *set & !bit != 0 || *set & bit == 0 {
                        *set ^= bit;
                    }
                }
                K::Enter => {
                    let (draft, replaces) = (draft.clone(), *replaces);
                    add(app, screen, draft, replaces);
                }
                // Narrow this binding to where nothing else has its keys.
                K::Char('u') => {
                    let (draft, replaces) = (draft.clone(), *replaces);
                    let mut pieces = vec![draft.scope];
                    for (other, _) in clashes(&app.keymap, &draft, replaces) {
                        for taken in other.reach() {
                            pieces = pieces.into_iter().flat_map(|p| p.minus(taken)).collect();
                        }
                    }
                    let narrowed: Vec<Binding> = pieces
                        .into_iter()
                        .map(|scope| Binding {
                            scope,
                            ..draft.clone()
                        })
                        .filter(|b| !b.reach().is_empty())
                        .collect();
                    if narrowed.is_empty() {
                        screen.message = Some((
                            "Nowhere is left for these keys; change them or take them".into(),
                            true,
                        ));
                        return;
                    }
                    let next = replace(&app.keymap, &draft, replaces, narrowed);
                    commit(
                        app,
                        screen,
                        next,
                        "Narrowed to where nothing else has these keys",
                    );
                }
                // Take these keys from the others where this one applies.
                K::Char('t') => {
                    let (draft, replaces) = (draft.clone(), *replaces);
                    let mut next = replace(&app.keymap, &draft, replaces, vec![draft.clone()]);
                    for (other, _) in clashes(&app.keymap, &draft, replaces) {
                        next = next.and_then(|keymap| {
                            let mut pieces = vec![other.scope];
                            for taken in draft.reach() {
                                pieces = pieces.into_iter().flat_map(|p| p.minus(taken)).collect();
                            }
                            let mut list: Vec<Binding> = keymap
                                .of(other.function, other.mode)
                                .into_iter()
                                .filter(|b| **b != other)
                                .cloned()
                                .collect();
                            list.extend(
                                pieces
                                    .into_iter()
                                    .map(|scope| Binding {
                                        scope,
                                        ..other.clone()
                                    })
                                    .filter(|b| !b.reach().is_empty()),
                            );
                            keymap.with(other.function, other.mode, list)
                        });
                    }
                    commit(
                        app,
                        screen,
                        next,
                        "Taken from the others where this applies",
                    );
                }
                _ => {}
            }
        }
    }
}
/// The bindings of other functions that `draft` clashes with, and where.
pub(crate) fn clashes(
    keymap: &Keymap,
    draft: &Binding,
    replaces: Option<usize>,
) -> Vec<(Binding, Scope)> {
    let replaced = replaces.and_then(|i| binding_rows(keymap, draft.function).get(i).cloned());
    keymap
        .bindings()
        .iter()
        .filter(|b| b.function != draft.function && Some(*b) != replaced.as_ref())
        .filter_map(|b| draft.clashes(b).first().map(|at| (b.clone(), *at)))
        .collect()
}
/// `keymap` with `draft`'s function's binding `replaces` (or none) taken out
/// and `with` put in.
fn replace(
    keymap: &Keymap,
    draft: &Binding,
    replaces: Option<usize>,
    with: Vec<Binding>,
) -> Result<Keymap, String> {
    let replaced = replaces.and_then(|i| binding_rows(keymap, draft.function).get(i).cloned());
    let mut list: Vec<Binding> = keymap
        .of(draft.function, draft.mode)
        .into_iter()
        .cloned()
        .collect();
    if let Some(old) = &replaced {
        list.retain(|b| b != old);
    }
    list.extend(with);
    let mut next = keymap.with(draft.function, draft.mode, list)?;
    // A binding moved to the other mode leaves its old one.
    if let Some(old) = replaced.filter(|old| old.mode != draft.mode) {
        let list = next
            .of(old.function, old.mode)
            .into_iter()
            .filter(|b| **b != old)
            .cloned()
            .collect();
        next = next.with(old.function, old.mode, list)?;
    }
    Ok(next)
}
/// Adds `draft`, or replaces binding `replaces` with it; where its keys
/// clash with another function's, its scope opens to resolve that first.
fn add(app: &mut App, screen: &mut KeysScreen, draft: Binding, replaces: Option<usize>) {
    if draft.reach().is_empty() {
        screen.message = Some((
            format!("{} works nowhere in this scope", draft.function.name()),
            true,
        ));
        return;
    }
    if !clashes(&app.keymap, &draft, replaces).is_empty() {
        // The scope editor names the clashes and how to settle them.
        screen.message = None;
        if let Some(editing) = screen.editing.as_mut() {
            let cursor = match &editing.step {
                EditStep::Scope { cursor, .. } => *cursor,
                _ => 0,
            };
            editing.step = EditStep::Scope {
                draft,
                replaces,
                cursor,
            };
        }
        return;
    }
    let next = replace(&app.keymap, &draft, replaces, vec![draft.clone()]);
    let done = if replaces.is_some() {
        "Changed"
    } else {
        "Added"
    };
    commit(app, screen, next, done);
}
/// Applies `next` to the session and saves it; a refused keymap changes
/// nothing. `true` when applied.
fn commit(
    app: &mut App,
    screen: &mut KeysScreen,
    next: Result<Keymap, String>,
    done: &str,
) -> bool {
    let next = match next {
        Ok(next) => next,
        Err(error) => {
            screen.message = Some((format!("Not saved: {error}"), true));
            return false;
        }
    };
    app.keymap = next;
    screen.message = Some(match (&app.keys_path, &app.keys_broken) {
        (None, _) => (
            format!("{done} for this session; --no-config saves nothing"),
            false,
        ),
        (Some(_), Some(_)) => (
            format!("{done} for this session; keys.toml has errors, so it is left alone"),
            true,
        ),
        (Some(path), None) => match app.keymap.save(path) {
            Ok(()) => (format!("{done}; saved to {}", path.display()), false),
            Err(error) => (
                format!("{done} for this session; could not save: {error}"),
                true,
            ),
        },
    });
    if let Some(editing) = screen.editing.as_mut() {
        editing.step = EditStep::Browse;
        let rows = binding_rows(&app.keymap, editing.function).len();
        editing.index = editing.index.min(rows.saturating_sub(1));
    }
    true
}
