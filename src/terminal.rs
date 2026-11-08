//! The terminal emulator fff runs programs in when `[open] terminal` or
//! `[open] fff_terminal` is unset: the one fff itself runs in (among its
//! parent processes, else by `$TERM_PROGRAM`), else `$TERMINAL`, else the
//! first known one installed; on macOS, Terminal.app at the last.
//!
//! Each known terminal has its own way to run a command, and to give fff's
//! windows the `fff-picker` class (or Wayland app id) where it can. The
//! commands keep the terminal in the foreground until the program exits:
//! the file chooser waits for it. The scripts for macOS's terminals do not
//! wait; nothing there does (no file chooser), and opening returns at once.
use crate::run::resolve;
use std::path::{Path, PathBuf};

/// The class (or app id) of fff's own windows, for window managers.
pub const CLASS: &str = "fff-picker";

/// The systems a known terminal's command line works on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Systems {
    All,
    MacOs,
    NotMacOs,
}

struct Known {
    /// The command, which is also the name its process runs under.
    name: &'static str,
    /// Other names its process may run under (a server, a GUI binary).
    processes: &'static [&'static str],
    /// `$TERM_PROGRAM` in its windows, where it sets one.
    term_program: Option<&'static str>,
    /// Arguments before a command to run in a new window.
    run: &'static [&'static str],
    /// The same, with `CLASS` for the window where the terminal has one.
    classed: &'static [&'static str],
    /// The whole command line instead, for a terminal that runs no command
    /// given on its own: the program's words follow it, run in a new
    /// window in the directory the line runs in.
    script: &'static [&'static str],
    /// Only runs under Wayland.
    wayland: bool,
    systems: Systems,
}

/// The AppleScript that runs its arguments in the directory `osascript` runs
/// in, in a new window of `$app`, through the shell there: macOS's own
/// terminals take no command on their command lines. `$tell` types it.
macro_rules! applescript {
    ($app:literal, $($tell:literal),+) => {
        &[
            "osascript",
            "-e",
            "on run argv",
            "-e",
            r#"set command to "cd " & quoted form of (do shell script "pwd") & " && exec""#,
            "-e",
            "repeat with a in argv",
            "-e",
            r#"set command to command & " " & quoted form of (a as text)"#,
            "-e",
            "end repeat",
            "-e",
            concat!("tell application \"", $app, "\""),
            "-e",
            "activate",
            $("-e", $tell,)+
            "-e",
            "end tell",
            "-e",
            "end run",
        ]
    };
}

