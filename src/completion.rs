//! Shell-completion helpers. Candidates are computed by the running binary
//! (`COMPLETE=<shell> picker`), so custom palettes come from the live config.
use crate::config::{Palette, default_path};
use clap_complete::engine::CompletionCandidate;
use std::{
    ffi::{OsStr, OsString},
    io::{self, Write},
    path::{Path, PathBuf},
};

/// Environment variable that switches the binary into completion mode.
pub const ENV_VAR: &str = "COMPLETE";

/// Shells with a registration script, as accepted by `--completions`.
pub fn shells() -> Vec<&'static str> {
    clap_complete::env::Shells::builtins().names().collect()
}

/// Print the registration script that `COMPLETE=<shell> picker` would print.
/// The script calls back into `program` (argv[0], made absolute when it is a
/// relative path with directories, matching `CompleteEnv`).
pub fn write_registration(shell: &str, program: &OsStr, out: &mut dyn Write) -> io::Result<()> {
    let shells = clap_complete::env::Shells::builtins();
    let completer = shells
        .completer(shell)
        .ok_or_else(|| io::Error::other(format!("Unsupported shell: {shell}")))?;
    let mut program = PathBuf::from(program);
    if program.is_relative()
        && program.components().count() > 1
        && let Ok(cwd) = std::env::current_dir()
    {
        program = cwd.join(program);
    }
    completer.write_registration(ENV_VAR, "picker", "picker", &program.to_string_lossy(), out)
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
            explicit = args.next().map(PathBuf::from);
        } else if let Some(path) = text.strip_prefix("--config=") {
            explicit = Some(PathBuf::from(path));
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

/// Named keys accepted by `--map-key`; single characters are also accepted.
pub const KEY_NAMES: &[&str] = &[
    "enter",
    "esc",
    "tab",
    "space",
    "backspace",
    "up",
    "down",
    "left",
    "right",
    "pageup",
    "pagedown",
    "f1",
    "f5",
];
const MODIFIERS: &[&str] = &["ctrl+", "alt+", "shift+"];

/// Complete `SOURCE=TARGET` one chord segment at a time: modifiers not yet
/// used, then key names; after `=`, `none` disables the source chord.
pub fn map_key_candidates(current: &OsStr) -> Vec<CompletionCandidate> {
    let current = current.to_string_lossy();
    let (side_prefix, chord) = match current.split_once('=') {
        Some((source, target)) => (format!("{source}="), target),
        None => (String::new(), current.as_ref()),
    };
    let split = chord.rfind('+').map_or(0, |i| i + 1);
    let (modifiers, fragment) = chord.split_at(split);
    let mut options: Vec<(&str, &str)> = MODIFIERS
        .iter()
        .filter(|m| !modifiers.contains(*m))
        .map(|m| (*m, "modifier"))
        .collect();
    options.extend(KEY_NAMES.iter().map(|k| (*k, "key")));
    if !side_prefix.is_empty() && modifiers.is_empty() {
        options.push(("none", "disable the source chord"));
    }
    options
        .into_iter()
        .filter(|(option, _)| option.starts_with(fragment))
        .map(|(option, help)| {
            CompletionCandidate::new(format!("{side_prefix}{modifiers}{option}"))
                .help(Some(help.into()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values(candidates: Vec<CompletionCandidate>) -> Vec<String> {
        candidates
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn key_names_are_accepted_by_map_key() {
        for name in KEY_NAMES {
            crate::keys::KeyMap::parse(&[format!("ctrl+{name}=alt+j"), format!("alt+j={name}")])
                .unwrap_or_else(|e| panic!("{name}: {e}"));
        }
    }

    #[test]
    fn map_key_completes_modifiers_names_and_none() {
        let start = values(map_key_candidates(OsStr::new("")));
        assert!(start.contains(&"ctrl+".into()) && start.contains(&"pagedown".into()));
        assert_eq!(
            values(map_key_candidates(OsStr::new("ctrl+pa"))),
            ["ctrl+pageup", "ctrl+pagedown"]
        );
        let modifiers = values(map_key_candidates(OsStr::new("ctrl+")));
        assert!(
            modifiers.contains(&"ctrl+alt+".into()) && !modifiers.contains(&"ctrl+ctrl+".into())
        );
        assert_eq!(
            values(map_key_candidates(OsStr::new("ctrl+n=n"))),
            ["ctrl+n=none"]
        );
        assert_eq!(
            values(map_key_candidates(OsStr::new("ctrl+n=alt+e"))),
            ["ctrl+n=alt+enter", "ctrl+n=alt+esc"]
        );
    }

    #[test]
    fn config_path_follows_the_completed_command_line() {
        let args = |list: &[&str]| list.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(
            config_path(args(&["picker", "--config", "a.toml"])),
            Some("a.toml".into())
        );
        assert_eq!(
            config_path(args(&["picker", "--config=b.toml"])),
            Some("b.toml".into())
        );
        assert_eq!(config_path(args(&["picker", "--no-config"])), None);
    }

    #[test]
    fn custom_palettes_are_read_from_toml() {
        let example = concat!(env!("CARGO_MANIFEST_DIR"), "/docs/config.example.toml");
        assert_eq!(custom_palettes(Path::new(example)), ["ember"]);
        assert!(custom_palettes(Path::new("/nonexistent/picker.toml")).is_empty());
    }
}
