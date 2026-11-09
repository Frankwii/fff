//! Running a program on chosen paths, without a shell. The typed line is split
//! like a shell would split words (quotes, backslashes, a leading `~`), but
//! nothing else is interpreted: the program is resolved on PATH before anything
//! runs, and the chosen paths are appended as separate arguments, so no file
//! name can be read as syntax.
use std::{
    ffi::OsString,
    fs, io,
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, ExitStatus},
};

/// Characters a shell would act on. Without a shell they would silently mean
/// something else, so they must be quoted to be passed literally.
const SHELL_SYNTAX: &[char] = &['|', '&', ';', '<', '>', '$', '`', '*', '?', '(', ')'];

/// Shell-style words: whitespace separates; '…' is literal; "…" allows \",
/// \\, \$ and \` (a bare `$` or backtick there is refused, as outside); a
/// backslash escapes any character; `~` or `~/` starts at the home folder.
pub fn split(text: &str, home: Option<&Path>) -> Result<Vec<String>, String> {
    let mut words = vec![];
    let mut word: Option<String> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if let Some(w) = word.take() {
                    words.push(w);
                }
            }
            '\'' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => w.push(c),
                        None => return Err("Unterminated ' quote".into()),
                    }
                }
            }
            '"' => {
                let w = word.get_or_insert_with(String::new);
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') if matches!(chars.peek(), Some('"' | '\\' | '$' | '`')) => {
                            w.push(chars.next().unwrap())
                        }
                        // A shell expands these even inside double quotes.
                        Some(c @ ('$' | '`')) => {
                            return Err(format!(
                                "fff runs commands without a shell; write \\{c} or '{c}' to pass '{c}' literally"
                            ));
                        }
                        Some(c) => w.push(c),
                        None => return Err("Unterminated \" quote".into()),
                    }
                }
            }
            '\\' => match chars.next() {
                Some(c) => word.get_or_insert_with(String::new).push(c),
                None => return Err("A trailing backslash escapes nothing".into()),
            },
            '~' if word.is_none()
                && chars.peek().is_none_or(|c| *c == '/' || c.is_whitespace()) =>
            {
                let home = home.ok_or("'~' needs $HOME to be set")?;
                word = Some(home.to_string_lossy().into_owned());
            }
            c if SHELL_SYNTAX.contains(&c) => {
                return Err(format!(
                    "fff runs commands without a shell; quote '{c}' to pass it literally"
                ));
            }
            c => word.get_or_insert_with(String::new).push(c),
        }
    }
    words.extend(word);
    Ok(words)
}

