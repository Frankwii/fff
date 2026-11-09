//! Key bindings. Everything a key can do is a [`Function`]: what it does,
//! where it works (fixed by design) and its default keys. A [`Binding`] puts
//! a key sequence on a function in Insert or Normal mode, within a [`Scope`]
//! of where it applies. Places are the cells of a product: the program (the
//! command fff runs as), what has the focus (the query, the results or the
//! marked list) and what is highlighted. Two bindings of one mode share
//! keys only where their scopes do not meet, so a key does exactly one
//! thing wherever it is pressed. A key goes to Neovim only where no
//! binding has it; the default keys typed as text leave the query in focus
//! to Neovim, and your own apply there unless their focus says otherwise.
//!
//! `keys.toml` replaces a function's bindings in a mode; the Keys screen (F1)
//! edits them. A keymap that would lock you in (nothing opens the Keys screen
//! or leaves fff somewhere) is refused.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as M};
use std::path::Path;

/// Insert mode types into the query; Normal mode is for commands, and for
/// editing the query as Neovim does where the query has the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mode {
    Insert,
    Normal,
}
impl Mode {
    pub const ALL: [Self; 2] = [Self::Insert, Self::Normal];
    /// The table in `keys.toml`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Normal => "normal",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Self::Insert => "Insert",
            Self::Normal => "Normal",
        }
    }
    pub fn other(self) -> Self {
        match self {
            Self::Insert => Self::Normal,
            Self::Normal => Self::Insert,
        }
    }
}

use crate::program::Program;
/// What keys act on: the query (always, in Insert mode), the results below
/// it, or the marked list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    Query,
    Results,
    Marks,
}
/// What the highlighted row is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Highlight {
    Dir,
    File,
    None,
}
/// The values of one dimension of the product, named as `keys.toml` writes
/// them.
pub trait Dimension: Copy + PartialEq + 'static {
    const ALL: &'static [Self];
    fn name(self) -> &'static str;
    fn bit(self) -> u16 {
        1 << Self::ALL.iter().position(|v| *v == self).unwrap()
    }
    fn every() -> u16 {
        (1 << Self::ALL.len()) - 1
    }
    fn named(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|v| v.name() == name)
    }
}
impl Dimension for Program {
    const ALL: &'static [Self] = &[
        Self::Roam,
        Self::Window,
        Self::Pick,
        Self::Seek,
        Self::ExplorerOpen,
        Self::ExplorerMultiple,
        Self::ExplorerFolder,
        Self::ExplorerFolderMultiple,
        Self::ExplorerSave,
        Self::Piped,
    ];
    fn name(self) -> &'static str {
        match self {
            Self::Roam => "roam",
            Self::Window => "window",
            Self::Pick => "pick",
            Self::Seek => "seek",
            Self::ExplorerOpen => "explorer-open",
            Self::ExplorerMultiple => "explorer-open-multiple",
            Self::ExplorerFolder => "explorer-folder",
            Self::ExplorerFolderMultiple => "explorer-folder-multiple",
            Self::ExplorerSave => "explorer-save",
            Self::Piped => "piped",
        }
    }
}
impl Dimension for Focus {
    const ALL: &'static [Self] = &[Self::Query, Self::Results, Self::Marks];
    fn name(self) -> &'static str {
        match self {
            Self::Query => "query",
            Self::Results => "results",
            Self::Marks => "marks",
        }
    }
}
impl Dimension for Mode {
    const ALL: &'static [Self] = &[Self::Insert, Self::Normal];
    fn name(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Normal => "normal",
        }
    }
}
impl Dimension for Highlight {
    const ALL: &'static [Self] = &[Self::Dir, Self::File, Self::None];
    fn name(self) -> &'static str {
        match self {
            Self::Dir => "dir",
            Self::File => "file",
            Self::None => "none",
        }
    }
}

/// One place a key can be pressed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub program: Program,
    pub focus: Focus,
    pub highlight: Highlight,
}
impl Cell {
    pub fn describe(self) -> String {
        format!(
            "{} · {} · {}",
            self.program.name(),
            self.focus.name(),
            self.highlight.name()
        )
    }
}

