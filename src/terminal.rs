//! The terminal emulator fff runs programs in when `[open] terminal` or
//! `[open] fff_terminal` is unset: the one fff itself runs in, else
//! `$TERMINAL`, else the first known one installed.
//!
//! Each known terminal has its own way to run a command, and to give fff's
//! windows the `fff-picker` class (or Wayland app id) where it can. The
//! commands keep the terminal in the foreground until the program exits:
//! the file chooser waits for it.
use crate::run::resolve;
use std::path::{Path, PathBuf};

/// The class (or app id) of fff's own windows, for window managers.
pub const CLASS: &str = "fff-picker";

struct Known {
    /// The command, which is also the name its process runs under.
    name: &'static str,
    /// Other names its process may run under (a server, a GUI binary).
    processes: &'static [&'static str],
    /// Arguments before a command to run in a new window.
    run: &'static [&'static str],
    /// The same, with `CLASS` for the window where the terminal has one.
    classed: &'static [&'static str],
    /// Only runs under Wayland.
    wayland: bool,
}

/// In order of preference when none is running fff or named by `$TERMINAL`.
const KNOWN: &[Known] = &[
    Known {
        name: "st",
        processes: &[],
        run: &["-e"],
        classed: &["-c", CLASS, "-e"],
        wayland: false,
    },
    Known {
        name: "alacritty",
        processes: &[],
        run: &["-e"],
        classed: &["--class", CLASS, "-e"],
        wayland: false,
    },
    Known {
        name: "kitty",
        processes: &[],
        run: &[],
        classed: &["--class", CLASS],
        wayland: false,
    },
    Known {
        name: "wezterm",
        processes: &["wezterm-gui"],
        run: &["start", "--always-new-process", "--"],
        classed: &["start", "--always-new-process", "--class", CLASS, "--"],
        wayland: false,
    },
    // Ghostty's class must be a dotted application id.
    Known {
        name: "ghostty",
        processes: &[],
        run: &["-e"],
        classed: &["-e"],
        wayland: false,
    },
    Known {
        name: "foot",
        processes: &["footclient"],
        run: &[],
        classed: &["--app-id", CLASS],
        wayland: true,
    },
    Known {
        name: "konsole",
        processes: &[],
        run: &["--nofork", "-e"],
        classed: &["--nofork", "-e"],
        wayland: false,
    },
    Known {
        name: "gnome-terminal",
        processes: &["gnome-terminal-server"],
        run: &["--wait", "--"],
        classed: &["--wait", "--"],
        wayland: false,
    },
    Known {
        name: "xfce4-terminal",
        processes: &[],
        run: &["--disable-server", "-x"],
        classed: &["--disable-server", "-x"],
        wayland: false,
    },
    Known {
        name: "urxvt",
        processes: &["rxvt", "urxvtd"],
        run: &["-e"],
        classed: &["-e"],
        wayland: false,
    },
    Known {
        name: "xterm",
        processes: &["uxterm"],
        run: &["-e"],
        classed: &["-class", CLASS, "-e"],
        wayland: false,
    },
];

/// What a detected terminal's command line is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Purpose {
    /// `[open] terminal`: a program opened from fff.
    Open,
    /// `[open] fff_terminal`: fff's own windows, with their class.
    Fff,
}

/// Where a terminal is looked for, so that tests can supply their own.
pub struct Environment<'a> {
    /// The executables of fff's ancestor processes, nearest first.
    pub ancestors: Vec<PathBuf>,
    /// `$TERMINAL`.
    pub terminal: Option<String>,
    /// A Wayland session (`$WAYLAND_DISPLAY`).
    pub wayland: bool,
    /// Where a command name is installed, if it is.
    pub installed: &'a dyn Fn(&str) -> Option<PathBuf>,
}

/// The command line that runs a program in a new terminal window, for
/// `purpose`, or `None` when no terminal is found.
pub fn detect(purpose: Purpose, env: &Environment) -> Option<String> {
    let line = |known: &Known, program: &str| {
        let args = match purpose {
            Purpose::Open => known.run,
            Purpose::Fff => known.classed,
        };
        std::iter::once(quote(program))
            .chain(args.iter().map(|a| a.to_string()))
            .collect::<Vec<_>>()
            .join(" ")
    };
    // The command for a terminal found as `path`: its name where that is
    // the one installed, else the path itself.
    let command = |known: &Known, path: &Path| match (env.installed)(known.name) {
        Some(_) => known.name.to_owned(),
        None => path.to_string_lossy().into_owned(),
    };
    for path in &env.ancestors {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(known) = by_process(name) {
            return Some(line(known, &command(known, path)));
        }
    }
    if let Some(terminal) = env.terminal.as_deref().filter(|t| !t.trim().is_empty()) {
        let terminal = terminal.trim();
        let name = Path::new(terminal).file_name().and_then(|n| n.to_str());
        return Some(match name.and_then(by_process) {
            Some(known) => line(known, terminal),
            // The convention `$TERMINAL` follows: `-e` runs a command.
            None => format!("{} -e", quote(terminal)),
        });
    }
    if (env.installed)("x-terminal-emulator").is_some() {
        return Some("x-terminal-emulator -e".into());
    }
    KNOWN
        .iter()
        .filter(|known| env.wayland || !known.wayland)
        .find(|known| (env.installed)(known.name).is_some())
        .map(|known| line(known, known.name))
}