/// Backslash-escapes what `split` would otherwise interpret.
pub fn escape(word: &str) -> String {
    let mut out = String::new();
    for c in word.chars() {
        if c.is_whitespace() || SHELL_SYNTAX.contains(&c) || matches!(c, '\'' | '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    if out.starts_with('~') {
        out.insert(0, '\\');
    }
    out
}

fn executable(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}
fn search_path() -> Vec<PathBuf> {
    // Empty and relative PATH entries depend on the current directory; never search them.
    std::env::var_os("PATH")
        .map(|p| {
            std::env::split_paths(&p)
                .filter(|d| d.is_absolute())
                .collect()
        })
        .unwrap_or_default()
}
/// Every executable name on PATH, sorted and unique.
pub fn commands() -> Vec<String> {
    let mut names = search_path()
        .iter()
        .filter_map(|dir| fs::read_dir(dir).ok())
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| executable(&e.path()))
        .filter_map(|e| e.file_name().into_string().ok())
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}
/// The executable a command word names: a path when it contains `/`
/// (relative to `cwd`), otherwise the first match on PATH.
pub fn resolve(word: &str, cwd: &Path) -> Result<PathBuf, String> {
    if word.contains('/') {
        let path = cwd.join(word);
        return if executable(&path) {
            Ok(path)
        } else {
            Err(format!("{word}: not an executable file"))
        };
    }
    search_path()
        .into_iter()
        .map(|dir| dir.join(word))
        .find(|p| executable(p))
        .ok_or_else(|| format!("{word}: command not found"))
}
/// Starts `command` in a session of its own: it has no controlling terminal,
/// so it can neither open /dev/tty nor be hung up with fff's.
pub(crate) fn detach(command: &mut Command) {
    // SAFETY: setsid is async-signal-safe, as pre_exec requires.
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
}
/// Like `resolve`, naming the closest command when there is no match.
///
/// Programs are found on PATH only: a name that is an alias in the user's
/// shell (`e` for `nvim`) is not one, which the error says.
pub fn resolve_or_suggest(word: &str, cwd: &Path) -> Result<PathBuf, String> {
    resolve(word, cwd).map_err(|error| match suggestion(word, &commands()) {
        Some(near) if !word.contains('/') => format!("{error}; did you mean {near}?"),
        _ if !word.contains('/') => {
            format!("{error}; fff does not use your shell's aliases, so name the program itself")
        }
        _ => error,
    })
}
/// The closest command name, allowing one edit for short words and two for
/// longer ones; very short words get no suggestion.
fn suggestion<'a>(word: &str, names: &'a [String]) -> Option<&'a str> {
    let allowed = match word.chars().count() {
        0..=2 => return None,
        3..=6 => 1,
        _ => 2,
    };
    let distance = |a: &str, b: &str| {
        let b = b.chars().collect::<Vec<_>>();
        let mut row = (0..=b.len()).collect::<Vec<_>>();
        for (i, ca) in a.chars().enumerate() {
            let mut previous = row[0];
            row[0] = i + 1;
            for (j, cb) in b.iter().enumerate() {
                let current = row[j + 1];
                row[j + 1] = (previous + usize::from(ca != *cb))
                    .min(row[j] + 1)
                    .min(current + 1);
                previous = current;
            }
        }
        row[b.len()]
    };
    names
        .iter()
        .map(|n| (distance(word, n), n))
        .filter(|(d, _)| *d <= allowed)
        .min()
        .map(|(_, n)| n.as_str())
}