/// A box in the product: a set of values in each dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub programs: u16,
    pub focuses: u16,
    pub highlights: u16,
}
impl Scope {
    pub const ALL: Self = Self {
        programs: (1 << <Program as Dimension>::ALL.len()) - 1,
        focuses: 0b111,
        highlights: 0b111,
    };
    /// The box of these values; an empty list is the whole dimension.
    pub fn of(programs: &[Program], focuses: &[Focus], highlights: &[Highlight]) -> Self {
        fn bits<D: Dimension>(values: &[D]) -> u16 {
            if values.is_empty() {
                D::every()
            } else {
                values.iter().fold(0, |bits, v| bits | v.bit())
            }
        }
        Self {
            programs: bits(programs),
            focuses: bits(focuses),
            highlights: bits(highlights),
        }
    }
    pub fn contains(self, cell: Cell) -> bool {
        self.programs & cell.program.bit() != 0
            && self.focuses & cell.focus.bit() != 0
            && self.highlights & cell.highlight.bit() != 0
    }
    pub fn is_empty(self) -> bool {
        self.programs == 0 || self.focuses == 0 || self.highlights == 0
    }
    /// Where both apply, if anywhere.
    pub fn meet(self, other: Self) -> Option<Self> {
        let meet = Self {
            programs: self.programs & other.programs,
            focuses: self.focuses & other.focuses,
            highlights: self.highlights & other.highlights,
        };
        (!meet.is_empty()).then_some(meet)
    }
    /// What is left of this box without `other`, as disjoint boxes.
    pub fn minus(self, other: Self) -> Vec<Self> {
        let Some(meet) = self.meet(other) else {
            return vec![self];
        };
        [
            Self {
                programs: self.programs & !meet.programs,
                ..self
            },
            Self {
                programs: meet.programs,
                focuses: self.focuses & !meet.focuses,
                ..self
            },
            Self {
                programs: meet.programs,
                focuses: meet.focuses,
                highlights: self.highlights & !meet.highlights,
            },
        ]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect()
    }
    /// The values set in dimension `D`.
    pub fn values<D: Dimension>(bits: u16) -> impl Iterator<Item = D> {
        D::ALL.iter().copied().filter(move |v| bits & v.bit() != 0)
    }
    /// `"window, explorer-* · dir"`; dimensions left whole are not named,
    /// so the whole product is `""`.
    pub fn describe(self) -> String {
        fn part<D: Dimension>(bits: u16) -> Option<String> {
            if bits == D::every() {
                return None;
            }
            let mut names: Vec<&str> = Scope::values::<D>(bits).map(D::name).collect();
            // The five explorer programs, named once.
            let explorers = names.iter().filter(|n| n.starts_with("explorer-")).count();
            if explorers == EXPLORERS.len() {
                names.retain(|n| !n.starts_with("explorer-"));
                names.push("explorer-*");
            }
            Some(names.join(", "))
        }
        [
            part::<Program>(self.programs),
            part::<Focus>(self.focuses),
            part::<Highlight>(self.highlights),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    }
}

/// One key of a sequence: a press, or a digit family (`<C-#>`: Ctrl with any
/// digit, `<#>`: 1–9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Press(KeyCode, M),
    Digits(M),
}
impl Key {
    /// A press as bindings compare it. Terminals disagree about Shift with
    /// characters (the kitty protocol sends Shift+g as `g` with SHIFT,
    /// others `G`): a letter says it by its case, other characters drop it,
    /// and Space keeps it (Shift+Space is a key of its own).
    pub fn press(event: KeyEvent) -> Self {
        let (code, mut modifiers) = (
            event.code,
            event.modifiers & (M::SHIFT | M::CONTROL | M::ALT),
        );
        let code = match code {
            KeyCode::Char(c) if c.is_alphabetic() => {
                if modifiers.contains(M::SHIFT) {
                    modifiers -= M::SHIFT;
                    KeyCode::Char(c.to_uppercase().next().unwrap_or(c))
                } else {
                    KeyCode::Char(c)
                }
            }
            KeyCode::Char(' ') => KeyCode::Char(' '),
            KeyCode::Char(c) => {
                modifiers -= M::SHIFT;
                KeyCode::Char(c)
            }
            // Shift+Tab as a legacy terminal sends it, however reported.
            KeyCode::BackTab => {
                modifiers -= M::SHIFT;
                KeyCode::BackTab
            }
            KeyCode::Tab if modifiers.contains(M::SHIFT) => {
                modifiers -= M::SHIFT;
                KeyCode::BackTab
            }
            code => code,
        };
        Self::Press(code, modifiers)
    }
    /// The digit `event` is when this key's family takes it.
    fn digit(self, event: Key) -> Option<u8> {
        match (self, event) {
            (Self::Digits(m), Self::Press(KeyCode::Char(c @ '0'..='9'), pressed))
                if m == pressed =>
            {
                // Plain digits are 1–9: 0 stays Neovim's.
                (c != '0' || !m.is_empty()).then(|| c as u8 - b'0')
            }
            _ => None,
        }
    }
    /// Whether a press of `event` is this key.
    pub fn takes(self, event: Key) -> bool {
        self == event || self.digit(event).is_some()
    }
    /// Whether some press is both keys.
    fn overlaps(self, other: Key) -> bool {
        match (self, other) {
            (Self::Press(..), Self::Press(..)) | (Self::Digits(_), Self::Digits(_)) => {
                self == other
            }
            (Self::Digits(_), press) => self.takes(press),
            (press, Self::Digits(_)) => other.takes(press),
        }
    }
    /// The event a legacy handler expects for this press.
    pub fn event(self) -> Option<KeyEvent> {
        match self {
            Self::Press(KeyCode::Char(c), m) if c.is_uppercase() => {
                Some(KeyEvent::new(KeyCode::Char(c), m | M::SHIFT))
            }
            Self::Press(code, m) => Some(KeyEvent::new(code, m)),
            Self::Digits(_) => None,
        }
    }
    /// A character typed into a field: text in Insert mode.
    fn printable(self) -> bool {
        match self {
            Self::Press(KeyCode::Char(' '), m) => !m.intersects(M::CONTROL | M::ALT | M::SHIFT),
            Self::Press(KeyCode::Char(_), m) | Self::Digits(m) => {
                !m.intersects(M::CONTROL | M::ALT)
            }
            Self::Press(..) => false,
        }
    }
    /// As the interface names it: `Alt+v`, `Ctrl+#`, `Shift+Space`, `Enter`.
    pub fn display(self) -> String {
        let (code, m) = match self {
            Self::Press(code, m) => (Some(code), m),
            Self::Digits(m) => (None, m),
        };
        let mut name = String::new();
        if m.contains(M::CONTROL) {
            name += "Ctrl+";
        }
        if m.contains(M::ALT) {
            name += "Alt+";
        }
        if m.contains(M::SHIFT) {
            name += "Shift+";
        }
        match code {
            None if m.is_empty() => name += "1–9",
            None => name += "0–9",
            Some(KeyCode::Char(' ')) => name += "Space",
            Some(KeyCode::Char(c)) => name.push(c),
            Some(KeyCode::Enter) => name += "Enter",
            Some(KeyCode::Esc) => name += "Esc",
            Some(KeyCode::Tab) => name += "Tab",
            Some(KeyCode::BackTab) => name += "Shift+Tab",
            Some(KeyCode::Backspace) => name += "Backspace",
            Some(KeyCode::Delete) => name += "Del",
            Some(KeyCode::Home) => name += "Home",
            Some(KeyCode::End) => name += "End",
            Some(KeyCode::Left) => name += "←",
            Some(KeyCode::Right) => name += "→",
            Some(KeyCode::Up) => name += "↑",
            Some(KeyCode::Down) => name += "↓",
            Some(KeyCode::PageUp) => name += "PgUp",
            Some(KeyCode::PageDown) => name += "PgDn",
            Some(KeyCode::F(n)) => name += &format!("F{n}"),
            Some(code) => name += &format!("{code:?}"),
        }
        name
    }
    /// As `keys.toml` writes it, in Vim's notation: `<M-v>`, `<CR>`, `g`.
    pub fn notation(self) -> String {
        let (code, m) = match self {
            Self::Press(code, m) => (Some(code), m),
            Self::Digits(m) => (None, m),
        };
        let name = match code {
            None => "#".to_string(),
            Some(KeyCode::Char(' ')) => "Space".into(),
            Some(KeyCode::Char('<')) => "lt".into(),
            Some(KeyCode::Char(c)) if m.is_empty() => return c.to_string(),
            Some(KeyCode::Char(c)) => c.to_string(),
            Some(KeyCode::Enter) => "CR".into(),
            Some(KeyCode::Esc) => "Esc".into(),
            Some(KeyCode::Tab) => "Tab".into(),
            Some(KeyCode::BackTab) => "S-Tab".into(),
            Some(KeyCode::Backspace) => "BS".into(),
            Some(KeyCode::Delete) => "Del".into(),
            Some(KeyCode::Home) => "Home".into(),
            Some(KeyCode::End) => "End".into(),
            Some(KeyCode::Left) => "Left".into(),
            Some(KeyCode::Right) => "Right".into(),
            Some(KeyCode::Up) => "Up".into(),
            Some(KeyCode::Down) => "Down".into(),
            Some(KeyCode::PageUp) => "PageUp".into(),
            Some(KeyCode::PageDown) => "PageDown".into(),
            Some(KeyCode::F(n)) => format!("F{n}"),
            Some(code) => format!("{code:?}"),
        };
        let mut prefix = String::new();
        if m.contains(M::CONTROL) {
            prefix += "C-";
        }
        if m.contains(M::ALT) {
            prefix += "M-";
        }
        // Ctrl reads a letter as lowercase unless Shift is written too.
        let shifted = matches!(code, Some(KeyCode::Char(c)) if c.is_ascii_uppercase())
            && m.contains(M::CONTROL);
        if m.contains(M::SHIFT) || shifted {
            prefix += "S-";
        }
        format!("<{prefix}{name}>")
    }
}
/// A sequence as the interface names it: printable keys run together
/// (`gg`, `g/`), others set apart (`Alt+g u`).
pub fn display(keys: &[Key]) -> String {
    let mut text = String::new();
    let mut spaced = false;
    for (i, key) in keys.iter().enumerate() {
        let name = key.display();
        let single = name.chars().count() == 1;
        if i > 0 && (spaced || !single) {
            text.push(' ');
        }
        spaced = !single;
        text += &name;
    }
    text
}
pub fn notation(keys: &[Key]) -> String {
    keys.iter().map(|k| k.notation()).collect()
}
/// Whether `keys.toml` can hold `keys`: their notation reads back as them,
/// and is not `none`, which stands for no key.
pub fn writable(keys: &[Key]) -> bool {
    let text = notation(keys);
    text != "none" && parse(&text).is_ok_and(|read| read == keys)
}
/// Vim's key notation: printable characters stand for themselves; `<...>`
/// names a key with optional `C-`, `M-` (or `A-`) and `S-` modifiers:
/// `<CR>`, `<Esc>`, `<Tab>`, `<BS>`, `<Del>`, `<Insert>`, `<Space>`,
/// `<lt>`, arrows (`<Up>`), `<PageUp>`, `<PageDown>`, `<Home>`, `<End>`,
/// `<F1>` and up, one character (`<M-h>`, `<M->>`; Ctrl keeps a capital
/// only with `S-`), or `#` for a digit family (`<C-#>`).
pub fn parse(text: &str) -> Result<Vec<Key>, String> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '<'
            && let Some(end) = rest.find('>')
            && end > 1
        {
            // `<M->>`: after the modifiers, `>` is the key itself.
            let end = if modifiers_only(&rest[1..end]) && rest[end + 1..].starts_with('>') {
                end + 1
            } else {
                end
            };
            keys.push(parse_named(&rest[1..end]).map_err(|e| format!("{e} in {text:?}"))?);
            rest = &rest[end + 1..];
            continue;
        }
        if c.is_control() {
            return Err(format!("control character in {text:?}"));
        }
        keys.push(Key::press(KeyEvent::new(KeyCode::Char(c), M::NONE)));
        rest = &rest[c.len_utf8()..];
    }
    if keys.is_empty() {
        return Err("empty key sequence".into());
    }
    if keys.len() > 1 && keys.iter().any(|k| matches!(k, Key::Digits(_))) {
        return Err(format!(
            "a digit family (#) is a key on its own, not in {text:?}"
        ));
    }
    Ok(keys)
}
/// Whether `text` is modifiers alone (`M-`, `C-S-`), each with its dash.
fn modifiers_only(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.len() >= 2
        && lower.len().is_multiple_of(2)
        && lower
            .as_bytes()
            .chunks(2)
            .all(|m| matches!(m, [b'c' | b'm' | b'a' | b's', b'-']))
}
fn parse_named(inner: &str) -> Result<Key, String> {
    let mut modifiers = M::NONE;
    let mut name = inner;
    loop {
        let lower = name.to_ascii_lowercase();
        let flag = if lower.starts_with("c-") {
            M::CONTROL
        } else if lower.starts_with("m-") || lower.starts_with("a-") {
            M::ALT
        } else if lower.starts_with("s-") {
            M::SHIFT
        } else {
            break;
        };
        // `<C-->`: a modifier of the key `-`.
        if name.len() == 2 {
            break;
        }
        modifiers |= flag;
        name = &name[2..];
    }
    if name == "#" {
        return Ok(Key::Digits(modifiers));
    }
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        let c = if modifiers.contains(M::CONTROL) && !modifiers.contains(M::SHIFT) {
            c.to_ascii_lowercase()
        } else {
            c
        };
        return Ok(Key::press(KeyEvent::new(KeyCode::Char(c), modifiers)));
    }
    let code = match name.to_ascii_lowercase().as_str() {
        "cr" | "enter" | "return" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "bs" | "backspace" => KeyCode::Backspace,
        "del" | "delete" => KeyCode::Delete,
        "space" => KeyCode::Char(' '),
        "lt" => KeyCode::Char('<'),
        "bar" => KeyCode::Char('|'),
        "insert" | "ins" => KeyCode::Insert,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        other => match other.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            Some(n @ 1..) => KeyCode::F(n),
            _ => return Err(format!("unknown key <{inner}>")),
        },
    };
    if code == KeyCode::Tab && modifiers.contains(M::SHIFT) {
        return Ok(Key::press(KeyEvent::new(KeyCode::BackTab, modifiers)));
    }
    Ok(Key::press(KeyEvent::new(code, modifiers)))
}

/// A fixed key named plainly (`"enter"`, `"ctrl+s"`, `"up"`), as the
/// interface names it (`Enter`, `Ctrl+s`, `↑`).
pub fn named(name: &str) -> String {
    let mut modifiers = String::new();
    let mut rest = name;
    for (long, short) in [("ctrl+", "C-"), ("alt+", "M-"), ("shift+", "S-")] {
        if let Some(after) = rest.strip_prefix(long) {
            modifiers += short;
            rest = after;
        }
    }
    parse(&format!("<{modifiers}{rest}>")).map_or_else(|_| name.to_string(), |keys| display(&keys))
}

/// Groups for the Keys screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Highlight,
    GoTo,
    Open,
    Finish,
    Marks,
    Search,
    View,
    Files,
    Session,
}
impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::Highlight => "Highlight",
            Self::GoTo => "Go to",
            Self::Open => "Open and run",
            Self::Finish => "Finish",
            Self::Marks => "Marks",
            Self::Search => "Search",
            Self::View => "View",
            Self::Files => "Files",
            Self::Session => "Session",
        }
    }
}

