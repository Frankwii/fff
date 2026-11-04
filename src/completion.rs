//! Shell-completion helpers. Candidates are computed by the running binary
//! (`COMPLETE=<shell> fff`), so custom palettes come from the live config.
use crate::config::{Palette, default_path};
use clap_complete::{
    engine::CompletionCandidate,
    env::{Bash, Elvish, EnvCompleter, Fish, Powershell, Shells},
};
use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Environment variable that switches the binary into completion mode.
pub const ENV_VAR: &str = "COMPLETE";

/// Shells with a registration script, as accepted by `fff setup --completions`:
/// clap's, with zsh's script made to work as a `_fff` file too.
pub const SHELLS: Shells<'static> = Shells(&[&Bash, &Elvish, &Fish, &Powershell, &Zsh]);

pub fn shells() -> Vec<&'static str> {
    SHELLS.names().collect()
}

/// clap's zsh completion, whose script also works as an autoloaded `_fff`
/// in `$fpath`: loaded that way it completes at once, where clap's would
/// only register itself on the first Tab and complete from the second.
pub struct Zsh;
impl EnvCompleter for Zsh {
    fn name(&self) -> &'static str {
        clap_complete::env::Zsh.name()
    }
    fn is(&self, name: &str) -> bool {
        clap_complete::env::Zsh.is(name)
    }
    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        let mut script = Vec::new();
        clap_complete::env::Zsh.write_registration(var, name, bin, completer, &mut script)?;
        let script = String::from_utf8_lossy(&script);
        let function = format!("_clap_dynamic_completer_{}", name.replace('-', "_"));
        let register = format!("compdef {function} {bin}");
        let Some((body, rest)) = script.rsplit_once(&register) else {
            return buf.write_all(script.as_bytes());
        };
        write!(
            buf,
            "{body}if [[ ${{funcstack[1]}} == _{bin} ]]; then\n    \
             # Autoloaded from $fpath: complete now.\n    \
             {function} \"$@\"\n\
             else\n    \
             # Sourced: register for the next Tab.\n    \
             {register}\n\
             fi{rest}"
        )
    }
    fn write_complete(
        &self,
        cmd: &mut clap::Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn Write,
    ) -> io::Result<()> {
        clap_complete::env::Zsh.write_complete(cmd, args, current_dir, buf)
    }
}

/// Print the registration script that `COMPLETE=<shell> fff` would print.
/// The script calls back into `program` (argv[0], made absolute when it is a
/// relative path with directories, matching `CompleteEnv`).
pub fn write_registration(shell: &str, program: &OsStr, out: &mut dyn Write) -> io::Result<()> {
    let completer = SHELLS
        .completer(shell)
        .ok_or_else(|| io::Error::other(format!("Unsupported shell: {shell}")))?;
    let mut program = PathBuf::from(program);
    if program.is_relative()
        && program.components().count() > 1
        && let Ok(cwd) = std::env::current_dir()
    {
        // `./target/release/fff` without its `.`.
        program = cwd.join(program).components().collect();
    }
    completer.write_registration(ENV_VAR, "fff", "fff", &program.to_string_lossy(), out)
}

/// Built-in palette names followed by custom `[palettes.NAME]` tables from the
/// configuration named on the command line being completed (or the default file).
pub fn palette_candidates(current: &OsStr) -> Vec<CompletionCandidate> {
    let current = current.to_string_lossy();
    let custom = config_path(std::env::args_os())
        .map(|path| custom_palettes(&path))
        .unwrap_or_default();
    let builtin = Palette::names()
        .iter()
        .map(|name| (name.to_string(), "built-in"));
    builtin
        .chain(custom.into_iter().map(|name| (name, "custom")))
        .filter(|(name, _)| name.starts_with(current.as_ref()))
        .map(|(name, kind)| CompletionCandidate::new(name).help(Some(kind.into())))
        .collect()
}

/// Custom palette names in a TOML file; unreadable or invalid files yield none.
pub fn custom_palettes(path: &Path) -> Vec<String> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| text.parse::<toml::Table>().ok())
        .and_then(|table| match table.get("palettes") {
            Some(toml::Value::Table(palettes)) => Some(palettes.keys().cloned().collect()),
            _ => None,
        })
        .unwrap_or_default()
}

/// The configuration file a command line would load: `--config PATH`, nothing
/// with `--no-config`, otherwise the default path.
fn config_path(args: impl IntoIterator<Item = OsString>) -> Option<PathBuf> {
    let mut explicit = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let text = arg.to_string_lossy();
        if text == "--no-config" {
            return None;
        } else if text == "--config" {
            explicit = args
                .next()
                .map(|path| expand(&path.to_string_lossy(), env_var));
        } else if let Some(path) = text.strip_prefix("--config=") {
            explicit = Some(expand(path, env_var));
        }
    }
    let env_path = |name| {
        std::env::var_os(name)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from)
    };
    explicit.or_else(|| {
        default_path(
            env_path("XDG_CONFIG_HOME").as_deref(),
            env_path("HOME").as_deref(),
        )
    })
}

