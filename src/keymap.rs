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

/// What fff was started as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Program {
    Roam,
    Window,
    Pick,
    Seek,
    Grep,
    ExplorerOpen,
    ExplorerMultiple,
    ExplorerFolder,
    ExplorerSave,
}
impl Program {
    /// What `switch_program` offers, by their letters in its chooser: the
    /// programs started from a shell. A file dialog answers its caller and
    /// a window of its own has no shell to leave into, so neither switches.
    pub const SWITCHABLE: [(Self, char); 4] = [
        (Self::Roam, 'r'),
        (Self::Pick, 'p'),
        (Self::Seek, 's'),
        (Self::Grep, 'g'),
    ];
    /// The depth it searches to without `--depth`: browsing shows a
    /// folder's own entries, picking looks a little deeper, and seeking
    /// deeper still.
    pub fn default_depth(self) -> usize {
        match self {
            Self::Pick => 3,
            Self::Seek | Self::Grep => 6,
            Self::Roam
            | Self::Window
            | Self::ExplorerOpen
            | Self::ExplorerMultiple
            | Self::ExplorerFolder
            | Self::ExplorerSave => 1,
        }
    }
}
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
        Self::Grep,
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
            Self::Grep => "grep",
            Self::ExplorerOpen => "explorer-open",
            Self::ExplorerMultiple => "explorer-multiple",
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
            KeyCode::BackTab => {
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
        if m.contains(M::SHIFT) {
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
/// Vim's key notation: printable characters stand for themselves; `<...>`
/// names a key with optional `C-`, `M-` (or `A-`) and `S-` modifiers:
/// `<CR>`, `<Esc>`, `<Tab>`, `<BS>`, `<Del>`, `<Space>`, `<lt>`, arrows
/// (`<Up>`), `<PageUp>`, `<PageDown>`, `<Home>`, `<End>`, `<F1>` to
/// `<F12>`, one character (`<M-h>`), or `#` for a digit family (`<C-#>`).
pub fn parse(text: &str) -> Result<Vec<Key>, String> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        if c == '<'
            && let Some(end) = rest.find('>')
            && end > 1
        {
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
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        other => match other.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            Some(n @ 1..=12) => KeyCode::F(n),
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

/// What a function does when its key is pressed: the default key the
/// view's own handlers take, or an action of the session's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exec {
    Key(KeyEvent),
    /// The pressed digit with these modifiers.
    Digit(M),
    /// A file operation, by its default letter.
    Operation(char),
    /// `true` leaves into the current folder.
    Quit(bool),
    /// `true` replaces fff with the program.
    Run(bool),
    EditConfig,
    /// Search to the pressed digit's depth.
    Depth,
    /// Mark every listed row; `true` inverts their marks.
    MarkRows(bool),
    /// The F1/F2 overlay: the Keys screen, or with `false` the Session tab.
    Overlay(bool),
    /// The first row, or the last with `true`.
    JumpEnd(bool),
    /// Choose another program to go on as.
    SwitchProgram,
    Cancel,
}

/// Groups for the Keys screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Navigate,
    Open,
    Marks,
    View,
    Files,
    Session,
}
impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Self::Navigate => "Navigate",
            Self::Open => "Open and finish",
            Self::Marks => "Marks",
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
functions! {
    Up "up" Navigate "Highlight the previous row.";
    Down "down" Navigate "Highlight the next row.";
    PageUp "page_up" Navigate "Move the highlight one page up.";
    PageDown "page_down" Navigate "Move the highlight one page down.";
    First "first" Navigate "Highlight the first row.";
    Last "last" Navigate "Highlight the last row.";
    GoParent "go_parent" Navigate "Go to the parent directory, keeping the query; the highlight and scroll start over.";
    GoInto "go_into" Navigate "Enter the highlighted directory.";
    GoTo "go_to" Navigate "Go to a typed path, or to words matching a bookmark or one of zoxide's frequent folders.";
    Bookmarks "bookmarks" Navigate "Open the bookmark chooser.";
    Trash "trash" Navigate "Open the trash like a folder; again, the next filesystem's trash.";
    Back "back" Navigate "Step back through the folders this tab visited.";
    Forward "forward" Navigate "Step forward through the folders this tab visited. Ctrl+i needs a terminal that tells it from Tab.";
    NextTab "next_tab" Navigate "Switch to the next of three tabs; each keeps its folder and history, marks are shared.";
    PrevTab "prev_tab" Navigate "Switch to the previous of three tabs.";
    SaveHere "save_here" Open "Save in the current folder under the typed name (the query), or the suggested one.";
    SaveAs "save_as" Open "Save over the highlighted file, after asking.";
    ChooseFolder "choose_folder" Open "Answer the dialog with the current folder.";
    ChooseFile "choose_file" Open "Answer the dialog with the highlighted file (in the marked list, the mark); when choosing several, the marked ones too.";
    PickFile "pick_file" Open "Print the highlighted file and leave, ignoring the marks.";
    PickWithMarks "pick_with_marks" Open "Print the marked paths and the highlighted file, then leave.";
    QuitIntoFolder "quit_into_folder" Open "Leave into the highlighted file's folder.";
    OpenAndQuit "open_and_quit" Open "Open the highlighted file with its opener and leave fff.";
    Reveal "reveal" Open "Show the highlighted file in its own folder, clearing the query.";
    QuitIntoDir "quit_into_dir" Open "Leave into the highlighted directory, or the current one when nothing is listed.";
    OpenBackground "open_background" Open "Open the highlighted file with its opener in the background; fff stays.";
    OpenHere "open_here" Open "Open the current or marked files with their openers in this terminal, then come back.";
    OpenRow "open_row" Open "Like go_into or open_background on visible row #; 0 is the last row.";
    EnterRow "enter_row" Open "Like Enter on visible row #; 0 is the last row.";
    Run "run" Open "Run a program on the current or marked paths, then come back.";
    RunAndQuit "run_and_quit" Open "Run a program on the current or marked paths in place of fff.";
    Edit "edit" Open "Open the current or marked paths in $EDITOR, then come back.";
    Mark "mark" Marks "Toggle the highlighted row's mark; in the results, also move down.";
    MarkAll "mark_all" Marks "Mark every listed row.";
    InvertMarks "invert_marks" Marks "Invert the marks of the listed rows.";
    MarkedList "marked_list" Marks "Focus the marked list, or leave it.";
    ReturnMarks "return_marks" Marks "Leave, returning the marked paths.";
    ClearQuery "clear_query" View "Clear the query; with none, pressed twice, clear every mark.";
    SwitchQuery "switch_query" View "In fff grep: type into the other query, contents or names.";
    Shallower "shallower" View "Search one level shallower; the minimum is 1.";
    Deeper "deeper" View "Search one level deeper.";
    Depth "depth" View "Search down to depth #.";
    Hidden "hidden" View "Show or hide hidden entries.";
    Directories "directories" View "Hide directories, listing only files; again to show them.";
    Files "files" View "Hide files, listing only directories; again to show them.";
    Preview "preview" View "Show or hide the preview pane.";
    ScrollPreviewDown "scroll_preview_down" View "Scroll the preview down by half its height, keeping the highlight.";
    ScrollPreviewUp "scroll_preview_up" View "Scroll the preview up by half its height, keeping the highlight.";
    EditExclusions "edit_exclusions" View "Edit this session's exclusions, one glob per line; matching entries and their subtrees are hidden.";
    EnforceExclusions "enforce_exclusions" View "Turn the exclusions on or off.";
    EditFilters "edit_filters" View "Edit this session's file filters, one glob per line; only files matching one are listed.";
    ToggleFilters "toggle_filters" View "Turn the file filters off or back on, without a rescan.";
    Rescan "rescan" View "Discover again, bypassing the cache.";
    Move "move" Files "Move the marked paths into the current folder; asks before overwriting.";
    Copy "copy" Files "Copy the marked paths into the current folder; asks before overwriting.";
    Link "link" Files "Symlink the marked paths into the current folder.";
    Delete "delete" Files "Move the current or marked entries to the trash; asks only about very large ones.";
    DeletePermanently "delete_permanently" Files "Review sizes, then delete the current or marked entries for good.";
    Rename "rename" Files "Rename the listed results in a buffer.";
    Extract "extract" Files "Extract the current or marked archives; asks when the layout is ambiguous.";
    New "new" Files "Create a new file or directory here.";
    Undo "undo" Files "Undo the last file operation; again for the one before.";
    Restore "restore" Files "In the trash: restore the current or marked entries where they were deleted from.";
    Quit "quit" Session "Leave without choosing: where you started (roam), printing nothing (pick, seek), or cancelling the dialog (explorer).";
    QuitCurrent "quit_current" Session "Leave into the current folder, ignoring marks; in a window of its own, start a shell there.";
    Cancel "cancel" Session "Leave with no output; while an operation runs, stop it instead.";
    Guide "guide" Session "Open the Keys screen; the same key closes it.";
    Session "session" Session "Open the Session tab: paths, scopes, policies and configuration errors.";
    Palette "palette" Session "Choose a palette, previewing the highlighted one.";
    ReloadPalette "reload_palette" Session "Reload the configured palette, dropping a session choice.";
    EditConfig "edit_config" Session "Edit the configuration in $EDITOR, then apply it.";
    SwitchProgram "switch_program" Session "Go on as fff roam, pick, seek or grep, keeping the query, highlight, marks, exclusions, filters and a depth you set.";
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
fn key(code: KeyCode, modifiers: M) -> Exec {
    Exec::Key(KeyEvent::new(code, modifiers))
}
fn alt(c: char) -> Exec {
    let shift = if c.is_uppercase() { M::SHIFT } else { M::NONE };
    key(KeyCode::Char(c), M::ALT | shift)
}

impl Function {
    /// Where it works in `mode`, as boxes; none where it does not. This is
    /// a design choice, not configuration.
    pub fn available(self, mode: Mode) -> Vec<Scope> {
        use Function::*;
        let normal = mode == Mode::Normal;
        let both = |s: Scope| vec![s];
        let only_normal = |s: Scope| if normal { vec![s] } else { vec![] };
        let only_insert = |s: Scope| if normal { vec![] } else { vec![s] };
        let results = scope(&[], &[Results], &[]);
        let lists = Scope::ALL;
        // Insert mode exists in the results only.
        let lists_or_results = if normal { lists } else { results };
        match self {
            Up | Down => both(lists_or_results),
            PageUp | PageDown | Shallower | Deeper | Hidden | Preview | ScrollPreviewDown
            | ScrollPreviewUp | EditExclusions | EnforceExclusions | EditFilters
            | ToggleFilters | Rescan | Palette | ReloadPalette | OpenRow | EnterRow => {
                both(results)
            }
            First | Last | MarkAll | InvertMarks | OpenHere | Run | RunAndQuit | Edit | Move
            | Copy | Link | Delete | DeletePermanently | Extract | Undo | Restore | Quit
            | EditConfig => only_normal(lists),
            Rename | New | ClearQuery | Depth => only_normal(results),
            SwitchQuery => both(scope(&[Grep], &[Results], &[])),
            GoParent | GoTo | Bookmarks | Trash | Back | Forward | NextTab | PrevTab => {
                both(scope(&NAVIGATE, &[Results], &[]))
            }
            Directories | Files => both(scope(&NAVIGATE, &[Results], &[])),
            GoInto if normal => vec![
                scope(&NAVIGATE, &[Results], &[H::Dir]),
                scope(&[Window], &[Marks], &[H::Dir]),
            ],
            GoInto => both(scope(&NAVIGATE, &[Results], &[H::Dir])),
            SaveHere => both(scope(&[ExplorerSave], &[Results], &[])),
            SaveAs => only_normal(scope(&[ExplorerSave], &[], &[H::File])),
            ChooseFolder => both(scope(
                &[ExplorerOpen, ExplorerMultiple, ExplorerFolder],
                &[Results],
                &[],
            )),
            ChooseFile if normal => vec![
                scope(&EXPLORERS[..3], &[], &[H::File]),
                scope(&EXPLORERS, &[Marks], &[H::Dir]),
            ],
            ChooseFile => both(scope(&EXPLORERS[..3], &[Results], &[H::File])),
            PickFile => only_normal(scope(&[Pick, Grep], &[], &[H::File])),
            PickWithMarks => only_insert(scope(&[Pick, Grep], &[Results], &[H::File])),
            QuitIntoFolder => both(scope(&[Seek], &[], &[H::File])),
            OpenAndQuit => only_normal(scope(&[Roam, Window], &[], &[H::File])),
            Reveal => only_insert(scope(&[Roam, Window], &[Results], &[H::File])),
            QuitIntoDir if normal => vec![
                scope(&[Roam, Pick], &[], &[H::Dir]),
                scope(&[Roam, Pick], &[Results], &[H::None]),
            ],
            QuitIntoDir => both(scope(&[Roam, Pick], &[Results], &[H::None])),
            OpenBackground => both(scope(&[], &[Results], &[H::File])),
            Mark | MarkedList | Guide | Session => both(lists_or_results),
            // Elsewhere it says where it works.
            ReturnMarks => both(lists_or_results),
            QuitCurrent => only_normal(scope(&[Roam, Window, Pick, Seek, Grep], &[], &[])),
            SwitchProgram => both(scope(
                &Program::SWITCHABLE.map(|(p, _)| p),
                if normal { &[] } else { &[Results] },
                &[],
            )),
            Cancel => both(lists_or_results),
        }
    }
    /// Whether it works in `cell` in `mode`.
    pub fn works(self, mode: Mode, cell: Cell) -> bool {
        self.available(mode).iter().any(|s| s.contains(cell))
    }
    /// What pressing its key does in `place`.
    pub fn exec(self, place: Place) -> Exec {
        use Function::*;
        use KeyCode as K;
        match self {
            Up => key(K::Up, M::NONE),
            Down => key(K::Down, M::NONE),
            PageUp => key(K::PageUp, M::NONE),
            PageDown => key(K::PageDown, M::NONE),
            First => Exec::JumpEnd(false),
            Last => Exec::JumpEnd(true),
            GoParent => alt('h'),
            GoInto if place == Marks => key(K::Enter, M::NONE),
            GoInto | OpenBackground => alt('l'),
            GoTo => alt('g'),
            Bookmarks => alt('b'),
            Trash => alt('t'),
            Back => key(K::Char('o'), M::CONTROL),
            Forward => key(K::Char('i'), M::CONTROL),
            NextTab => key(K::Up, M::ALT),
            PrevTab => key(K::Down, M::ALT),
            SaveHere => key(K::Char('s'), M::CONTROL),
            SaveAs | ChooseFile | PickFile | PickWithMarks | QuitIntoFolder | OpenAndQuit
            | Reveal | QuitIntoDir => key(K::Enter, M::NONE),
            ChooseFolder => Exec::Quit(true),
            OpenHere => Exec::Operation('o'),
            OpenRow => Exec::Digit(M::ALT),
            EnterRow => Exec::Digit(M::CONTROL),
            Run => Exec::Run(false),
            RunAndQuit => Exec::Run(true),
            Edit => Exec::Operation('E'),
            Mark => key(K::Char(' '), M::SHIFT),
            MarkAll => Exec::MarkRows(false),
            InvertMarks => Exec::MarkRows(true),
            MarkedList => key(K::Char(' '), M::ALT),
            ReturnMarks => key(K::Char('w'), M::CONTROL),
            ClearQuery => key(K::Esc, M::NONE),
            SwitchQuery => alt('/'),
            Shallower => key(K::Left, M::ALT),
            Deeper => key(K::Right, M::ALT),
            Depth => Exec::Depth,
            Hidden => alt('.'),
            Directories => alt('d'),
            Files => alt('D'),
            Preview => alt('v'),
            ScrollPreviewDown => key(K::Char('j'), M::CONTROL),
            ScrollPreviewUp => key(K::Char('k'), M::CONTROL),
            EditExclusions => alt('e'),
            EnforceExclusions => alt('H'),
            EditFilters => alt('f'),
            ToggleFilters => alt('F'),
            Rescan => key(K::F(5), M::NONE),
            Move => Exec::Operation('v'),
            Copy => Exec::Operation('p'),
            Link => Exec::Operation('s'),
            Delete => Exec::Operation('x'),
            DeletePermanently => Exec::Operation('X'),
            Rename => Exec::Operation('r'),
            Extract => Exec::Operation('e'),
            New => Exec::Operation('n'),
            Undo => Exec::Operation('u'),
            Restore => Exec::Operation('R'),
            Quit => Exec::Quit(false),
            QuitCurrent => Exec::Quit(true),
            Cancel => Exec::Cancel,
            Guide => Exec::Overlay(true),
            Session => Exec::Overlay(false),
            Palette => alt('p'),
            ReloadPalette => alt('P'),
            EditConfig => Exec::EditConfig,
            SwitchProgram => Exec::SwitchProgram,
        }
    }
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|f| f.name() == name)
    }
    /// The bindings it has without `keys.toml`.
    pub fn defaults(self, mode: Mode) -> Vec<Binding> {
        use Function::*;
        let all = Scope::ALL;
        let insert: &[(&str, Scope)] = match self {
            Up => &[("<M-k>", all), ("<Up>", all)],
            Down => &[("<M-j>", all), ("<Down>", all)],
            PageUp => &[("<PageUp>", all)],
            PageDown => &[("<PageDown>", all)],
            GoParent => &[("<M-h>", all)],
            GoTo => &[("<M-g>", all)],
            Bookmarks => &[("<M-b>", all)],
            Trash => &[("<M-t>", all)],
            Back => &[("<C-o>", all)],
            Forward => &[("<C-i>", all)],
            NextTab => &[("<M-Up>", all)],
            PrevTab => &[("<M-Down>", all)],
            OpenBackground => &[("<M-l>", all)],
            OpenRow => &[("<M-#>", all)],
            EnterRow => &[("<C-#>", all)],
            Mark => &[("<S-Space>", all), ("<Tab>", all)],
            MarkedList => &[("<M-Space>", all)],
            ReturnMarks => &[("<C-w>", all)],
            Shallower => &[("<M-Left>", all)],
            Deeper => &[("<M-Right>", all)],
            Hidden => &[("<M-.>", all)],
            Directories => &[("<M-d>", all)],
            Files => &[("<M-D>", all)],
            Preview => &[("<M-v>", all)],
            ScrollPreviewDown => &[("<C-j>", all)],
            ScrollPreviewUp => &[("<C-k>", all)],
            EditExclusions => &[("<M-e>", all)],
            EnforceExclusions => &[("<M-H>", all)],
            EditFilters => &[("<M-f>", all)],
            ToggleFilters => &[("<M-F>", all)],
            Rescan => &[("<F5>", all)],
            Cancel => &[("<C-c>", all)],
            Guide => &[("<F1>", all)],
            Session => &[("<F2>", all)],
            Palette => &[("<M-p>", all)],
            ReloadPalette => &[("<M-P>", all)],
            SwitchQuery => &[("<M-/>", all)],
            _ => &[],
        };
        let enter_insert: &[(&str, Scope)] = match self {
            SaveHere => &[("<CR>", all), ("<C-s>", all)],
            ChooseFolder => &[("<C-s>", scope(&[ExplorerFolder], &[], &[]))],
            GoInto => &[
                ("<M-l>", all),
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
                        &[],
                    ),
                ),
            ],
            ChooseFile | PickWithMarks | QuitIntoFolder | Reveal | QuitIntoDir => &[("<CR>", all)],
            _ => &[],
        };
        let normal: &[(&str, Scope)] = match self {
            Up => &[
                ("k", all),
                ("<M-k>", all),
                ("<Up>", scope(&[Seek, Grep], &[], &[])),
                ("<Up>", scope(&[], &[Marks], &[])),
            ],
            Down => &[
                ("j", all),
                ("<M-j>", all),
                ("<Down>", scope(&[Seek, Grep], &[], &[])),
                ("<Down>", scope(&[], &[Marks], &[])),
            ],
            PageUp => &[("<PageUp>", all)],
            PageDown => &[("<PageDown>", all)],
            First => &[("gg", all)],
            Last => &[("G", all)],
            GoParent => &[("h", all), ("<M-h>", all)],
            GoInto => &[
                ("l", all),
                ("<M-l>", all),
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
                        &[],
                        &[],
                    ),
                ),
            ],
            GoTo => &[("g/", all)],
            Bookmarks => &[("b", all)],
            Trash => &[("gt", all)],
            Back => &[("<C-o>", all)],
            Forward => &[("<C-i>", all)],
            NextTab => &[("<Up>", all), ("<M-Up>", all)],
            PrevTab => &[("<Down>", all), ("<M-Down>", all)],
            SaveHere => &[("!", all), ("<C-s>", all)],
            SaveAs | ChooseFile | PickFile | QuitIntoFolder | OpenAndQuit | QuitIntoDir => {
                &[("<CR>", all)]
            }
            ChooseFolder => &[
                ("!", all),
                ("<C-s>", scope(&[ExplorerFolder], &[], &[])),
                ("<CR>", scope(&[ExplorerFolder], &[], &[H::None])),
            ],
            OpenBackground => &[("l", all), ("<M-l>", all)],
            OpenHere => &[("o", all)],
            OpenRow => &[("<M-#>", all)],
            EnterRow => &[("<C-#>", all)],
            Run => &[("gx", all)],
            RunAndQuit => &[("gX", all)],
            Edit => &[("e", all)],
            Mark => &[("<Space>", all), ("<S-Space>", all)],
            MarkAll => &[("m", all)],
            InvertMarks => &[("M", all)],
            MarkedList => &[("<M-Space>", all), ("<Esc>", scope(&[], &[Marks], &[]))],
            ReturnMarks => &[("<C-w>", all)],
            ClearQuery => &[("<Esc>", all)],
            SwitchQuery => &[("<M-/>", all)],
            Shallower => &[("-", all), ("<M-Left>", all)],
            Deeper => &[("+", all), ("<M-Right>", all)],
            Depth => &[("<#>", all)],
            Hidden => &[(".", all)],
            Directories => &[("d", all)],
            Files => &[("D", all)],
            Preview => &[("P", all)],
            ScrollPreviewDown => &[("<C-j>", all)],
            ScrollPreviewUp => &[("<C-k>", all)],
            EditExclusions => &[("gE", all)],
            EnforceExclusions => &[("ge", all)],
            EditFilters => &[("gF", all)],
            ToggleFilters => &[("gf", all)],
            Rescan => &[("<F5>", all)],
            Move => &[("v", all)],
            Copy => &[("p", all)],
            Link => &[("s", all)],
            Delete => &[("x", all)],
            DeletePermanently => &[("X", all)],
            Rename => &[("r", all)],
            Extract => &[("E", all)],
            New => &[("n", all)],
            Undo => &[("u", all)],
            Restore => &[("R", all)],
            Quit => &[("q", all)],
            QuitCurrent => &[("!", all)],
            Cancel => &[("<C-c>", all)],
            Guide => &[("<F1>", all)],
            Session => &[("<F2>", all)],
            Palette => &[("gp", all)],
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
        "Neovim's command line; :u undoes query edits.",
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
        "New, edit, delete or reload bookmarks (Normal mode; with Alt while typing).",
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
        "Act like go_into / open_background on the row (in explorer, like Enter).",
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
    pub fn new(mut custom: Vec<((Function, Mode), Vec<Binding>)>) -> Result<Self, String> {
        custom.sort_by_key(|(at, _)| *at);
        custom.dedup_by_key(|(at, _)| *at);
        let mut chosen: Vec<Binding> = custom.iter().flat_map(|(_, b)| b.clone()).collect();
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
        let keymap = Self {
            bindings: chosen,
            custom,
        };
        keymap.check_escape()?;
        Ok(keymap)
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
                    .any(|b| b.function == Function::MarkedList && b.applies(cell))
                {
                    return Err(format!(
                        "nothing would leave the marked list in {}; marked_list needs a key there",
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
                if !bound(Function::Guide, &Mode::ALL) {
                    return Err(format!(
                        "nothing would open the Keys screen in {}; guide needs a key there",
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
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        let mut document: toml_edit::DocumentMut = text
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
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        std::fs::write(path, document.to_string()).map_err(|e| format!("{}: {e}", path.display()))
    }
}
/// Rules a binding's keys follow wherever it applies.
fn check_keys(binding: &Binding) -> Result<(), String> {
    let at = || format!("{}.{}", binding.mode.name(), binding.function.name());
    let first = binding.keys[0];
    match binding.mode {
        Mode::Insert if first.printable() => Err(format!(
            "{}: {} starts with a key typed as text; Insert-mode keys start with Ctrl, Alt or a named key",
            at(),
            notation(&binding.keys)
        )),
        Mode::Insert if first == Key::press(KeyEvent::new(KeyCode::Esc, M::NONE)) => Err(format!(
            "{}: <Esc> leaves Insert mode and cannot be bound there",
            at()
        )),
        Mode::Normal
            if ['i', 'a', 'I', 'A', ':']
                .iter()
                .any(|&c| first == Key::press(KeyEvent::new(KeyCode::Char(c), M::NONE))) =>
        {
            Err(format!(
                "{}: {} starts with a key that enters Insert mode or Neovim's command line",
                at(),
                notation(&binding.keys)
            ))
        }
        _ => Ok(()),
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
/// `{ keys = "<CR>", program = [...], place = [...], highlight = [...] }`.
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
                    if !["keys", "program", "place", "highlight"].contains(&name.as_str()) {
                        return Err(format!(
                            "unknown field {name:?}; use keys, program, place or highlight"
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
                    places: set::<Place>(table, "place")?,
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
            table.insert("place", v);
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
    fn enter_does_one_thing_in_each_place() {
        let keymap = Keymap::default();
        let enter = KeyEvent::new(KeyCode::Enter, M::NONE);
        let normal =
            |program, highlight| run(&keymap, Mode::Normal, cell(program, highlight), &[enter]);
        assert_eq!(normal(Roam, H::Dir), Step::Run(Function::QuitIntoDir, None));
        assert_eq!(normal(Window, H::Dir), Step::Run(Function::GoInto, None));
        assert_eq!(
            normal(Roam, H::File),
            Step::Run(Function::OpenAndQuit, None)
        );
        assert_eq!(normal(Pick, H::File), Step::Run(Function::PickFile, None));
        assert_eq!(
            normal(Seek, H::File),
            Step::Run(Function::QuitIntoFolder, None)
        );
        assert_eq!(
            normal(ExplorerSave, H::File),
            Step::Run(Function::SaveAs, None)
        );
        assert_eq!(normal(Window, H::None), Step::Pass);
        let insert =
            |program, highlight| run(&keymap, Mode::Insert, cell(program, highlight), &[enter]);
        assert_eq!(insert(Roam, H::File), Step::Run(Function::Reveal, None));
        assert_eq!(
            insert(Pick, H::File),
            Step::Run(Function::PickWithMarks, None)
        );
        assert_eq!(
            insert(ExplorerSave, H::Dir),
            Step::Run(Function::SaveHere, None)
        );
        assert_eq!(insert(Roam, H::Dir), Step::Run(Function::GoInto, None));
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
            Step::Run(Function::First, None)
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
            Step::Run(Function::Depth, Some(3))
        );
        assert_eq!(run(&keymap, Mode::Normal, here, &[press('0')]), Step::Pass);
        let ctrl = KeyEvent::new(KeyCode::Char('0'), M::CONTROL);
        assert_eq!(
            run(&keymap, Mode::Insert, here, &[ctrl]),
            Step::Run(Function::EnterRow, Some(0))
        );
        // Navigation is off in seek: d reaches Neovim there.
        assert_eq!(
            run(&keymap, Mode::Normal, cell(Seek, H::File), &[press('d')]),
            Step::Pass
        );
        assert_eq!(
            run(&keymap, Mode::Normal, here, &[press('d')]),
            Step::Run(Function::Directories, None)
        );
        // Typing is text.
        assert_eq!(run(&keymap, Mode::Insert, here, &[press('q')]), Step::Pass);
    }
    #[test]
    fn keys_toml_replaces_scopes_and_refuses_clashes_and_lockouts() {
        let keymap = Keymap::parse(
            "[normal]\ngo_into = ['l', { keys = '<CR>', program = ['roam'], highlight = ['dir'] }]\nquit = 'Q'\n",
        )
        .unwrap();
        let enter = KeyEvent::new(KeyCode::Enter, M::NONE);
        // The custom <CR> takes roam's directories from quit_into_dir.
        assert_eq!(
            run(&keymap, Mode::Normal, cell(Roam, H::Dir), &[enter]),
            Step::Run(Function::GoInto, None)
        );
        assert_eq!(
            run(&keymap, Mode::Normal, cell(Roam, H::File), &[press('q')]),
            Step::Pass
        );
        // Two custom bindings may not share a key where both apply.
        assert!(Keymap::parse("[normal]\nquit = 'Q'\nhidden = 'Q'\n").is_err());
        // Nor can the Keys screen or every way out go.
        assert!(Keymap::parse("[insert]\nguide = 'none'\n[normal]\nguide = 'none'\n").is_err());
        assert!(
            Keymap::parse("[insert]\ncancel = 'none'\n[normal]\ncancel = 'none'\nquit = 'none'\n")
                .is_err()
        );
        assert!(Keymap::parse("[insert]\ncancel = 'none'\n[normal]\ncancel = 'none'\n").is_ok());
        assert!(Keymap::parse("[normal]\nmarked_list = 'none'\n").is_err());
        // Typing and the way into Insert mode stay.
        assert!(Keymap::parse("[insert]\nquit = 'q'\n").is_err());
        assert!(Keymap::parse("[normal]\nhidden = 'i'\n").is_err());
        assert!(Keymap::parse("[insert]\nhidden = '<Esc>'\n").is_err());
        assert!(Keymap::parse("[normal]\nnope = 'x'\n").is_err());
        assert!(Keymap::parse("[normal]\ngo_into = { keys = 'L', program = ['seek'] }\n").is_err());
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
                Function::Hidden,
                Mode::Insert,
                vec![Binding {
                    function: Function::Hidden,
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
        assert_eq!(again.of(Function::Hidden, Mode::Insert).len(), 1);
        // Back to the defaults, the entry goes.
        again.reset(Function::Hidden).unwrap().save(&path).unwrap();
        assert!(!std::fs::read_to_string(&path).unwrap().contains("hidden"));
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
