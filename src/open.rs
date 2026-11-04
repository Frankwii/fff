//! Openers: which program opens a file, chosen by extension or name pattern
//! (`[open]` in the configuration), falling back to `$EDITOR`.
//!
//! Navigation keys open files in the background: GUI programs detached from
//! the terminal (their own session, no terminal input or output), terminal
//! programs in a new terminal window (`[open] terminal`). `o` instead hands
//! fff's terminal to the opener, like `E`.
use crate::run::{Plan, resolve_or_suggest, split};
use serde::Deserialize;
use std::{
    fs::OpenOptions,
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// `[open]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OpenConfig {
    /// Prefix that runs a command in a new terminal window, e.g. `st -e`.
    pub terminal: String,
    /// Prefix for fff's own windows, which `fff setup` installs (the desktop
    /// entry, the file chooser): a class a window manager can float.
    pub fff_terminal: String,
    /// `[[open.rules]]`, first match wins.
    pub rules: Vec<Rule>,
}
impl Default for OpenConfig {
    fn default() -> Self {
        Self {
            terminal: "st -e".into(),
            fff_terminal: "st -c fff-picker -e".into(),
            rules: vec![],
        }
    }
}
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    /// The program and its arguments; the files are added after them.
    pub command: String,
    /// Case-insensitive extensions, the dot optional: `["pdf", "tar.gz"]`.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Globs matched against the file name: `["Makefile", "*.tar.gz"]`.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// The program needs a terminal (an editor, a pager): navigation keys open
    /// it in a new terminal window instead of in the background.
    #[serde(default)]
    pub terminal: bool,
    /// Arguments added before the file when it opens from a contents
    /// match, with placeholders for where the match is (see `GrepMatch`),
    /// e.g. `--page {page} --find {query}` for zathura.
    #[serde(default)]
    pub at_grep_match: String,
}

/// How a file is opened: the command line and whether it needs a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opener {
    pub line: String,
    pub terminal: bool,
    /// `at_grep_match`, empty when the rule has none.
    pub at_grep_match: String,
}
/// A contents match a file opens at: what `at_grep_match`'s placeholders
/// become.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrepMatch {
    /// `{page}`: its page in a PDF or DjVu; else its line.
    pub page: Option<u32>,
    /// `{line}`: its line in the text (in a document, in its extracted text).
    pub line: u32,
    /// `{percent}`: how far into the file it is, 0 to 100.
    pub percent: u8,
    /// `{query}`: the contents query, as typed (spaces at its ends dropped).
    pub query: String,
    /// `{match}`: the text that matched, as it reads in the file.
    pub matched: String,
    /// `{nth}` and `{count}`: which of the file's matches it is, from 1, and
    /// how many the file has.
    pub nth: usize,
    pub count: usize,
}
impl GrepMatch {
    /// The placeholders `at_grep_match` takes, each with its value, for the
    /// file at `path`.
    fn values(&self, path: &Path) -> [(&'static str, String); 8] {
        [
            ("{path}", path.to_string_lossy().into_owned()),
            ("{page}", self.page.unwrap_or(self.line).to_string()),
            ("{line}", self.line.to_string()),
            ("{percent}", self.percent.to_string()),
            ("{query}", self.query.clone()),
            ("{match}", self.matched.clone()),
            ("{nth}", self.nth.to_string()),
            ("{count}", self.count.to_string()),
        ]
    }
    /// `at_grep_match`'s words with the placeholders filled in, split like
    /// a typed command first, so that a value with spaces stays one word.
    /// Each word is filled in one pass: a value that itself reads like a
    /// placeholder (a query `{page}`) stays as it is.
    fn arguments(
        &self,
        template: &str,
        path: &Path,
        home: Option<&Path>,
    ) -> Result<Vec<String>, String> {
        let words =
            split(template, home).map_err(|e| format!("at_grep_match {template:?}: {e}"))?;
        let values = self.values(path);
        Ok(words
            .into_iter()
            .map(|word| {
                let mut out = String::new();
                let mut rest = word.as_str();
                'scan: while !rest.is_empty() {
                    for (name, value) in &values {
                        if let Some(after) = rest.strip_prefix(name) {
                            out.push_str(value);
                            rest = after;
                            continue 'scan;
                        }
                    }
                    let c = rest.chars().next().expect("not empty");
                    out.push(c);
                    rest = &rest[c.len_utf8()..];
                }
                out
            })
            .collect())
    }
}