/// The command prompt: the typed line, the paths it will receive, and Tab
/// completion state.
#[derive(Clone, Debug)]
pub struct Prompt {
    pub paths: Vec<PathBuf>,
    /// Exit fff and become the program (gX) instead of returning (gx).
    pub exit: bool,
    pub text: String,
    /// Candidates of the last Tab, shown below the input; `chosen` cycles them.
    pub candidates: Vec<String>,
    pub chosen: Option<usize>,
    /// The line as Tab last left it; any other edit restarts completion.
    completed: String,
    commands: Option<Vec<String>>,
}
impl Prompt {
    pub fn new(paths: Vec<PathBuf>, exit: bool) -> Self {
        Self {
            paths,
            exit,
            text: String::new(),
            candidates: vec![],
            chosen: None,
            completed: String::new(),
            commands: None,
        }
    }
    /// Where the word being completed starts, and whether it is the command.
    fn current_word(&self) -> (usize, bool) {
        let mut start = 0;
        let mut words = 0;
        let mut escaped = false;
        let mut quote = None;
        for (i, c) in self.text.char_indices() {
            match (c, quote) {
                _ if escaped => escaped = false,
                // Inside single quotes a backslash is literal, as in `split`.
                ('\\', None | Some('"')) => escaped = true,
                ('\'' | '"', None) => quote = Some(c),
                (q, Some(open)) if q == open => quote = None,
                (c, None) if c.is_whitespace() => {
                    if i > start {
                        words += 1;
                    }
                    start = i + c.len_utf8();
                }
                _ => {}
            }
        }
        (start, words == 0)
    }
    /// Tab: complete the last word. A single candidate is inserted (with a
    /// space after a command or file); several insert their common prefix, and
    /// pressing Tab again cycles through them. The command is completed from
    /// PATH and a word with a `/` or a leading `~` as a path; any other word
    /// is what the user's shell completes it with (`zsh::matches`: options,
    /// subcommands, files), or a path when it offers nothing or is not asked.
    pub fn complete(&mut self, cwd: &Path, home: Option<&Path>) {
        self.complete_with(cwd, home, crate::zsh::matches);
    }
    /// `complete`, asking `shell` (the folder and the line) for the matches.
    fn complete_with(
        &mut self,
        cwd: &Path,
        home: Option<&Path>,
        shell: impl FnOnce(&Path, &str) -> Option<Vec<crate::zsh::Match>>,
    ) {
        let (start, command) = self.current_word();
        if self.text == self.completed && !self.candidates.is_empty() {
            let next = self.chosen.map_or(0, |i| (i + 1) % self.candidates.len());
            self.chosen = Some(next);
            self.text = format!("{}{}", &self.text[..start], self.candidates[next]);
            self.completed = self.text.clone();
            return;
        }
        let word = &self.text[start..];
        // Only a line fff itself would take: nothing in it is shell syntax.
        let asked = (!command
            && !word.contains('/')
            && !word.starts_with('~')
            && split(&self.text, home).is_ok())
        .then(|| shell(cwd, &self.text))
        .flatten()
        .filter(|matches| !matches.is_empty());
        let candidates = if let Some(matches) = asked {
            let mut words: Vec<String> = matches
                .iter()
                .map(|m| {
                    let folder = m.file && !m.word.ends_with('/') && cwd.join(&m.word).is_dir();
                    format!("{}{}", escape(&m.word), if folder { "/" } else { "" })
                })
                .collect();
            words.sort();
            words.dedup();
            words
        } else if command && !word.contains('/') {
            let commands = self.commands.get_or_insert_with(commands);
            commands
                .iter()
                .filter(|c| c.starts_with(word))
                .map(|c| escape(c))
                .collect()
        } else {
            paths(word, cwd, home, command)
        };
        self.chosen = None;
        self.candidates = candidates;
        match self.candidates.as_slice() {
            [] => {}
            [only] => {
                // A folder goes on, as does an option awaiting its value.
                let space = if only.ends_with(['/', '=']) { "" } else { " " };
                self.text = format!("{}{only}{space}", &self.text[..start]);
                self.candidates.clear();
            }
            many => {
                let common = many.iter().skip(1).fold(many[0].clone(), |prefix, c| {
                    prefix
                        .chars()
                        .zip(c.chars())
                        .take_while(|(a, b)| a == b)
                        .map(|(a, _)| a)
                        .collect()
                });
                if common.len() > word.len() {
                    self.text = format!("{}{common}", &self.text[..start]);
                }
            }
        }
        self.completed = self.text.clone();
    }
    /// What the program word resolves to now, for a live indication.
    pub fn program(&self, cwd: &Path, home: Option<&Path>) -> Option<Result<PathBuf, String>> {
        let words = split(&self.text, home).ok()?;
        words.first().map(|w| resolve(w, cwd))
    }
    /// Validates the line into a plan; any problem aborts with a message.
    pub fn plan(&self, cwd: &Path, home: Option<&Path>) -> Result<Plan, String> {
        let words = split(&self.text, home)?;
        let Some((name, args)) = words.split_first() else {
            return Err("No command typed; nothing ran".into());
        };
        Ok(Plan {
            program: resolve_or_suggest(name, cwd)?,
            name: name.clone(),
            args: args.to_vec(),
            paths: self.paths.clone(),
            cwd: cwd.to_owned(),
            exit: self.exit,
            line: self.text.trim().to_owned(),
        })
    }
}
/// Completions for a path word: entries of its directory starting with its
/// last component; directories end in `/`. Hidden entries only for a `.` prefix.
/// For the command word, only directories and executables are offered.
fn paths(word: &str, cwd: &Path, home: Option<&Path>, command: bool) -> Vec<String> {
    // A bare `~` names the home folder: complete inside it.
    let word = if word == "~" { "~/" } else { word };
    let typed = split(word, home)
        .ok()
        .and_then(|w| w.into_iter().next())
        .unwrap_or_default();
    let (dir, prefix) = match typed.rfind('/') {
        Some(i) => (&typed[..=i], &typed[i + 1..]),
        None => ("", typed.as_str()),
    };
    // Keep what was typed for the directory part; complete only the name.
    let shown = match word.rfind('/') {
        Some(i) => &word[..=i],
        None => "",
    };
    let Ok(entries) = fs::read_dir(cwd.join(if dir.is_empty() { "." } else { dir })) else {
        return vec![];
    };
    let mut out = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            if !name.starts_with(prefix) || (name.starts_with('.') && !prefix.starts_with('.')) {
                return None;
            }
            let directory = e.path().is_dir();
            if command && !directory && !executable(&e.path()) {
                return None;
            }
            Some(format!(
                "{shown}{}{}",
                escape(&name),
                if directory { "/" } else { "" }
            ))
        })
        .collect::<Vec<_>>();
    out.sort();
    out
}

