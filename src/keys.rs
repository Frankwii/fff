//! Invocation-time key remapping. Targets use the documented default key names.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

#[derive(Default)]
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
                Some(parse_key(to)?)
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
        "f1" => KeyCode::F(1),
        "f5" => KeyCode::F(5),
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