impl OpenConfig {
    /// Rejects rules that can never match or have no command.
    pub fn validate(&self) -> Result<(), String> {
        // Only the syntax matters here; `~` expands when a command runs.
        let split = |line: &str| split(line, Some(Path::new("/")));
        if split(&self.terminal).map_or(true, |w| w.is_empty()) {
            return Err("open.terminal must name a terminal command, e.g. \"st -e\"".into());
        }
        if split(&self.fff_terminal).map_or(true, |w| w.is_empty()) {
            return Err(
                "open.fff_terminal must name a terminal command, e.g. \"st -c fff-picker -e\""
                    .into(),
            );
        }
        for (i, rule) in self.rules.iter().enumerate() {
            let at = format!("open.rules[{}]", i + 1);
            if split(&rule.command).map_or(true, |w| w.is_empty()) {
                return Err(format!("{at}: command must name a program"));
            }
            if rule.extensions.is_empty() && rule.patterns.is_empty() {
                return Err(format!("{at}: give extensions or patterns to match"));
            }
            if rule
                .extensions
                .iter()
                .any(|e| e.trim_start_matches('.').is_empty())
            {
                return Err(format!(
                    "{at}: an extension is empty; match names without one with patterns"
                ));
            }
            for pattern in &rule.patterns {
                crate::glob(pattern).map_err(|e| format!("{at}: pattern {pattern:?}: {e}"))?;
            }
            if split(&rule.at_grep_match).is_err() {
                return Err(format!(
                    "{at}: at_grep_match must split like a command line"
                ));
            }
        }
        Ok(())
    }
    /// Programs named by rules and by configured terminals that are not
    /// installed. The built-in terminals are not checked: a configuration
    /// that never names one is not wrong for lacking it.
    pub fn missing(&self, cwd: &Path, home: Option<&Path>) -> Vec<String> {
        let defaults = Self::default();
        [
            (&self.terminal, &defaults.terminal),
            (&self.fff_terminal, &defaults.fff_terminal),
        ]
        .into_iter()
        .filter(|(line, default)| line != default)
        .map(|(line, _)| line)
        .chain(self.rules.iter().map(|r| &r.command))
        .filter_map(|line| {
            let name = split(line, home).ok()?.into_iter().next()?;
            resolve_or_suggest(&name, cwd).err()
        })
        .collect()
    }
    /// The first rule matching the file's extension or name; else `$EDITOR`.
    pub fn opener(&self, path: &Path) -> Result<Opener, String> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_default();
        let lowercase = name.to_lowercase();
        let matched = self.rules.iter().find(|rule| {
            rule.extensions.iter().any(|e| has_extension(&lowercase, e))
                || rule
                    .patterns
                    .iter()
                    .any(|p| crate::glob(p).is_ok_and(|g| g.is_match(name.as_ref())))
        });
        if let Some(rule) = matched {
            return Ok(Opener {
                line: rule.command.clone(),
                terminal: rule.terminal,
                at_grep_match: rule.at_grep_match.clone(),
            });
        }
        match std::env::var("EDITOR") {
            Ok(editor) if !editor.trim().is_empty() => Ok(Opener {
                line: editor,
                terminal: true,
                at_grep_match: String::new(),
            }),
            _ => Err(format!(
                "No opener for {name}; add an [[open.rules]] entry or set $EDITOR"
            )),
        }
    }
}

/// A plan for `line` on `paths`, resolved like a typed command.
pub fn plan(
    line: &str,
    paths: Vec<PathBuf>,
    cwd: &Path,
    home: Option<&Path>,
) -> Result<Plan, String> {
    let words = split(line, home).map_err(|e| format!("{line}: {e}"))?;
    let (name, args) = words
        .split_first()
        .ok_or_else(|| format!("{line:?} names no program"))?;
    Ok(Plan {
        program: resolve_or_suggest(name, cwd)?,
        name: name.clone(),
        args: args.to_vec(),
        paths,
        cwd: cwd.to_owned(),
        exit: false,
        line: line.trim().to_owned(),
    })
}

/// Where background programs' output goes: next to the remembered state.
pub fn log_path() -> Option<PathBuf> {
    crate::state::path().map(|p| p.with_file_name("open.log"))
}

/// Starts a program detached from fff's terminal: a new session (closing the
/// terminal or quitting fff does not stop it), no input, output appended to
/// the log. A watcher reaps it and posts a notification if it fails.
pub fn launch(plan: &Plan) -> Result<(), String> {
    let log = log_path().and_then(|p| {
        std::fs::create_dir_all(p.parent()?).ok()?;
        OpenOptions::new().create(true).append(true).open(p).ok()
    });
    let output = |log: &Option<std::fs::File>| match log.as_ref().and_then(|f| f.try_clone().ok()) {
        Some(file) => Stdio::from(file),
        None => Stdio::null(),
    };
    let mut command = Command::new(&plan.program);
    command
        .arg0(&plan.name)
        .args(&plan.args)
        .args(&plan.paths)
        .current_dir(&plan.cwd)
        .stdin(Stdio::null())
        .stdout(output(&log))
        .stderr(output(&log));
    crate::run::detach(&mut command);
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", plan.name))?;
    let name = plan.name.clone();
    std::thread::spawn(move || {
        if let Ok(status) = child.wait()
            && !status.success()
        {
            let log = log_path()
                .map(|p| format!("; see {}", p.display()))
                .unwrap_or_default();
            crate::notify::post(format!("{name} {}{log}", crate::run::describe(status)));
        }
    });
    Ok(())
}

