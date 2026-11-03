//! Invocation-time key remapping. Targets use the documented default key names.
//! The interface names keys through `name`, so that a remapped chord is
//! shown as the key that now does its work.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Clone, Debug, Default)]
pub struct KeyMap(Vec<(KeyEvent, Option<KeyEvent>)>);
impl KeyMap {
    pub fn parse(rules: &[String]) -> Result<Self, String> {
        let mut mappings = Vec::new();
        for rule in rules {
            let (from, to) = rule
                .split_once('=')
                .ok_or_else(|| "Key map must be SOURCE=TARGET (or SOURCE=none)".to_string())?;
            let from = parse_key(from)?;
            if matches!(from.code, KeyCode::Char(_))
                && !from
                    .modifiers
                    .intersects(KeyModifiers::ALT | KeyModifiers::CONTROL)
            {
                return Err("Printable text is reserved for query and prompt editing".into());
            }
            let to = if to == "none" {
                None
            } else {
                Some(parse_key(target(to))?)
            };
            mappings.push((from, to));
        }
        Ok(Self(mappings))
    }
    pub fn translate(&self, key: KeyEvent) -> Option<KeyEvent> {
        self.0
            .iter()
            .rev()
            .find(|(from, _)| same_chord(*from, key))
            .map(|(_, to)| {
                to.map(|mut target| {
                    target.kind = key.kind;
                    target
                })
            })
            .unwrap_or(Some(key))
    }
    /// The key doing what the default chord `default` (named as a target,
    /// such as `"alt+v"`) does: itself while it is not remapped, else the
    /// first chord mapped to it; `None` when no key does.
    pub fn chord(&self, default: &str) -> Option<KeyEvent> {
        let target = parse_key(default).ok()?;
        if self.keeps(default) {
            return Some(target);
        }
        let does = |key: KeyEvent| self.translate(key).is_some_and(|k| same_chord(k, target));
        self.0
            .iter()
            .map(|(from, _)| *from)
            .find(|&from| does(from))
    }
    /// Whether the default chord `default` still does its own work.
    pub fn keeps(&self, default: &str) -> bool {
        parse_key(default).is_ok_and(|target| {
            self.translate(target)
                .is_some_and(|k| same_chord(k, target))
        })
    }
    /// `chord`, as the interface names it: `Alt+v`, `Ctrl+j`, `Enter`.
    /// What is not a chord (Neovim's `ZZ`) is named as written: no key map
    /// changes it.
    pub fn name(&self, default: &str) -> Option<String> {
        match parse_key(default) {
            Ok(_) => self.chord(default).map(display),
            Err(_) => Some(default.into()),
        }
    }
}
/// A chord as the interface names it: `Ctrl+`, `Alt+` and `Shift+` (not for
/// an uppercase letter, which says it), then the key: `Space`, `Enter`,
/// `Esc`, `Tab`, arrows as `←` `→` `↑` `↓`, `PgUp`, `PgDn`, `F1`.
pub fn display(key: KeyEvent) -> String {
    let mut name = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        name += "Ctrl+";
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        name += "Alt+";
    }
    let letter = matches!(key.code, KeyCode::Char(c) if c.is_alphabetic());
    if key.modifiers.contains(KeyModifiers::SHIFT) && !letter {
        name += "Shift+";
    }
    match key.code {
        KeyCode::Char(' ') => name += "Space",
        KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::SHIFT) => {
            name.extend(c.to_uppercase())
        }
        KeyCode::Char(c) => name.push(c),
        KeyCode::Enter => name += "Enter",
        KeyCode::Esc => name += "Esc",
        KeyCode::Tab => name += "Tab",
        KeyCode::BackTab => name += "Shift+Tab",
        KeyCode::Backspace => name += "Backspace",
        KeyCode::Left => name += "←",
        KeyCode::Right => name += "→",
        KeyCode::Up => name += "↑",
        KeyCode::Down => name += "↓",
        KeyCode::PageUp => name += "PgUp",
        KeyCode::PageDown => name += "PgDn",
        KeyCode::F(n) => name += &format!("F{n}"),
        code => name += &format!("{code:?}"),
    }
    name
}
/// Kitty-protocol terminals report Alt+Shift+p as `p` with SHIFT, legacy ones as
/// `P`; both spellings name the same chord.
fn same_chord(a: KeyEvent, b: KeyEvent) -> bool {
    fn fold(key: KeyEvent) -> (KeyCode, KeyModifiers) {
        match key.code {
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::SHIFT) => {
                (KeyCode::Char(c.to_ascii_uppercase()), key.modifiers)
            }
            code => (code, key.modifiers),
        }
    }
    fold(a) == fold(b)
}
/// Preliminary: what a target may be called instead of its default chord,
/// `"f4" = "session"` for `"f4" = "f2"`. Names shared with `[keys.normal]`
/// mean the same action there.
pub const TARGETS: &[(&str, &str)] = &[
    ("guide", "f1"),
    ("session", "f2"),
    ("rescan", "f5"),
    ("palette", "alt+p"),
    ("reload_palette", "alt+P"),
    ("bookmarks", "alt+b"),
    ("go_to", "alt+g"),
    ("trash", "alt+t"),
    ("hidden", "alt+."),
    ("directories", "alt+d"),
    ("files", "alt+D"),
    ("preview", "alt+v"),
    ("scroll_preview_down", "ctrl+j"),
    ("scroll_preview_up", "ctrl+k"),
    ("exclusions", "alt+e"),
    ("enforce_exclusions", "alt+H"),
    ("filters", "alt+f"),
    ("shallower", "alt+left"),
    ("deeper", "alt+right"),
    ("parent", "alt+h"),
    ("open", "alt+l"),
    ("back", "ctrl+o"),
    ("forward", "ctrl+i"),
    ("next_tab", "alt+up"),
    ("prev_tab", "alt+down"),
    ("marks", "alt+space"),
    ("return_marks", "ctrl+w"),
];
/// The default chord a target names: itself, or the chord of its name.
fn target(name: &str) -> &str {
    TARGETS
        .iter()
        .find(|(action, _)| *action == name)
        .map_or(name, |(_, chord)| chord)
}
/// `f1` to `f12`.
fn function_key(name: &str) -> Option<u8> {
    let n = name.strip_prefix('f')?.parse().ok()?;
    (1..=12).contains(&n).then_some(n)
}
fn parse_key(name: &str) -> Result<KeyEvent, String> {
    let mut rest = name;
    let mut modifiers = KeyModifiers::NONE;
    loop {
        if let Some(next) = rest.strip_prefix("ctrl+") {
            modifiers |= KeyModifiers::CONTROL;
            rest = next;
        } else if let Some(next) = rest.strip_prefix("alt+") {
            modifiers |= KeyModifiers::ALT;
            rest = next;
        } else if let Some(next) = rest.strip_prefix("shift+") {
            modifiers |= KeyModifiers::SHIFT;
            rest = next;
        } else {
            break;
        }
    }
    let code = match rest {
        _ if let Some(n) = function_key(rest) => KeyCode::F(n),
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "backspace" => KeyCode::Backspace,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "space" => KeyCode::Char(' '),
        _ => {
            let mut chars = rest.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Char(c),
                _ => return Err(format!("Unknown key: {name}")),
            }
        }
    };
    // Enhanced terminals report the shift modifier for uppercase letters.
    if matches!(code, KeyCode::Char(c) if c.is_uppercase()) {
        modifiers |= KeyModifiers::SHIFT;
    }
    Ok(KeyEvent::new(code, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keys_are_named_as_what_now_does_their_work() {
        let map = KeyMap::default();
        assert_eq!(map.name("alt+v").as_deref(), Some("Alt+v"));
        assert_eq!(map.name("alt+P").as_deref(), Some("Alt+P"));
        assert_eq!(map.name("shift+space").as_deref(), Some("Shift+Space"));
        assert_eq!(map.name("alt+right").as_deref(), Some("Alt+→"));
        assert_eq!(map.name("pagedown").as_deref(), Some("PgDn"));
        assert_eq!(map.name("ctrl+j").as_deref(), Some("Ctrl+j"));
        // A chord mapped elsewhere is named by the chord now mapped to it.
        let map = KeyMap::parse(&["ctrl+t=alt+v".into(), "alt+v=none".into()]).unwrap();
        assert_eq!(map.name("alt+v").as_deref(), Some("Ctrl+t"));
        // Still working itself, it keeps its own name.
        let map = KeyMap::parse(&["ctrl+t=alt+v".into()]).unwrap();
        assert_eq!(map.name("alt+v").as_deref(), Some("Alt+v"));
        // Function keys up to F12 stand for one another; a target can be
        // named by what it does.
        let map = KeyMap::parse(&["f4=f2".into(), "f2=none".into()]).unwrap();
        assert_eq!(map.name("f2").as_deref(), Some("F4"));
        let map = KeyMap::parse(&["f4=session".into(), "f2=none".into()]).unwrap();
        assert_eq!(map.name("f2").as_deref(), Some("F4"));
        let map = KeyMap::parse(&["ctrl+b=bookmarks".into()]).unwrap();
        assert_eq!(map.chord("alt+b"), Some(parse_key("alt+b").unwrap()));
        assert!(map.translate(parse_key("ctrl+b").unwrap()) == parse_key("alt+b").ok());
        for (_, chord) in TARGETS {
            assert!(parse_key(chord).is_ok(), "{chord}");
        }
        assert!(KeyMap::parse(&["f13=f2".into()]).is_err());
        // Nothing does it any more.
        let map = KeyMap::parse(&["alt+v=alt+b".into()]).unwrap();
        assert_eq!(map.name("alt+v"), None);
        assert_eq!(map.name("alt+b").as_deref(), Some("Alt+b"));
        // The last rule for a chord wins, as when translating.
        let map = KeyMap::parse(&[
            "ctrl+t=alt+v".into(),
            "ctrl+t=none".into(),
            "alt+v=none".into(),
        ])
        .unwrap();
        assert_eq!(map.name("alt+v"), None);
    }
}
