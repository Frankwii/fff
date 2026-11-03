//! The Keys screen (F1): a picker like the results, over every function and
//! its keys. Its query is a Neovim field and its list moves with your own
//! keys (`up`, `down`, `first`, `clear_query`...); Tab searches by pressing
//! keys instead. Enter opens a function's editor, a sheet like the bookmark
//! form, where changes are staged: Enter saves them (refused when they would
//! clash or lock you in), Escape drops them. The `guide` key closes the
//! screen from anywhere in it, dropping what was not saved.
use super::*;
use crate::keymap::{Binding, Cell, Dimension, Function, Key, Keymap, Mode, Scope};

/// How the picker searches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Search {
    /// The query filters names, descriptions and key names.
    Text,
    /// Keys pressed are recorded, not typed.
    Capturing(Vec<KeyEvent>),
    /// Recording stopped: the keys filter, the list moves.
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
    pub editor: Option<Editor>,
}
/// A row of the picker.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Listed {
    Function(Function),
    /// An entry of [`crate::keymap::FIXED`].
    Fixed(usize),
    /// A key search's match.
    Binding(Binding),
}
/// One function's keys being edited. Changes are staged in `custom` until
/// Enter saves them.
pub(crate) struct Editor {
    pub function: Function,
    pub custom: Vec<((Function, Mode), Vec<Binding>)>,
    /// The highlighted key, by its row.
    pub index: usize,
    pub step: EditStep,
    /// What the last action came to; `true` for an error.
    pub message: Option<(String, bool)>,
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
    /// Where a key applies: a row per dimension, a column per value.
    Scope {
        draft: Binding,
        /// The binding it replaces.
        replaces: Option<Binding>,
        row: usize,
        column: usize,
    },
}
/// The scope editor's rows: each dimension's label and values (name, bit).
pub(crate) fn dimensions() -> [(&'static str, Vec<(&'static str, u16)>); 3] {
    fn values<D: Dimension>(name: impl Fn(D) -> &'static str) -> Vec<(&'static str, u16)> {
        D::ALL.iter().map(|&v| (name(v), v.bit())).collect()
    }
    use crate::keymap::{Highlight, Place};
    [
        ("Program", values::<crate::keymap::Program>(|p| p.name())),
        (
            "List",
            values::<Place>(|p| match p {
                Place::Results => "results",
                Place::Marks => "marked list",
            }),
        ),
        (
            "Highlight",
            values::<Highlight>(|h| match h {
                Highlight::Dir => "a directory",
                Highlight::File => "a file",
                Highlight::None => "nothing",
            }),
        ),
    ]
}
/// The bits of dimension `row` in `scope`.
pub(crate) fn bits(scope: &mut Scope, row: usize) -> &mut u16 {
    match row {
        0 => &mut scope.programs,
        1 => &mut scope.places,
        _ => &mut scope.highlights,
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
            editor: None,
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
    /// The picker's rows: functions (and fixed keys) matching the query, or
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
            // Those that apply here first.
            found.sort_by_key(|b| (!self.applies_here(b), b.mode, b.display(), b.function));
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
            // Names first, then descriptions and keys holding every word.
            let name = f
                .name()
                .contains(&self.query.trim().replace(' ', "_").to_lowercase())
                .then_some(100_000 - i as i64);
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
        // Fixed keys, when searched for or with everything shown.
        if self.everywhere || !words.is_empty() {
            for (i, (keys, place, what)) in crate::keymap::FIXED.iter().enumerate() {
                let text = format!("{keys} {place} {what}").to_lowercase();
                if words.iter().all(|w| text.contains(w.as_str())) {
                    rows.push((-10_000 - i as i64, Listed::Fixed(i)));
                }
            }
        }
        rows.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
        rows.into_iter().map(|(_, row)| row).collect()
    }
}
impl Editor {
    fn new(keymap: &Keymap, function: Function, index: usize) -> Self {
        Self {
            function,
            custom: keymap.custom().to_vec(),
            index,
            step: EditStep::Browse,
            message: None,
        }
    }
    /// The keymap as edited so far, unchecked.
    pub fn keymap(&self) -> Keymap {
        Keymap::assemble(self.custom.clone())
    }
    /// The function's keys as edited, Insert mode first.
    pub fn rows(&self) -> Vec<Binding> {
        binding_rows(&self.keymap(), self.function)
    }
    /// Whether anything changed since the editor opened.
    pub fn changed(&self, keymap: &Keymap) -> bool {
        let mut a = self.custom.clone();
        let mut b = keymap.custom().to_vec();
        a.sort_by_key(|(at, _)| *at);
        b.sort_by_key(|(at, _)| *at);
        a != b
    }
    /// Stages `function`'s keys in `mode` as `bindings`.
    fn set(&mut self, function: Function, mode: Mode, bindings: Vec<Binding>) {
        let mut unique: Vec<Binding> = Vec::new();
        for b in bindings {
            if !unique.contains(&b) {
                unique.push(b);
            }
        }
        self.custom.retain(|(at, _)| *at != (function, mode));
        if unique != function.defaults(mode) {
            self.custom.push(((function, mode), unique));
        }
    }
    /// Stages `with` in place of `replaces` (or beside the others).
    fn replace(&mut self, replaces: Option<&Binding>, with: Vec<Binding>, mode: Mode) {
        if let Some(old) = replaces {
            let rest = self
                .keymap()
                .of(old.function, old.mode)
                .into_iter()
                .filter(|b| *b != old)
                .cloned()
                .collect();
            self.set(old.function, old.mode, rest);
        }
        let mut list: Vec<Binding> = self
            .keymap()
            .of(self.function, mode)
            .into_iter()
            .cloned()
            .collect();
        list.extend(with);
        self.set(self.function, mode, list);
    }
    /// Other functions' keys that `draft` clashes with, and where.
    pub fn clashes(&self, draft: &Binding, replaces: Option<&Binding>) -> Vec<(Binding, Scope)> {
        self.keymap()
            .bindings()
            .iter()
            .filter(|b| b.function != draft.function && Some(*b) != replaces)
            .filter_map(|b| draft.clashes(b).first().map(|at| (b.clone(), *at)))
            .collect()
    }
}
/// A function's keys, Insert mode first.
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
    /// Opens the Keys screen, ready to type.
    pub(super) fn open_keys(&mut self, here: Cell) {
        self.editor.reset_insert(Field::Keys);
        self.focus = Focus::Keys(Box::new(KeysScreen::new(here)));
    }
    /// A key on the Keys screen.
    pub(super) fn keys_key(&mut self, app: &mut App, key: KeyEvent) -> io::Result<Option<Finish>> {
        let Focus::Keys(screen) = &mut self.focus else {
            return Ok(None);
        };
        let press = Key::press(key);
        let guide = Mode::ALL.into_iter().any(|m| {
            app.keymap
                .of(Function::Guide, m)
                .iter()
                .any(|b| b.keys.len() == 1 && b.keys[0] == press)
        });
        // The guide key closes the screen from anywhere in it, dropping
        // what was not saved; while keys are recorded, they are recorded.
        let recording = matches!(screen.search, Search::Capturing(_))
            || screen
                .editor
                .as_ref()
                .is_some_and(|e| matches!(e.step, EditStep::Capture { .. }));
        if guide && !recording {
            let dropped = screen
                .editor
                .as_ref()
                .is_some_and(|e| e.changed(&app.keymap));
            self.focus = Focus::Results;
            if dropped {
                app.status = "Changes to keys not saved".into();
            }
            return Ok(None);
        }
        if let Some(editor) = screen.editor.as_mut() {
            if editor_key(app, editor, key) {
                screen.editor = None;
            }
            return Ok(None);
        }
        match &mut screen.search {
            Search::Capturing(keys) => {
                match key.code {
                    K::Tab => screen.search = Search::Captured(std::mem::take(keys)),
                    K::Esc => screen.search = Search::Text,
                    K::Backspace => {
                        keys.pop();
                    }
                    _ => keys.push(key),
                }
                screen.index = 0;
                return Ok(None);
            }
            Search::Captured(_) if matches!(key.code, K::Tab | K::Esc) => {
                screen.search = Search::Text;
                screen.index = 0;
                return Ok(None);
            }
            Search::Text if key.code == K::Tab => {
                screen.search = Search::Capturing(Vec::new());
                screen.index = 0;
                return Ok(None);
            }
            _ => {}
        }
        if key.code == K::Char('a') && key.modifiers == M::CONTROL {
            screen.everywhere = !screen.everywhere;
            screen.index = 0;
            return Ok(None);
        }
        if key.code == K::Enter {
            let rows = screen.listed(&app.keymap);
            let (function, row) = match rows.get(screen.index) {
                Some(Listed::Function(f)) => (*f, 0),
                Some(Listed::Binding(b)) => (
                    b.function,
                    binding_rows(&app.keymap, b.function)
                        .iter()
                        .position(|r| r == b)
                        .unwrap_or(0),
                ),
                _ => return Ok(None),
            };
            screen.editor = Some(Editor::new(&app.keymap, function, row));
            return Ok(None);
        }
        // Leaving Insert mode and Neovim's command line are Neovim's.
        let typing = matches!(screen.search, Search::Text);
        if typing
            && app.pending.is_empty()
            && let Some(view) = &app.edit_view
            && ((key.code == K::Esc && !view.normal())
                || (view.command() && !key.modifiers.intersects(M::ALT | M::CONTROL)))
        {
            deliver(app, &mut self.focus, &mut self.editor, key);
            return Ok(None);
        }
        // Otherwise your own keys: those that move through a list, clear a
        // query or leave do so here too.
        let mode = app.mode();
        let cell = Cell {
            highlight: crate::keymap::Highlight::None,
            place: crate::keymap::Place::Results,
            ..screen.here
        };
        loop {
            match app.keymap.feed(&mut app.pending, mode, cell, key) {
                Step::Pending => return Ok(None),
                Step::Flush(keys) => {
                    if typing {
                        for pending in keys {
                            deliver(app, &mut self.focus, &mut self.editor, pending);
                        }
                    }
                }
                Step::Pass => {
                    if typing {
                        deliver(app, &mut self.focus, &mut self.editor, key);
                    }
                    return Ok(None);
                }
                Step::Run(function, _) => return Ok(self.keys_function(app, function, key)),
            }
        }
    }
    /// One of your functions pressed on the Keys screen.
    fn keys_function(
        &mut self,
        app: &mut App,
        function: Function,
        key: KeyEvent,
    ) -> Option<Finish> {
        let page = self.drawn.page.max(1);
        let Focus::Keys(screen) = &mut self.focus else {
            return None;
        };
        let last = screen.listed(&app.keymap).len().saturating_sub(1);
        let index = &mut screen.index;
        match function {
            Function::Up => *index = index.saturating_sub(1),
            Function::Down => *index = (*index + 1).min(last),
            Function::PageUp => *index = index.saturating_sub(page),
            Function::PageDown => *index = (*index + page).min(last),
            Function::First => *index = 0,
            Function::Last => *index = last,
            Function::ClearQuery => {
                screen.query.clear();
                screen.search = Search::Text;
                *index = 0;
            }
            Function::Session => self.focus = Focus::Info { offset: 0 },
            Function::Cancel => return Some(Finish::Cancelled),
            Function::Quit => return self.quit(app, false),
            Function::QuitCurrent => return self.quit(app, true),
            other => {
                app.status = format!(
                    "{} ({}) does nothing on the Keys screen",
                    Key::press(key).display(),
                    other.name()
                );
            }
        }
        None
    }
}
/// A key in a function's editor; `true` closes it.
fn editor_key(app: &mut App, editor: &mut Editor, key: KeyEvent) -> bool {
    let function = editor.function;
    let rows = editor.rows();
    let plain = key.modifiers.is_empty() || key.modifiers == M::ALT;
    let down = key.code == K::Down || (plain && key.code == K::Char('j'));
    let up = key.code == K::Up || (plain && key.code == K::Char('k'));
    match &mut editor.step {
        EditStep::Browse => match key.code {
            K::Esc => {
                if editor.changed(&app.keymap) {
                    app.status = "Changes to keys not saved".into();
                }
                return true;
            }
            K::Enter => return !editor.changed(&app.keymap) || save(app, editor),
            _ if up => editor.index = editor.index.saturating_sub(1),
            _ if down => editor.index = (editor.index + 1).min(rows.len().saturating_sub(1)),
            K::Char('a') => {
                editor.message = None;
                let modes: Vec<Mode> = Mode::ALL
                    .into_iter()
                    .filter(|m| !function.available(*m).is_empty())
                    .collect();
                editor.step = match modes[..] {
                    [mode] => EditStep::Capture {
                        mode,
                        keys: Vec::new(),
                    },
                    _ => EditStep::ChooseMode,
                };
            }
            K::Char('s') => {
                if let Some(binding) = rows.get(editor.index) {
                    editor.message = None;
                    editor.step = EditStep::Scope {
                        draft: binding.clone(),
                        replaces: Some(binding.clone()),
                        row: 0,
                        column: 0,
                    };
                }
            }
            K::Char('d') => {
                if let Some(binding) = rows.get(editor.index).cloned() {
                    editor.replace(Some(&binding), Vec::new(), binding.mode);
                    editor.index = editor.index.min(editor.rows().len().saturating_sub(1));
                    editor.message =
                        Some((format!("{} removed; Enter saves", binding.display()), false));
                }
            }
            K::Char('R') => {
                editor.custom.retain(|((f, _), _)| *f != function);
                editor.index = 0;
                editor.message = Some(("Default keys back; Enter saves".into(), false));
            }
            _ => {}
        },
        EditStep::ChooseMode => match key.code {
            K::Char('i') => {
                editor.step = EditStep::Capture {
                    mode: Mode::Insert,
                    keys: Vec::new(),
                }
            }
            K::Char('n') => {
                editor.step = EditStep::Capture {
                    mode: Mode::Normal,
                    keys: Vec::new(),
                }
            }
            K::Esc => editor.step = EditStep::Browse,
            _ => {}
        },
        // The first key may be any key; after it, Enter finishes,
        // Backspace takes the last back and Escape gives up.
        EditStep::Capture { mode, keys } => {
            if keys.is_empty() {
                let mut pressed = Key::press(key);
                if takes_digits(function)
                    && let Key::Press(K::Char(c), m) = pressed
                    && c.is_ascii_digit()
                {
                    pressed = Key::Digits(m);
                }
                keys.push(pressed);
                return false;
            }
            match key.code {
                K::Esc => editor.step = EditStep::Browse,
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
                    stage(editor, draft, None);
                }
                _ if matches!(keys[0], Key::Digits(_)) => {
                    editor.message =
                        Some(("A digit stands for every digit, on its own".into(), true));
                }
                _ => keys.push(Key::press(key)),
            }
        }
        EditStep::Scope {
            draft,
            replaces,
            row,
            column,
        } => {
            let dims = dimensions();
            let left = key.code == K::Left || (plain && key.code == K::Char('h'));
            let right = key.code == K::Right || (plain && key.code == K::Char('l'));
            match key.code {
                K::Esc => editor.step = EditStep::Browse,
                _ if up => {
                    *row = row.saturating_sub(1);
                    *column = (*column).min(dims[*row].1.len() - 1);
                }
                _ if down => {
                    *row = (*row + 1).min(dims.len() - 1);
                    *column = (*column).min(dims[*row].1.len() - 1);
                }
                _ if left => *column = column.saturating_sub(1),
                _ if right => *column = (*column + 1).min(dims[*row].1.len() - 1),
                K::Char(' ') => {
                    let bit = dims[*row].1[*column].1;
                    let set = bits(&mut draft.scope, *row);
                    // A dimension keeps at least one value.
                    if *set & !bit != 0 || *set & bit == 0 {
                        *set ^= bit;
                    }
                }
                K::Enter => {
                    let (draft, replaces) = (draft.clone(), replaces.clone());
                    stage(editor, draft, replaces);
                }
                // Narrow this key to where nothing else has it.
                K::Char('u') => {
                    let (draft, replaces) = (draft.clone(), replaces.clone());
                    narrow(editor, draft, replaces);
                }
                // Take this key from the others where it applies.
                K::Char('t') => {
                    let (draft, replaces) = (draft.clone(), replaces.clone());
                    take(editor, draft, replaces);
                }
                _ => {}
            }
        }
    }
    false
}
/// Stages `draft` in place of `replaces`; where its keys clash with
/// another function's, the scope editor opens to settle that first.
fn stage(editor: &mut Editor, draft: Binding, replaces: Option<Binding>) {
    if draft.reach().is_empty() {
        editor.message = Some((
            format!("{} works nowhere in this scope", draft.function.name()),
            true,
        ));
        return;
    }
    if !editor.clashes(&draft, replaces.as_ref()).is_empty() {
        editor.message = None;
        let (row, column) = match &editor.step {
            EditStep::Scope { row, column, .. } => (*row, *column),
            _ => (0, 0),
        };
        editor.step = EditStep::Scope {
            draft,
            replaces,
            row,
            column,
        };
        return;
    }
    let shown = draft.display();
    let mode = draft.mode;
    let added = replaces.is_none();
    editor.replace(replaces.as_ref(), vec![draft], mode);
    editor.step = EditStep::Browse;
    editor.message = Some((
        format!(
            "{shown} {}; Enter saves",
            if added { "added" } else { "changed" }
        ),
        false,
    ));
}
/// Stages `draft` only where no other function has its keys.
fn narrow(editor: &mut Editor, draft: Binding, replaces: Option<Binding>) {
    let mut pieces = vec![draft.scope];
    for (other, _) in editor.clashes(&draft, replaces.as_ref()) {
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
        editor.message = Some((
            "Nowhere is left for this key: change it, or take it".into(),
            true,
        ));
        return;
    }
    editor.replace(replaces.as_ref(), narrowed, draft.mode);
    editor.step = EditStep::Browse;
    editor.message = Some((
        format!(
            "{} only where nothing else has it; Enter saves",
            draft.display()
        ),
        false,
    ));
}
/// Stages `draft`, taking its keys from the others where it applies.
fn take(editor: &mut Editor, draft: Binding, replaces: Option<Binding>) {
    let clashes = editor.clashes(&draft, replaces.as_ref());
    editor.replace(replaces.as_ref(), vec![draft.clone()], draft.mode);
    for (other, _) in clashes {
        let mut pieces = vec![other.scope];
        for taken in draft.reach() {
            pieces = pieces.into_iter().flat_map(|p| p.minus(taken)).collect();
        }
        let mut list: Vec<Binding> = editor
            .keymap()
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
        editor.set(other.function, other.mode, list);
    }
    editor.step = EditStep::Browse;
    editor.message = Some((
        format!(
            "{} taken from the others where it applies; Enter saves",
            draft.display()
        ),
        false,
    ));
}
/// Saves the staged keys: applied to the session and written to
/// `keys.toml`. A keymap that clashes or locks you in is refused and the
/// editor stays open. `true` closes it.
fn save(app: &mut App, editor: &mut Editor) -> bool {
    let next = match Keymap::new(editor.custom.clone()) {
        Ok(next) => next,
        Err(error) => {
            editor.message = Some((format!("Not saved: {error}"), true));
            return false;
        }
    };
    app.keymap = next;
    let (status, ok) = match (&app.keys_path, &app.keys_broken) {
        (None, _) => (
            "Keys saved for this session; --no-config writes nothing".to_string(),
            true,
        ),
        (Some(_), Some(_)) => (
            "Keys saved for this session; keys.toml has errors, so it is left alone".into(),
            false,
        ),
        (Some(path), None) => match app.keymap.save(path) {
            Ok(()) => (
                format!("Keys saved to {}", tilde_or_path(path, app.home.as_deref())),
                true,
            ),
            Err(error) => (
                format!("Keys saved for this session; could not write: {error}"),
                false,
            ),
        },
    };
    app.status = status;
    if ok {
        app.status_success = app.status.clone();
    }
    true
}
fn tilde_or_path(path: &std::path::Path, home: Option<&std::path::Path>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}