/// In order of preference when none is running fff or named by `$TERMINAL`.
const KNOWN: &[Known] = &[
    Known {
        name: "st",
        processes: &[],
        term_program: None,
        run: &["-e"],
        classed: &["-c", CLASS, "-e"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    Known {
        name: "alacritty",
        processes: &[],
        term_program: None,
        run: &["-e"],
        classed: &["--class", CLASS, "-e"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    Known {
        name: "kitty",
        processes: &[],
        term_program: None,
        run: &[],
        classed: &["--class", CLASS],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    Known {
        name: "wezterm",
        processes: &["wezterm-gui"],
        term_program: Some("WezTerm"),
        run: &["start", "--always-new-process", "--"],
        classed: &["start", "--always-new-process", "--class", CLASS, "--"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    // Ghostty's class must be a dotted application id.
    Known {
        name: "ghostty",
        processes: &[],
        term_program: Some("ghostty"),
        run: &["-e"],
        classed: &["-e"],
        script: &[],
        wayland: false,
        systems: Systems::NotMacOs,
    },
    // On macOS, Ghostty's command runs no window: a new instance of the app
    // does, which would otherwise stay open after it.
    Known {
        name: "ghostty",
        processes: &[],
        term_program: Some("ghostty"),
        run: &[],
        classed: &[],
        script: &[
            "sh",
            "-c",
            r#"exec open -na Ghostty --args --quit-after-last-window-closed=true --working-directory="$(pwd)" -e "$@""#,
            "sh",
        ],
        wayland: false,
        systems: Systems::MacOs,
    },
    Known {
        name: "foot",
        processes: &["footclient"],
        term_program: None,
        run: &[],
        classed: &["--app-id", CLASS],
        script: &[],
        wayland: true,
        systems: Systems::All,
    },
    Known {
        name: "konsole",
        processes: &[],
        term_program: None,
        run: &["--nofork", "-e"],
        classed: &["--nofork", "-e"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    Known {
        name: "gnome-terminal",
        processes: &["gnome-terminal-server"],
        term_program: None,
        run: &["--wait", "--"],
        classed: &["--wait", "--"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    Known {
        name: "xfce4-terminal",
        processes: &[],
        term_program: None,
        run: &["--disable-server", "-x"],
        classed: &["--disable-server", "-x"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    Known {
        name: "urxvt",
        processes: &["rxvt", "urxvtd"],
        term_program: None,
        run: &["-e"],
        classed: &["-e"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    Known {
        name: "xterm",
        processes: &["uxterm"],
        term_program: None,
        run: &["-e"],
        classed: &["-class", CLASS, "-e"],
        script: &[],
        wayland: false,
        systems: Systems::All,
    },
    // Not commands, so never found installed: Terminal.app is the last
    // resort on macOS. iTerm2's shells descend from a server of its own,
    // not from iTerm2.
    Known {
        name: "Terminal",
        processes: &[],
        term_program: Some("Apple_Terminal"),
        run: &[],
        classed: &[],
        script: applescript!("Terminal", "do script command"),
        wayland: false,
        systems: Systems::MacOs,
    },
    Known {
        name: "iTerm2",
        processes: &[],
        term_program: Some("iTerm.app"),
        run: &[],
        classed: &[],
        script: applescript!(
            "iTerm2",
            "set w to (create window with default profile)",
            "tell current session of w to write text command"
        ),
        wayland: false,
        systems: Systems::MacOs,
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
    /// `$TERM_PROGRAM`.
    pub term_program: Option<String>,
    /// `$TERMINAL`.
    pub terminal: Option<String>,
    /// A Wayland session (`$WAYLAND_DISPLAY`).
    pub wayland: bool,
    /// Running on macOS.
    pub macos: bool,
    /// Where a command name is installed, if it is.
    pub installed: &'a dyn Fn(&str) -> Option<PathBuf>,
}

/// The command line that runs a program in a new terminal window, for
/// `purpose`, or `None` when no terminal is found.
pub fn detect(purpose: Purpose, env: &Environment) -> Option<String> {
    let line = |known: &Known, program: &str| {
        if !known.script.is_empty() {
            return known
                .script
                .iter()
                .map(|w| quote(w))
                .collect::<Vec<_>>()
                .join(" ");
        }
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
    let here = |known: &&Known| match known.systems {
        Systems::All => true,
        Systems::MacOs => env.macos,
        Systems::NotMacOs => !env.macos,
    };
    let by_process = |name: &str| {
        KNOWN
            .iter()
            .filter(here)
            .find(|k| k.name == name || k.processes.contains(&name))
    };
    for path in &env.ancestors {
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some(known) = by_process(name) {
            return Some(line(known, &command(known, path)));
        }
    }
    // Set by the terminal and inherited by what runs in it, also where its
    // process is no ancestor of fff's. Only a script or an installed
    // command can run it.
    if let Some(program) = env.term_program.as_deref() {
        let found = KNOWN
            .iter()
            .filter(here)
            .filter(|k| k.term_program == Some(program))
            .find(|k| !k.script.is_empty() || (env.installed)(k.name).is_some());
        if let Some(known) = found {
            return Some(line(known, known.name));
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
        .filter(here)
        .filter(|known| env.wayland || !known.wayland)
        .find(|known| (env.installed)(known.name).is_some())
        .or_else(|| env.macos.then(|| by_process("Terminal")).flatten())
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
            term_program: std::env::var("TERM_PROGRAM").ok(),
            terminal: std::env::var("TERMINAL").ok(),
            wayland: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            macos: cfg!(target_os = "macos"),
            installed: &installed,
        };
        (detect(Purpose::Open, &env), detect(Purpose::Fff, &env))
    });
    match purpose {
        Purpose::Open => open.clone(),
        Purpose::Fff => fff.clone(),
    }
}

/// The executables of this process's ancestors, nearest first.
fn ancestors() -> Vec<PathBuf> {
    let mut found = vec![];
    let mut pid = std::process::id();
    // A cycle cannot happen, but a bound costs nothing.
    for _ in 0..64 {
        let Some(parent) = crate::platform::parent(pid) else {
            break;
        };
        if parent <= 1 {
            break;
        }
        if let Some(exe) = crate::platform::executable(&parent.to_string()) {
            found.push(exe);
        }
        pid = parent;
    }
    found
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
        detect_on(
            false, purpose, ancestors, None, terminal, wayland, installed,
        )
    }

    /// On macOS, opening (the file chooser is not there).
    fn detect_mac(
        ancestors: &[&str],
        term_program: Option<&str>,
        terminal: Option<&str>,
        installed: &[&str],
    ) -> Option<String> {
        let open = Purpose::Open;
        detect_on(
            true,
            open,
            ancestors,
            term_program,
            terminal,
            false,
            installed,
        )
    }

    fn detect_on(
        macos: bool,
        purpose: Purpose,
        ancestors: &[&str],
        term_program: Option<&str>,
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
                term_program: term_program.map(str::to_owned),
                terminal: terminal.map(str::to_owned),
                wayland,
                macos,
                installed: &installed,
            },
        )
    }

    /// The words of a detected line.
    fn words_of(line: Option<String>) -> Vec<String> {
        crate::run::split(&line.expect("a terminal"), None).unwrap()
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
    fn macos_terminals_by_process_then_by_term_program() {
        let shells = ["/bin/zsh", "/usr/bin/login"];
        let terminal_app = "/System/Applications/Utilities/Terminal.app/Contents/MacOS/Terminal";
        let words = words_of(detect_mac(
            &[shells[0], shells[1], terminal_app],
            None,
            None,
            &[],
        ));
        assert_eq!(words[..3], ["osascript", "-e", "on run argv"]);
        assert!(words.iter().any(|w| w == r#"tell application "Terminal""#));
        assert!(words.iter().any(|w| w == "do script command"));
        // An app's own binary, not on the PATH.
        let kitty = "/Applications/kitty.app/Contents/MacOS/kitty";
        let open = detect_mac(&[shells[0], kitty], Some("Apple_Terminal"), None, &[]);
        assert_eq!(open.as_deref(), Some(kitty));
        let ghostty = "/Applications/Ghostty.app/Contents/MacOS/ghostty";
        let words = words_of(detect_mac(&[shells[0], ghostty], None, None, &[]));
        assert_eq!(words[..2], ["sh", "-c"]);
        assert!(words[2].starts_with("exec open -na Ghostty --args"));
        // iTerm2's shells descend from its server; inside tmux, from tmux's.
        let words = words_of(detect_mac(&shells, Some("iTerm.app"), Some("kitty"), &[]));
        assert!(words.iter().any(|w| w == r#"tell application "iTerm2""#));
        let open = detect_mac(&shells, Some("WezTerm"), None, &["wezterm"]);
        assert_eq!(
            open.as_deref(),
            Some("wezterm start --always-new-process --")
        );
    }

    #[test]
    fn macos_falls_back_to_terminal_app() {
        let shells = ["/bin/zsh", "/opt/homebrew/bin/tmux"];
        let open = detect_mac(&shells, Some("tmux"), Some("alacritty"), &[]);
        assert_eq!(open.as_deref(), Some("alacritty -e"));
        let open = detect_mac(&shells, Some("tmux"), None, &["alacritty"]);
        assert_eq!(open.as_deref(), Some("alacritty -e"));
        // WezTerm's command not on the PATH: nothing to run it with.
        let words = words_of(detect_mac(&shells, Some("WezTerm"), None, &[]));
        assert!(words.iter().any(|w| w == r#"tell application "Terminal""#));
        let terminal = detect_mac(&shells, None, Some("Terminal"), &[]);
        assert_eq!(terminal, detect_mac(&shells, None, None, &[]));
    }

    #[test]
    fn macos_terminals_are_not_linux_ones() {
        let shells = ["/usr/bin/zsh"];
        let ghostty = ["/usr/bin/ghostty"];
        let open = detect_with(Purpose::Open, &ghostty, None, false, &["ghostty"]);
        assert_eq!(open.as_deref(), Some("ghostty -e"));
        // `$TERM_PROGRAM` that came along over ssh.
        let open = detect_on(
            false,
            Purpose::Open,
            &shells,
            Some("Apple_Terminal"),
            None,
            false,
            &["st"],
        );
        assert_eq!(open.as_deref(), Some("st -e"));
        let open = detect_on(
            false,
            Purpose::Open,
            &shells,
            Some("ghostty"),
            None,
            false,
            &["ghostty", "st"],
        );
        assert_eq!(open.as_deref(), Some("ghostty -e"));
        assert_eq!(
            detect_with(Purpose::Open, &shells, Some("Terminal"), false, &[]).as_deref(),
            Some("Terminal -e")
        );
        assert_eq!(detect_with(Purpose::Open, &shells, None, false, &[]), None);
    }

    #[test]
    fn ghostty_script_runs_the_words_in_the_directory() {
        let dir = std::env::temp_dir().join(format!("fff-terminal-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let work = dir.join("a 'dir'");
        std::fs::create_dir_all(&work).unwrap();
        // `open` stands in for macOS's, printing its arguments.
        let open = dir.join("open");
        std::fs::write(&open, "#!/bin/sh\nprintf '%s\\n' \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ghostty = KNOWN
            .iter()
            .find(|k| k.name == "ghostty" && !k.script.is_empty());
        let script = ghostty.unwrap().script;
        let output = std::process::Command::new(script[0])
            .args(&script[1..])
            .args(["/bin/vi", "it's a file", "$HOME"])
            .current_dir(&work)
            .env("PATH", format!("{}:/usr/bin:/bin", dir.display()))
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let printed = String::from_utf8(output.stdout).unwrap();
        let expected = format!(
            "-na\nGhostty\n--args\n--quit-after-last-window-closed=true\n\
             --working-directory={}\n-e\n/bin/vi\nit's a file\n$HOME\n",
            work.display()
        );
        assert_eq!(printed, expected);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn detected_lines_split_back_into_their_words() {
        for known in KNOWN {
            let line = known.script.iter().map(|w| quote(w)).collect::<Vec<_>>();
            let words = crate::run::split(&line.join(" "), None).unwrap();
            assert_eq!(words, known.script);
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
