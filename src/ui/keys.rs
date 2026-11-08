//! The Keys screen (F1): a picker like the results, over every function and
//! its keys. Its query is a Neovim field that searches names, descriptions
//! and key names (a query that is exactly a key's name lists what that key
//! does), and its list moves with your own keys. In Normal mode, `r` and a
//! key writes that key's name into the query.
//!
//! Space goes a level deeper everywhere, Enter is done and Escape goes back:
//! Space opens a function's keys, a sheet like the workspace form, and Space
//! on one of its keys opens where that key applies: a checklist for each of
//! the mode, the program, the focus and the highlight. Changes are staged
//! until Enter saves them (refused when they would clash or lock you in).
//! The `show_keys` key closes the screen from anywhere in it, dropping what
//! was not saved.
use super::*;
use crate::keymap::{Binding, Cell, Dimension, Function, Key, Keymap, Mode, Scope};

pub(crate) struct KeysScreen {
    /// Where F1 was pressed.
    pub here: Cell,
    /// Every function, not only those that work where F1 was pressed.
    pub everywhere: bool,
    /// The Keys view: a row per key and place, instead of per function.
    pub keys: bool,
    pub query: String,
    /// `r` was pressed: the next key's name goes into the query.
    pub recording: bool,
    pub index: usize,
    /// How far the details pane is scrolled.
    pub details: usize,
    pub editor: Option<Editor>,
}
/// A row of the picker.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Listed {
    Function(Function),
    /// An entry of [`crate::keymap::FIXED`].
    Fixed(usize),
    /// One place of a key the query names.
    Key(Place),
}
/// Where one key of a function applies: the modes and the box of the
/// other three dimensions, made of a binding per mode. A key that applies
/// in places one box cannot hold has several.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Place {
    pub function: Function,
    pub keys: Vec<Key>,
    pub modes: u16,
    pub scope: Scope,
    pub bindings: Vec<Binding>,
}
impl Place {
    pub fn display(&self) -> String {
        crate::keymap::display(&self.keys)
    }
}
/// The bits ticked in `column`: mode, program, focus or highlight.
pub(crate) fn ticks(modes: u16, scope: Scope, column: usize) -> u16 {
    match column {
        0 => modes,
        1 => scope.programs,
        2 => scope.focuses,
        _ => scope.highlights,
    }
}
/// One function's keys being edited. Changes are staged in `custom` until
/// Enter saves them.
pub(crate) struct Editor {
    pub function: Function,
    pub custom: Vec<((Function, Mode), Vec<Binding>)>,
    /// The highlighted place, by its row.
    pub index: usize,
    pub step: EditStep,
    /// What the last action came to; `true` for an error.
    pub message: Option<(String, bool)>,
}
pub(crate) enum EditStep {
    Browse,
    /// `n`: the keys of a new place, as pressed; `R`: the keys to put in
    /// place of `replaces`'s, where it applied.
    Capture {
        keys: Vec<Key>,
        replaces: Option<Place>,
    },
    /// `D`: whether to put the function's default keys back.
    Defaults,
    /// Where a key applies: a column per dimension, a row per value; the
    /// cursor is on value `row` of column `column`.
    Where {
        keys: Vec<Key>,
        modes: u16,
        scope: Scope,
        /// The place it replaces.
        replaces: Option<Place>,
        column: usize,
        row: usize,
    },
}
/// The columns of where a key applies: each dimension's heading and its
/// values (name, bit).
pub(crate) fn dimensions() -> [(&'static str, Vec<(&'static str, u16)>); 4] {
    fn values<D: Dimension>(name: impl Fn(D) -> &'static str) -> Vec<(&'static str, u16)> {
        D::ALL.iter().map(|&v| (name(v), v.bit())).collect()
    }
    use crate::keymap::{Focus, Highlight};
    [
        (
            "Mode",
            values::<Mode>(|m| match m {
                Mode::Insert => "Insert",
                Mode::Normal => "Normal",
            }),
        ),
        ("Program", values::<crate::program::Program>(|p| p.name())),
        (
            "Focus",
            values::<Focus>(|p| match p {
                Focus::Query => "query",
                Focus::Results => "results",
                Focus::Marks => "marked list",
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
/// `ticks` with `column` set to `bits`.
pub(crate) fn with_column(modes: u16, scope: Scope, column: usize, bits: u16) -> (u16, Scope) {
    let mut scope = scope;
    let modes = match column {
        0 => bits,
        1 => {
            scope.programs = bits;
            modes
        }
        2 => {
            scope.focuses = bits;
            modes
        }
        _ => {
            scope.highlights = bits;
            modes
        }
    };
    (modes, scope)
}
/// Whether value `bit` of `column` can apply anywhere with the others
/// columns' ticks: some mode ticked there takes `keys` and `function` works
/// there. Without keys, only where the function works counts.
pub(crate) fn workable(
    function: Function,
    keys: Option<&[Key]>,
    modes: u16,
    scope: Scope,
    column: usize,
    bit: u16,
) -> bool {
    let (modes, scope) = with_column(modes, scope, column, bit);
    Mode::ALL.into_iter().any(|mode| {
        modes & mode.bit() != 0
            && keys.is_none_or(|keys| crate::keymap::problem(mode, keys).is_none())
            && function
                .available(mode)
                .iter()
                .any(|a| a.meet(scope).is_some())
    })
}
/// The bindings a place's ticks make: one per mode that takes the keys.
pub(crate) fn bindings_of(
    function: Function,
    keys: &[Key],
    modes: u16,
    scope: Scope,
) -> Vec<Binding> {
    Mode::ALL
        .into_iter()
        .filter(|m| modes & m.bit() != 0 && crate::keymap::problem(*m, keys).is_none())
        .map(|mode| Binding {
            function,
            mode,
            keys: keys.to_vec(),
            scope,
        })
        .filter(|b| !b.reach().is_empty())
        .collect()
}
/// A function's keys as places: bindings of the same keys and box in both
/// modes are one place, and a key's places are next to each other.
pub(crate) fn places(keymap: &Keymap, function: Function) -> Vec<Place> {
    let mut places: Vec<Place> = Vec::new();
    for mode in Mode::ALL {
        for b in keymap.of(function, mode) {
            match places
                .iter_mut()
                .find(|p| p.keys == b.keys && p.scope == b.scope)
            {
                Some(place) => {
                    place.modes |= mode.bit();
                    place.bindings.push(b.clone());
                }
                None => places.push(Place {
                    function,
                    keys: b.keys.clone(),
                    modes: mode.bit(),
                    scope: b.scope,
                    bindings: vec![b.clone()],
                }),
            }
        }
    }
    let order: Vec<Vec<Key>> = places.iter().fold(Vec::new(), |mut order, p| {
        if !order.contains(&p.keys) {
            order.push(p.keys.clone());
        }
        order
    });
    places.sort_by_key(|p| order.iter().position(|k| *k == p.keys));
    places
}

impl KeysScreen {
    pub fn new(here: Cell) -> Self {
        Self {
            here,
            everywhere: false,
            keys: false,
            query: String::new(),
            recording: false,
            index: 0,
            details: 0,
            editor: None,
        }
    }
    /// Whether a box reaches where F1 was pressed, whatever is highlighted:
    /// the results count with their query.
    pub fn reaches(&self, scope: Scope) -> bool {
        use crate::keymap::Focus;
        let focuses = match self.here.focus {
            Focus::Marks => Focus::Marks.bit(),
            Focus::Query | Focus::Results => Focus::Query.bit() | Focus::Results.bit(),
        };
        scope.programs & self.here.program.bit() != 0 && scope.focuses & focuses != 0
    }
    pub fn works_here(&self, function: Function) -> bool {
        Mode::ALL
            .into_iter()
            .any(|m| function.available(m).into_iter().any(|s| self.reaches(s)))
    }
    pub fn applies_here(&self, place: &Place) -> bool {
        place
            .bindings
            .iter()
            .any(|b| b.reach().into_iter().any(|s| self.reaches(s)))
    }
    /// The picker's rows: in the Keys view, every key's places matching
    /// the query; otherwise the functions (and fixed keys) matching it.
    pub fn listed(&self, keymap: &Keymap) -> Vec<Listed> {
        let query = self.query.trim();
        if self.keys {
            return self.listed_keys(keymap, query);
        }
        let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
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
                .contains(&query.replace(' ', "_").to_lowercase())
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
impl KeysScreen {
    /// The Keys view's rows: places whose key's name holds the query (an
    /// exact name first, then one starting with it), or whose function's
    /// name does.
    fn listed_keys(&self, keymap: &Keymap, query: &str) -> Vec<Listed> {
        let lower = query.to_lowercase();
        let mut rows: Vec<(u8, usize, Place)> = Function::ALL
            .iter()
            .flat_map(|&f| places(keymap, f))
            .filter(|p| self.everywhere || self.applies_here(p))
            .enumerate()
            .filter_map(|(i, p)| {
                let name = p.display();
                let notation = crate::keymap::notation(&p.keys);
                let rank = if query.is_empty() || name == query || notation == query {
                    0
                } else if name.to_lowercase().starts_with(&lower) {
                    1
                } else if name.to_lowercase().contains(&lower)
                    || notation.to_lowercase().contains(&lower)
                {
                    2
                } else if p.function.name().contains(&lower.replace(' ', "_")) {
                    3
                } else {
                    return None;
                };
                Some((rank, i, p))
            })
            .collect();
        // Ranked, then by key, so a key's places stay together.
        rows.sort_by(|a, b| {
            (a.0, a.2.display().to_lowercase(), a.1).cmp(&(b.0, b.2.display().to_lowercase(), b.1))
        });
        rows.into_iter().map(|(_, _, p)| Listed::Key(p)).collect()
    }
}
impl Editor {
    pub(crate) fn new(keymap: &Keymap, function: Function, index: usize) -> Self {
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
    /// The function's places as edited.
    pub fn rows(&self) -> Vec<Place> {
        places(&self.keymap(), self.function)
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
    /// Stages `with` in place of the bindings `replaces`.
    fn replace(&mut self, replaces: &[Binding], with: Vec<Binding>) {
        for mode in Mode::ALL {
            let keymap = self.keymap();
            let mut list: Vec<Binding> = keymap
                .of(self.function, mode)
                .into_iter()
                .filter(|b| !replaces.contains(b))
                .cloned()
                .collect();
            list.extend(with.iter().filter(|b| b.mode == mode).cloned());
            self.set(self.function, mode, list);
        }
    }
    /// Other functions' keys that `drafts` clash with, and where.
    pub fn clashes(&self, drafts: &[Binding], replaces: &[Binding]) -> Vec<(Binding, Scope)> {
        let keymap = self.keymap();
        let mut found = Vec::new();
        for b in keymap.bindings() {
            if b.function == self.function || replaces.contains(b) {
                continue;
            }
            for draft in drafts {
                if let Some(at) = draft.clashes(b).first() {
                    found.push((b.clone(), *at));
                }
            }
        }
        found
    }
}
/// Functions whose key is a digit family: a digit pressed for them stands
/// for all digits.
fn takes_digits(function: Function) -> bool {
    matches!(
        function,
        Function::SearchToDepth | Function::OpenRow | Function::ConfirmRow
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
        // `r` was pressed: this key's name goes into the query.
        if screen.recording {
            screen.recording = false;
            let name = Key::press(key).display();
            let joined = screen.query.chars().last().is_none_or(char::is_whitespace)
                || (name.chars().count() == 1
                    && screen
                        .query
                        .split_whitespace()
                        .last()
                        .is_some_and(|w| w.chars().count() == 1));
            if !joined {
                screen.query.push(' ');
            }
            screen.query += &name;
            // A key's name is a search for keys.
            screen.keys = true;
            screen.index = 0;
            screen.details = 0;
            return Ok(None);
        }
        let press = Key::press(key);
        let close = Mode::ALL.into_iter().any(|m| {
            app.keymap
                .of(Function::ShowKeys, m)
                .iter()
                .any(|b| b.keys.len() == 1 && b.keys[0] == press)
        });
        // The show_keys key closes the screen from anywhere in it, dropping
        // what was not saved; while keys are recorded, they are recorded.
        let capturing = screen
            .editor
            .as_ref()
            .is_some_and(|e| matches!(e.step, EditStep::Capture { .. }));
        if close && !capturing {
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
        let normal = app.mode() == Mode::Normal && app.pending.is_empty();
        if normal && key.code == K::Char('r') && key.modifiers.is_empty() {
            screen.recording = true;
            return Ok(None);
        }
        if key.code == K::Char('a') && key.modifiers == M::CONTROL {
            screen.everywhere = !screen.everywhere;
            screen.index = 0;
            return Ok(None);
        }
        // Tab switches between the Functions and the Keys view.
        if key.code == K::Tab && key.modifiers.is_empty() {
            screen.keys = !screen.keys;
            screen.index = 0;
            screen.details = 0;
            return Ok(None);
        }
        // Enter, or Space where it is not typed, goes a level deeper.
        let space = key.code == K::Char(' ') && key.modifiers.is_empty() && normal;
        if key.code == K::Enter || space {
            let rows = screen.listed(&app.keymap);
            let (function, row) = match rows.get(screen.index) {
                Some(Listed::Function(f)) => (*f, 0),
                Some(Listed::Key(place)) => (
                    place.function,
                    places(&app.keymap, place.function)
                        .iter()
                        .position(|p| p == place)
                        .unwrap_or(0),
                ),
                _ => return Ok(None),
            };
            screen.editor = Some(Editor::new(&app.keymap, function, row));
            return Ok(None);
        }
        // Leaving Insert mode and Neovim's command line are Neovim's.
        if app.pending.is_empty()
            && let Some(view) = &app.edit_view
            && ((key.code == K::Esc && !view.normal())
                || (view.command() && !key.modifiers.intersects(M::ALT | M::CONTROL)))
        {
            deliver(app, &mut self.focus, &mut self.editor, key);
            return Ok(None);
        }
        // Otherwise your own keys: those that move through a list, scroll
        // the preview, clear a query or quit do so here too.
        let mode = app.mode();
        let cell = Cell {
            highlight: crate::keymap::Highlight::None,
            focus: if mode == Mode::Insert {
                crate::keymap::Focus::Query
            } else {
                crate::keymap::Focus::Results
            },
            ..screen.here
        };
        loop {
            match app.keymap.feed(&mut app.pending, mode, cell, key) {
                Step::Pending => return Ok(None),
                Step::Flush(keys) => {
                    for pending in keys {
                        deliver(app, &mut self.focus, &mut self.editor, pending);
                    }
                }
                Step::Pass => {
                    deliver(app, &mut self.focus, &mut self.editor, key);
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
        let before = screen.index;
        let index = &mut screen.index;
        match function {
            Function::HighlightPrevious => *index = index.saturating_sub(1),
            Function::HighlightNext => *index = (*index + 1).min(last),
            Function::HighlightPreviousPage => *index = index.saturating_sub(page),
            Function::HighlightNextPage => *index = (*index + page).min(last),
            Function::HighlightFirst => *index = 0,
            Function::HighlightLast => *index = last,
            Function::ScrollPreviewDown => screen.details += 5,
            Function::ScrollPreviewUp => screen.details = screen.details.saturating_sub(5),
            Function::ClearQuery => {
                screen.query.clear();
                *index = 0;
            }
            Function::ShowSession => self.focus = Focus::Info { offset: 0 },
            Function::Cancel => return Some(Finish::Cancelled),
            Function::Quit => return self.quit(app, false),
            Function::QuitHere => return self.quit(app, true),
            other => {
                app.status = format!(
                    "{} ({}) does nothing on the Keys screen",
                    Key::press(key).display(),
                    other.name()
                );
            }
        }
        if let Focus::Keys(screen) = &mut self.focus
            && screen.index != before
        {
            screen.details = 0;
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
            // Round the rows, from the last back to the first.
            _ if up || down => editor.index = cycle(editor.index, rows.len(), down),
            // Space goes a level deeper: where the key applies.
            K::Char(' ') => {
                if let Some(place) = rows.get(editor.index) {
                    editor.message = None;
                    editor.step = EditStep::Where {
                        keys: place.keys.clone(),
                        modes: place.modes,
                        scope: place.scope,
                        replaces: Some(place.clone()),
                        column: 0,
                        row: 0,
                    };
                }
            }
            K::Char('n') => {
                editor.message = None;
                editor.step = EditStep::Capture {
                    keys: Vec::new(),
                    replaces: None,
                };
            }
            K::Char('R') => {
                if let Some(place) = rows.get(editor.index) {
                    editor.message = None;
                    editor.step = EditStep::Capture {
                        keys: Vec::new(),
                        replaces: Some(place.clone()),
                    };
                }
            }
            K::Char('d') => {
                if let Some(place) = rows.get(editor.index).cloned() {
                    editor.replace(&place.bindings, Vec::new());
                    editor.index = editor.index.min(editor.rows().len().saturating_sub(1));
                    editor.message =
                        Some((format!("{} removed; Enter saves", place.display()), false));
                }
            }
            K::Char('D') => {
                if editor.custom.iter().any(|((f, _), _)| *f == function) {
                    editor.message = None;
                    editor.step = EditStep::Defaults;
                } else {
                    editor.message = Some((
                        format!("{} already has its default keys", function.name()),
                        false,
                    ));
                }
            }
            _ => {}
        },
        EditStep::Defaults => match key.code {
            K::Enter | K::Char('y') => {
                editor.custom.retain(|((f, _), _)| *f != function);
                editor.index = 0;
                editor.step = EditStep::Browse;
                editor.message = Some(("Default keys back; Enter saves".into(), false));
            }
            K::Esc | K::Char('n') => editor.step = EditStep::Browse,
            _ => {}
        },
        // The first key may be any key; after it, Enter goes on to where
        // the key applies, Backspace takes the last back and Escape gives up.
        EditStep::Capture { keys, replaces } => {
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
                // Saved, it would read back as other keys, or none.
                K::Enter if !crate::keymap::writable(keys) => {
                    editor.message = Some((
                        format!(
                            "{} cannot be written to keys.toml",
                            crate::keymap::display(keys)
                        ),
                        true,
                    ));
                    editor.step = EditStep::Browse;
                }
                K::Enter if replaces.is_some() => {
                    let keys = std::mem::take(keys);
                    let place = replaces.take().unwrap();
                    editor.step = EditStep::Browse;
                    replace_keys(editor, place, keys);
                }
                K::Enter => {
                    let keys = std::mem::take(keys);
                    // Every mode that takes the keys, everywhere.
                    let modes = Mode::ALL
                        .into_iter()
                        .filter(|m| {
                            crate::keymap::problem(*m, &keys).is_none()
                                && !function.available(*m).is_empty()
                        })
                        .fold(0, |bits, m| bits | m.bit());
                    if modes == 0 {
                        editor.message = Some((
                            format!(
                                "{} cannot be bound for {} in either mode",
                                crate::keymap::display(&keys),
                                function.name()
                            ),
                            true,
                        ));
                        editor.step = EditStep::Browse;
                        return false;
                    }
                    editor.step = EditStep::Where {
                        keys,
                        modes,
                        scope: Scope::ALL,
                        replaces: None,
                        column: 0,
                        row: 0,
                    };
                }
                _ if matches!(keys[0], Key::Digits(_)) => {
                    editor.message =
                        Some(("A digit stands for every digit, on its own".into(), true));
                }
                _ => keys.push(Key::press(key)),
            }
        }
        EditStep::Where {
            keys,
            modes,
            scope,
            replaces,
            column,
            row,
        } => {
            let dims = dimensions();
            let left = key.code == K::Left || (plain && key.code == K::Char('h'));
            let right = key.code == K::Right || (plain && key.code == K::Char('l'));
            // Tab and Shift+Tab go round the columns.
            if matches!(key.code, K::Tab | K::BackTab) {
                let count = dims.len();
                *column = if key.code == K::Tab {
                    (*column + 1) % count
                } else {
                    (*column + count - 1) % count
                };
                *row = (*row).min(dims[*column].1.len() - 1);
                return false;
            }
            let replaced = replaces
                .as_ref()
                .map(|p| p.bindings.clone())
                .unwrap_or_default();
            let drafts = bindings_of(function, keys, *modes, *scope);
            match key.code {
                K::Esc => editor.step = EditStep::Browse,
                _ if up => *row = row.saturating_sub(1),
                _ if down => *row = (*row + 1).min(dims[*column].1.len() - 1),
                _ if left => {
                    *column = column.saturating_sub(1);
                    *row = (*row).min(dims[*column].1.len() - 1);
                }
                _ if right => {
                    *column = (*column + 1).min(dims.len() - 1);
                    *row = (*row).min(dims[*column].1.len() - 1);
                }
                K::Char(' ') => {
                    let bit = dims[*column].1[*row].1;
                    let set = ticks(*modes, *scope, *column);
                    // A column keeps at least one tick.
                    if set & !bit != 0 || set & bit == 0 {
                        (*modes, *scope) = with_column(*modes, *scope, *column, set ^ bit);
                    }
                }
                K::Enter => {
                    let shown = crate::keymap::display(keys);
                    if drafts.is_empty() {
                        editor.message =
                            Some((format!("{shown} applies nowhere with these ticks"), true));
                    } else if editor.clashes(&drafts, &replaced).is_empty() {
                        editor.replace(&replaced, drafts);
                        editor.step = EditStep::Browse;
                        editor.message = Some((format!("{shown} staged; Enter saves"), false));
                    } else {
                        editor.message = Some(("It clashes: untick, or press u or t".into(), true));
                    }
                }
                // Keep it only where nothing else has these keys.
                K::Char('u') => {
                    let shown = crate::keymap::display(keys);
                    let mut kept = Vec::new();
                    for draft in drafts.iter() {
                        let mut pieces = vec![draft.scope];
                        for (other, _) in editor.clashes(std::slice::from_ref(draft), &replaced) {
                            for taken in other.reach() {
                                pieces = pieces.into_iter().flat_map(|p| p.minus(taken)).collect();
                            }
                        }
                        kept.extend(
                            pieces
                                .into_iter()
                                .map(|scope| Binding {
                                    scope,
                                    ..draft.clone()
                                })
                                .filter(|b| !b.reach().is_empty()),
                        );
                    }
                    if kept.is_empty() {
                        editor.message = Some((
                            format!("Nowhere is left for {shown}: change it, or take it"),
                            true,
                        ));
                    } else {
                        editor.replace(&replaced, kept);
                        editor.step = EditStep::Browse;
                        editor.message = Some((
                            format!("{shown} kept only where nothing else has it; Enter saves"),
                            false,
                        ));
                    }
                }
                // Take these keys from the others where this applies.
                K::Char('t') => {
                    let shown = crate::keymap::display(keys);
                    let clashes = editor.clashes(&drafts, &replaced);
                    editor.replace(&replaced, drafts.clone());
                    for (other, _) in clashes {
                        let mut pieces = vec![other.scope];
                        for draft in &drafts {
                            for taken in draft.reach() {
                                pieces = pieces.into_iter().flat_map(|p| p.minus(taken)).collect();
                            }
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
                        format!("{shown} taken from the others where it applies; Enter saves"),
                        false,
                    ));
                }
                _ => {}
            }
        }
    }
    false
}
/// `R`: `keys` in place of `place`'s, where it applied. Where they clash,
/// where they apply opens to settle it; elsewhere they are staged at once.
fn replace_keys(editor: &mut Editor, place: Place, keys: Vec<Key>) {
    let function = editor.function;
    let (old, new) = (place.display(), crate::keymap::display(&keys));
    if keys == place.keys {
        editor.message = Some((format!("{new} is the key already"), false));
        return;
    }
    let drafts = bindings_of(function, &keys, place.modes, place.scope);
    let modes = drafts.iter().fold(0, |bits, b| bits | b.mode.bit());
    if drafts.is_empty() {
        editor.message = Some((format!("{new} cannot be bound where {old} applies"), true));
        return;
    }
    if !editor.clashes(&drafts, &place.bindings).is_empty() {
        editor.message = Some((format!("{new} clashes where {old} applied"), true));
        editor.step = EditStep::Where {
            keys,
            modes,
            scope: place.scope,
            replaces: Some(place),
            column: 0,
            row: 0,
        };
        return;
    }
    editor.replace(&place.bindings, drafts);
    // The highlight follows the key to its row.
    editor.index = editor
        .rows()
        .iter()
        .position(|p| p.keys == keys)
        .unwrap_or(editor.index);
    let lost = place.modes & !modes;
    let only = Mode::ALL
        .into_iter()
        .filter(|m| lost & m.bit() != 0)
        .map(|m| format!("; {new} cannot be bound in {} mode", m.title()))
        .collect::<String>();
    editor.message = Some((format!("{old} replaced by {new}{only}; Enter saves"), false));
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