fn env_var(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|s| !s.is_empty())
}

/// Shells hand completion scripts the words as typed, so expand a leading `~`,
/// `$NAME` or `${NAME}` the way the shell will when the command runs.
fn expand(word: &str, var: impl Fn(&str) -> Option<OsString>) -> PathBuf {
    let (name, rest) = if let Some(rest) = word.strip_prefix('~') {
        if !(rest.is_empty() || rest.starts_with('/')) {
            return PathBuf::from(word);
        }
        ("HOME", rest)
    } else if let Some(braced) = word.strip_prefix("${") {
        match braced.split_once('}') {
            Some(split) => split,
            None => return PathBuf::from(word),
        }
    } else if let Some(bare) = word.strip_prefix('$') {
        let end = bare
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .unwrap_or(bare.len());
        bare.split_at(end)
    } else {
        return PathBuf::from(word);
    };
    match var(name) {
        Some(value) if !name.is_empty() => {
            let mut path = value;
            path.push(rest);
            PathBuf::from(path)
        }
        _ => PathBuf::from(word),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_path_follows_the_completed_command_line() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            config_path(args(&["fff", "--config", "a.toml"])),
            Some("a.toml".into())
        );
        assert_eq!(
            config_path(args(&["fff", "--config=b.toml"])),
            Some("b.toml".into())
        );
        assert_eq!(config_path(args(&["fff", "--no-config"])), None);
    }

    #[test]
    fn config_path_expands_home_and_variables() {
        let var = |name: &str| match name {
            "HOME" => Some(OsString::from("/home/u")),
            "CFG" => Some(OsString::from("/etc/p")),
            _ => None,
        };
        assert_eq!(expand("~/x.toml", var), PathBuf::from("/home/u/x.toml"));
        assert_eq!(expand("$HOME/x.toml", var), PathBuf::from("/home/u/x.toml"));
        assert_eq!(expand("${CFG}/x.toml", var), PathBuf::from("/etc/p/x.toml"));
        assert_eq!(expand("~other/x", var), PathBuf::from("~other/x"));
        assert_eq!(expand("$UNSET/x", var), PathBuf::from("$UNSET/x"));
        assert_eq!(expand("rel/x.toml", var), PathBuf::from("rel/x.toml"));
    }

    #[test]
    fn custom_palettes_are_read_from_toml() {
        let example = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/config.example.toml");
        assert_eq!(custom_palettes(Path::new(example)), ["ember"]);
        assert!(custom_palettes(Path::new("/nonexistent/fff.toml")).is_empty());
    }
}

/// Shells `fff setup --init` can wrap.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum InitShell {
    Bash,
    Zsh,
    Fish,
}
/// A shell function named `fff` that runs the real binary and then changes
/// to the directory it chose, like nnn's quitcd scripts. A process cannot
/// change its parent shell's directory, so the shell does it: fff writes the
/// directory (exact bytes, no newline) to a private temporary file.
pub fn wrapper(shell: InitShell) -> &'static str {
    match shell {
        InitShell::Bash | InitShell::Zsh => {
            r#"# fff: change to the directory chosen on exit. Load with:
#   eval "$(fff setup --init zsh)"   (or bash)
fff() {
    local fff_cd_file fff_dir fff_status
    fff_cd_file=$(mktemp "${TMPDIR:-/tmp}/fff-cd.XXXXXX") || return
    command fff --cd-file "$fff_cd_file" "$@"
    fff_status=$?
    if [ -s "$fff_cd_file" ]; then
        # The x keeps a trailing newline in a directory name intact.
        fff_dir=$(cat -- "$fff_cd_file"; printf x)
        cd -- "${fff_dir%x}" || fff_status=$?
    fi
    rm -f -- "$fff_cd_file"
    return "$fff_status"
}
"#
        }
        InitShell::Fish => {
            r#"# fff: change to the directory chosen on exit. Load with:
#   fff setup --init fish | source
function fff --wraps fff --description 'fff, changing directory on exit'
    set -l fff_cd_file (mktemp -t fff-cd.XXXXXX); or return
    command fff --cd-file $fff_cd_file $argv
    set -l fff_status $status
    if test -s $fff_cd_file
        cd -- (string collect -- (cat -- $fff_cd_file)); or set fff_status $status
    end
    rm -f -- $fff_cd_file
    return $fff_status
end
"#
        }
    }
}