macro_rules! functions {
    ($($variant:ident $name:literal $group:ident $description:literal;)*) => {
        /// Everything a key can be bound to.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum Function { $($variant,)* }
        impl Function {
            pub const ALL: &'static [Self] = &[$(Self::$variant,)*];
            /// The name `keys.toml` uses.
            pub fn name(self) -> &'static str {
                match self { $(Self::$variant => $name,)* }
            }
            pub fn group(self) -> Group {
                match self { $(Self::$variant => Group::$group,)* }
            }
            /// What it does, in a sentence or two.
            pub fn description(self) -> &'static str {
                match self { $(Self::$variant => $description,)* }
            }
        }
    };
}
// Names are a verb and what it acts on, and each description starts with
// that verb: `highlight_` moves the highlight, `go_to_` changes the current
// directory, `toggle_` turns something on or off, `show_` opens a screen or
// chooser, `print_` answers with paths and leaves, `quit_` leaves.
functions! {
    HighlightPrevious "highlight_previous" Highlight "Highlight the row above, in the list that has the focus: the results, or the marked list.";
    HighlightNext "highlight_next" Highlight "Highlight the row below, in the list that has the focus: the results, or the marked list.";
    HighlightPreviousPage "highlight_previous_page" Highlight "Highlight the row a screenful up in the results, scrolling them back a page.";
    HighlightNextPage "highlight_next_page" Highlight "Highlight the row a screenful down in the results, scrolling them on a page.";
    HighlightFirst "highlight_first" Highlight "Highlight the first row of the list that has the focus; from the query in Normal mode, the first result, giving the results the focus.";
    HighlightLast "highlight_last" Highlight "Highlight the last row of the list that has the focus; from the query in Normal mode, the last result, giving the results the focus.";
    HighlightRow "highlight_row" Highlight "Highlight row # (the number shown before each row on screen). 0 is the last row on screen.";
    HighlightNextInRow "highlight_next_in_row" Highlight "Highlight row # (the number shown before each row on screen), then do what highlight_next does there: highlight the row below it. 0 is the last row on screen.";
    GoToParent "go_to_parent" GoTo "Go up to the parent of the current directory. The query is kept, so the parent's entries are filtered by it too.";
    GoToHighlighted "go_to_highlighted" GoTo "Go to the highlighted entry: into it if it is a directory; if it is a file, to the directory holding it, with the file highlighted (the query is cleared so that the file shows).";
    GoToPath "go_to_path" GoTo "Go to a directory typed in a prompt. Words typed there also suggest your workspaces and, with zoxide installed, the directories you use most.";
    GoToTrash "go_to_trash" GoTo "Go to the trash and browse it like any directory; restore puts entries back where they were. Pressed again in the trash, go to the next filesystem's trash.";
    GoToOlderJump "go_to_older_jump" GoTo "Go to the older directory in this tab's jump list: the one you were in before, as Neovim's Ctrl+o goes to an older position. Each tab keeps its own jump list.";
    GoToNewerJump "go_to_newer_jump" GoTo "Go to the newer directory in this tab's jump list, back towards where go_to_older_jump started, as Neovim's Ctrl+i does. Ctrl+i needs a terminal that tells it apart from Tab.";
    GoToNextTab "go_to_next_tab" GoTo "Go to the next tab. fff has three, each with its own directory and history; the marks are shared by all three.";
    GoToPreviousTab "go_to_previous_tab" GoTo "Go to the previous tab. fff has three, each with its own directory and history.";
    OpenInBackground "open_in_background" Open "Open the highlighted file with the program set for it (the [open] section of the configuration) and keep browsing: fff does not wait for it. Not in dialogs choosing files or where to save, which choose files rather than open them.";
    OpenInTerminal "open_in_terminal" Open "Open the highlighted file, or the marked ones, with their programs inside this terminal, one after another, then come back to fff.";
    OpenAndQuit "open_and_quit" Open "Open the highlighted file with the program set for it and quit fff. A program that runs in a terminal takes fff's place in this one.";
    OpenRow "open_row" Open "Highlight row # (the number shown before each row on screen), then do what open_and_quit does there: open its file and quit fff. On a directory's row it only highlights it. 0 is the last row on screen.";
    EditFiles "edit_files" Open "Edit the highlighted file, or the marked ones, in your $EDITOR inside this terminal, then come back to fff.";
    Run "run" Open "Run a program on the highlighted entry, or the marked ones: type its command line, check it, and come back to fff when it ends.";
    RunAndQuit "run_and_quit" Open "Run a program on the highlighted entry, or the marked ones, in place of fff, which quits; fff exits with the program's exit status.";
    PrintHighlighted "print_highlighted" Finish "Print the highlighted path and quit, ignoring any marks: a file or a directory in fff pick, a file in file dialogs, a directory in folder dialogs.";
    PrintHighlightedAndMarks "print_highlighted_and_marks" Finish "Print the marked paths and the highlighted one, then quit, where the answer may be several paths: fff pick, and dialogs choosing several files or folders.";
    PrintMarks "print_marks" Finish "Print the marked paths and quit, where the answer may be several paths: fff pick, and dialogs choosing several files or folders.";
    PrintHere "print_here" Finish "Print the current directory and quit: the answer of fff pick, or of a folder dialog.";
    SaveHere "save_here" Finish "Save in the current directory, under the name typed in the query or, with the query empty, the suggested one (the save dialog, fff explorer save).";
    SaveOver "save_over" Finish "Save over the highlighted file, after asking whether to replace it (the save dialog, fff explorer save).";
    QuitIntoHighlighted "quit_into_highlighted" Finish "Quit into the highlighted directory, or the one holding the highlighted file; with nothing highlighted, into the current directory. Your shell's directory changes to it, with the shell integration (fff roam and fff seek).";
    QuitHere "quit_here" Finish "Quit into the current directory, ignoring the marks: your shell's directory changes to it, with the shell integration (fff roam). In a terminal window of its own (fff window), fff leaves a shell there.";
    Quit "quit" Finish "Quit without choosing anything: back in the directory you started from (fff roam), printing nothing (fff pick, fff seek), or cancelling the dialog (fff explorer).";
    Cancel "cancel" Finish "Cancel: quit at once from anywhere in fff, printing nothing. While a file operation checks its entries, stop that instead; while operations run, ask first.";
    ToggleMark "toggle_mark" Marks "Toggle the highlighted row's mark: marked or not. Marks collect paths for file operations and answers. In the results the highlight then moves on (with a contents query, to the next file), so pressing it again marks the next row.";
    MarkAll "mark_all" Marks "Mark every row of the list that has the focus: in the results, every entry the query matches, also those off screen.";
    InvertMarks "invert_marks" Marks "Invert the marks of every row of the list that has the focus: marked ones lose their mark, the others get one.";
    ClearMarks "clear_marks" Marks "Clear every mark.";
    ToggleMarkedList "toggle_marked_list" Marks "Toggle the focus between the results and the marked list, the pane listing every mark, where keys act on the marks instead.";
    ToggleQueryFocus "toggle_query_focus" Search "Toggle the focus between the results and the query above them; typing into the query gives it the focus too. With the focus on the query, Normal mode edits it as Neovim does: the default keys typed as text (h, l, x, p, u…) go to Neovim there, while keys not typed as text (Tab, arrows, Alt and Ctrl keys) still act on the results. Keys you bind apply there too, unless their focus leaves the query out.";
    ClearQuery "clear_query" Search "Clear the query.";
    SwitchQuery "switch_query" Search "Switch typing between the two queries: names, which filters entries by name, and contents, which searches inside the files the names query lists, one row per match. The contents bar shows while it is typed into or set.";
    SearchShallower "search_shallower" Search "Search one level less deep below the current directory. The least is depth 1: the directory's own entries.";
    SearchDeeper "search_deeper" Search "Search one level deeper below the current directory.";
    SearchToDepth "search_to_depth" Search "Search down to depth #, a digit from 1 to 9: 1 lists the current directory's own entries, 2 also those of its subdirectories, and so on.";
    ToggleHidden "toggle_hidden" Search "Toggle hidden entries (whose names start with a dot): listed or not.";
    ToggleDirectories "toggle_directories" Search "Toggle directories: listed, or left out so that only files are listed.";
    ToggleFiles "toggle_files" Search "Toggle files: listed, or left out so that only directories are listed.";
    EditExclusions "edit_exclusions" Search "Edit this session's exclusions in a buffer, one glob pattern per line (such as *.o or node_modules): matching entries, and everything below them, are not listed. ZZ applies them.";
    ToggleExclusions "toggle_exclusions" Search "Toggle the exclusions: applied, or ignored so that excluded entries are listed too.";
    EditFilters "edit_filters" Search "Edit this session's file filters in a buffer, one glob pattern per line (such as *.pdf): only files matching one of them are listed. ZZ applies them.";
    ToggleFilters "toggle_filters" Search "Toggle the file filters: applied, or off so that every file is listed. They are kept for turning back on.";
    Rescan "rescan" Search "Rescan the current directory and everything below it from disk, ignoring what fff has cached.";
    TogglePreview "toggle_preview" View "Toggle the preview pane, which shows the highlighted entry's contents: shown or hidden.";
    TogglePreviewWrap "toggle_preview_wrap" View "Toggle word wrapping in the preview: long lines wrapped onto the next rows, or cut at its edge.";
    ScrollPreviewDown "scroll_preview_down" View "Scroll the preview down by half its height, leaving the highlight where it is.";
    ScrollPreviewUp "scroll_preview_up" View "Scroll the preview up by half its height, leaving the highlight where it is.";
    ToggleLabels "toggle_labels" View "Toggle how rows are labelled: by name, with just the parent folders that tell same-named entries apart (folder1/main.pdf), or by their whole path below the directory searched. fff pick starts with names, the others with paths; the choice stays when switch_program changes program.";
    ShowPalettes "show_palettes" View "Show the color palettes: the highlighted one is tried out after a moment, and Enter keeps it for this session.";
    ShowSorting "show_sorting" View "Show the sorting picker: sort the results by name, size, a date, extension, kind or depth, or in the program's own order; Tab switches between ascending and descending. The query still decides what is listed.";
    ReloadPalette "reload_palette" View "Reload the palette the configuration names, dropping one chosen with show_palettes for this session.";
    MoveMarked "move_marked" Files "Move the marked entries into the current directory. Asks first when that would overwrite anything.";
    CopyMarked "copy_marked" Files "Copy the marked entries into the current directory. Asks first when that would overwrite anything.";
    LinkMarked "link_marked" Files "Link the marked entries into the current directory, as symbolic links pointing to them.";
    Trash "trash" Files "Move the highlighted entry, or the marked ones, to the trash, from where restore puts them back. Asks first only about very large ones.";
    Delete "delete" Files "Delete the highlighted entry, or the marked ones, for good, after listing them with their sizes for a review.";
    Rename "rename" Files "Rename the entries the results list by editing their names in a buffer, one per line; writing it (ZZ) renames them.";
    Extract "extract" Files "Extract the highlighted archive, or the marked ones, into a folder a prompt asks for.";
    Create "create" Files "Create a new file or directory in the current directory: choose which, then type its name.";
    Undo "undo" Files "Undo the last file operation of this session; pressed again, the one before it. One still running is stopped, then undone. Its entries are marked again.";
    Restore "restore" Files "In the trash: put the highlighted entry, or the marked ones, back where they were deleted from.";
    ToggleJobList "toggle_job_list" Files "Toggle the focus between the results and the jobs list: the file operations still running, shown below the marks. There x stops the highlighted one and undoes what it had done.";
    ShowKeys "show_keys" Session "Show the Keys screen, to look keys up and change them; the same key closes it.";
    ShowSession "show_session" Session "Show the Session tab: the exact paths, search scopes and policies in effect, the marks, and any errors in the configuration.";
    ShowWorkspaces "show_workspaces" Session "Show the workspaces, directories saved to go to at once: go to one, or add, edit, delete and reorder them.";
    EditConfig "edit_config" Session "Edit the configuration file in your $EDITOR, then apply it to this session at once.";
    SwitchProgram "switch_program" Session "Go on as fff roam (browse), pick (choose paths) or seek (find files), keeping the query, highlight, marks, exclusions, filters and any depth you set.";
}