/// The terminal for `purpose` in this session, found once per process.
pub fn current(purpose: Purpose) -> Option<String> {
    use std::sync::OnceLock;
    static FOUND: OnceLock<(Option<String>, Option<String>)> = OnceLock::new();
    let (open, fff) = FOUND.get_or_init(|| {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let installed = |name: &str| resolve(name, &cwd).ok();
        let env = Environment {
            ancestors: ancestors(),
            terminal: std::env::var("TERMINAL").ok(),
            wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            installed: &installed,
        };
        (detect(Purpose::Open, &env), detect(Purpose::Fff, &env))
    });
    match purpose {
        Purpose::Open => open.clone(),
        Purpose::Fff => fff.clone(),
    }
}

fn by_process(name: &str) -> Option<&'static Known> {
    KNOWN
        .iter()
        .find(|k| k.name == name || k.processes.contains(&name))
}

/// The executables of this process's ancestors, nearest first, from
/// `/proc`; none where there is no `/proc`.
fn ancestors() -> Vec<PathBuf> {
    let mut found = vec![];
    let mut pid = std::process::id();
    // A cycle cannot happen, but a bound costs nothing.
    for _ in 0..64 {
        let Some(parent) = std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .ok()
            .and_then(|stat| parent_pid(&stat))
        else {
            break;
        };
        if parent <= 1 {
            break;
        }
        if let Ok(exe) = std::fs::read_link(format!("/proc/{parent}/exe")) {
            found.push(exe);
        }
        pid = parent;
    }
    found
}

/// The parent's id in a `/proc/PID/stat` line: the second field after the
/// command name, which is in parentheses and may itself hold any of them.
fn parent_pid(stat: &str) -> Option<u32> {
    let (_, rest) = stat.rsplit_once(')')?;
    rest.split_whitespace().nth(1)?.parse().ok()
}

/// `word` as one word of a command line.
fn quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "/._-+".contains(c))
    {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', r"'\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn detect_with(
        purpose: Purpose,
        ancestors: &[&str],
        terminal: Option<&str>,
        wayland: bool,
        installed: &[&str],
    ) -> Option<String> {
        let installed = |name: &str| {
            installed
                .contains(&name)
                .then(|| Path::new("/usr/bin").join(name))
        };
        detect(
            purpose,
            &Environment {
                ancestors: ancestors.iter().map(PathBuf::from).collect(),
                terminal: terminal.map(str::to_owned),
                wayland,
                installed: &installed,
            },
        )
    }

    #[test]
    fn the_terminal_fff_runs_in_comes_first() {
        let ancestors = ["/usr/bin/zsh", "/usr/bin/alacritty", "/usr/bin/i3"];
        let open = detect_with(Purpose::Open, &ancestors, Some("st"), false, &["alacritty"]);
        assert_eq!(open.as_deref(), Some("alacritty -e"));
        let fff = detect_with(Purpose::Fff, &ancestors, Some("st"), false, &["alacritty"]);
        assert_eq!(fff.as_deref(), Some("alacritty --class fff-picker -e"));
        // Not on the PATH: its own path.
        let ancestors = ["/bin/sh", "/opt/my st/st"];
        let open = detect_with(Purpose::Open, &ancestors, None, false, &[]);
        assert_eq!(open.as_deref(), Some("'/opt/my st/st' -e"));
        // A server process stands for its terminal.
        let ancestors = ["/usr/libexec/gnome-terminal-server"];
        let open = detect_with(Purpose::Open, &ancestors, None, false, &["gnome-terminal"]);
        assert_eq!(open.as_deref(), Some("gnome-terminal --wait --"));
    }

    #[test]
    fn then_terminal_variable_then_installed_terminals() {
        let shells = ["/usr/bin/zsh", "/usr/bin/tmux"];
        let open = detect_with(Purpose::Fff, &shells, Some("kitty"), false, &["st"]);
        assert_eq!(open.as_deref(), Some("kitty --class fff-picker"));
        let open = detect_with(Purpose::Open, &shells, Some("myterm"), false, &["st"]);
        assert_eq!(open.as_deref(), Some("myterm -e"));
        let open = detect_with(Purpose::Open, &shells, Some(" "), false, &["xterm", "st"]);
        assert_eq!(open.as_deref(), Some("st -e"));
        let open = detect_with(
            Purpose::Open,
            &shells,
            None,
            false,
            &["x-terminal-emulator", "st"],
        );
        assert_eq!(open.as_deref(), Some("x-terminal-emulator -e"));
        // foot needs Wayland.
        let open = detect_with(Purpose::Open, &shells, None, false, &["foot", "xterm"]);
        assert_eq!(open.as_deref(), Some("xterm -e"));
        let open = detect_with(Purpose::Fff, &shells, None, true, &["foot", "xterm"]);
        assert_eq!(open.as_deref(), Some("foot --app-id fff-picker"));
        assert_eq!(detect_with(Purpose::Open, &shells, None, true, &[]), None);
    }

    #[test]
    fn parent_pid_reads_past_the_command_name() {
        assert_eq!(parent_pid("42 (a) b) S 7 42 42"), Some(7));
        assert_eq!(parent_pid("42 (st) S 1 42"), Some(1));
        assert_eq!(parent_pid("garbage"), None);
    }

    #[test]
    fn detected_lines_split_back_into_their_words() {
        for known in KNOWN {
            for args in [known.run, known.classed] {
                let line = std::iter::once(known.name)
                    .chain(args.iter().copied())
                    .collect::<Vec<_>>()
                    .join(" ");
                let words = crate::run::split(&line, None).unwrap();
                assert_eq!(words[1..], *args);
            }
        }
        let words = crate::run::split(&quote("/a b/it's"), None).unwrap();
        assert_eq!(words, ["/a b/it's"]);
    }
}
