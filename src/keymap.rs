//! Key bindings. Everything a key can do is a [`Function`]: what it does,
//! where it works (fixed by design) and its default keys. A [`Binding`] puts
//! a key sequence on a function in Insert or Normal mode, within a [`Scope`]
//! of where it applies. Places are the cells of a product: the program (the
//! command fff runs as), the list in focus and what is highlighted. Two
//! bindings of one mode share keys only where their scopes do not meet, so
//! a key does exactly one thing wherever it is pressed.
//!
//! `keys.toml` replaces a function's bindings in a mode; the Keys screen (F1)
//! edits them. A keymap that would lock you in (nothing opens the Keys screen
//! or leaves fff somewhere) is refused.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers as M};
use std::path::Path;

/// Insert mode types into the query; Normal mode is for commands.
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
/// The list a key acts on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
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
        Self::ExplorerSave,
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
            Self::ExplorerSave => "explorer-save",
        }
    }
}
impl Dimension for Place {
    const ALL: &'static [Self] = &[Self::Results, Self::Marks];
    fn name(self) -> &'static str {
        match self {
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
    pub place: Place,
    pub highlight: Highlight,
}
impl Cell {
    pub fn describe(self) -> String {
        format!(
            "{} · {} · {}",
            self.program.name(),
            self.place.name(),
            self.highlight.name()
        )
    }
}

/// A box in the product: a set of values in each dimension.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    pub programs: u16,
    pub places: u16,
    pub highlights: u16,
}
impl Scope {
    pub const ALL: Self = Self {
        programs: (1 << <Program as Dimension>::ALL.len()) - 1,
        places: 0b11,
        highlights: 0b111,
    };
    /// The box of these values; an empty list is the whole dimension.
    pub fn of(programs: &[Program], places: &[Place], highlights: &[Highlight]) -> Self {
        fn bits<D: Dimension>(values: &[D]) -> u16 {
            if values.is_empty() {
                D::every()
            } else {
                values.iter().fold(0, |bits, v| bits | v.bit())
            }
        }
        Self {
            programs: bits(programs),
            places: bits(places),
            highlights: bits(highlights),
        }
    }
    pub fn contains(self, cell: Cell) -> bool {
        self.programs & cell.program.bit() != 0
            && self.places & cell.place.bit() != 0
            && self.highlights & cell.highlight.bit() != 0
    }
    pub fn is_empty(self) -> bool {
        self.programs == 0 || self.places == 0 || self.highlights == 0
    }
    /// Where both apply, if anywhere.
    pub fn meet(self, other: Self) -> Option<Self> {
        let meet = Self {
            programs: self.programs & other.programs,
            places: self.places & other.places,
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
                places: self.places & !meet.places,
                ..self
            },
            Self {
                programs: meet.programs,
                places: meet.places,
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
            // The four explorer programs, named once.
            let explorers = names.iter().filter(|n| n.starts_with("explorer-")).count();
            if explorers == 4 {
                names.retain(|n| !n.starts_with("explorer-"));
                names.push("explorer-*");
            }
            Some(names.join(", "))
        }
        [
            part::<Program>(self.programs),
            part::<Place>(self.places),
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
    HighlightPrevious "highlight_previous" Highlight "Highlight the previous row of the list in focus: the results, or the marked list.";
    HighlightNext "highlight_next" Highlight "Highlight the next row of the list in focus: the results, or the marked list.";
    HighlightPreviousPage "highlight_previous_page" Highlight "Highlight the row one page up, scrolling the results a page back.";
    HighlightNextPage "highlight_next_page" Highlight "Highlight the row one page down, scrolling the results a page on.";
    HighlightFirst "highlight_first" Highlight "Highlight the first row of the list in focus.";
    HighlightLast "highlight_last" Highlight "Highlight the last row of the list in focus.";
    GoToParent "go_to_parent" GoTo "Go to the parent of the current directory. The query stays, so the results there are filtered by it too.";
    GoToHighlighted "go_to_highlighted" GoTo "Go to the highlighted entry: into it when it is a directory; when it is a file, to the directory holding it, with the file highlighted and the query cleared so that it shows.";
    GoToPath "go_to_path" GoTo "Go to a path typed in a prompt. Words typed there also suggest bookmarks and zoxide's frequent directories to go to.";
    GoToTrash "go_to_trash" GoTo "Go to the trash and browse it like any directory; restore puts entries back. Pressed again in the trash, go to the next filesystem's trash.";
    GoBack "go_back" GoTo "Go back to the directory this tab was in before, like a browser's back button.";
    GoForward "go_forward" GoTo "Go forward again to the directory go_back left. Ctrl+i needs a terminal that tells it apart from Tab.";
    GoToNextTab "go_to_next_tab" GoTo "Go to the next of the three tabs. Each tab keeps its own directory and history; the marks are shared by all three.";
    GoToPreviousTab "go_to_previous_tab" GoTo "Go to the previous of the three tabs.";
    OpenInBackground "open_in_background" Open "Open the highlighted file with its opener (see [open] in the configuration) without waiting for it: fff stays as it is.";
    OpenInTerminal "open_in_terminal" Open "Open the current or marked files with their openers inside this terminal, one opener after another, then come back to fff.";
    OpenAndQuit "open_and_quit" Open "Open the highlighted file with its opener and quit fff. A terminal opener takes fff's place in this terminal.";
    OpenRow "open_row" Open "Do on visible row # what l does on the highlighted one: go into a directory, or open a file in the background. 0 is the last visible row.";
    ConfirmRow "confirm_row" Open "Do on visible row # what Enter does there by default. 0 is the last visible row.";
    EditFiles "edit_files" Open "Edit the current or marked files in $EDITOR, in this terminal, then come back to fff.";
    Run "run" Open "Run a program on the current or marked paths: type its command line, review it, and come back to fff when it ends.";
    RunAndQuit "run_and_quit" Open "Run a program on the current or marked paths in place of fff, which quits; the program's exit status becomes fff's.";
    PrintHighlighted "print_highlighted" Finish "Print the highlighted path and quit, ignoring the marks: the answer of fff pick, or of a file dialog (fff explorer).";
    PrintHighlightedAndMarks "print_highlighted_and_marks" Finish "Print the marked paths and the highlighted file, then quit: the answer where you may choose several paths.";
    PrintMarks "print_marks" Finish "Print the marked paths and quit, where the answer can be several paths (fff pick, and dialogs for several files). Elsewhere it says why it does nothing.";
    SaveHere "save_here" Finish "Save in the current directory, under the name typed in the query or else the suggested one (fff explorer save).";
    SaveOver "save_over" Finish "Save over the highlighted file, after asking whether to replace it (fff explorer save).";
    QuitIntoHighlighted "quit_into_highlighted" Finish "Quit into the highlighted directory, or the directory holding the highlighted file; with nothing highlighted, into the current directory. Your shell follows (fff roam, with the shell integration); fff pick prints it.";
    QuitHere "quit_here" Finish "Quit into the current directory, ignoring the marks: your shell follows in fff roam, fff pick prints it, and a file dialog answers with it. In a window of its own, fff becomes a shell there.";
    Quit "quit" Finish "Quit without choosing anything: back where you started (fff roam), printing nothing (fff pick, fff seek), or cancelling the dialog (fff explorer).";
    Cancel "cancel" Finish "Cancel: quit at once, printing nothing, from anywhere in fff. While a file operation runs, stop it instead.";
    ToggleMark "toggle_mark" Marks "Toggle the highlighted row's mark. In the results, the next row is highlighted after (with a contents query, the next file's), so pressing it again marks on down.";
    MarkAll "mark_all" Marks "Mark every listed row.";
    InvertMarks "invert_marks" Marks "Invert the marks of the listed rows: marked ones lose their mark, the others get one.";
    ToggleMarkedList "toggle_marked_list" Marks "Toggle the focus between the results and the marked list, where keys act on the marks.";
    ClearQuery "clear_query" Search "Clear the query. With it already clear, pressing this twice in a row clears every mark instead.";
    SwitchQuery "switch_query" Search "Type into the other query, contents or names. The contents query lists one row per match of the files the names query lists; its bar shows while it is typed into or set.";
    SearchShallower "search_shallower" Search "Search one level less deep below the current directory; the least is 1, its own entries.";
    SearchDeeper "search_deeper" Search "Search one level deeper below the current directory.";
    SearchToDepth "search_to_depth" Search "Search down to depth #, a digit from 1 to 9.";
    ToggleHidden "toggle_hidden" Search "Toggle hidden entries (names starting with a dot): listed or not.";
    ToggleDirectories "toggle_directories" Search "Toggle directories: listed, or hidden so that only files are listed.";
    ToggleFiles "toggle_files" Search "Toggle files: listed, or hidden so that only directories are listed.";
    EditExclusions "edit_exclusions" Search "Edit this session's exclusions in a buffer, one glob per line; matching entries, and everything below them, are hidden.";
    ToggleExclusions "toggle_exclusions" Search "Toggle the exclusions: enforced, or ignored so that excluded entries are listed too.";
    EditFilters "edit_filters" Search "Edit this session's file filters in a buffer, one glob per line; only files matching one of them are listed.";
    ToggleFilters "toggle_filters" Search "Toggle the file filters: applied, or off so that every file is listed. They are kept for turning back on.";
    Rescan "rescan" Search "Rescan the current directory and below, bypassing the cache.";
    TogglePreview "toggle_preview" View "Toggle the preview pane, which shows the highlighted entry's contents: shown or hidden.";
    TogglePreviewWrap "toggle_preview_wrap" View "Toggle word wrapping in the preview: long lines wrapped onto the next rows, or cut at its edge.";
    ScrollPreviewDown "scroll_preview_down" View "Scroll the preview down by half its height, leaving the highlight where it is.";
    ScrollPreviewUp "scroll_preview_up" View "Scroll the preview up by half its height, leaving the highlight where it is.";
    ToggleLabels "toggle_labels" View "Toggle how rows are labelled: by name, with only the parent folders that tell same-named entries apart (folder1/main.pdf), or by their path below the folder searched. fff pick starts with names, the others with paths; the choice stays when switch_program changes program.";
    ShowPalettes "show_palettes" View "Show the palettes: the highlighted one is previewed after a moment, and Enter keeps it for the session.";
    ReloadPalette "reload_palette" View "Reload the palette the configuration names, dropping one chosen for the session.";
    MoveMarked "move_marked" Files "Move the marked paths into the current directory. Asks first when that would overwrite anything.";
    CopyMarked "copy_marked" Files "Copy the marked paths into the current directory. Asks first when that would overwrite anything.";
    LinkMarked "link_marked" Files "Link the marked paths into the current directory, as symbolic links pointing to them.";
    Trash "trash" Files "Move the current or marked entries to the trash, from where restore puts them back. Asks only about very large ones.";
    Delete "delete" Files "Delete the current or marked entries for good, after showing their sizes for a review.";
    Rename "rename" Files "Rename the listed results by editing their names in a buffer; writing the buffer renames them.";
    Extract "extract" Files "Extract the current or marked archives into a folder a prompt asks for.";
    Create "create" Files "Create a new file or directory in the current directory: choose which, then type its name.";
    Undo "undo" Files "Undo the last file operation of this session; pressed again, the one before it.";
    Restore "restore" Files "In the trash: put the current or marked entries back where they were deleted from.";
    ShowKeys "show_keys" Session "Show the Keys screen, to look keys up and change them; the same key closes it.";
    ShowSession "show_session" Session "Show the Session tab: exact paths, search scopes, policies, marks and any errors in the configuration.";
    ShowBookmarks "show_bookmarks" Session "Show the bookmarks: go to one, or add, edit, delete and reorder them.";
    EditConfig "edit_config" Session "Edit the configuration file in $EDITOR, then apply it to this session at once.";
    SwitchProgram "switch_program" Session "Go on as fff roam, pick or seek, keeping the query, highlight, marks, exclusions, filters and a depth you set.";
}

use Highlight as H;
use Place::{Marks, Results};
use Program::*;
const EXPLORERS: [Program; 4] = [ExplorerOpen, ExplorerMultiple, ExplorerFolder, ExplorerSave];
/// Programs that navigate: all but seek.
const NAVIGATE: [Program; 7] = [
    Roam,
    Window,
    Pick,
    ExplorerOpen,
    ExplorerMultiple,
    ExplorerFolder,
    ExplorerSave,
];
fn scope(programs: &[Program], places: &[Place], highlights: &[Highlight]) -> Scope {
    Scope::of(programs, places, highlights)
}
impl Function {
    /// Where it works in `mode`, as boxes; none where it does not. This is
    /// a design choice, not configuration.
    pub fn available(self, mode: Mode) -> Vec<Scope> {
        use Function::*;
        let normal = mode == Mode::Normal;
        let both = |s: Scope| vec![s];
        let only_normal = |s: Scope| if normal { vec![s] } else { vec![] };
        let results = scope(&[], &[Results], &[]);
        let lists = Scope::ALL;
        // Insert mode exists in the results only.
        let lists_or_results = if normal { lists } else { results };
        let file = [H::File];
        match self {
            HighlightPrevious | HighlightNext | ToggleMark | ToggleMarkedList | ShowKeys
            | ShowSession | Cancel | PrintMarks => both(lists_or_results),
            HighlightPreviousPage
            | HighlightNextPage
            | SearchShallower
            | SearchDeeper
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
            | ReloadPalette
            | OpenRow
            | ConfirmRow => both(results),
            // Opening files has no letter for a key in Insert mode, so it can work there.
            OpenInTerminal | EditFiles => both(lists_or_results),
            HighlightFirst | HighlightLast | MarkAll | InvertMarks | Run | RunAndQuit
            | MoveMarked | CopyMarked | LinkMarked | Trash | Delete | Extract | Undo | Restore
            | Quit | EditConfig => only_normal(lists),
            Rename | Create | ClearQuery | SearchToDepth => only_normal(results),
            SwitchQuery => both(scope(&[], &[Results], &[])),
            GoToParent | GoToPath | ShowBookmarks | GoToTrash | GoBack | GoForward
            | GoToNextTab | GoToPreviousTab | ToggleDirectories | ToggleFiles => {
                both(scope(&NAVIGATE, &[Results], &[]))
            }
            GoToHighlighted if normal => vec![scope(&NAVIGATE, &[], &[H::Dir, H::File])],
            GoToHighlighted => both(scope(&NAVIGATE, &[Results], &[H::Dir, H::File])),
            OpenInBackground => both(scope(&[], &[Results], &file)),
            OpenAndQuit if normal => vec![scope(&[Roam, Window], &[], &file)],
            OpenAndQuit => vec![scope(&[Roam, Window], &[Results], &file)],
            SaveHere => both(scope(&[ExplorerSave], &[Results], &[])),
            SaveOver => only_normal(scope(&[ExplorerSave], &[], &file)),
            PrintHighlighted if normal => vec![
                scope(&[Pick, ExplorerOpen, ExplorerFolder], &[], &file),
                scope(&[ExplorerMultiple], &[Marks], &file),
                scope(&EXPLORERS, &[Marks], &[H::Dir]),
            ],
            PrintHighlighted => both(scope(&[ExplorerOpen, ExplorerFolder], &[Results], &file)),
            PrintHighlightedAndMarks if normal => {
                vec![scope(&[ExplorerMultiple], &[Results], &file)]
            }
            PrintHighlightedAndMarks => vec![
                scope(&[Pick], &[Results], &file),
                scope(&[ExplorerMultiple], &[Results], &file),
            ],
            QuitIntoHighlighted if normal => vec![
                scope(&[Roam, Pick], &[], &[H::Dir]),
                scope(&[Roam, Pick], &[Results], &[H::None]),
                scope(&[Seek], &[], &file),
            ],
            QuitIntoHighlighted => vec![
                scope(&[Roam, Pick, ExplorerFolder], &[Results], &[H::None]),
                scope(&[Seek], &[Results], &file),
            ],
            QuitHere if normal => vec![
                scope(&[Roam, Window, Pick, Seek], &[], &[]),
                scope(
                    &[ExplorerOpen, ExplorerMultiple, ExplorerFolder],
                    &[Results],
                    &[],
                ),
            ],
            QuitHere => both(scope(
                &[ExplorerOpen, ExplorerMultiple, ExplorerFolder],
                &[Results],
                &[],
            )),
            SwitchProgram => both(scope(
                &Program::SWITCHABLE.map(|(p, _)| p),
                if normal { &[] } else { &[Results] },
                &[],
            )),
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
    /// the mouse's fixed keys and `confirm_row` do.
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
        let file = [H::File];
        let dirs = scope(&[], &[], &[H::Dir]);
        let insert: &[(&str, Scope)] = match self {
            HighlightPrevious => &[("<M-k>", all), ("<Up>", all)],
            HighlightNext => &[("<M-j>", all), ("<Down>", all)],
            HighlightPreviousPage => &[("<PageUp>", all)],
            HighlightNextPage => &[("<PageDown>", all)],
            GoToParent => &[("<M-h>", all)],
            GoToPath => &[("<M-g>", all)],
            ShowBookmarks => &[("<M-b>", all)],
            GoToTrash => &[("<M-t>", all)],
            GoBack => &[("<C-o>", all)],
            GoForward => &[("<C-i>", all)],
            GoToNextTab => &[("<M-Up>", all)],
            GoToPreviousTab => &[("<M-Down>", all)],
            OpenInBackground => &[("<M-l>", all)],
            OpenInTerminal => &[("<M-o>", all)],
            OpenRow => &[("<M-#>", all)],
            ConfirmRow => &[("<C-#>", all)],
            ToggleMark => &[("<S-Space>", all)],
            SwitchQuery => &[("<Tab>", all), ("<M-/>", all)],
            ToggleMarkedList => &[("<M-Space>", all)],
            PrintMarks => &[("<C-w>", all)],
            SearchShallower => &[("<M-Left>", all)],
            SearchDeeper => &[("<M-Right>", all)],
            ToggleHidden => &[("<M-.>", all)],
            ToggleDirectories => &[("<M-d>", all)],
            ToggleFiles => &[("<M-D>", all)],
            TogglePreview => &[("<M-v>", all)],
            ScrollPreviewDown => &[("<C-j>", all)],
            ScrollPreviewUp => &[("<C-k>", all)],
            EditExclusions => &[("<M-e>", all)],
            ToggleExclusions => &[("<M-H>", all)],
            EditFilters => &[("<M-f>", all)],
            ToggleFilters => &[("<M-F>", all)],
            Rescan => &[("<F5>", all)],
            Cancel => &[("<C-c>", all)],
            ShowKeys => &[("<F1>", all)],
            ShowSession => &[("<F2>", all)],
            ShowPalettes => &[("<M-p>", all)],
            ReloadPalette => &[("<M-P>", all)],
            _ => &[],
        };
        let enter_insert: &[(&str, Scope)] = match self {
            SaveHere => &[("<CR>", all), ("<C-s>", all)],
            QuitHere => &[("<C-s>", scope(&[ExplorerFolder], &[], &[]))],
            GoToHighlighted => &[
                ("<M-l>", dirs),
                (
                    "<CR>",
                    scope(
                        &[
                            Roam,
                            Window,
                            Pick,
                            ExplorerOpen,
                            ExplorerMultiple,
                            ExplorerFolder,
                        ],
                        &[],
                        &[H::Dir],
                    ),
                ),
                ("<CR>", scope(&[Roam, Window], &[], &file)),
            ],
            PrintHighlighted | PrintHighlightedAndMarks | QuitIntoHighlighted => &[("<CR>", all)],
            _ => &[],
        };
        let normal: &[(&str, Scope)] = match self {
            HighlightPrevious => &[
                ("k", all),
                ("<M-k>", all),
                ("<Up>", scope(&[Seek], &[], &[])),
                ("<Up>", scope(&[], &[Marks], &[])),
            ],
            HighlightNext => &[
                ("j", all),
                ("<M-j>", all),
                ("<Down>", scope(&[Seek], &[], &[])),
                ("<Down>", scope(&[], &[Marks], &[])),
            ],
            HighlightPreviousPage => &[("<PageUp>", all)],
            HighlightNextPage => &[("<PageDown>", all)],
            HighlightFirst => &[("gg", all)],
            HighlightLast => &[("G", all)],
            GoToParent => &[("h", all), ("<M-h>", all)],
            GoToHighlighted => &[
                ("l", dirs),
                ("<M-l>", dirs),
                (
                    "<CR>",
                    scope(
                        &[
                            Window,
                            ExplorerOpen,
                            ExplorerMultiple,
                            ExplorerFolder,
                            ExplorerSave,
                        ],
                        &[Results],
                        &[H::Dir],
                    ),
                ),
                ("<CR>", scope(&[Window], &[Marks], &[H::Dir])),
            ],
            GoToPath => &[("g/", all)],
            ShowBookmarks => &[("b", all)],
            GoToTrash => &[("gt", all)],
            GoBack => &[("<C-o>", all)],
            GoForward => &[("<C-i>", all)],
            GoToNextTab => &[("<Up>", all), ("<M-Up>", all)],
            GoToPreviousTab => &[("<Down>", all), ("<M-Down>", all)],
            SaveHere => &[
                ("!", all),
                ("<C-s>", all),
                ("<CR>", scope(&[], &[Results], &[H::None])),
            ],
            SaveOver
            | PrintHighlighted
            | PrintHighlightedAndMarks
            | QuitIntoHighlighted
            | OpenAndQuit => &[("<CR>", all)],
            OpenInBackground => &[("l", all), ("<M-l>", all)],
            OpenInTerminal => &[("o", all)],
            OpenRow => &[("<M-#>", all)],
            ConfirmRow => &[("<C-#>", all)],
            Run => &[("gx", all)],
            RunAndQuit => &[("gX", all)],
            EditFiles => &[("e", all)],
            ToggleMark => &[("<Space>", all), ("<S-Space>", all)],
            MarkAll => &[("m", all)],
            InvertMarks => &[("M", all)],
            ToggleMarkedList => &[("<M-Space>", all), ("<Esc>", scope(&[], &[Marks], &[]))],
            PrintMarks => &[("<C-w>", all)],
            ClearQuery => &[("<Esc>", all)],
            SwitchQuery => &[("<Tab>", all), ("<M-/>", all)],
            SearchShallower => &[("-", all), ("<M-Left>", all)],
            SearchDeeper => &[("+", all), ("<M-Right>", all)],
            SearchToDepth => &[("<#>", all)],
            ToggleHidden => &[(".", all)],
            ToggleDirectories => &[("d", all)],
            ToggleFiles => &[("D", all)],
            TogglePreview => &[("P", all)],
            ToggleLabels => &[("gd", all)],
            ScrollPreviewDown => &[("<C-j>", all)],
            ScrollPreviewUp => &[("<C-k>", all)],
            EditExclusions => &[("gE", all)],
            ToggleExclusions => &[("ge", all)],
            EditFilters => &[("gF", all)],
            ToggleFilters => &[("gf", all)],
            Rescan => &[("<F5>", all)],
            MoveMarked => &[("v", all)],
            CopyMarked => &[("p", all)],
            LinkMarked => &[("s", all)],
            Trash => &[("x", all)],
            Delete => &[("X", all)],
            Rename => &[("r", all)],
            Extract => &[("E", all)],
            Create => &[("n", all)],
            Undo => &[("u", all)],
            Restore => &[("R", all)],
            Quit => &[("q", all)],
            QuitHere => &[
                ("!", all),
                ("<C-s>", scope(&[ExplorerFolder], &[], &[])),
                ("<CR>", scope(&[ExplorerFolder], &[], &[H::None])),
            ],
            Cancel => &[("<C-c>", all)],
            ShowKeys => &[("<F1>", all)],
            ShowSession => &[("<F2>", all)],
            ShowPalettes => &[("gp", all)],
            ReloadPalette => &[("gP", all)],
            EditConfig => &[("gc", all)],
            SwitchProgram => &[("gm", all)],
            _ => &[],
        };
        let specs: Vec<(&str, Scope)> = match mode {
            Mode::Insert => insert.iter().chain(enter_insert).copied().collect(),
            Mode::Normal => normal.to_vec(),
        };
        specs
            .into_iter()
            .map(|(text, scope)| Binding {
                function: self,
                mode,
                keys: parse(text).expect("default keys parse"),
                scope,
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
        "Move through the bookmark, palette and icon choosers (Normal mode).",
    ),
    (
        "↑ ↓",
        "choosers",
        "Move; in the bookmark chooser, move the highlighted bookmark.",
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
        "bookmarks",
        "New, edit, delete or reload bookmarks (Normal mode; with Alt in Insert mode).",
    ),
    (
        "Alt+0–9",
        "bookmarks",
        "Open the visible bookmark #; 0 is the last.",
    ),
    (
        "Tab  Shift+Tab",
        "bookmark form",
        "Switch fields; Space chooses the icon on its row.",
    ),
    ("click", "mouse", "Highlight a row, clearing the marks."),
    (
        "double click",
        "mouse",
        "Open the row, as open_row would (in explorer, confirm it, as Enter would).",
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
    /// in the results, and something leaves the marked list, so that no
    /// keymap can lock you in.
    fn check_escape(&self) -> Result<(), String> {
        for &program in Program::ALL {
            for &highlight in Highlight::ALL {
                let cell = Cell {
                    program,
                    place: Marks,
                    highlight,
                };
                if !self
                    .bindings
                    .iter()
                    .any(|b| b.function == Function::ToggleMarkedList && b.applies(cell))
                {
                    return Err(format!(
                        "nothing would leave the marked list in {}; toggle_marked_list needs a key there",
                        cell.describe()
                    ));
                }
            }
            for &highlight in Highlight::ALL {
                let cell = Cell {
                    program,
                    place: Results,
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
        crate::bookmarks::atomic_write(path, before.as_deref(), &document.to_string())
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
/// `{ keys = "<CR>", program = [...], list = [...], highlight = [...] }`.
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
                    if !["keys", "program", "list", "highlight"].contains(&name.as_str()) {
                        return Err(format!(
                            "unknown field {name:?}; use keys, program, list or highlight"
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
                    places: set::<Place>(table, "list")?,
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
        if let Some(v) = names::<Place>(b.scope.places) {
            table.insert("list", v);
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
            place: Results,
            highlight,
        }
    }
    fn run(keymap: &Keymap, mode: Mode, cell: Cell, keys: &[KeyEvent]) -> Step {
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
            .chain([KeyCode::Insert, KeyCode::F(5), KeyCode::F(13), KeyCode::BackTab])
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
        assert_eq!(parse("<M->>").unwrap(), [Key::Press(KeyCode::Char('>'), M::ALT)]);
        assert_eq!(parse("<C-M->>x").unwrap().len(), 2);
        // Alt or Ctrl with `#` reads as any digit: it cannot be written.
        let alt_hash = Key::press(KeyEvent::new(KeyCode::Char('#'), M::ALT));
        assert!(!writable(&[alt_hash]));
        assert_eq!(parse("<C-S-A>").unwrap(), [Key::Press(KeyCode::Char('A'), M::CONTROL)]);
        // A sequence spelling "none" would read back as no key.
        let none: Vec<Key> = "none".chars().map(|c| parse(&c.to_string()).unwrap()[0]).collect();
        assert!(!writable(&none));
        // A key without a name in the notation is refused.
        assert!(!writable(&[Key::Press(KeyCode::CapsLock, M::NONE)]));
    }
    #[test]
    fn tab_switches_queries_and_space_m_and_shift_m_mark() {
        let keymap = Keymap::default();
        let tab = KeyEvent::new(KeyCode::Tab, M::NONE);
        let shift_space = KeyEvent::new(KeyCode::Char(' '), M::SHIFT);
        for program in <Program as Dimension>::ALL.iter().copied() {
            let here = cell(program, H::File);
            for mode in Mode::ALL {
                assert_eq!(
                    run(&keymap, mode, here, &[tab]),
                    Step::Run(Function::SwitchQuery, None),
                    "{program:?} {mode:?}"
                );
                assert_eq!(
                    run(&keymap, mode, here, &[shift_space]),
                    Step::Run(Function::ToggleMark, None)
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
        let normal =
            |program, highlight| run(&keymap, Mode::Normal, cell(program, highlight), &[enter]);
        assert_eq!(
            normal(Roam, H::Dir),
            Step::Run(Function::QuitIntoHighlighted, None)
        );
        assert_eq!(
            normal(Window, H::Dir),
            Step::Run(Function::GoToHighlighted, None)
        );
        assert_eq!(
            normal(Roam, H::File),
            Step::Run(Function::OpenAndQuit, None)
        );
        assert_eq!(
            normal(Pick, H::File),
            Step::Run(Function::PrintHighlighted, None)
        );
        assert_eq!(
            normal(Seek, H::File),
            Step::Run(Function::QuitIntoHighlighted, None)
        );
        assert_eq!(
            normal(ExplorerSave, H::File),
            Step::Run(Function::SaveOver, None)
        );
        assert_eq!(normal(Window, H::None), Step::Pass);
        let insert =
            |program, highlight| run(&keymap, Mode::Insert, cell(program, highlight), &[enter]);
        assert_eq!(
            insert(Roam, H::File),
            Step::Run(Function::GoToHighlighted, None)
        );
        assert_eq!(
            insert(Pick, H::File),
            Step::Run(Function::PrintHighlightedAndMarks, None)
        );
        assert_eq!(
            insert(ExplorerSave, H::Dir),
            Step::Run(Function::SaveHere, None)
        );
        assert_eq!(
            insert(Roam, H::Dir),
            Step::Run(Function::GoToHighlighted, None)
        );
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
            Step::Run(Function::SearchToDepth, Some(3))
        );
        assert_eq!(run(&keymap, Mode::Normal, here, &[press('0')]), Step::Pass);
        let ctrl = KeyEvent::new(KeyCode::Char('0'), M::CONTROL);
        assert_eq!(
            run(&keymap, Mode::Insert, here, &[ctrl]),
            Step::Run(Function::ConfirmRow, Some(0))
        );
        // Navigation is off in seek: d reaches Neovim there.
        assert_eq!(
            run(&keymap, Mode::Normal, cell(Seek, H::File), &[press('d')]),
            Step::Pass
        );
        assert_eq!(
            run(&keymap, Mode::Normal, here, &[press('d')]),
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
    fn keys_toml_names_lists_and_programs_as_the_keys_screen_does() {
        let keymap = Keymap::parse(
            "[normal]\nquit = ['Q', { keys = 'q', program = ['explorer-open-multiple'], list = ['marks'] }]\n",
        )
        .unwrap();
        let quit: Vec<Binding> = keymap.of(Function::Quit, Mode::Normal).into_iter().cloned().collect();
        let saved = write_bindings(&quit).to_string();
        assert!(saved.contains("list = [\"marks\"]"), "{saved}");
        assert!(saved.contains("explorer-open-multiple"), "{saved}");
        assert!(Keymap::parse("[normal]\nquit = { keys = 'Q', place = ['marks'] }\n").is_err());
        assert!(
            Keymap::parse("[normal]\nquit = { keys = 'Q', program = ['explorer-multiple'] }\n")
                .is_err()
        );
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
                    * s.places.count_ones() as usize
                    * s.highlights.count_ones() as usize
            })
            .sum();
        assert_eq!(cells, 2 * 2 * 3 - 2);
        assert_eq!(b.describe(), "window · dir");
        assert_eq!(Scope::of(&EXPLORERS, &[], &[]).describe(), "explorer-*");
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