use Focus::{Marks, Query, Results};
use Highlight as H;
use Program::*;
/// The results and the query above them: what works on the results works
/// from their query too.
const RESULTS: [Focus; 2] = [Query, Results];
const EXPLORERS: [Program; 5] = [
    ExplorerOpen,
    ExplorerMultiple,
    ExplorerFolder,
    ExplorerFolderMultiple,
    ExplorerSave,
];
/// Programs that navigate: all but seek and a list read from stdin.
const NAVIGATE: [Program; 8] = [
    Roam,
    Window,
    Pick,
    ExplorerOpen,
    ExplorerMultiple,
    ExplorerFolder,
    ExplorerFolderMultiple,
    ExplorerSave,
];
/// Programs listing folders as well as files: all but seek.
const BOTH_KINDS: [Program; 9] = [
    Roam,
    Window,
    Pick,
    ExplorerOpen,
    ExplorerMultiple,
    ExplorerFolder,
    ExplorerFolderMultiple,
    ExplorerSave,
    Piped,
];
/// Programs that open files: not the dialogs choosing files or where to
/// save, which choose them rather than open them.
const OPENING: [Program; 6] = [
    Roam,
    Window,
    Pick,
    Seek,
    ExplorerFolder,
    ExplorerFolderMultiple,
];
/// Programs whose answer can be several paths, the marks among them.
const SEVERAL: [Program; 4] = [Pick, Piped, ExplorerMultiple, ExplorerFolderMultiple];
fn scope(programs: &[Program], focuses: &[Focus], highlights: &[Highlight]) -> Scope {
    Scope::of(programs, focuses, highlights)
}
impl Function {
    /// Where it works in `mode`, as boxes; none where it does not. This is
    /// a design choice, not configuration, and the same in both modes but
    /// for the focus: Insert mode types into the query.
    pub fn available(self, mode: Mode) -> Vec<Scope> {
        let piped = if self.works_piped() { 0 } else { Piped.bit() };
        let focuses = match mode {
            Mode::Insert => Query.bit(),
            Mode::Normal => Scope::ALL.focuses,
        };
        self.boxes()
            .into_iter()
            .map(|s| Scope {
                programs: s.programs & !piped,
                focuses: s.focuses & focuses,
                ..s
            })
            .filter(|s| !s.is_empty())
            .collect()
    }
    /// Whether it works on a list read from stdin (`Program::Piped`), which
    /// neither navigates, changes files, opens them nor lists anything
    /// else: moving the highlight, the marks and queries, the view, and
    /// printing or leaving.
    fn works_piped(self) -> bool {
        use Function::*;
        matches!(
            self,
            HighlightPrevious
                | HighlightNext
                | HighlightPreviousPage
                | HighlightNextPage
                | HighlightFirst
                | HighlightLast
                | HighlightRow
                | HighlightNextInRow
                | PrintHighlighted
                | PrintHighlightedAndMarks
                | PrintMarks
                | Quit
                | Cancel
                | ToggleMark
                | MarkAll
                | InvertMarks
                | ClearMarks
                | ToggleMarkedList
                | ClearQuery
                | ToggleQueryFocus
                | SwitchQuery
                | ToggleDirectories
                | ToggleFiles
                | TogglePreview
                | TogglePreviewWrap
                | ScrollPreviewDown
                | ScrollPreviewUp
                | ToggleLabels
                | ShowPalettes
                | ShowSorting
                | ReloadPalette
                | ShowKeys
                | ShowSession
        )
    }
    /// `available`, before what does not work on a list or in a mode is
    /// taken out. Each function does one thing wherever it works: one that
    /// would mean nothing in a program does not work there.
    fn boxes(self) -> Vec<Scope> {
        use Function::*;
        let lists = Scope::ALL;
        let results = scope(&[], &RESULTS, &[]);
        let file = [H::File];
        let entry = [H::Dir, H::File];
        match self {
            HighlightPrevious | HighlightNext | HighlightFirst | HighlightLast | ToggleMark
            | MarkAll | InvertMarks | ClearMarks | ToggleMarkedList | ToggleJobList | ShowKeys
            | ShowSession | Cancel | Quit | OpenInTerminal | EditFiles | Run | RunAndQuit
            | MoveMarked | CopyMarked | LinkMarked | Trash | Delete | Extract | Undo | Restore
            | EditConfig => vec![lists],
            HighlightPreviousPage
            | HighlightNextPage
            | HighlightRow
            | HighlightNextInRow
            | SearchShallower
            | SearchDeeper
            | SearchToDepth
            | ToggleHidden
            | TogglePreview
            | TogglePreviewWrap
            | ToggleLabels
            | ScrollPreviewDown
            | ScrollPreviewUp
            | EditExclusions
            | ToggleExclusions
            | EditFilters
            | ToggleFilters
            | Rescan
            | ShowPalettes
            | ShowSorting
            | ReloadPalette
            | Rename
            | Create
            | ClearQuery
            | ToggleQueryFocus
            // A workspace can switch program, so it opens in every one.
            | SwitchQuery
            | ShowWorkspaces => vec![results],
            GoToParent | GoToPath | GoToTrash | GoToOlderJump | GoToNewerJump | GoToNextTab
            | GoToPreviousTab => vec![scope(&NAVIGATE, &RESULTS, &[])],
            ToggleDirectories | ToggleFiles => vec![scope(&BOTH_KINDS, &RESULTS, &[])],
            GoToHighlighted => vec![scope(&NAVIGATE, &[], &entry)],
            OpenInBackground => vec![scope(&OPENING, &RESULTS, &file)],
            // Whatever is highlighted: the row it opens is its number's.
            OpenRow => vec![scope(&[Roam, Window], &RESULTS, &[])],
            OpenAndQuit => vec![scope(&[Roam, Window], &[], &file)],
            SaveHere => vec![scope(&[ExplorerSave], &RESULTS, &[])],
            SaveOver => vec![scope(&[ExplorerSave], &[], &file)],
            // The answer is a path: a file or a folder, as the program asks.
            PrintHighlighted => vec![
                scope(&[Pick, Piped], &[], &entry),
                scope(&[ExplorerOpen, ExplorerMultiple], &[], &file),
                scope(&[ExplorerFolder, ExplorerFolderMultiple], &[], &[H::Dir]),
            ],
            PrintHighlightedAndMarks => vec![
                scope(&[Pick, Piped], &[], &entry),
                scope(&[ExplorerMultiple], &[], &file),
                scope(&[ExplorerFolderMultiple], &[], &[H::Dir]),
            ],
            PrintMarks => vec![scope(&SEVERAL, &[], &[])],
            PrintHere => vec![scope(&[Pick, ExplorerFolder, ExplorerFolderMultiple], &[], &[])],
            // Your shell follows: the programs started from it that move.
            QuitIntoHighlighted => vec![scope(&[Roam, Seek], &[], &[])],
            QuitHere => vec![scope(&[Roam, Window], &[], &[])],
            SwitchProgram => vec![scope(&Program::SWITCHABLE.map(|(p, _)| p), &[], &[])],
        }
    }
    /// Whether it works in `cell` in `mode`.
    pub fn works(self, mode: Mode, cell: Cell) -> bool {
        self.available(mode).iter().any(|s| s.contains(cell))
    }
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }
    /// The function `key` alone has by default in `mode` at `cell`: what
    /// the mouse's fixed keys do.
    pub fn default_of(key: Key, mode: Mode, cell: Cell) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| {
            f.defaults(mode)
                .iter()
                .any(|b| b.keys == [key] && b.applies(cell))
        })
    }
    /// The bindings it has without `keys.toml`.
    pub fn defaults(self, mode: Mode) -> Vec<Binding> {
        use Function::*;
        let all = Scope::ALL;
        // Enter goes into a folder where fff stays: in a window of its own,
        // and in the dialogs but the save dialog, where it saves.
        let staying = [
            Window,
            ExplorerOpen,
            ExplorerMultiple,
            ExplorerFolder,
            ExplorerFolderMultiple,
        ];
        let enters = scope(&staying, &[], &[H::Dir]);
        let enters_results = scope(&staying, &RESULTS, &[H::Dir]);
        let folders = [ExplorerFolder, ExplorerFolderMultiple];
        let lists = scope(&[], &[Results, Marks], &[]);
        let results = scope(&[], &[Results], &[]);
        let query = scope(&[], &[Query], &[]);
        let insert: &[(&str, Scope)] = match self {
            HighlightPrevious => &[("<M-k>", all)],
            HighlightNext => &[("<M-j>", all)],
            GoToParent => &[("<M-h>", all)],
            GoToHighlighted => &[("<M-l>", all), ("<CR>", enters)],
            GoToOlderJump => &[("<C-o>", all)],
            GoToNewerJump => &[("<C-i>", all)],
            GoToNextTab => &[("<Up>", all)],
            GoToPreviousTab => &[("<Down>", all)],
            OpenAndQuit => &[("<CR>", all)],
            OpenRow => &[("<C-#>", all)],
            HighlightNextInRow => &[("<M-#>", all)],
            PrintHighlighted => &[("<S-CR>", all), ("<CR>", scope(&[ExplorerOpen], &[], &[]))],
            PrintHighlightedAndMarks => {
                &[("<CR>", scope(&[Pick, Piped, ExplorerMultiple], &[], &[]))]
            }
            PrintMarks => &[("<C-CR>", all)],
            SaveHere => &[("<CR>", all), ("<C-s>", all)],
            PrintHere => &[("<C-s>", scope(&folders, &[], &[]))],
            QuitIntoHighlighted => &[("<CR>", scope(&[Seek], &[], &[]))],
            SwitchQuery => &[("<Tab>", all)],
            ToggleQueryFocus => &[("<S-Tab>", all)],
            ClearMarks => &[("<C-m>", all)],
            Rescan => &[("<F5>", all)],
            Cancel => &[("<C-c>", all)],
            ShowKeys => &[("<F1>", all)],
            ShowSession => &[("<F2>", all)],
            _ => &[],
        };
        let normal: &[(&str, Scope)] = match self {
            HighlightPrevious => &[("k", all)],
            HighlightNext => &[("j", all)],
            HighlightPreviousPage => &[("K", all)],
            HighlightNextPage => &[("J", all)],
            HighlightFirst => &[("gg", all), ("j", query)],
            HighlightLast => &[("G", all), ("k", query)],
            GoToParent => &[("h", lists)],
            GoToHighlighted => &[
                ("l", lists),
                ("<CR>", enters_results),
                ("<CR>", scope(&[Window], &[Marks], &[H::Dir])),
            ],
            GoToPath => &[("/", lists)],
            ShowWorkspaces => &[("gw", all)],
            GoToTrash => &[("gt", all)],
            GoToOlderJump => &[("<C-o>", all)],
            GoToNewerJump => &[("<C-i>", all)],
            GoToNextTab => &[("<Up>", all)],
            GoToPreviousTab => &[("<Down>", all)],
            SaveHere => &[
                ("!", all),
                ("<C-s>", all),
                ("<CR>", scope(&[], &RESULTS, &[])),
            ],
            // Enter in the results saves under the typed name; on a mark it
            // offers to replace that file.
            SaveOver => &[("<CR>", scope(&[], &[Marks], &[]))],
            PrintHighlighted => &[
                ("<S-CR>", all),
                ("<CR>", scope(&[ExplorerOpen], &[], &[])),
                ("<CR>", scope(&folders, &[Marks], &[])),
            ],
            PrintHighlightedAndMarks => {
                &[("<CR>", scope(&[Pick, Piped, ExplorerMultiple], &[], &[]))]
            }
            PrintMarks => &[("<C-CR>", all)],
            PrintHere => &[
                ("!", all),
                ("<C-s>", scope(&folders, &[], &[])),
                ("<CR>", scope(&folders, &[], &[H::None])),
                ("<CR>", scope(&[Pick], &RESULTS, &[H::None])),
            ],
            QuitIntoHighlighted => &[("g!", lists), ("<CR>", scope(&[Seek], &[], &[]))],
            OpenAndQuit => &[("<CR>", all)],
            OpenInBackground => &[("L", all)],
            OpenRow => &[("<C-#>", all)],
            HighlightRow => &[("<#>", lists)],
            HighlightNextInRow => &[("<M-#>", all)],
            Run => &[("gx", all)],
            RunAndQuit => &[("gX", all)],
            EditFiles => &[("e", all)],
            ToggleMark => &[("<Space>", all)],
            MarkAll => &[("m", all)],
            InvertMarks => &[("M", all)],
            ClearMarks => &[("<C-m>", all)],
            ToggleMarkedList => &[("<C-k>", all)],
            ToggleJobList => &[("<C-j>", all)],
            ClearQuery => &[("<Esc>", results)],
            ToggleQueryFocus => &[("<Esc>", query), ("<Tab>", results), ("<S-Tab>", all)],
            SwitchQuery => &[("<Tab>", scope(&[], &[Query, Marks], &[]))],
            SearchShallower => &[("-", all)],
            SearchDeeper => &[("+", all)],
            SearchToDepth => &[("<C-M-#>", all)],
            ToggleHidden => &[("g.", all)],
            ToggleDirectories => &[("gd", all)],
            ToggleFiles => &[("gD", all)],
            TogglePreview => &[("gpp", all)],
            TogglePreviewWrap => &[("gpw", all)],
            ToggleLabels => &[("gl", all)],
            ScrollPreviewDown => &[("<PageDown>", all)],
            ScrollPreviewUp => &[("<PageUp>", all)],
            EditExclusions => &[("gE", all)],
            ToggleExclusions => &[("ge", all)],
            EditFilters => &[("gF", all)],
            ToggleFilters => &[("gf", all)],
            Rescan => &[("<F5>", all)],
            MoveMarked => &[("v", results)],
            CopyMarked => &[("p", results)],
            LinkMarked => &[("s", results)],
            Trash => &[("x", results)],
            Delete => &[("X", results)],
            Rename => &[("r", all)],
            Extract => &[("E", results)],
            Create => &[("n", all)],
            Undo => &[("u", results)],
            Restore => &[("R", results)],
            Quit => &[("q", all)],
            QuitHere => &[("!", all)],
            Cancel => &[("<C-c>", all)],
            ShowKeys => &[("<F1>", all)],
            ShowSession => &[("<F2>", all)],
            ShowPalettes => &[("gP", all)],
            ShowSorting => &[("gs", all)],
            EditConfig => &[("gc", all)],
            SwitchProgram => &[("gm", all)],
            _ => &[],
        };
        let specs: Vec<(&str, Scope)> = match mode {
            Mode::Insert => insert.to_vec(),
            Mode::Normal => normal.to_vec(),
        };
        specs
            .into_iter()
            .map(|(text, scope)| {
                let keys = parse(text).expect("default keys parse");
                // A default key typed as text leaves the query in focus to
                // Neovim: there h moves the cursor and x deletes. Those
                // meant for the query too keep it: j and k go down from it,
                // - and + change the depth (Neovim's line motions, idle on a
                // one-line query), q quits rather than record a macro, and gc
                // would comment a one-line query out.
                let typed = mode == Mode::Normal
                    && keys[0].printable()
                    && !matches!(
                        (self, text),
                        (HighlightFirst, "j")
                            | (HighlightLast, "k")
                            | (SearchShallower, "-")
                            | (SearchDeeper, "+")
                            | (Quit, "q")
                            | (EditConfig, "gc")
                    );
                Binding {
                    function: self,
                    mode,
                    keys,
                    scope: Scope {
                        focuses: if typed {
                            scope.focuses & !Query.bit()
                        } else {
                            scope.focuses
                        },
                        ..scope
                    },
                }
            })
            .collect()
    }
}

