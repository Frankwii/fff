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
    /// Case-insensitive extensions without the dot: `["pdf", "djvu"]`.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Globs matched against the file name: `["Makefile", "*.tar.gz"]`.
    #[serde(default)]
    pub patterns: Vec<String>,
    /// The program needs a terminal (an editor, a pager): navigation keys open
    /// it in a new terminal window instead of in the background.
    #[serde(default)]
    pub terminal: bool,
}

/// How a file is opened: the command line and whether it needs a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Opener {
    pub line: String,
    pub terminal: bool,
}

impl OpenConfig {
    /// Rejects rules that can never match or have no command.
    pub fn validate(&self) -> Result<(), String> {
        if split(&self.terminal, None).map_or(true, |w| w.is_empty()) {
            return Err("open.terminal must name a terminal command, e.g. \"st -e\"".into());
        }
        if split(&self.fff_terminal, None).map_or(true, |w| w.is_empty()) {
            return Err(
                "open.fff_terminal must name a terminal command, e.g. \"st -c fff-picker -e\""
                    .into(),
            );
        }
        for (i, rule) in self.rules.iter().enumerate() {
            let at = format!("open.rules[{}]", i + 1);
            if split(&rule.command, None).map_or(true, |w| w.is_empty()) {
                return Err(format!("{at}: command must name a program"));
            }
            if rule.extensions.is_empty() && rule.patterns.is_empty() {
                return Err(format!("{at}: give extensions or patterns to match"));
            }
            for pattern in &rule.patterns {
                crate::glob(pattern).map_err(|e| format!("{at}: pattern {pattern:?}: {e}"))?;
            }
        }
        Ok(())
    }
    /// Programs named by rules and the terminals that are not installed.
    pub fn missing(&self, cwd: &Path) -> Vec<String> {
        [&self.terminal, &self.fff_terminal]
            .into_iter()
            .chain(self.rules.iter().map(|r| &r.command))
            .filter_map(|line| {
                let name = split(line, None).ok()?.into_iter().next()?;
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
        let extension = path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let matched = self.rules.iter().find(|rule| {
            rule.extensions
                .iter()
                .any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(&extension))
                || rule
                    .patterns
                    .iter()
                    .any(|p| crate::glob(p).is_ok_and(|g| g.is_match(name.as_ref())))
        });
        if let Some(rule) = matched {
            return Ok(Opener {
                line: rule.command.clone(),
                terminal: rule.terminal,
            });
        }
        match std::env::var("EDITOR") {
            Ok(editor) if !editor.trim().is_empty() => Ok(Opener {
                line: editor,
                terminal: true,
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
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
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
    cwd: &Path,
    home: Option<&Path>,
) -> Result<Plan, String> {
    let opener = config.opener(path)?;
    let inner = plan(&opener.line, vec![path.to_owned()], cwd, home)?;
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
        // Unmatched files fall back to $EDITOR, as a terminal program.
        if let Ok(editor) = std::env::var("EDITOR")
            && !editor.trim().is_empty()
        {
            assert_eq!(line("/a/notes.txt").unwrap(), (editor, true));
        }
        assert!(config("[[rules]]\ncommand = \"x\"").validate().is_err());
        assert!(config("terminal = \"\"").validate().is_err());
        assert!(config("fff_terminal = \"\"").validate().is_err());
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
        let plan = background_plan(&c, Path::new("/tmp/x.txt"), Path::new("/"), None).unwrap();
        assert_eq!(plan.name, "sh");
        assert_eq!(plan.args[0], "-c");
        assert!(plan.args[1].ends_with("/cat"));
        assert_eq!(plan.args[2], "-n");
        assert_eq!(plan.paths, [PathBuf::from("/tmp/x.txt")]);
    }
}