/// The plan for opening `path` from a navigation key: the opener itself, or,
/// when it needs a terminal, the opener inside a new terminal window.
pub fn background_plan(
    config: &OpenConfig,
    path: &Path,
    at: Option<&GrepMatch>,
    cwd: &Path,
    home: Option<&Path>,
) -> Result<Plan, String> {
    let opener = config.opener(path)?;
    let mut inner = plan(&opener.line, vec![path.to_owned()], cwd, home)?;
    if let Some(at) = at.filter(|_| !opener.at_grep_match.trim().is_empty()) {
        inner
            .args
            .extend(at.arguments(&opener.at_grep_match, path, home)?);
        // Named by `{path}`, the file is where the arguments put it.
        if opener.at_grep_match.contains("{path}") {
            inner.paths.clear();
        }
    }
    if !opener.terminal {
        return Ok(inner);
    }
    let mut outer =
        plan(&config.terminal, vec![], cwd, home).map_err(|e| format!("[open] terminal: {e}"))?;
    outer
        .args
        .push(inner.program.to_string_lossy().into_owned());
    outer.args.extend(inner.args);
    outer.paths = inner.paths;
    outer.line = format!("{} {}", config.terminal, opener.line);
    Ok(outer)
}

/// Whether the lowercase file name `name` ends with `.extension` after a
/// nonempty stem: `tar.gz` and `gz` match `x.tar.gz`, `pdf` matches `b.pdf`,
/// and nothing matches the whole of a hidden name such as `.pdf`.
fn has_extension(name: &str, extension: &str) -> bool {
    let extension = extension.trim_start_matches('.').to_lowercase();
    name.strip_suffix(extension.as_str())
        .and_then(|rest| rest.strip_suffix('.'))
        .is_some_and(|stem| !stem.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn config(toml: &str) -> OpenConfig {
        toml::from_str(toml).unwrap()
    }
    #[test]
    fn rules_match_extensions_and_patterns_in_order_then_editor() {
        let c = config(
            r#"
            terminal = "st -e"
            [[rules]]
            command = "zathura"
            extensions = ["pdf", ".djvu", "EPUB"]
            [[rules]]
            command = "nvim"
            patterns = ["Makefile", "*.tar.gz"]
            terminal = true
            "#,
        );
        c.validate().unwrap();
        let line = |p: &str| c.opener(Path::new(p)).map(|o| (o.line, o.terminal));
        assert_eq!(line("/a/b.PDF").unwrap(), ("zathura".into(), false));
        assert_eq!(line("/a/b.epub").unwrap(), ("zathura".into(), false));
        assert_eq!(line("/a/b.djvu").unwrap(), ("zathura".into(), false));
        assert_eq!(line("/a/Makefile").unwrap(), ("nvim".into(), true));
        assert_eq!(line("/a/x.tar.gz").unwrap(), ("nvim".into(), true));
        // Extensions may have several parts, and match only after a name.
        let archives = config(
            "[[rules]]\ncommand = \"tar tf\"\nextensions = [\"tar.gz\", \".TGZ\"]\n\
             [[rules]]\ncommand = \"gunzip\"\nextensions = [\"gz\"]",
        );
        archives.validate().unwrap();
        let opener = |p: &str| archives.opener(Path::new(p)).map(|o| o.line).ok();
        assert_eq!(opener("/a/x.TAR.GZ").as_deref(), Some("tar tf"));
        assert_eq!(opener("/a/x.tgz").as_deref(), Some("tar tf"));
        assert_eq!(opener("/a/x.gz").as_deref(), Some("gunzip"));
        assert_eq!(opener("/a/.tar.gz").as_deref(), Some("gunzip"));
        assert_ne!(opener("/a/xtar.gz").as_deref(), Some("tar tf"));
        assert_ne!(opener("/a/.gz").as_deref(), Some("gunzip"));
        assert!(
            config("[[rules]]\ncommand = \"x\"\nextensions = [\".\"]")
                .validate()
                .is_err()
        );
        // Unmatched files fall back to $EDITOR, as a terminal program.
        if let Ok(editor) = std::env::var("EDITOR")
            && !editor.trim().is_empty()
        {
            assert_eq!(line("/a/notes.txt").unwrap(), (editor, true));
        }
        assert!(config("[[rules]]\ncommand = \"x\"").validate().is_err());
        assert!(config("terminal = \"\"").validate().is_err());
        assert!(config("fff_terminal = \"\"").validate().is_err());
        // `~` expands when the command runs, not while validating.
        assert!(
            config("fff_terminal = \"~/bin/st -e\"\n[[rules]]\ncommand = \"~/bin/view\"\nextensions = [\"a\"]")
                .validate()
                .is_ok()
        );
        assert!(
            config("[[rules]]\ncommand = \"\"\nextensions = [\"a\"]")
                .validate()
                .is_err()
        );
    }
    #[test]
    fn terminal_openers_run_inside_the_configured_terminal() {
        let c = config(
            "terminal = \"sh -c\"\n[[rules]]\ncommand = \"cat -n\"\nextensions = [\"txt\"]\nterminal = true",
        );
        let plan =
            background_plan(&c, Path::new("/tmp/x.txt"), None, Path::new("/"), None).unwrap();
        assert_eq!(plan.name, "sh");
        assert_eq!(plan.args[0], "-c");
        assert!(plan.args[1].ends_with("/cat"));
        assert_eq!(plan.args[2], "-n");
        assert_eq!(plan.paths, [PathBuf::from("/tmp/x.txt")]);
    }
    #[test]
    fn a_grep_match_opens_where_it_is() {
        let c = config(
            r#"
            [[rules]]
            command = "cat -n"
            extensions = ["pdf"]
            at_grep_match = "--page {page} --find {query}"
            [[rules]]
            command = "cat"
            extensions = ["txt"]
            at_grep_match = "+{line} -c /{match} {nth}/{count} {percent}%"
            "#,
        );
        c.validate().unwrap();
        let at = GrepMatch {
            page: Some(12),
            line: 340,
            percent: 7,
            query: "canard explosion".into(),
            matched: "Canard Explosion".into(),
            nth: 2,
            count: 5,
        };
        let plan = background_plan(&c, Path::new("/a/b.pdf"), Some(&at), Path::new("/"), None);
        let plan = plan.unwrap();
        // Before the file; a query with spaces stays one argument.
        assert_eq!(
            plan.args,
            ["-n", "--page", "12", "--find", "canard explosion"]
        );
        assert_eq!(plan.paths, [PathBuf::from("/a/b.pdf")]);
        let plan = background_plan(&c, Path::new("/a/b.txt"), Some(&at), Path::new("/"), None);
        assert_eq!(
            plan.unwrap().args,
            ["+340", "-c", "/Canard Explosion", "2/5", "7%"]
        );
        // A value that reads like a placeholder is not filled in again.
        let tricky = GrepMatch {
            query: "{line}".into(),
            ..at.clone()
        };
        let plan = background_plan(
            &c,
            Path::new("/a/b.pdf"),
            Some(&tricky),
            Path::new("/"),
            None,
        );
        assert_eq!(plan.unwrap().args[4], "{line}");
        // `{path}` puts the file among the arguments instead of at the end.
        let mupdf = config(
            "[[rules]]\ncommand = \"cat\"\nextensions = [\"pdf\"]\nat_grep_match = \"{path} {page}\"",
        );
        let plan = background_plan(
            &mupdf,
            Path::new("/a/b.pdf"),
            Some(&at),
            Path::new("/"),
            None,
        );
        let plan = plan.unwrap();
        assert_eq!(plan.args, ["/a/b.pdf", "12"]);
        assert!(plan.paths.is_empty());
        // The documented shell wrapper: the values are the script's arguments.
        let shell = config(
            r#"
            [[rules]]
            command = "sh -c"
            extensions = ["pdf"]
            at_grep_match = '''
            'notify-send "Match $3 of $4" "$2"; exec zathura --page "$1" --find "$2" "$5"'
            fff {page} {query} {nth} {count} {path}
            '''
            "#,
        );
        shell.validate().unwrap();
        let plan = background_plan(
            &shell,
            Path::new("/a/b.pdf"),
            Some(&at),
            Path::new("/"),
            None,
        );
        let plan = plan.unwrap();
        assert_eq!(
            &plan.args[2..],
            ["fff", "12", "canard explosion", "2", "5", "/a/b.pdf"]
        );
        assert!(plan.args[1].starts_with("notify-send"));
        assert!(plan.paths.is_empty());
        // Opened from anywhere but a match, nothing is added.
        let plan = background_plan(&c, Path::new("/a/b.pdf"), None, Path::new("/"), None);
        assert_eq!(plan.unwrap().args, ["-n"]);
    }
}