/// A validated command: the resolved program, the typed arguments, then the paths.
#[derive(Clone, Debug)]
pub struct Plan {
    pub program: PathBuf,
    /// The command word as typed, used as the program's argv[0].
    pub name: String,
    pub args: Vec<String>,
    pub paths: Vec<PathBuf>,
    pub cwd: PathBuf,
    pub exit: bool,
    pub line: String,
}
impl Plan {
    /// The program reads and writes the terminal directly: fff's stdout may
    /// be a pipe carrying selected paths.
    fn command(&self) -> io::Result<Command> {
        let tty = crate::platform::open_terminal;
        let mut command = Command::new(&self.program);
        command
            .arg0(&self.name)
            .args(&self.args)
            .args(self.paths.iter().map(|p| OsString::from(p.as_os_str())))
            .current_dir(&self.cwd)
            .stdin(tty()?)
            .stdout(tty()?)
            .stderr(tty()?);
        Ok(command)
    }
    /// Runs the program to completion. fff ignores Ctrl+c and Ctrl+\ while it
    /// waits, so they reach only the program.
    pub fn wait(&self) -> io::Result<ExitStatus> {
        let mut child = self.command()?.spawn()?;
        let held = [libc::SIGINT, libc::SIGQUIT].map(|signal| unsafe {
            let mut previous: libc::sigaction = std::mem::zeroed();
            let mut ignore: libc::sigaction = std::mem::zeroed();
            ignore.sa_sigaction = libc::SIG_IGN;
            libc::sigaction(signal, &ignore, &mut previous);
            (signal, previous)
        });
        let status = child.wait();
        for (signal, previous) in held {
            unsafe {
                libc::sigaction(signal, &previous, std::ptr::null_mut());
            }
        }
        status
    }
    /// Replaces fff with the program; returns only if that fails.
    pub fn exec(&self) -> io::Error {
        match self.command() {
            Ok(mut command) => command.exec(),
            Err(error) => error,
        }
    }
}
/// E: `$EDITOR`, split and resolved like a typed command, on the given paths.
pub fn editor_plan(paths: Vec<PathBuf>, cwd: &Path, home: Option<&Path>) -> Result<Plan, String> {
    let value = match std::env::var("EDITOR") {
        Ok(value) => value,
        Err(std::env::VarError::NotPresent) => {
            return Err(
                "$EDITOR is not set; set it to your editor, e.g. export EDITOR=nvim".into(),
            );
        }
        Err(std::env::VarError::NotUnicode(_)) => {
            return Err("$EDITOR is not valid UTF-8".into());
        }
    };
    let words = split(&value, home).map_err(|e| format!("$EDITOR ({value}): {e}"))?;
    let Some((name, args)) = words.split_first() else {
        return Err("$EDITOR is empty; set it to your editor, e.g. export EDITOR=nvim".into());
    };
    Ok(Plan {
        program: resolve_or_suggest(name, cwd).map_err(|e| format!("$EDITOR: {e}"))?,
        name: name.clone(),
        args: args.to_vec(),
        paths,
        cwd: cwd.to_owned(),
        exit: false,
        line: value.trim().to_owned(),
    })
}
/// `!` in `fff window`: `$SHELL` (else `/bin/sh`) in `cwd`, replacing fff.
pub fn shell_plan(cwd: &Path) -> Result<Plan, String> {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "/bin/sh".into());
    let name = Path::new(&shell)
        .file_name()
        .map_or(shell.clone(), |n| n.to_string_lossy().into_owned());
    Ok(Plan {
        program: resolve_or_suggest(&shell, cwd).map_err(|e| format!("$SHELL: {e}"))?,
        name,
        args: vec![],
        paths: vec![],
        cwd: cwd.to_owned(),
        exit: true,
        line: shell,
    })
}
/// After a failed program, keeps its messages on screen until Enter.
pub fn pause_after_failure(line: &str, status: ExitStatus) {
    use io::{BufRead, Write};
    let Ok(mut tty) = crate::platform::open_terminal() else {
        return;
    };
    let _ = write!(
        tty,
        "\nfff: {line} {}. Press Enter to return to fff.",
        describe(status)
    );
    let _ = tty.flush();
    let _ = io::BufReader::new(&tty).read_line(&mut String::new());
}
/// "exited with status 1" or "was stopped by signal 9".
pub fn describe(status: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (status.code(), status.signal()) {
        (Some(code), _) => format!("exited with status {code}"),
        (_, Some(signal)) => format!("was stopped by signal {signal}"),
        _ => "ended".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn words_split_like_a_shell_without_running_one() {
        let home = Path::new("/home/me");
        let s = |t: &str| split(t, Some(home));
        assert_eq!(s("dragon -x").unwrap(), ["dragon", "-x"]);
        assert_eq!(
            s(r#"a 'b c' "d \"e\"" f\ g"#).unwrap(),
            ["a", "b c", "d \"e\"", "f g"]
        );
        assert_eq!(
            s("~/bin/x ~ a~").unwrap(),
            ["/home/me/bin/x", "/home/me", "a~"]
        );
        assert_eq!(s("x '~'").unwrap(), ["x", "~"]);
        // Inside double quotes, as in a shell, $ and ` need a backslash.
        assert_eq!(s(r#""\$HOME \`x\`""#).unwrap(), ["$HOME `x`"]);
        for bad in [r#""$HOME""#, r#""a `b`""#] {
            assert!(s(bad).unwrap_err().contains("literally"), "{bad}");
        }
        assert_eq!(s("x '|' \\*").unwrap(), ["x", "|", "*"]);
        for bad in [
            "a | b", "a > f", "a; b", "a &", "$HOME", "*.txt", "'open", "\"open", "x\\",
        ] {
            assert!(s(bad).is_err(), "{bad}");
        }
        assert!(s("  ").unwrap().is_empty());
        for word in ["a b", "it's", "*", "~x", "back\\slash"] {
            assert_eq!(s(&escape(word)).unwrap(), [word], "{word}");
        }
    }
    #[test]
    fn programs_resolve_on_path_or_by_explicit_path() {
        assert!(resolve("sh", Path::new("/")).unwrap().ends_with("sh"));
        let error = resolve("shh", Path::new("/")).unwrap_err();
        assert_eq!(error, "shh: command not found");
        assert!(resolve("/bin/sh", Path::new("/")).is_ok());
        assert!(resolve("./definitely-missing", Path::new("/")).is_err());
        let names = ["dragon".to_string(), "nvim".to_string()];
        assert_eq!(suggestion("dragn", &names), Some("dragon"));
        assert_eq!(suggestion("emacs", &names), None);
        assert_eq!(suggestion("nv", &names), None);
        assert_eq!(suggestion("nvi", &names), Some("nvim"));
    }
    #[test]
    fn tab_completes_command_names_then_paths_and_cycles() {
        let dir = std::env::temp_dir().join(format!("fff-run-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub dir")).unwrap();
        fs::write(dir.join("alpha.txt"), b"").unwrap();
        fs::write(dir.join("alps.txt"), b"").unwrap();
        fs::write(dir.join(".hidden"), b"").unwrap();
        let mut p = Prompt::new(vec![], false);
        p.commands = Some(vec!["dragon".into(), "drop".into(), "nvim".into()]);
        p.text = "nv".into();
        p.complete(&dir, None);
        assert_eq!(p.text, "nvim ");
        p.text = "dr".into();
        p.complete(&dir, None);
        assert_eq!(p.text, "dr");
        assert_eq!(p.candidates, ["dragon", "drop"]);
        p.complete(&dir, None);
        assert_eq!(p.text, "dragon");
        p.complete(&dir, None);
        assert_eq!(p.text, "drop");
        p.text = "nvim al".into();
        p.complete(&dir, None);
        assert_eq!(p.text, "nvim alp");
        p.text = "nvim su".into();
        p.complete(&dir, None);
        assert_eq!(p.text, "nvim sub\\ dir/");
        p.text = "nvim .h".into();
        p.complete(&dir, None);
        assert_eq!(p.text, "nvim .hidden ");
        p.text = "nvim ".into();
        p.complete(&dir, None);
        assert!(!p.candidates.iter().any(|c| c.starts_with('.')));
        let plan = Prompt {
            text: "nvim -O".into(),
            paths: vec![dir.join("alpha.txt")],
            ..Prompt::new(vec![], true)
        };
        let error = plan.plan(&dir, None);
        // nvim may be absent on a test machine; then the error names it.
        if let Ok(plan) = error {
            assert_eq!(plan.args, ["-O"]);
            assert_eq!(plan.paths, [dir.join("alpha.txt")]);
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}

#[cfg(test)]
mod flag_tests {
    use super::*;
    #[test]
    fn words_end_at_spaces_outside_quotes() {
        let mut p = Prompt::new(vec![], false);
        for (text, start) in [
            ("ls 'a\\' b", 8),
            ("ls \"a\\\" b", 3),
            ("ls a\\ b", 3),
            ("ls", 0),
        ] {
            p.text = text.into();
            assert_eq!(p.current_word().0, start, "{text}");
        }
    }
    #[test]
    fn a_bare_tilde_completes_inside_home() {
        let home = std::env::temp_dir().join(format!("fff-tilde-{}", std::process::id()));
        let _ = fs::remove_dir_all(&home);
        fs::create_dir_all(home.join("only")).unwrap();
        let cwd = Path::new("/");
        assert_eq!(paths("~", cwd, Some(&home), false), ["~/only/"]);
        fs::remove_dir_all(&home).unwrap();
    }
    #[test]
    fn other_words_are_what_the_shell_completes_them_with() {
        use crate::zsh::Match;
        let dir = std::env::temp_dir().join(format!("fff-shell-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub dir")).unwrap();
        fs::write(dir.join("checks"), b"").unwrap();
        let word = |word: &str, file| Match {
            word: word.into(),
            file,
        };
        let asked = std::cell::Cell::new(0);
        let tab = |text: &str, matches: Option<Vec<Match>>| {
            let mut p = Prompt::new(vec![], false);
            p.commands = Some(vec!["git".into()]);
            p.text = text.into();
            p.complete_with(&dir, None, |_, _| {
                asked.set(asked.get() + 1);
                matches
            });
            (p.text, p.candidates)
        };
        let subcommands = vec![word("checkout", false), word("cherry", false)];
        // Its matches replace the word; files of the same start are not mixed in.
        let (text, candidates) = tab("git ch", Some(subcommands));
        assert_eq!(text, "git che");
        assert_eq!(candidates, ["checkout", "cherry"]);
        // One match is inserted: an option awaiting its value goes on, and a
        // folder is one by what is there, quoted as fff reads it.
        assert_eq!(
            tab("git --col", Some(vec![word("--color=", false)])).0,
            "git --color="
        );
        assert_eq!(
            tab("git --am", Some(vec![word("--amend", false)])).0,
            "git --amend "
        );
        assert_eq!(
            tab("git su", Some(vec![word("sub dir", true)])).0,
            "git sub\\ dir/"
        );
        // Nothing offered, or no shell: a path, as before.
        assert_eq!(tab("git ch", Some(vec![])).0, "git checks ");
        assert_eq!(tab("git ch", None).0, "git checks ");
        assert_eq!(asked.get(), 6);
        // The command, a path, and a line fff would refuse are not asked.
        for text in ["gi", "git ./ch", "git ~", "git $(x) ch", "git 'ch"] {
            tab(text, Some(vec![word("never", false)]));
        }
        assert_eq!(asked.get(), 6);
        fs::remove_dir_all(&dir).unwrap();
    }
}
