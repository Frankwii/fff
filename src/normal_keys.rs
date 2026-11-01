//! Configurable Normal-mode sequences for list actions. Keys that do not
//! complete a binding are handed back unchanged so Neovim still receives them.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ListAction {
    /// Same as Alt+h in the view.
    Left,
    /// Same as Alt+j in the view.
    Down,
    /// Same as Alt+k in the view.
    Up,
    /// Same as Alt+l in the view.
    Right,
    /// Highlight the first row.
    First,
    /// Highlight the last row.
    Last,
    /// Toggle the highlighted mark, like Tab.
    Mark,
    /// Return the invocation directory and exit.
    Quit,
    /// Return the current browsing directory and exit.
    QuitCurrent,
    /// Run a program on the current or marked paths, then come back.
    Run,
    /// Run a program on the current or marked paths in place of fff.
    RunExit,
    /// File operations; see `operation_key`.
    Move,
    Copy,
    Link,
    Delete,
    Rename,
    Extract,
    Edit,
    New,
    /// Same as Alt+← in the results: search one level shallower.
    Shallower,
    /// Same as Alt+→ in the results: search one level deeper.
    Deeper,
    /// Same as Alt+b in the results: open the bookmark chooser.
    Bookmarks,
    /// Same as Alt+. in the results: show or hide hidden entries.
    Hidden,
    /// Same as Alt+g in the results: the go-to prompt.
    GoTo,
    /// Same as Alt+↓ in the results: the previous tab.
    PrevTab,
    /// Same as Alt+↑ in the results: the next tab.
    NextTab,
}
impl ListAction {
    pub const ALL: [Self; 26] = [
        Self::Left,
        Self::Down,
        Self::Up,
        Self::Right,
        Self::First,
        Self::Last,
        Self::Mark,
        Self::Quit,
        Self::QuitCurrent,
        Self::Run,
        Self::RunExit,
        Self::Move,
        Self::Copy,
        Self::Link,
        Self::Delete,
        Self::Rename,
        Self::Extract,
        Self::Edit,
        Self::New,
        Self::Shallower,
        Self::Deeper,
        Self::Bookmarks,
        Self::Hidden,
        Self::GoTo,
        Self::PrevTab,
        Self::NextTab,
    ];
    /// The default letter of a file operation: the view handles operations by
    /// it, whatever sequence the configuration binds.
    pub fn operation_key(self) -> Option<char> {
        Some(match self {
            Self::Move => 'v',
            Self::Copy => 'p',
            Self::Link => 's',
            Self::Delete => 'x',
            Self::Rename => 'r',
            Self::Extract => 'e',
            Self::Edit => 'E',
            Self::New => 'n',
            _ => return None,
        })
    }
    /// The default chord whose view-specific behavior this action repeats, if any.
    pub fn alt_key(self) -> Option<KeyEvent> {
        let c = match self {
            Self::Left => 'h',
            Self::Down => 'j',
            Self::Up => 'k',
            Self::Right => 'l',
            Self::Mark => return Some(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE)),
            Self::Shallower => return Some(KeyEvent::new(KeyCode::Left, KeyModifiers::ALT)),
            Self::Deeper => return Some(KeyEvent::new(KeyCode::Right, KeyModifiers::ALT)),
            Self::PrevTab => return Some(KeyEvent::new(KeyCode::Down, KeyModifiers::ALT)),
            Self::NextTab => return Some(KeyEvent::new(KeyCode::Up, KeyModifiers::ALT)),
            Self::Bookmarks => 'b',
            Self::Hidden => '.',
            Self::GoTo => 'g',
            Self::First
            | Self::Last
            | Self::Quit
            | Self::QuitCurrent
            | Self::Run
            | Self::RunExit
            | Self::Move
            | Self::Copy
            | Self::Link
            | Self::Delete
            | Self::Rename
            | Self::Extract
            | Self::Edit
            | Self::New => return None,
        };
        Some(KeyEvent::new(KeyCode::Char(c), KeyModifiers::ALT))
    }
}