/// Keys that cannot be bound, shown on the Keys screen: (keys, where, what).
pub const FIXED: &[(&str, &str, &str)] = &[
    ("Esc", "Insert", "Leave Insert mode."),
    ("i  a", "Normal", "Start typing in the query (Neovim)."),
    (
        ":",
        "Normal",
        "Show the functions that work here, to run one by name (Enter) or go back (Esc).",
    ),
    (
        "ZZ  ZQ",
        "buffers",
        "Save and close / discard and close an exclusions, filters or rename buffer; :wq and :q! too.",
    ),
    (
        "j k gg G",
        "choosers",
        "Move through the workspace, palette and icon choosers (Normal mode).",
    ),
    (
        "↑ ↓",
        "choosers",
        "Move; in the workspace chooser, move the highlighted workspace.",
    ),
    (
        "q  !",
        "choosers, go-to prompt",
        "Leave fff from Normal mode, as in the results.",
    ),
    (
        "Enter  Esc",
        "choosers, prompts, sheets",
        "Confirm / cancel.",
    ),
    (
        "n e d r",
        "workspaces",
        "New, edit, delete or reload workspaces (Normal mode; with Alt in Insert mode).",
    ),
    (
        "Alt+0–9",
        "workspaces",
        "Open the visible workspace #; 0 is the last.",
    ),
    (
        "Tab  Shift+Tab",
        "workspace form",
        "Switch fields; Space chooses the icon on its row.",
    ),
    (
        "j k  x  u  Esc",
        "jobs list",
        "Move; stop the highlighted operation and undo what it did (u also marks its entries again); go back.",
    ),
    (
        "Enter  Esc",
        "quitting while operations run",
        "Stop them and quit once they have ended / stay.",
    ),
    ("click", "mouse", "Highlight a row, clearing the marks."),
    (
        "double click",
        "mouse",
        "Open the row, as open_row would (in a file dialog, choose it).",
    ),
    ("drag", "mouse", "Mark the rows it spans."),
    ("wheel", "mouse", "Scroll the results."),
    ("click 1 2 3", "mouse", "Switch to that tab."),
    (
        "drop files",
        "mouse",
        "Copy them here, after asking (Normal mode).",
    ),
];

