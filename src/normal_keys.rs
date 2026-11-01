//! Normal-mode list keys: configurable plain-key sequences that move a list
//! highlight while the field editor is in Neovim Normal mode. Keys that do not
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
}
impl ListAction {
    pub const ALL: [Self; 6] = [
        Self::Left,
        Self::Down,
        Self::Up,
        Self::Right,
        Self::First,
        Self::Last,
    ];
    /// The Alt chord whose view-specific behavior this action repeats, if any.
    pub fn alt_key(self) -> Option<KeyEvent> {
        let c = match self {
            Self::Left => 'h',
            Self::Down => 'j',
            Self::Up => 'k',
            Self::Right => 'l',
            Self::First | Self::Last => return None,
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
    bindings: Vec<(Vec<KeyEvent>, ListAction)>,
    pending: Vec<KeyEvent>,
}
impl Default for NormalKeys {
    fn default() -> Self {
        NormalKeysConfig::default().build().unwrap()
    }
}
impl NormalKeys {
    pub fn pending(&self) -> bool {
        !self.pending.is_empty()
    }
    /// Forget a partial sequence without delivering it.
    pub fn clear(&mut self) {
        self.pending.clear();
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
    // Terminals disagree about reporting Shift with an uppercase character.
    let strip = |k: KeyEvent| match k.code {
        KeyCode::Char(c) => (KeyCode::Char(c), k.modifiers - KeyModifiers::SHIFT),
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
        }
    }
}
impl NormalKeysConfig {
    pub fn build(&self) -> Result<NormalKeys, String> {
        let mut bindings: Vec<(Vec<KeyEvent>, ListAction, &str)> = Vec::new();
        for action in ListAction::ALL {
            let (name, value) = match action {
                ListAction::Left => ("left", &self.left),
                ListAction::Down => ("down", &self.down),
                ListAction::Up => ("up", &self.up),
                ListAction::Right => ("right", &self.right),
                ListAction::First => ("first", &self.first),
                ListAction::Last => ("last", &self.last),
            };
            for text in value.list() {
                let keys = parse_sequence(text).map_err(|e| format!("keys.normal.{name}: {e}"))?;
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
        Ok(NormalKeys {
            bindings: bindings.into_iter().map(|(k, a, _)| (k, a)).collect(),
            pending: Vec::new(),
        })
    }
}
/// Printable characters, one key each; `<space>` and `<lt>` name space and `<`.
fn parse_sequence(text: &str) -> Result<Vec<KeyEvent>, String> {
    let mut keys = Vec::new();
    let mut rest = text;
    while let Some(c) = rest.chars().next() {
        let (key, len) = if c == '<' {
            let end = rest
                .find('>')
                .ok_or_else(|| format!("unterminated key name in {text:?}"))?;
            let key = match rest[1..end].to_ascii_lowercase().as_str() {
                "space" => ' ',
                "lt" => '<',
                other => {
                    return Err(format!(
                        "unsupported key <{other}> in {text:?}; use printable keys, <space>, or <lt>"
                    ));
                }
            };
            (key, end + 1)
        } else if c.is_control() {
            return Err(format!("control character in {text:?}"));
        } else {
            (c, c.len_utf8())
        };
        let modifiers = if key.is_uppercase() {
            KeyModifiers::SHIFT
        } else {
            KeyModifiers::NONE
        };
        keys.push(KeyEvent::new(KeyCode::Char(key), modifiers));
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
        assert_eq!(k.feed(key('w'), all), Step::Pass);
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
            toml::from_str("first=['<space>t','gg']\nlast='none'\ndown=''\nup=[]").unwrap();
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