/// Result of feeding one key to [`NormalKeys`].
#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    /// Not part of any binding; handle the key as usual.
    Pass,
    /// A prefix of a binding; wait for the next key.
    Pending,
    /// A binding completed.
    Action(ListAction),
    /// A pending prefix did not continue into a binding. Deliver these earlier
    /// keys first (to Neovim), then handle the current key as usual.
    Flush(Vec<KeyEvent>),
}

#[derive(Clone, Debug)]
pub struct NormalKeys {
    /// Plain digits 1–9 set the results' search depth (`depth_digits`).
    pub depth_digits: bool,
    bindings: Vec<(Vec<KeyEvent>, ListAction)>,
    /// Configured sequences as written, for the F1 key guide.
    names: Vec<(ListAction, String)>,
    pending: Vec<KeyEvent>,
}
impl Default for NormalKeys {
    fn default() -> Self {
        NormalKeysConfig::default().build().unwrap()
    }
}
impl NormalKeys {
    /// The sequences bound to an action, e.g. `"gg"`, joined with " / ";
    /// arrow names are shown as arrows.
    pub fn describe(&self, action: ListAction) -> String {
        self.names
            .iter()
            .filter(|(a, _)| *a == action)
            .map(|(_, name)| {
                name.replace("<left>", "←")
                    .replace("<right>", "→")
                    .replace("<up>", "↑")
                    .replace("<down>", "↓")
            })
            .collect::<Vec<_>>()
            .join(" / ")
    }
    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
    /// Hand back a partial sequence so it can be delivered unchanged.
    pub fn take_pending(&mut self) -> Vec<KeyEvent> {
        std::mem::take(&mut self.pending)
    }
    /// `supported` is the list view's opt-in hook: bindings for actions it does
    /// not support behave as if unbound, so their keys reach the editor.
    pub fn feed(&mut self, key: KeyEvent, supported: impl Fn(ListAction) -> bool) -> Step {
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let mut sequence = self.pending.clone();
        sequence.push(key);
        let candidates = || {
            self.bindings
                .iter()
                .filter(|(_, action)| supported(*action))
                .filter(|(keys, _)| keys.len() >= sequence.len())
                .filter(|(keys, _)| keys.iter().zip(&sequence).all(|(a, b)| same(*a, *b)))
        };
        if plain {
            if let Some((_, action)) = candidates().find(|(keys, _)| keys.len() == sequence.len()) {
                let action = *action;
                self.pending.clear();
                return Step::Action(action);
            }
            if candidates().next().is_some() {
                self.pending = sequence;
                return Step::Pending;
            }
        }
        if self.pending.is_empty() {
            Step::Pass
        } else {
            Step::Flush(std::mem::take(&mut self.pending))
        }
    }
}
fn same(a: KeyEvent, b: KeyEvent) -> bool {
    // Terminals disagree about reporting Shift with an uppercase character;
    // the kitty protocol sends Shift+g as `g` with SHIFT.
    let strip = |k: KeyEvent| match k.code {
        KeyCode::Char(c) if k.modifiers.contains(KeyModifiers::SHIFT) => (
            KeyCode::Char(c.to_ascii_uppercase()),
            k.modifiers - KeyModifiers::SHIFT,
        ),
        KeyCode::Char(c) => (KeyCode::Char(c), k.modifiers),
        code => (code, k.modifiers),
    };
    strip(a) == strip(b)
}

/// One sequence or several alternatives; `"none"`, `""`, or `[]` disables.
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Sequences {
    One(String),
    Many(Vec<String>),
}
impl Sequences {
    fn list(&self) -> Vec<&str> {
        match self {
            Self::One(s) => vec![s.as_str()],
            Self::Many(v) => v.iter().map(String::as_str).collect(),
        }
        .into_iter()
        .filter(|s| !s.is_empty() && *s != "none")
        .collect()
    }
}
fn one(s: &str) -> Sequences {
    Sequences::One(s.into())
}