/// A key sequence on a function in one mode, applying within `scope`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub function: Function,
    pub mode: Mode,
    pub keys: Vec<Key>,
    pub scope: Scope,
}
impl Binding {
    /// Where it applies: its scope, within where its function works.
    pub fn reach(&self) -> Vec<Scope> {
        self.function
            .available(self.mode)
            .into_iter()
            .filter_map(|a| a.meet(self.scope))
            .collect()
    }
    pub fn applies(&self, cell: Cell) -> bool {
        self.scope.contains(cell) && self.function.works(self.mode, cell)
    }
    /// Where both apply with keys one of which starts the other, if they
    /// do: the boxes where pressing them is ambiguous.
    pub fn clashes(&self, other: &Self) -> Vec<Scope> {
        if self.mode != other.mode || !keys_clash(&self.keys, &other.keys) {
            return vec![];
        }
        let theirs = other.reach();
        self.reach()
            .into_iter()
            .flat_map(|a| theirs.iter().filter_map(move |b| a.meet(*b)))
            .collect()
    }
    pub fn display(&self) -> String {
        display(&self.keys)
    }
    /// Where it applies, in words: `""` where its function works
    /// everywhere and the scope does not narrow it.
    pub fn describe_reach(&self) -> String {
        let reach = self.reach();
        if reach == [Scope::ALL] {
            return String::new();
        }
        reach
            .iter()
            .map(|s| s.describe())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Whether one sequence starts the other: pressing it would be ambiguous.
fn keys_clash(a: &[Key], b: &[Key]) -> bool {
    a.iter().zip(b).all(|(a, b)| a.overlaps(*b))
}

/// What a key fed to the keymap came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// No binding has it here: handle it as unbound.
    Pass,
    /// It starts a binding: wait for the next key.
    Pending,
    /// A binding completed; the digit a digit family took.
    Run(Function, Option<u8>),
    /// The keys held back did not continue into a binding: deliver them,
    /// then handle the current key again.
    Flush(Vec<KeyEvent>),
}

/// Every binding in effect, and which functions `keys.toml` sets.
#[derive(Clone, Debug)]
pub struct Keymap {
    bindings: Vec<Binding>,
    custom: Vec<((Function, Mode), Vec<Binding>)>,
}
impl Default for Keymap {
    fn default() -> Self {
        Self::new(Vec::new()).expect("the default keys are valid")
    }
}
impl Keymap {
    /// The defaults, with `custom` replacing a function's bindings in a
    /// mode. A default binding that clashes with a custom one gives way;
    /// any other clash is an error, as is a keymap that locks you in.
    pub fn new(custom: Vec<((Function, Mode), Vec<Binding>)>) -> Result<Self, String> {
        let keymap = Self::assemble(custom);
        keymap.validate()?;
        Ok(keymap)
    }
    /// The defaults with `custom` applied, as `new` would make them but
    /// unchecked: what an edit not saved yet looks like.
    pub fn assemble(mut custom: Vec<((Function, Mode), Vec<Binding>)>) -> Self {
        custom.sort_by_key(|(at, _)| *at);
        custom.dedup_by_key(|(at, _)| *at);
        let mut chosen: Vec<Binding> = custom.iter().flat_map(|(_, b)| b.clone()).collect();
        let set: Vec<(Function, Mode)> = custom.iter().map(|(at, _)| *at).collect();
        let defaults = Function::ALL.iter().flat_map(|&f| {
            Mode::ALL
                .into_iter()
                .filter(|&m| !set.contains(&(f, m)))
                .flat_map(move |m| f.defaults(m))
                .collect::<Vec<_>>()
        });
        // A default gives way where a custom binding with clashing keys
        // applies, and keeps the rest of its scope.
        let mut kept = Vec::new();
        for default in defaults {
            let mut pieces = vec![default.scope];
            for c in chosen
                .iter()
                .filter(|c| c.mode == default.mode && keys_clash(&c.keys, &default.keys))
            {
                for taken in c.reach() {
                    pieces = pieces.into_iter().flat_map(|p| p.minus(taken)).collect();
                }
            }
            kept.extend(
                pieces
                    .into_iter()
                    .map(|scope| Binding {
                        scope,
                        ..default.clone()
                    })
                    .filter(|b| !b.reach().is_empty()),
            );
        }
        chosen.extend(kept);
        Self {
            bindings: chosen,
            custom,
        }
    }
    /// What `new` refuses: bad keys, keys that never apply, custom
    /// bindings that clash, and a keymap that locks you in.
    pub fn validate(&self) -> Result<(), String> {
        let chosen: Vec<&Binding> = self.custom.iter().flat_map(|(_, b)| b).collect();
        for binding in &chosen {
            check_keys(binding)?;
            if binding.reach().is_empty() {
                return Err(format!(
                    "{}.{}: {} never applies: {} does not work there",
                    binding.mode.name(),
                    binding.function.name(),
                    notation(&binding.keys),
                    binding.function.name()
                ));
            }
        }
        for (i, a) in chosen.iter().enumerate() {
            for b in &chosen[i + 1..] {
                if a.function != b.function
                    && let Some(at) = a.clashes(b).first()
                {
                    return Err(clash(a, b, *at));
                }
            }
        }
        self.check_escape()
    }
    pub fn bindings(&self) -> &[Binding] {
        &self.bindings
    }
    /// The bindings of `function` in `mode`.
    pub fn of(&self, function: Function, mode: Mode) -> Vec<&Binding> {
        self.bindings
            .iter()
            .filter(|b| b.function == function && b.mode == mode)
            .collect()
    }
    /// Whether `keys.toml` sets `function` in `mode`.
    pub fn is_custom(&self, function: Function, mode: Mode) -> bool {
        self.custom.iter().any(|(at, _)| *at == (function, mode))
    }
    pub fn custom(&self) -> &[((Function, Mode), Vec<Binding>)] {
        &self.custom
    }
    /// This keymap with `function`'s bindings in `mode` replaced; the
    /// defaults when they equal them.
    pub fn with(
        &self,
        function: Function,
        mode: Mode,
        bindings: Vec<Binding>,
    ) -> Result<Self, String> {
        let mut unique: Vec<Binding> = Vec::new();
        for b in bindings {
            if !unique.contains(&b) {
                unique.push(b);
            }
        }
        let bindings = unique;
        let mut custom = self.custom.clone();
        custom.retain(|(at, _)| *at != (function, mode));
        if bindings != function.defaults(mode) {
            custom.push(((function, mode), bindings));
        }
        Self::new(custom)
    }
    /// This keymap with `function` back to its defaults in both modes.
    pub fn reset(&self, function: Function) -> Result<Self, String> {
        let mut custom = self.custom.clone();
        custom.retain(|((f, _), _)| *f != function);
        Self::new(custom)
    }
    /// Feeds one key: `pending` holds the keys of a sequence begun.
    pub fn feed(
        &self,
        pending: &mut Vec<KeyEvent>,
        mode: Mode,
        cell: Cell,
        event: KeyEvent,
    ) -> Step {
        let mut sequence: Vec<Key> = pending.iter().map(|e| Key::press(*e)).collect();
        sequence.push(Key::press(event));
        let candidates = || {
            self.bindings.iter().filter(|b| {
                b.mode == mode
                    && b.keys.len() >= sequence.len()
                    && b.keys.iter().zip(&sequence).all(|(k, s)| k.takes(*s))
                    && b.applies(cell)
            })
        };
        if let Some(done) = candidates().find(|b| b.keys.len() == sequence.len()) {
            pending.clear();
            let digit = done
                .keys
                .last()
                .and_then(|k| k.digit(*sequence.last().unwrap()));
            return Step::Run(done.function, digit);
        }
        if candidates().next().is_some() {
            pending.push(event);
            return Step::Pending;
        }
        if pending.is_empty() {
            Step::Pass
        } else {
            Step::Flush(std::mem::take(pending))
        }
    }
    /// The binding of `function` to name in `mode` in `cell`: one that
    /// applies there, else any.
    pub fn first(&self, function: Function, mode: Mode, cell: Option<Cell>) -> Option<&Binding> {
        let bindings = self.of(function, mode);
        cell.and_then(|cell| bindings.iter().find(|b| b.applies(cell)).copied())
            .or_else(|| bindings.first().copied())
    }
    /// Whether a lone press of `event` cancels in `mode`.
    pub fn cancels(&self, mode: Mode, event: KeyEvent) -> bool {
        let press = Key::press(event);
        self.of(Function::Cancel, mode)
            .iter()
            .any(|b| b.keys.len() == 1 && b.keys[0].takes(press))
    }
    /// Something opens the Keys screen and something leaves fff everywhere
    /// in the results, and something leaves the marked list and the query
    /// for them (Escape leaves Insert mode), so that no keymap can lock you
    /// in.
    fn check_escape(&self) -> Result<(), String> {
        let leaves = |function: Function, mode: Mode, cell: Cell| {
            self.bindings
                .iter()
                .any(|b| b.function == function && b.mode == mode && b.applies(cell))
        };
        for &program in Program::ALL {
            for &highlight in Highlight::ALL {
                let cell = Cell {
                    program,
                    focus: Marks,
                    highlight,
                };
                if !leaves(Function::ToggleMarkedList, Mode::Normal, cell) {
                    return Err(format!(
                        "nothing would leave the marked list in {}; toggle_marked_list needs a key there",
                        cell.describe()
                    ));
                }
                let cell = Cell {
                    focus: Query,
                    ..cell
                };
                if !leaves(Function::ToggleQueryFocus, Mode::Normal, cell) {
                    return Err(format!(
                        "nothing would leave the query in {}; toggle_query_focus needs a Normal-mode key there",
                        cell.describe()
                    ));
                }
            }
            for &highlight in Highlight::ALL {
                let cell = Cell {
                    program,
                    focus: Results,
                    highlight,
                };
                let bound = |f: Function, modes: &[Mode]| {
                    self.bindings
                        .iter()
                        .any(|b| b.function == f && modes.contains(&b.mode) && b.applies(cell))
                };
                if !bound(Function::ShowKeys, &Mode::ALL) {
                    return Err(format!(
                        "nothing would open the Keys screen in {}; show_keys needs a key there",
                        cell.describe()
                    ));
                }
                if !bound(Function::Cancel, &Mode::ALL) && !bound(Function::Quit, &[Mode::Normal]) {
                    return Err(format!(
                        "nothing would leave fff in {}; cancel or quit needs a key there",
                        cell.describe()
                    ));
                }
            }
        }
        Ok(())
    }
    /// Reads `keys.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut custom = Vec::new();
        for (section, entries) in &table {
            let mode = Mode::ALL
                .into_iter()
                .find(|m| m.name() == section)
                .ok_or_else(|| format!("unknown table [{section}]; use [insert] or [normal]"))?;
            let entries = entries
                .as_table()
                .ok_or_else(|| format!("{section} must be a table"))?;
            for (name, value) in entries {
                let function = Function::named(name)
                    .ok_or_else(|| format!("{section}.{name}: no such function"))?;
                let bindings = read_bindings(function, mode, value)
                    .map_err(|e| format!("{section}.{name}: {e}"))?;
                custom.push(((function, mode), bindings));
            }
        }
        Self::new(custom)
    }
    /// Reads the file at `path`; no file is the defaults.
    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }
    /// Writes the custom bindings to `path`, keeping its comments and
    /// layout where they still apply.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        let before = match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut document: toml_edit::DocumentMut = before
            .as_deref()
            .unwrap_or_default()
            .parse()
            .map_err(|e| format!("{}: {e}", path.display()))?;
        for mode in Mode::ALL {
            let entries: Vec<_> = self
                .custom
                .iter()
                .filter(|((_, m), _)| *m == mode)
                .collect();
            if entries.is_empty() {
                document.remove(mode.name());
                continue;
            }
            let table = document
                .entry(mode.name())
                .or_insert(toml_edit::table())
                .as_table_mut()
                .ok_or_else(|| format!("{}: {} is not a table", path.display(), mode.name()))?;
            let keep: Vec<String> = entries.iter().map(|((f, _), _)| f.name().into()).collect();
            let stale: Vec<String> = table
                .iter()
                .map(|(k, _)| k.to_string())
                .filter(|k| !keep.contains(k))
                .collect();
            for name in stale {
                table.remove(&name);
            }
            for ((function, _), bindings) in entries {
                let value = write_bindings(bindings);
                let same = table
                    .get(function.name())
                    .and_then(|item| item.as_value())
                    .is_some_and(|v| v.to_string().trim() == value.to_string().trim());
                if !same {
                    table.insert(function.name(), toml_edit::Item::Value(value));
                }
            }
        }
        // Whole or not at all: a keys.toml cut short would lose every key.
        crate::workspaces::atomic_write(path, before.as_deref(), &document.to_string())
            .map_err(|e| format!("{}: {e}", path.display()))
    }
}
/// Rules a binding's keys follow wherever it applies.
fn check_keys(binding: &Binding) -> Result<(), String> {
    match problem(binding.mode, &binding.keys) {
        Some(why) => Err(format!(
            "{}.{}: {} {why}",
            binding.mode.name(),
            binding.function.name(),
            notation(&binding.keys)
        )),
        None => Ok(()),
    }
}
/// Why `keys` cannot be bound in `mode`, if they cannot: a key typed as
/// text (or Escape) cannot start an Insert-mode key, and Normal mode keeps
/// the ways into Insert mode and Neovim's command line.
pub fn problem(mode: Mode, keys: &[Key]) -> Option<&'static str> {
    let first = *keys.first()?;
    let plain = |c: char| first == Key::press(KeyEvent::new(KeyCode::Char(c), M::NONE));
    match mode {
        Mode::Insert if first.printable() => Some(
            "starts with a key typed as text; Insert-mode keys start with Ctrl, Alt or a named key",
        ),
        Mode::Insert if first == Key::press(KeyEvent::new(KeyCode::Esc, M::NONE)) => {
            Some("starts with Escape, which leaves Insert mode")
        }
        Mode::Normal if ['i', 'a', 'I', 'A', ':'].into_iter().any(plain) => {
            Some("starts with a key that enters Insert mode or Neovim's command line")
        }
        _ => None,
    }
}
pub fn clash(a: &Binding, b: &Binding, at: Scope) -> String {
    let at = at.describe();
    format!(
        "{} {}: {} and {} both apply{}{}",
        a.mode.name(),
        notation(&a.keys),
        a.function.name(),
        b.function.name(),
        if at.is_empty() { "" } else { " in " },
        if at.is_empty() {
            "everywhere"
        } else {
            at.as_str()
        }
    )
}
/// One entry of `keys.toml`: keys, `"none"`, or a list of keys and tables
/// `{ keys = "<CR>", program = [...], focus = [...], highlight = [...] }`.
fn read_bindings(
    function: Function,
    mode: Mode,
    value: &toml::Value,
) -> Result<Vec<Binding>, String> {
    let items = match value {
        toml::Value::Array(items) => items.clone(),
        other => vec![other.clone()],
    };
    let mut bindings = Vec::new();
    for item in items {
        let (text, scope) = match &item {
            toml::Value::String(text) => (text.clone(), Scope::ALL),
            toml::Value::Table(table) => {
                let text = table
                    .get("keys")
                    .and_then(|v| v.as_str())
                    .ok_or("a table needs keys = \"...\"")?
                    .to_string();
                for name in table.keys() {
                    if !["keys", "program", "focus", "highlight"].contains(&name.as_str()) {
                        return Err(format!(
                            "unknown field {name:?}; use keys, program, focus or highlight"
                        ));
                    }
                }
                fn set<D: Dimension>(table: &toml::Table, field: &str) -> Result<u16, String> {
                    let Some(value) = table.get(field) else {
                        return Ok(D::every());
                    };
                    let names = value
                        .as_array()
                        .ok_or_else(|| format!("{field} must be a list"))?;
                    let mut bits = 0;
                    for name in names {
                        let name = name.as_str().unwrap_or_default();
                        let value = D::named(name).ok_or_else(|| {
                            format!(
                                "{field}: unknown {name:?}; use {}",
                                D::ALL
                                    .iter()
                                    .map(|v| v.name())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        })?;
                        bits |= value.bit();
                    }
                    if bits == 0 {
                        return Err(format!("{field} must name something"));
                    }
                    Ok(bits)
                }
                let scope = Scope {
                    programs: set::<Program>(table, "program")?,
                    focuses: set::<Focus>(table, "focus")?,
                    highlights: set::<Highlight>(table, "highlight")?,
                };
                (text, scope)
            }
            _ => return Err("expected keys, \"none\" or a list".into()),
        };
        if text.is_empty() || text == "none" {
            continue;
        }
        bindings.push(Binding {
            function,
            mode,
            keys: parse(&text)?,
            scope,
        });
    }
    Ok(bindings)
}
fn write_bindings(bindings: &[Binding]) -> toml_edit::Value {
    if bindings.is_empty() {
        return "none".into();
    }
    let item = |b: &Binding| -> toml_edit::Value {
        let keys = notation(&b.keys);
        if b.scope == Scope::ALL {
            return keys.into();
        }
        let mut table = toml_edit::InlineTable::new();
        table.insert("keys", keys.into());
        fn names<D: Dimension>(bits: u16) -> Option<toml_edit::Value> {
            (bits != D::every())
                .then(|| toml_edit::Value::Array(Scope::values::<D>(bits).map(D::name).collect()))
        }
        if let Some(v) = names::<Program>(b.scope.programs) {
            table.insert("program", v);
        }
        if let Some(v) = names::<Focus>(b.scope.focuses) {
            table.insert("focus", v);
        }
        if let Some(v) = names::<Highlight>(b.scope.highlights) {
            table.insert("highlight", v);
        }
        toml_edit::Value::InlineTable(table)
    };
    if let [one] = bindings
        && one.scope == Scope::ALL
    {
        return item(one);
    }
    toml_edit::Value::Array(bindings.iter().map(item).collect())
}
/// `keys.toml` beside the configuration file.
pub fn path_beside(config: &Path) -> std::path::PathBuf {
    config.with_file_name("keys.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    fn press(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), M::NONE)
    }
    fn cell(program: Program, highlight: Highlight) -> Cell {
        Cell {
            program,
            focus: Results,
            highlight,
        }
    }
    fn run(keymap: &Keymap, mode: Mode, cell: Cell, keys: &[KeyEvent]) -> Step {
        // Insert mode types into the query, as the app reads it.
        let cell = match mode {
            Mode::Insert => Cell {
                focus: Query,
                ..cell
            },
            Mode::Normal => cell,
        };
        let mut pending = Vec::new();
        let mut last = Step::Pass;
        for key in keys {
            last = keymap.feed(&mut pending, mode, cell, *key);
        }
        last
    }
    #[test]
    fn notation_round_trips_and_names_keys() {
        for text in [
            "<M-v>",
            "<CR>",
            "gg",
            "g/",
            "<C-#>",
            "<#>",
            "<S-Space>",
            "<M-P>",
            "<lt>",
            "<F5>",
            "<M-g>u",
        ] {
            let keys = parse(text).unwrap();
            assert_eq!(notation(&keys), text, "{text}");
        }
        assert_eq!(display(&parse("<M-v>").unwrap()), "Alt+v");
        assert_eq!(display(&parse("<M-g>u").unwrap()), "Alt+g u");
        assert_eq!(display(&parse("gg").unwrap()), "gg");
        assert_eq!(display(&parse("<C-#>").unwrap()), "Ctrl+0–9");
        assert_eq!(display(&parse("<M-Right>").unwrap()), "Alt+→");
        assert!(parse("<Nope>").is_err());
        assert!(parse("g<#>").is_err());
        // A kitty Shift+g is G.
        let kitty = KeyEvent::new(KeyCode::Char('g'), M::SHIFT);
        assert_eq!(Key::press(kitty), parse("G").unwrap()[0]);
        let alt_shift = KeyEvent::new(KeyCode::Char('p'), M::ALT | M::SHIFT);
        assert_eq!(Key::press(alt_shift), parse("<M-P>").unwrap()[0]);
    }
    #[test]
    fn every_key_pressed_is_written_as_it_reads_back() {
        // What the Keys screen may capture is saved in keys.toml.
        let mut codes: Vec<KeyCode> = "aZ0<>-|\" é"
            .chars()
            .map(KeyCode::Char)
            .chain([
                KeyCode::Insert,
                KeyCode::F(5),
                KeyCode::F(13),
                KeyCode::BackTab,
            ])
            .collect();
        codes.extend([KeyCode::Enter, KeyCode::Esc, KeyCode::Tab, KeyCode::Delete]);
        let all = [M::NONE, M::SHIFT, M::CONTROL, M::ALT, M::CONTROL | M::SHIFT];
        for code in codes {
            for &m in &all {
                let key = Key::press(KeyEvent::new(code, m));
                let text = notation(&[key]);
                assert_eq!(parse(&text), Ok(vec![key]), "{code:?} {m:?} as {text}");
                assert!(writable(&[key]), "{text}");
            }
        }
        // As in Vim, `>` after the modifiers is the key: `<M->>` is Alt+>.
        assert_eq!(
            parse("<M->>").unwrap(),
            [Key::Press(KeyCode::Char('>'), M::ALT)]
        );
        assert_eq!(parse("<C-M->>x").unwrap().len(), 2);
        // Alt or Ctrl with `#` reads as any digit: it cannot be written.
        let alt_hash = Key::press(KeyEvent::new(KeyCode::Char('#'), M::ALT));
        assert!(!writable(&[alt_hash]));
        assert_eq!(
            parse("<C-S-A>").unwrap(),
            [Key::Press(KeyCode::Char('A'), M::CONTROL)]
        );
        // A sequence spelling "none" would read back as no key.
        let none: Vec<Key> = "none"
            .chars()
            .map(|c| parse(&c.to_string()).unwrap()[0])
            .collect();
        assert!(!writable(&none));
        // A key without a name in the notation is refused.
        assert!(!writable(&[Key::Press(KeyCode::CapsLock, M::NONE)]));
    }
    #[test]
    fn tab_switches_queries_and_space_m_and_shift_m_mark() {
        let keymap = Keymap::default();
        let tab = KeyEvent::new(KeyCode::Tab, M::NONE);
        let shift_tab = KeyEvent::new(KeyCode::BackTab, M::SHIFT);
        for program in <Program as Dimension>::ALL.iter().copied() {
            let here = cell(program, H::File);
            let query = Cell {
                focus: Query,
                ..here
            };
            // Tab switches queries from the query; from the results it
            // goes up into the query first.
            for (mode, cell, function) in [
                (Mode::Insert, query, Function::SwitchQuery),
                (Mode::Normal, query, Function::SwitchQuery),
                (Mode::Normal, here, Function::ToggleQueryFocus),
            ] {
                assert_eq!(
                    run(&keymap, mode, cell, &[tab]),
                    Step::Run(function, None),
                    "{program:?} {mode:?} {:?}",
                    cell.focus
                );
            }
            // Shift+Tab toggles the query's focus, in Insert mode too.
            for (mode, cell) in [
                (Mode::Insert, query),
                (Mode::Normal, query),
                (Mode::Normal, here),
            ] {
                assert_eq!(
                    run(&keymap, mode, cell, &[shift_tab]),
                    Step::Run(Function::ToggleQueryFocus, None),
                    "{program:?} {mode:?} {:?}",
                    cell.focus
                );
            }
            let normal = |key| run(&keymap, Mode::Normal, here, &[key]);
            assert_eq!(normal(press(' ')), Step::Run(Function::ToggleMark, None));
            assert_eq!(normal(press('m')), Step::Run(Function::MarkAll, None));
            let shift_m = KeyEvent::new(KeyCode::Char('M'), M::SHIFT);
            assert_eq!(normal(shift_m), Step::Run(Function::InvertMarks, None));
        }
    }
    #[test]
    fn enter_does_one_thing_in_each_place() {
        let keymap = Keymap::default();
        let enter = KeyEvent::new(KeyCode::Enter, M::NONE);
        for mode in Mode::ALL {
            let enter = |program, highlight| run(&keymap, mode, cell(program, highlight), &[enter]);
            let runs = |function| Step::Run(function, None);
            // Into a folder where fff stays; out of fff with a file.
            assert_eq!(enter(Roam, H::Dir), Step::Pass, "{mode:?}");
            assert_eq!(enter(Roam, H::File), runs(Function::OpenAndQuit));
            assert_eq!(enter(Window, H::Dir), runs(Function::GoToHighlighted));
            assert_eq!(enter(Window, H::File), runs(Function::OpenAndQuit));
            assert_eq!(enter(Seek, H::File), runs(Function::QuitIntoHighlighted));
            for highlight in [H::Dir, H::File] {
                assert_eq!(
                    enter(Pick, highlight),
                    runs(Function::PrintHighlightedAndMarks)
                );
            }
            assert_eq!(
                enter(ExplorerOpen, H::File),
                runs(Function::PrintHighlighted)
            );
            assert_eq!(
                enter(ExplorerMultiple, H::File),
                runs(Function::PrintHighlightedAndMarks)
            );
            assert_eq!(
                enter(ExplorerFolderMultiple, H::Dir),
                runs(Function::GoToHighlighted)
            );
            for highlight in [H::Dir, H::File, H::None] {
                assert_eq!(enter(ExplorerSave, highlight), runs(Function::SaveHere));
            }
        }
        let normal =
            |program, highlight| run(&keymap, Mode::Normal, cell(program, highlight), &[enter]);
        assert_eq!(normal(Pick, H::None), Step::Run(Function::PrintHere, None));
        assert_eq!(normal(Window, H::None), Step::Pass);
    }
    #[test]
    fn sequences_wait_and_flush_and_digits_carry() {
        let keymap = Keymap::default();
        let here = cell(Roam, H::File);
        let mut pending = Vec::new();
        assert_eq!(
            keymap.feed(&mut pending, Mode::Normal, here, press('g')),
            Step::Pending
        );
        assert_eq!(
            keymap.feed(&mut pending, Mode::Normal, here, press('g')),
            Step::Run(Function::HighlightFirst, None)
        );
        assert_eq!(
            keymap.feed(&mut pending, Mode::Normal, here, press('g')),
            Step::Pending
        );
        assert_eq!(
            keymap.feed(&mut pending, Mode::Normal, here, press('U')),
            Step::Flush(vec![press('g')])
        );
        assert_eq!(
            run(&keymap, Mode::Normal, here, &[press('3')]),
            Step::Run(Function::HighlightRow, Some(3))
        );
        let ctrl_alt = KeyEvent::new(KeyCode::Char('3'), M::CONTROL | M::ALT);
        assert_eq!(
            run(&keymap, Mode::Normal, here, &[ctrl_alt]),
            Step::Run(Function::SearchToDepth, Some(3))
        );
        let alt = KeyEvent::new(KeyCode::Char('0'), M::CONTROL);
        assert_eq!(
            run(&keymap, Mode::Insert, here, &[alt]),
            Step::Run(Function::OpenRow, Some(0))
        );
        // Seek lists only files: gd does nothing there.
        let gd = [press('g'), press('d')];
        assert_ne!(
            run(&keymap, Mode::Normal, cell(Seek, H::File), &gd),
            Step::Run(Function::ToggleDirectories, None)
        );
        assert_eq!(
            run(&keymap, Mode::Normal, here, &gd),
            Step::Run(Function::ToggleDirectories, None)
        );
        // Typing is text.
        assert_eq!(run(&keymap, Mode::Insert, here, &[press('q')]), Step::Pass);
    }
    #[test]
    fn keys_toml_replaces_scopes_and_refuses_clashes_and_lockouts() {
        let keymap = Keymap::parse(
            "[normal]\ngo_to_highlighted = ['l', { keys = '<CR>', program = ['roam'], highlight = ['dir'] }]\nquit = 'Q'\n",
        )
        .unwrap();
        let enter = KeyEvent::new(KeyCode::Enter, M::NONE);
        // The custom <CR> takes roam's directories from quit_into_dir.
        assert_eq!(
            run(&keymap, Mode::Normal, cell(Roam, H::Dir), &[enter]),
            Step::Run(Function::GoToHighlighted, None)
        );
        assert_eq!(
            run(&keymap, Mode::Normal, cell(Roam, H::File), &[press('q')]),
            Step::Pass
        );
        // Two custom bindings may not share a key where both apply.
        assert!(Keymap::parse("[normal]\nquit = 'Q'\ntoggle_hidden = 'Q'\n").is_err());
        // Nor can the Keys screen or every way out go.
        assert!(
            Keymap::parse("[insert]\nshow_keys = 'none'\n[normal]\nshow_keys = 'none'\n").is_err()
        );
        assert!(
            Keymap::parse("[insert]\ncancel = 'none'\n[normal]\ncancel = 'none'\nquit = 'none'\n")
                .is_err()
        );
        assert!(Keymap::parse("[insert]\ncancel = 'none'\n[normal]\ncancel = 'none'\n").is_ok());
        assert!(Keymap::parse("[normal]\ntoggle_marked_list = 'none'\n").is_err());
        // Typing and the way into Insert mode stay.
        assert!(Keymap::parse("[insert]\nquit = 'q'\n").is_err());
        assert!(Keymap::parse("[normal]\ntoggle_hidden = 'i'\n").is_err());
        assert!(Keymap::parse("[insert]\ntoggle_hidden = '<Esc>'\n").is_err());
        assert!(Keymap::parse("[normal]\nnope = 'x'\n").is_err());
        assert!(
            Keymap::parse("[normal]\ngo_to_highlighted = { keys = 'L', program = ['seek'] }\n")
                .is_err()
        );
    }
    #[test]
    fn keys_toml_names_focuses_and_programs_as_the_keys_screen_does() {
        let keymap = Keymap::parse(
            "[normal]\nquit = ['Q', { keys = 'q', program = ['explorer-open-multiple'], focus = ['marks'] }]\n",
        )
        .unwrap();
        let quit: Vec<Binding> = keymap
            .of(Function::Quit, Mode::Normal)
            .into_iter()
            .cloned()
            .collect();
        let saved = write_bindings(&quit).to_string();
        assert!(saved.contains("focus = [\"marks\"]"), "{saved}");
        assert!(saved.contains("explorer-open-multiple"), "{saved}");
        assert!(Keymap::parse("[normal]\nquit = { keys = 'Q', list = ['marks'] }\n").is_err());
        assert!(
            Keymap::parse("[normal]\nquit = { keys = 'Q', program = ['explorer-multiple'] }\n")
                .is_err()
        );
    }
    #[test]
    fn default_keys_typed_as_text_leave_the_query_in_focus_to_neovim() {
        let keymap = Keymap::default();
        let query = Cell {
            focus: Query,
            ..cell(Roam, H::Dir)
        };
        let esc = KeyEvent::new(KeyCode::Esc, M::NONE);
        let enter = KeyEvent::new(KeyCode::Enter, M::NONE);
        let tab = KeyEvent::new(KeyCode::Tab, M::NONE);
        let normal = |cell, keys: &[KeyEvent]| run(&keymap, Mode::Normal, cell, keys);
        // Typing gives the query the focus; Escape there takes it back,
        // then clears.
        assert_eq!(normal(cell(Roam, H::Dir), &[press('i')]), Step::Pass);
        assert_eq!(
            normal(query, &[esc]),
            Step::Run(Function::ToggleQueryFocus, None)
        );
        assert_eq!(
            normal(cell(Roam, H::Dir), &[esc]),
            Step::Run(Function::ClearQuery, None)
        );
        // By default h, x, v, `.`, the digits and `g` sequences edit the
        // query there.
        for c in ['h', 'l', 'x', 'v', 'u', 'p', '.', '3', ' '] {
            assert_eq!(normal(query, &[press(c)]), Step::Pass, "{c}");
        }
        assert_eq!(
            normal(query, &[press('g'), press('u')]),
            Step::Flush(vec![press('g')])
        );
        // But - and + change the depth from there too, q quits and gc edits
        // the configuration.
        assert_eq!(
            normal(query, &[press('q')]),
            Step::Run(Function::Quit, None)
        );
        assert_eq!(
            normal(query, &[press('-')]),
            Step::Run(Function::SearchShallower, None)
        );
        assert_eq!(
            normal(query, &[press('+')]),
            Step::Run(Function::SearchDeeper, None)
        );
        assert_eq!(
            normal(query, &[press('g'), press('c')]),
            Step::Run(Function::EditConfig, None)
        );
        // j and k go down into the results, to the first or last row.
        assert_eq!(
            normal(query, &[press('j')]),
            Step::Run(Function::HighlightFirst, None)
        );
        assert_eq!(
            normal(query, &[press('k')]),
            Step::Run(Function::HighlightLast, None)
        );
        assert_eq!(
            normal(cell(Roam, H::Dir), &[press('j')]),
            Step::Run(Function::HighlightNext, None)
        );
        // Other keys still act on the results below it.
        let file = Cell {
            highlight: H::File,
            ..query
        };
        assert_eq!(
            normal(file, &[enter]),
            Step::Run(Function::OpenAndQuit, None)
        );
        assert_eq!(
            normal(query, &[tab]),
            Step::Run(Function::SwitchQuery, None)
        );
        // A key you bind comes before Neovim's wherever its focus says.
        let keymap = Keymap::parse("[normal]\ntrash = 'x'\n").unwrap();
        assert_eq!(
            run(&keymap, Mode::Normal, query, &[press('x')]),
            Step::Run(Function::Trash, None)
        );
        let keymap =
            Keymap::parse("[normal]\ntrash = { keys = 'x', focus = ['results', 'marks'] }\n")
                .unwrap();
        assert_eq!(run(&keymap, Mode::Normal, query, &[press('x')]), Step::Pass);
        // Something must leave the query.
        let only = |keys: &str| {
            format!("[normal]\ntoggle_query_focus = {{ keys = '{keys}', focus = ['results'] }}\n")
        };
        assert!(Keymap::parse(&only("/")).is_err());
        assert!(Keymap::parse("[normal]\ntoggle_query_focus = '/'\n").is_ok());
    }
    #[test]
    fn saving_writes_only_what_differs() {
        let dir = std::env::temp_dir().join(format!("fff-keymap-{}", std::process::id()));
        let path = dir.join("keys.toml");
        let _ = std::fs::remove_dir_all(&dir);
        let keymap = Keymap::default();
        let ours = parse("<M-x>").unwrap();
        let next = keymap
            .with(
                Function::ToggleHidden,
                Mode::Insert,
                vec![Binding {
                    function: Function::ToggleHidden,
                    mode: Mode::Insert,
                    keys: ours,
                    scope: Scope::of(&[Roam], &[], &[]),
                }],
            )
            .unwrap();
        next.save(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("hidden"), "{text}");
        assert!(text.contains("roam"), "{text}");
        let again = Keymap::load(&path).unwrap();
        assert_eq!(again.of(Function::ToggleHidden, Mode::Insert).len(), 1);
        // Back to the defaults, the entry goes.
        again
            .reset(Function::ToggleHidden)
            .unwrap()
            .save(&path)
            .unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("hidden"));
        // A keys.toml linked from elsewhere (a dotfiles folder) is written
        // whole in its place, keeping the link and its mode.
        use std::os::unix::fs::PermissionsExt;
        let real = dir.join("dotfiles-keys.toml");
        std::fs::rename(&path, &real).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o640)).unwrap();
        std::os::unix::fs::symlink(&real, &path).unwrap();
        next.save(&path).unwrap();
        assert!(path.symlink_metadata().unwrap().is_symlink());
        assert!(std::fs::read_to_string(&real).unwrap().contains("hidden"));
        let mode = std::fs::metadata(&real).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640);
        let leftovers = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(leftovers, 2, "only the file and its link");
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn scopes_meet_and_subtract_into_boxes() {
        let a = Scope::of(&[Roam, Window], &[], &[]);
        let b = Scope::of(&[Window], &[], &[H::Dir]);
        let left = a.minus(b);
        assert!(left.iter().all(|s| s.meet(b).is_none()));
        let cells: usize = left
            .iter()
            .map(|s| {
                s.programs.count_ones() as usize
                    * s.focuses.count_ones() as usize
                    * s.highlights.count_ones() as usize
            })
            .sum();
        assert_eq!(cells, 2 * 3 * 3 - 3);
        assert_eq!(b.describe(), "window · dir");
        assert_eq!(Scope::of(&EXPLORERS, &[], &[]).describe(), "explorer-*");
    }
    #[test]
    fn a_function_works_in_the_same_places_in_both_modes() {
        // Insert mode types into the query: there both modes agree.
        for &function in Function::ALL {
            for &program in Program::ALL {
                for &highlight in Highlight::ALL {
                    let here = Cell {
                        program,
                        focus: Query,
                        highlight,
                    };
                    assert_eq!(
                        function.works(Mode::Insert, here),
                        function.works(Mode::Normal, here),
                        "{} in {}",
                        function.name(),
                        here.describe()
                    );
                }
            }
        }
        // Where a function would mean nothing, it does not work.
        let works =
            |f: Function, program, highlight| f.works(Mode::Normal, cell(program, highlight));
        assert!(!works(Function::QuitIntoHighlighted, Pick, H::Dir));
        assert!(works(Function::QuitIntoHighlighted, Seek, H::None));
        assert!(!works(Function::PrintMarks, Roam, H::File));
        assert!(works(Function::PrintMarks, ExplorerFolderMultiple, H::None));
        assert!(!works(Function::PrintHighlighted, ExplorerFolder, H::File));
        assert!(!works(Function::QuitHere, Pick, H::None));
        assert!(works(Function::PrintHere, Pick, H::None));
        assert!(!works(Function::OpenInBackground, ExplorerOpen, H::File));
    }
    #[test]
    fn defaults_never_clash() {
        let keymap = Keymap::default();
        let all = keymap.bindings();
        for (i, a) in all.iter().enumerate() {
            for b in &all[i + 1..] {
                if a.function != b.function {
                    assert!(a.clashes(b).is_empty(), "{}", clash(a, b, a.clashes(b)[0]));
                }
            }
        }
    }
    #[test]
    fn every_default_binding_applies_somewhere() {
        for &f in Function::ALL {
            for mode in Mode::ALL {
                for b in f.defaults(mode) {
                    assert!(!b.reach().is_empty(), "{} {}", f.name(), notation(&b.keys));
                }
            }
        }
    }
}
