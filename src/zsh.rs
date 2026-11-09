//! What completes a word of the command prompt (gx/gX), asked of the user's
//! own shell rather than worked out here: a zsh kept beside the session,
//! with the user's configuration, answers with the matches its completion
//! system finds for a line (options, subcommands, whatever its functions
//! know). It is started on a thread of its own when a prompt opens
//! (`warm`), so the first Tab rarely waits for it.
//!
//! zsh has no interface for this, so the shell runs on a pseudo-terminal of
//! its own, which nothing reads, and talks over two pipes it inherits: a
//! request (the folder and the line) wakes a line-editor widget, which
//! completes the line with `compadd` wrapped to write each match back, and
//! inserts and lists nothing. Nothing of the line is typed into the shell
//! or run by it.
use std::{
    ffi::OsStr,
    fs::File,
    io::{self, Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

/// How long the shell may take to start, with the user's configuration.
const START: Duration = Duration::from_secs(3);
/// How long Tab waits for an answer; a shell that takes longer is ended.
const ANSWER: Duration = Duration::from_millis(1500);
/// Matches taken from one answer, at most.
const MOST: usize = 2000;

/// What the shell runs once started: `compadd` writes the matches it adds
/// to `$FFF_OUT`, each as its kind (`f` a file, `-` anything else) and the
/// whole word, ended by a NUL; a request on `$FFF_IN` (the folder and the
/// line, each ended by a NUL) is completed by the plain completer alone,
/// and answered with its matches and then `!`.
const SETUP: &str = r##"unset HISTFILE FFF_ZSH
typeset -g +x FFF_IN FFF_OUT
unalias -m '*'
PROMPT= RPROMPT=
precmd_functions=() preexec_functions=() chpwd_functions=()
(( $+functions[compdef] )) || { autoload -Uz compinit && compinit -u -D }
compadd() {
  local __fff_a
  for __fff_a in "$@"; do
    [[ $__fff_a == (-|--) ]] && break
    [[ $__fff_a == -[^-]*[OAD]* || $__fff_a == -[OAD]* ]] && { builtin compadd "$@"; return }
  done
  local -a __fff_h
  builtin compadd -O __fff_h "$@"
  builtin compadd "$@"
  local __fff_r=$?
  (( $#__fff_h )) || return $__fff_r
  setopt localoptions extendedglob norcexpandparam noshwordsplit
  local -A __fff_P __fff_p __fff_s __fff_S
  zparseopts -E P:=__fff_P p:=__fff_p s:=__fff_s S:=__fff_S
  local __fff_o="${${@[1,(i)(-|--)]}//-default-/}" __fff_k=- __fff_q= __fff_w __fff_e=$__fff_S[-S]
  [[ " $__fff_o" == *\ -[[:alnum:]]#f* ]] && __fff_k=f
  [[ " $__fff_o" == *\ -[[:alnum:]]#Q* ]] && __fff_q=1
  [[ $__fff_e == [[:space:]]# ]] && __fff_e=
  local -a __fff_out
  for __fff_a in "${__fff_h[@]}"; do
    __fff_w=$IPREFIX$__fff_P[-P]$__fff_p[-p]$__fff_a$__fff_s[-s]$__fff_e
    [[ -n $__fff_q ]] && __fff_w=${(Q)__fff_w}
    __fff_out+=("$__fff_k$__fff_w")
  done
  print -rNu$FFF_OUT -- "${__fff_out[@]}"
  return $__fff_r
}
__fff_matches() { _main_complete _complete; compstate[insert]= compstate[list]= }
zle -C __fff_complete complete-word __fff_matches
__fff_request() {
  local __fff_cwd __fff_line
  IFS= read -r -u $FFF_IN -d $'\0' __fff_cwd && IFS= read -r -u $FFF_IN -d $'\0' __fff_line || { zle -F $FFF_IN; exit }
  builtin cd -q -- $__fff_cwd 2>/dev/null
  BUFFER=$__fff_line CURSOR=$#__fff_line
  zle __fff_complete
  BUFFER=
  print -rNu$FFF_OUT -- '!'
}
zle -N __fff_request
zle -Fw $FFF_IN __fff_request
print -rNu$FFF_OUT -- '!'
"##;

/// A word the shell offers in place of the one being completed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// The whole word, as the program would receive it: nothing is quoted.
    pub word: String,
    /// It names a file or folder.
    pub file: bool,
}

/// A zsh waiting for requests.
struct Helper {
    child: Child,
    requests: io::PipeWriter,
    /// What the shell wrote back, as it arrives, and what of it is not
    /// read yet.
    answers: mpsc::Receiver<Vec<u8>>,
    unread: Vec<u8>,
}
impl Helper {
    /// Starts `zsh` with the user's configuration (and `env`), waiting until
    /// it takes requests; `None` when it does not within `START`.
    fn start(zsh: &Path, env: &[(&str, &OsStr)]) -> Option<Self> {
        let (tty, slave) = pty().ok()?;
        let (requests_in, requests) = io::pipe().ok()?;
        let (mut answers_in, answers_out) = io::pipe().ok()?;
        let inherited = [requests_in.as_raw_fd(), answers_out.as_raw_fd()];
        let mut command = Command::new(zsh);
        command
            // The line starting it, typed below, is kept out of the history.
            .args(["-i", "-o", "histignorespace"])
            .env("FFF_ZSH", SETUP)
            .env("FFF_IN", inherited[0].to_string())
            .env("FFF_OUT", inherited[1].to_string())
            .envs(env.iter().copied())
            .stdin(Stdio::from(slave.try_clone().ok()?))
            .stdout(Stdio::from(slave.try_clone().ok()?))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls, as pre_exec requires. The
        // shell leads a session of its own with the pseudo-terminal as its
        // terminal, and keeps the two pipe ends past exec.
        unsafe {
            command.pre_exec(move || {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                for fd in inherited {
                    if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = command.spawn().ok()?;
        drop((command, requests_in, answers_out));
        let (sender, answers) = mpsc::channel();
        std::thread::spawn(move || {
            let mut chunk = [0; 8192];
            while let Ok(n @ 1..) = answers_in.read(&mut chunk) {
                if sender.send(chunk[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let mut helper = Self {
            child,
            requests,
            answers,
            unread: Vec::new(),
        };
        // What the shell prints goes nowhere, and never fills the terminal.
        let mut typed = tty.try_clone().ok()?;
        std::thread::spawn(move || {
            let mut tty = tty;
            let mut chunk = [0; 4096];
            while let Ok(1..) = tty.read(&mut chunk) {}
        });
        typed.write_all(b" eval \"$FFF_ZSH\"\n").ok()?;
        helper.answer(Instant::now() + START)?;
        Some(helper)
    }
    /// The matches for the last word of `line`, completed in `cwd`; `None`
    /// when the shell did not answer in time.
    fn ask(&mut self, cwd: &Path, line: &str) -> Option<Vec<Match>> {
        let cwd = cwd.as_os_str().as_bytes();
        if cwd.contains(&0) || line.contains('\0') {
            return Some(Vec::new());
        }
        let request = [cwd, b"\0", line.as_bytes(), b"\0"].concat();
        self.requests.write_all(&request).ok()?;
        self.answer(Instant::now() + ANSWER)
    }
    /// Reads matches up to the end of an answer.
    fn answer(&mut self, deadline: Instant) -> Option<Vec<Match>> {
        let mut matches = Vec::new();
        loop {
            while let Some(end) = self.unread.iter().position(|&b| b == 0) {
                let record: Vec<u8> = self.unread.drain(..=end).collect();
                match &record[..end] {
                    b"!" => return Some(matches),
                    [kind @ (b'f' | b'-'), word @ ..] if matches.len() < MOST => {
                        if let Ok(word) = std::str::from_utf8(word)
                            && !word.is_empty()
                            && !word.contains(char::is_control)
                        {
                            matches.push(Match {
                                word: word.to_owned(),
                                file: *kind == b'f',
                            });
                        }
                    }
                    _ => {}
                }
            }
            let left = deadline.saturating_duration_since(Instant::now());
            self.unread.extend(self.answers.recv_timeout(left).ok()?);
        }
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        // The shell and what its completion functions started, then reaped.
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A pseudo-terminal: the side fff holds and the side the shell runs on,
/// neither kept by programs fff starts.
fn pty() -> io::Result<(File, File)> {
    let (mut master, mut slave) = (0, 0);
    let mut size = libc::winsize {
        ws_row: 50,
        ws_col: 200,
        ws_xpixel: 0,
        ws_ypixel: 0,
    };
    // SAFETY: openpty writes the two descriptors, which are then owned
    // here; the name is not asked for and the size outlives the call.
    unsafe {
        if libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut::<libc::termios>() as _,
            &mut size as *mut libc::winsize as _,
        ) != 0
        {
            return Err(io::Error::last_os_error());
        }
        let ends = (File::from_raw_fd(master), File::from_raw_fd(slave));
        for fd in [master, slave] {
            libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        Ok(ends)
    }
}

enum State {
    /// None runs: the next prompt starts one.
    Idle,
    Running(Helper),
    /// It did not start: not tried again this session.
    Unavailable,
}
static HELPER: Mutex<State> = Mutex::new(State::Idle);

/// The user's shell when it is zsh: `$SHELL`, by its name. Another zsh
/// installed is not theirs to configure, and would greet a new user.
fn users() -> Option<PathBuf> {
    let shell = PathBuf::from(std::env::var_os("SHELL")?);
    (shell.is_absolute() && shell.file_name()? == "zsh" && shell.is_file()).then_some(shell)
}

/// A command prompt opened: starts the shell on a thread of its own, unless
/// one runs or the user's shell is not zsh.
pub fn warm() {
    // Tests complete without the shell of whoever runs them.
    if cfg!(test) {
        return;
    }
    if let Ok(state) = HELPER.try_lock()
        && !matches!(*state, State::Idle)
    {
        return;
    }
    let Some(zsh) = users() else {
        return;
    };
    std::thread::spawn(move || {
        let mut state = HELPER.lock().unwrap();
        if matches!(*state, State::Idle) {
            *state = match Helper::start(&zsh, &[]) {
                Some(helper) => State::Running(helper),
                None => State::Unavailable,
            };
        }
    });
}

/// What the shell completes the last word of `line` with, in `cwd`; `None`
/// without a shell to ask. Waits for one still starting, and at most
/// `ANSWER` for its answer: a shell that takes longer is ended, and the
/// next prompt starts another.
pub fn matches(cwd: &Path, line: &str) -> Option<Vec<Match>> {
    let mut state = HELPER.lock().unwrap();
    let State::Running(helper) = &mut *state else {
        return None;
    };
    let found = helper.ask(cwd, line);
    if found.is_none() {
        *state = State::Idle;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt};

    #[test]
    fn zsh_answers_with_what_its_own_configuration_completes() {
        // zsh may be absent on a test machine.
        let Ok(zsh) = crate::run::resolve("zsh", Path::new("/")) else {
            return;
        };
        let dir = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("fff-zsh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("work/sub dir")).unwrap();
        let work = dir.join("work");
        fs::write(work.join("alpha.txt"), b"").unwrap();
        fs::write(work.join("alps.txt"), b"").unwrap();
        // A program that ignores its arguments, as a script may.
        let ran = dir.join("ran");
        fs::write(
            work.join("deploy"),
            format!("#!/bin/sh\necho ran > '{}'\n", ran.display()),
        )
        .unwrap();
        fs::set_permissions(work.join("deploy"), fs::Permissions::from_mode(0o755)).unwrap();
        // The configuration is read: it says what completes `tool`, and its
        // alias does not stand for the program fff would run.
        // Where this zsh reads its configuration with `dir` as the home
        // folder: a system file may name another place than the home.
        let asked = Command::new(&zsh)
            .args(["-c", "print -rn -- ${ZDOTDIR:-$HOME}"])
            .env("HOME", &dir)
            .env_remove("ZDOTDIR")
            .output()
            .unwrap();
        let config = PathBuf::from(OsStr::from_bytes(&asked.stdout));
        assert!(config.starts_with(&dir), "{config:?}");
        fs::create_dir_all(&config).unwrap();
        fs::write(
            config.join(".zshrc"),
            "autoload -Uz compinit && compinit -u -D\n\
             _tool() { _arguments '--and-exit[stop]' '--color=[when]:when:(always never)' '*:file:_files' }\n\
             compdef _tool tool\nalias tool='ls'\n",
        )
        .unwrap();
        let env = [("HOME", dir.as_os_str()), ("ZDOTDIR", config.as_os_str())];
        let mut helper = Helper::start(&zsh, &env).expect("zsh starts");
        let mut words = |line: &str| {
            let mut found = helper.ask(&work, line).expect("an answer");
            found.sort_by(|a, b| a.word.cmp(&b.word));
            found
                .into_iter()
                .map(|m| format!("{}{}", if m.file { "f:" } else { "" }, m.word))
                .collect::<Vec<_>>()
        };
        assert_eq!(words("tool --an"), ["--and-exit"]);
        assert_eq!(words("tool --color=a"), ["--color=always"]);
        assert_eq!(words("tool al"), ["f:alpha.txt", "f:alps.txt"]);
        assert_eq!(words("tool su"), ["f:sub dir"]);
        // Keys bound in a configuration mean nothing in a line: none is typed.
        assert_eq!(words("tool \t\u{15}al"), [""; 0]);
        // An unknown program is never run to learn its options.
        assert_eq!(words("./deploy -"), [""; 0]);
        assert_eq!(words("./deploy --he"), [""; 0]);
        assert!(!ran.exists());
        drop(helper);
        fs::remove_dir_all(&dir).unwrap();
    }
}