/// TOML `[keys.normal]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NormalKeysConfig {
    pub left: Sequences,
    pub down: Sequences,
    pub up: Sequences,
    pub right: Sequences,
    pub first: Sequences,
    pub last: Sequences,
    pub mark: Sequences,
    /// Accepted for configuration compatibility; buffer editors use Neovim writes.
    pub apply: Sequences,
    pub quit: Sequences,
    pub quit_current: Sequences,
    pub run: Sequences,
    pub run_exit: Sequences,
    #[serde(rename = "move")]
    pub move_: Sequences,
    pub copy: Sequences,
    pub link: Sequences,
    pub delete: Sequences,
    pub rename: Sequences,
    pub extract: Sequences,
    pub edit: Sequences,
    pub new: Sequences,
    pub shallower: Sequences,
    pub deeper: Sequences,
    pub bookmarks: Sequences,
    pub hidden: Sequences,
    pub go_to: Sequences,
    pub prev_tab: Sequences,
    pub next_tab: Sequences,
    /// Plain digits 1–9 set the search depth in Normal-mode results.
    pub depth_digits: bool,
}
impl Default for NormalKeysConfig {
    fn default() -> Self {
        Self {
            left: one("h"),
            down: one("j"),
            up: one("k"),
            right: one("l"),
            first: one("gg"),
            last: one("G"),
            mark: one("<space>"),
            apply: one("none"),
            quit: one("q"),
            quit_current: one("!"),
            run: one("gx"),
            run_exit: one("gX"),
            move_: one("v"),
            copy: one("p"),
            link: one("s"),
            delete: one("x"),
            rename: one("r"),
            extract: one("e"),
            edit: one("E"),
            new: one("n"),
            shallower: one("<left>"),
            deeper: one("<right>"),
            bookmarks: one("b"),
            hidden: one("."),
            go_to: one("/"),
            prev_tab: one("<down>"),
            next_tab: one("<up>"),
            depth_digits: true,
        }
    }
}
impl NormalKeysConfig {
    fn sequences(&self, action: ListAction) -> (&'static str, &Sequences) {
        match action {
            ListAction::Left => ("left", &self.left),
            ListAction::Down => ("down", &self.down),
            ListAction::Up => ("up", &self.up),
            ListAction::Right => ("right", &self.right),
            ListAction::First => ("first", &self.first),
            ListAction::Last => ("last", &self.last),
            ListAction::Mark => ("mark", &self.mark),
            ListAction::Quit => ("quit", &self.quit),
            ListAction::QuitCurrent => ("quit_current", &self.quit_current),
            ListAction::Run => ("run", &self.run),
            ListAction::RunExit => ("run_exit", &self.run_exit),
            ListAction::Move => ("move", &self.move_),
            ListAction::Copy => ("copy", &self.copy),
            ListAction::Link => ("link", &self.link),
            ListAction::Delete => ("delete", &self.delete),
            ListAction::Rename => ("rename", &self.rename),
            ListAction::Extract => ("extract", &self.extract),
            ListAction::Edit => ("edit", &self.edit),
            ListAction::New => ("new", &self.new),
            ListAction::Shallower => ("shallower", &self.shallower),
            ListAction::Deeper => ("deeper", &self.deeper),
            ListAction::Bookmarks => ("bookmarks", &self.bookmarks),
            ListAction::Hidden => ("hidden", &self.hidden),
            ListAction::GoTo => ("go_to", &self.go_to),
            ListAction::PrevTab => ("prev_tab", &self.prev_tab),
            ListAction::NextTab => ("next_tab", &self.next_tab),
        }
    }
    pub fn build(&self) -> Result<NormalKeys, String> {
        let mut bindings: Vec<(Vec<KeyEvent>, ListAction, &str)> = Vec::new();
        for action in ListAction::ALL {
            let (name, value) = self.sequences(action);
            for text in value.list() {
                let keys = parse_sequence(text).map_err(|e| format!("keys.normal.{name}: {e}"))?;
                if self.depth_digits
                    && matches!(keys[0].code, KeyCode::Char(c) if c.is_ascii_digit() && c != '0')
                {
                    return Err(format!(
                        "keys.normal.{name}: {text:?} starts with a digit, which sets the depth; set depth_digits = false first"
                    ));
                }
                for (other, _, other_name) in &bindings {
                    let shorter = keys.len().min(other.len());
                    if keys[..shorter]
                        .iter()
                        .zip(&other[..shorter])
                        .all(|(a, b)| same(*a, *b))
                    {
                        return Err(format!(
                            "keys.normal.{name}: {text:?} conflicts with a keys.normal.{other_name} sequence (equal or prefix)"
                        ));
                    }
                }
                bindings.push((keys, action, name));
            }
        }
        let mut names = Vec::new();
        for action in ListAction::ALL {
            let (_, value) = self.sequences(action);
            names.extend(value.list().into_iter().map(|s| (action, s.to_string())));
        }
        Ok(NormalKeys {
            depth_digits: self.depth_digits,
            names,
            bindings: bindings.into_iter().map(|(k, a, _)| (k, a)).collect(),
            pending: Vec::new(),
        })
    }
}
/// Printable characters, one key each; `<space>` and `<lt>` name space and `<`,
/// `<left>`, `<right>`, `<up>` and `<down>` the arrow keys.
fn parse_sequence(text: &str) -> Result<Vec<KeyEvent>, String> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let (key, len) = if c == '<' {
            let end = rest
                .find('>')
                .ok_or_else(|| format!("unterminated key name in {text:?}"))?;
            let key = match rest[1..end].to_ascii_lowercase().as_str() {
                "space" => KeyCode::Char(' '),
                "lt" => KeyCode::Char('<'),
                "left" => KeyCode::Left,
                "right" => KeyCode::Right,
                "up" => KeyCode::Up,
                "down" => KeyCode::Down,
                other => {
                    return Err(format!(
                        "unsupported key <{other}> in {text:?}; use printable keys, <space>, <lt>, or an arrow (<left> <right> <up> <down>)"
                    ));
                }
            };
            (key, end + 1)
        } else if c.is_control() {
            return Err(format!("control character in {text:?}"));
        } else {
            (KeyCode::Char(c), c.len_utf8())
        };
        let modifiers = if matches!(key, KeyCode::Char(c) if c.is_uppercase()) {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        keys.push(KeyEvent::new(key, modifiers));
        rest = &rest[len..];
    }
    Ok(keys)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }
    fn all(_: ListAction) -> bool {
        true
    }
    #[test]
    fn defaults_match_single_keys_and_gg() {
        let mut k = NormalKeys::default();
        assert_eq!(k.feed(key('j'), all), Step::Action(ListAction::Down));
        assert_eq!(k.feed(key('g'), all), Step::Pending);
        assert!(k.pending());
        assert_eq!(k.feed(key('g'), all), Step::Action(ListAction::First));
        assert!(!k.pending());
        let shifted = KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT);
        assert_eq!(k.feed(shifted, all), Step::Action(ListAction::Last));
        assert_eq!(k.feed(key('G'), all), Step::Action(ListAction::Last));
        assert_eq!(k.feed(key('q'), all), Step::Action(ListAction::Quit));
        assert_eq!(k.feed(key('!'), all), Step::Action(ListAction::QuitCurrent));
        assert_eq!(k.feed(key('w'), all), Step::Pass);
        assert_eq!(k.feed(key(' '), all), Step::Action(ListAction::Mark));
        assert_eq!(k.feed(key('Z'), all), Step::Pass);
    }
    #[test]
    fn defaults_bind_arrows_and_results_shortcuts() {
        let mut k = NormalKeys::default();
        let arrow = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(
            k.feed(arrow(KeyCode::Left), all),
            Step::Action(ListAction::Shallower)
        );
        assert_eq!(
            k.feed(arrow(KeyCode::Right), all),
            Step::Action(ListAction::Deeper)
        );
        assert_eq!(k.feed(key('b'), all), Step::Action(ListAction::Bookmarks));
        assert_eq!(k.feed(key('.'), all), Step::Action(ListAction::Hidden));
        assert_eq!(k.feed(key('/'), all), Step::Action(ListAction::GoTo));
        let alt = |code| Some(KeyEvent::new(code, KeyModifiers::ALT));
        assert_eq!(ListAction::Shallower.alt_key(), alt(KeyCode::Left));
        assert_eq!(ListAction::GoTo.alt_key(), alt(KeyCode::Char('g')));
        assert_eq!(k.describe(ListAction::Deeper), "→");
        assert_eq!(
            k.feed(arrow(KeyCode::Up), all),
            Step::Action(ListAction::NextTab)
        );
        assert_eq!(
            k.feed(arrow(KeyCode::Down), all),
            Step::Action(ListAction::PrevTab)
        );
        assert_eq!(ListAction::NextTab.alt_key(), alt(KeyCode::Up));
        assert!(k.depth_digits);
        let alt_left = KeyEvent::new(KeyCode::Left, KeyModifiers::ALT);
        assert_eq!(k.feed(alt_left, all), Step::Pass);
    }
    #[test]
    fn kitty_shifted_lowercase_matches_uppercase_binding() {
        // Kitty-protocol terminals report Shift+g as `g` with SHIFT.
        let mut k = NormalKeys::default();
        let kitty = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::SHIFT);
        assert_eq!(k.feed(kitty, all), Step::Action(ListAction::Last));
        assert!(!k.pending());
        assert_eq!(k.feed(key('g'), all), Step::Pending);
        assert_eq!(k.feed(kitty, all), Step::Flush(vec![key('g')]));
    }
    #[test]
    fn unmatched_continuations_are_flushed_intact() {
        let mut k = NormalKeys::default();
        assert_eq!(k.feed(key('g'), all), Step::Pending);
        assert_eq!(k.feed(key('U'), all), Step::Flush(vec![key('g')]));
        assert!(!k.pending());
        assert_eq!(k.feed(key('g'), all), Step::Pending);
        let alt = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT);
        assert_eq!(k.feed(alt, all), Step::Flush(vec![key('g')]));
        assert_eq!(k.feed(alt, all), Step::Pass);
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        assert_eq!(k.feed(key('g'), all), Step::Pending);
        assert_eq!(k.feed(esc, all), Step::Flush(vec![key('g')]));
    }
    #[test]
    fn unsupported_actions_behave_as_unbound() {
        let mut k = NormalKeys::default();
        let vertical = |a: ListAction| !matches!(a, ListAction::Left | ListAction::Right);
        assert_eq!(k.feed(key('h'), vertical), Step::Pass);
        assert_eq!(k.feed(key('k'), vertical), Step::Action(ListAction::Up));
    }
    #[test]
    fn configuration_rebinds_disables_and_rejects_conflicts() {
        let config: NormalKeysConfig =
            toml::from_str("mark='none'\nfirst=['<space>t','gg']\nlast='none'\ndown=''\nup=[]")
                .unwrap();
        let mut k = config.build().unwrap();
        assert_eq!(k.feed(key('G'), all), Step::Pass);
        assert_eq!(k.feed(key('j'), all), Step::Pass);
        assert_eq!(k.feed(key('k'), all), Step::Pass);
        assert_eq!(k.feed(key(' '), all), Step::Pending);
        assert_eq!(k.feed(key('t'), all), Step::Action(ListAction::First));
        for bad in [
            "first='g'\nlast='gg'",
            "down='j'\nup='j'",
            "first='<tab>'",
            "first='<g'",
        ] {
            let config: NormalKeysConfig = toml::from_str(bad).unwrap();
            assert!(config.build().is_err(), "{bad}");
        }
        assert!(toml::from_str::<NormalKeysConfig>("top='g'").is_err());
    }
}
