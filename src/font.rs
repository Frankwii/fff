//! Resizing the terminal's font. A terminal program cannot set the font
//! itself; it asks the terminal, and only some listen: kitty through its
//! remote control (sent as an escape sequence, so it needs
//! `allow_remote_control`), xterm through the OSC 50 font sequence, which
//! steps through its font menu. Inside tmux or screen the request would
//! reach the multiplexer, not the terminal, so none is sent.

/// A terminal that takes font requests.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terminal {
    Kitty,
    Xterm,
}
impl Terminal {
    /// The terminal fff runs in, from the environment it set.
    pub fn detect(var: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        if var("TMUX").is_some() || var("STY").is_some() {
            return Err("Inside tmux or screen, the font is the outer terminal's to change".into());
        }
        if var("KITTY_WINDOW_ID").is_some() || var("TERM").as_deref() == Some("xterm-kitty") {
            return Ok(Self::Kitty);
        }
        if var("XTERM_VERSION").is_some() {
            return Ok(Self::Xterm);
        }
        Err("This terminal cannot change its font from fff; kitty (with remote control) and xterm can".into())
    }
    /// The request for a font one step larger, or smaller.
    pub fn request(self, larger: bool) -> String {
        let sign = if larger { '+' } else { '-' };
        match self {
            Self::Kitty => format!(
                "\x1bP@kitty-cmd{{\"cmd\":\"set-font-size\",\"version\":[0,14,2],\"no_response\":true,\
                 \"payload\":{{\"size\":1,\"increment_op\":\"{sign}\"}}}}\x1b\\"
            ),
            Self::Xterm => format!("\x1b]50;#{sign}1\x07"),
        }
    }
    /// What the status line says once the request is sent: neither
    /// terminal answers, so whether it took is seen on the screen.
    pub fn sent(self, larger: bool) -> String {
        let size = if larger { "larger" } else { "smaller" };
        match self {
            Self::Kitty => {
                format!("Asked kitty for a {size} font (it needs allow_remote_control)")
            }
            Self::Xterm => format!("Asked xterm for a {size} font"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminals_are_told_apart_and_asked_their_own_way() {
        let env = |pairs: &'static [(&'static str, &'static str)]| {
            move |name: &str| {
                pairs
                    .iter()
                    .find(|(k, _)| *k == name)
                    .map(|(_, v)| v.to_string())
            }
        };
        assert_eq!(
            Terminal::detect(env(&[("KITTY_WINDOW_ID", "1")])),
            Ok(Terminal::Kitty)
        );
        assert_eq!(
            Terminal::detect(env(&[("TERM", "xterm-kitty")])),
            Ok(Terminal::Kitty)
        );
        assert_eq!(
            Terminal::detect(env(&[("XTERM_VERSION", "XTerm(390)")])),
            Ok(Terminal::Xterm)
        );
        assert!(Terminal::detect(env(&[("TERM", "foot")])).is_err());
        assert!(Terminal::detect(env(&[("TMUX", "x"), ("KITTY_WINDOW_ID", "1")])).is_err());
        assert_eq!(Terminal::Xterm.request(true), "\x1b]50;#+1\x07");
        let kitty = Terminal::Kitty.request(false);
        assert!(kitty.starts_with("\x1bP@kitty-cmd{") && kitty.ends_with("}\x1b\\"));
        assert!(kitty.contains("\"increment_op\":\"-\""));
    }
}
