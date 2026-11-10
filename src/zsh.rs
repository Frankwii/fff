//! gx and gX when the user's shell is zsh: the command line is the user's
//! own zsh, at the bottom of fff's screen. It starts with the user's
//! configuration, edits the line with its own line editor (`vared`), the
//! chosen paths already on it, and runs it as it runs any line: aliases,
//! functions, Tab and history are the user's. `src/zsh/line.zsh` is what it
//! runs, given as `-c` with the paths as its arguments.
use crate::run::Plan;
use std::{
    ffi::OsString,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

const LINE: &str = include_str!("zsh/line.zsh");

/// The user's shell when it is zsh: `$SHELL`, by its name.
pub fn users() -> Option<PathBuf> {
    let shell = PathBuf::from(std::env::var_os("SHELL")?);
    (shell.is_absolute() && shell.file_name()? == "zsh" && shell.is_file()).then_some(shell)
}

/// A zsh ready to take one line, and where it says what ran.
pub struct Line {
    pub plan: Plan,
    typed: Option<Typed>,
}
impl Line {
    /// zsh in `cwd` with `paths` on its line; `exit` (gX) when it takes
    /// fff's place.
    pub fn new(zsh: &Path, cwd: &Path, paths: &[PathBuf], exit: bool) -> Self {
        let prompt = if exit {
            "Run and exit ❯ "
        } else {
            "Run ❯ "
        };
        let mut env: Vec<(String, OsString)> =
            vec![("FFF_PROMPT".into(), format!("%B{prompt}%b").into())];
        let typed = (!exit).then(Typed::new);
        if let Some(typed) = &typed {
            env.push(("FFF_LINE".into(), typed.0.clone().into()));
        }
        let plan = Plan {
            program: zsh.to_owned(),
            name: "zsh".into(),
            args: ["-i", "-c", LINE, "zsh"].map(String::from).into(),
            paths: paths.iter().map(|path| word(path, cwd)).collect(),
            cwd: cwd.to_owned(),
            exit,
            line: "zsh".into(),
            env,
        };
        Self { plan, typed }
    }
    /// The line that ran, once zsh has ended; `None` when nothing did.
    pub fn typed(&self) -> Option<String> {
        let line = fs::read(&self.typed.as_ref()?.0).ok()?;
        Some(String::from_utf8_lossy(&line).trim().to_owned()).filter(|l| !l.is_empty())
    }
}

/// Where zsh writes the line it ran, removed with the `Line`.
struct Typed(PathBuf);
impl Typed {
    fn new() -> Self {
        let base = std::env::var_os("XDG_RUNTIME_DIR")
            .map(PathBuf::from)
            .filter(|d| d.is_absolute() && d.is_dir())
            .unwrap_or_else(std::env::temp_dir);
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self(base.join(format!("fff-zsh-{}-{stamp}", std::process::id())))
    }
}
impl Drop for Typed {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// How a path is written on the line: relative to `cwd` when inside it, so
/// `nvim notes.md` rather than its whole path, and never read as an option.
fn word(path: &Path, cwd: &Path) -> PathBuf {
    match path.strip_prefix(cwd) {
        Ok(relative) if relative.as_os_str().as_bytes().starts_with(b"-") => {
            Path::new(".").join(relative)
        }
        Ok(relative) if !relative.as_os_str().is_empty() => relative.to_owned(),
        _ => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs::File,
        io::{Read, Write},
        os::{fd::FromRawFd, unix::process::CommandExt},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    #[test]
    fn paths_on_the_line_are_short_and_never_options() {
        let cwd = Path::new("/w");
        assert_eq!(word(Path::new("/w/a b"), cwd), Path::new("a b"));
        assert_eq!(word(Path::new("/w/-rf"), cwd), Path::new("./-rf"));
        assert_eq!(word(Path::new("/w/sub/x"), cwd), Path::new("sub/x"));
        assert_eq!(word(Path::new("/other/x"), cwd), Path::new("/other/x"));
        assert_eq!(word(Path::new("/w"), cwd), Path::new("/w"));
    }

    /// A pseudo-terminal: the side the test holds and the side zsh runs on.
    fn pty() -> (File, File) {
        let (mut master, mut slave) = (0, 0);
        let mut size = libc::winsize {
            ws_row: 24,
            ws_col: 200,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: openpty writes the two descriptors, owned from here on.
        unsafe {
            assert_eq!(
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut::<libc::termios>() as _,
                    &mut size as *mut libc::winsize as _,
                ),
                0
            );
            (File::from_raw_fd(master), File::from_raw_fd(slave))
        }
    }

    #[test]
    fn the_line_is_zsh_with_the_users_aliases_functions_and_completion() {
        // zsh may be absent on a test machine.
        let Ok(zsh) = crate::run::resolve("zsh", Path::new("/")) else {
            return;
        };
        let dir = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("fff-zsh-line-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let work = dir.join("work");
        fs::create_dir_all(&work).unwrap();
        fs::write(work.join("my notes.md"), b"").unwrap();
        let out = dir.join("out");
        // Where this zsh reads its configuration with `dir` as the home
        // folder: a system file may name another place than the home.
        let asked = Command::new(&zsh)
            .args(["-c", "print -rn -- ${ZDOTDIR:-$HOME}"])
            .env("HOME", &dir)
            .env_remove("ZDOTDIR")
            .output()
            .unwrap();
        let config = PathBuf::from(std::ffi::OsStr::from_bytes(&asked.stdout));
        assert!(config.starts_with(&dir), "{config:?}");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join(".zshrc"),
            format!(
                "bindkey -e\n\
                 autoload -Uz compinit && compinit -u -D\n\
                 record() {{ printf '%s\\n' \"$@\" >> '{}' }}\n\
                 _record() {{ _arguments '--and-exit[stop]' '*:file:_files' }}\n\
                 compdef _record record\n\
                 alias r='record --via-alias'\n",
                out.display()
            ),
        )
        .unwrap();
        // Runs one line: `keys` typed, then what zsh wrote back and the line
        // it says ran.
        let run = |keys: &[u8], exit: bool| {
            let line = Line::new(&zsh, &work, &[work.join("my notes.md")], exit);
            let (mut tty, slave) = pty();
            let mut command = Command::new(&line.plan.program);
            command
                .args(&line.plan.args)
                .args(&line.plan.paths)
                .envs(line.plan.env.iter().map(|(k, v)| (k, v)))
                .env("HOME", &dir)
                .env_remove("ZDOTDIR")
                .env("TERM", "xterm")
                .current_dir(&line.plan.cwd)
                .stdin(Stdio::from(slave.try_clone().unwrap()))
                .stdout(Stdio::from(slave.try_clone().unwrap()))
                .stderr(Stdio::from(slave));
            // SAFETY: only async-signal-safe calls, as pre_exec requires.
            unsafe {
                command.pre_exec(|| {
                    libc::setsid();
                    libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                    Ok(())
                });
            }
            let mut child = command.spawn().unwrap();
            drop(command);
            let mut reader = tty.try_clone().unwrap();
            let (sender, received) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let mut chunk = [0; 4096];
                while let Ok(n @ 1..) = reader.read(&mut chunk) {
                    let _ = sender.send(chunk[..n].to_vec());
                }
            });
            let mut screen = Vec::new();
            let mut until = |token: &[u8]| {
                let deadline = Instant::now() + Duration::from_secs(20);
                while !screen.windows(token.len()).any(|w| w == token) {
                    let left = deadline.saturating_duration_since(Instant::now());
                    match received.recv_timeout(left) {
                        Ok(bytes) => screen.extend(bytes),
                        Err(_) => {
                            panic!("no {:?} in {:?}", token, String::from_utf8_lossy(&screen))
                        }
                    }
                }
            };
            until("Run".as_bytes());
            for key in keys.split(|&b| b == b'|') {
                tty.write_all(key).unwrap();
                std::thread::sleep(Duration::from_millis(300));
            }
            let status = child.wait().unwrap();
            while let Ok(bytes) = received.recv_timeout(Duration::from_millis(100)) {
                screen.extend(bytes);
            }
            (
                status.code(),
                line.typed(),
                String::from_utf8_lossy(&screen).into_owned(),
            )
        };
        let ran = || fs::read_to_string(&out).unwrap_or_default();
        // The paths wait after the cursor; zsh completes the function's
        // options from the configuration, and runs it.
        let (code, typed, _) = run(b"record --an|\t|\r", false);
        assert_eq!(code, Some(0));
        assert_eq!(typed.as_deref(), Some("record --and-exit 'my notes.md'"));
        assert_eq!(ran(), "--and-exit\nmy notes.md\n");
        // An alias is what it is in a terminal.
        fs::remove_file(&out).unwrap();
        run(b"r\r", false);
        assert_eq!(ran(), "--via-alias\nmy notes.md\n");
        // What a command returns is zsh's status; an empty line or Ctrl+C
        // runs nothing.
        assert_eq!(run(b"\x01\x0bfalse\r", false).0, Some(1));
        assert_eq!(run(b"\x01\x0b\r", false).1, None);
        fs::remove_file(&out).unwrap();
        assert_eq!(run(b"r\x03", false).1, None);
        assert_eq!(ran(), "");
        // gX: nothing written back for fff, which is gone; the line stays
        // above what it prints.
        let _ = fs::remove_file(&out);
        let (code, typed, screen) = run(b"r\r", true);
        assert_eq!((code, typed), (Some(0), None));
        // Ended by a newline, as only the line printed after editing is.
        assert!(screen.contains("r 'my notes.md'\r\n"), "{screen}");
        assert_eq!(ran(), "--via-alias\nmy notes.md\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
