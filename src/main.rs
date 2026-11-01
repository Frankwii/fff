use clap::{CommandFactory, Parser, builder::styling};
use clap_complete::{ArgValueCompleter, CompleteEnv};
use picker::{App, Policy, completion, config::Settings, glob, keys::KeyMap, ui, write_paths};
use std::{io, path::PathBuf, process::ExitCode};

const STYLES: styling::Styles = styling::Styles::styled()
    .header(styling::AnsiColor::Green.on_default().bold())
    .usage(styling::AnsiColor::Green.on_default().bold())
    .literal(styling::AnsiColor::Cyan.on_default().bold())
    .placeholder(styling::AnsiColor::Cyan.on_default())
    .valid(styling::AnsiColor::Cyan.on_default())
    .invalid(styling::AnsiColor::Yellow.on_default())
    .error(styling::AnsiColor::Red.on_default().bold());

const LONG_ABOUT: &str = "\
Interactive fuzzy file picker for the terminal.

Type to fuzzy-filter file and directory basenames below the current directory,
navigate with Enter, Alt+h and Alt+l, mark several paths with Tab, and confirm.
The interface draws on /dev/tty; stdout receives only the selected paths, so
picker works inside pipes and command substitution.

With no ROOTS, browsing starts in the invocation directory and follows
navigation. Each ROOT is pinned: its descendants stay in the results wherever
you navigate.";

const AFTER_HELP: &str = color_print::cstr!(
    "<bold><green>Examples:</green></bold>
  <cyan>picker</cyan>                          browse from the current directory
  <cyan>vim \"$(picker -g '*.rs')\"</cyan>       open one picked Rust file
  <cyan>picker -0 | xargs -0 ls -ld</cyan>     pass marked paths safely to another tool

Use <cyan>--help</cyan> for details, keys, exit codes and shell completions."
);

const AFTER_LONG_HELP: &str = color_print::cstr!(
    "<bold><green>Examples:</green></bold>
  <cyan>picker</cyan>
      Browse from the current directory, three levels deep.
  <cyan>picker ~/notes ~/papers --depth 2 --glob '*.tex'</cyan>
      Search two pinned roots for TeX files.
  <cyan>vim \"$(picker --relative)\"</cyan>
      Open the picked file using a path relative to here.
  <cyan>picker --exclude build --exclude '*.log' -0 | xargs -0 ls -ld</cyan>
      Act on every marked path with NUL-separated records.
  <cyan>picker --map-key ctrl+n=down --map-key ctrl+p=up</cyan>
      Add Emacs-style movement keys.

<bold><green>Keys:</green></bold> (complete table in the README; F1 shows session details)
  <cyan>type</cyan>  filter          <cyan>Enter</cyan>  open directory / confirm   <cyan>Tab</cyan>  mark
  <cyan>Alt+h</cyan> parent          <cyan>Alt+l</cyan>  enter directory            <cyan>Alt+Space</cyan>  marked list
  <cyan>Alt+←/→</cyan> depth         <cyan>Alt+g</cyan>  location prompt            <cyan>Alt+b</cyan>  bookmarks
  <cyan>Escape</cyan> clear query    <cyan>Ctrl+w</cyan> return marks               <cyan>Ctrl+c</cyan> cancel

<bold><green>Configuration file:</green></bold>
  Optional TOML at $XDG_CONFIG_HOME/picker/config.toml or
  ~/.config/picker/config.toml: palettes, bookmarks, editor backend and cache.
  Check it with <cyan>picker --check-config</cyan>.

<bold><green>Exit status:</green></bold>
  <cyan>0</cyan>    paths were selected (or the configuration is valid)
  <cyan>1</cyan>    startup, configuration or I/O error
  <cyan>2</cyan>    invalid command-line usage
  <cyan>130</cyan>  cancelled with Ctrl+c; nothing is written to stdout

<bold><green>Shell completions:</green></bold>
  bash  <cyan>source <<(picker --completions bash)</cyan>   in ~/.bashrc
  zsh   <cyan>source <<(picker --completions zsh)</cyan>    in ~/.zshrc
  fish  <cyan>picker --completions fish | source</cyan>    in ~/.config/fish/config.fish
  Completions cover options, root and config paths, editors, palettes
  (including custom ones from the configuration) and --map-key chords."
);

#[derive(Parser)]
#[command(
    version,
    about = "Interactive fuzzy file picker: browse in the terminal, print selected paths to stdout",
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP,
    after_long_help = AFTER_LONG_HELP,
    styles = STYLES,
    max_term_width = 100
)]
struct Args {
    /// Pinned search roots; without any, browse from the current directory
    ///
    /// Every ROOT stays in the results while you navigate; the current directory
    /// is searched in addition and is never pinned. Duplicates are ignored.
    #[arg(value_hint = clap::ValueHint::DirPath, help_heading = "Search")]
    roots: Vec<PathBuf>,
    /// Maximum discovery depth below each scope (immediate children = 1)
    ///
    /// Change it during the session with Alt+Left / Alt+Right.
    #[arg(short, long, default_value_t = 3, value_name = "N", help_heading = "Search",
        value_parser = clap::value_parser!(u32).range(1..))]
    depth: u32,
    /// Only list files whose basename matches GLOB (directories stay visible)
    ///
    /// Uses globset syntax (*, ?, [abc], **); quote it so the shell does not
    /// expand it.
    #[arg(short, long, value_name = "GLOB", help_heading = "Search")]
    glob: Option<String>,
    /// Exclude a basename or root-relative glob; repeat for several rules
    ///
    /// A matching directory prunes its whole subtree. Edit the rules during the
    /// session with Alt+e and toggle enforcement with Alt+H.
    #[arg(long = "exclude", value_name = "GLOB", help_heading = "Search")]
    exclusions: Vec<String>,
    /// Show hidden (dot) entries from the start; Alt+. toggles them
    #[arg(long, help_heading = "Search")]
    hidden: bool,
    /// Start with exclusion rules disabled; toggle with Alt+H
    #[arg(long, help_heading = "Search")]
    no_exclusions: bool,
    /// Print paths relative to the invocation directory instead of absolute
    #[arg(long, help_heading = "Output")]
    relative: bool,
    /// Terminate each output path with NUL instead of newline (for xargs -0)
    #[arg(short = '0', long, help_heading = "Output")]
    null: bool,
    /// Text-field editor backend; overrides [editor] backend in the configuration
    ///
    /// nvim (the default) embeds a clean Neovim (no user config or plugins) in
    /// every text field; basic supports typing and Backspace. Without an explicit
    /// choice, a missing or broken nvim falls back to basic quietly. See
    /// docs/EDITING.md.
    #[arg(long, value_enum, help_heading = "Interface")]
    editor: Option<picker::config::EditorBackend>,
    /// Color palette for this session; overrides [ui] palette
    ///
    /// Built-in names include midnight, light, forest, nord, gruvbox-dark,
    /// catppuccin-mocha and tokyonight-night; custom [palettes.NAME] tables from
    /// the configuration work too. Shell completion lists all of them.
    #[arg(long, value_name = "NAME", help_heading = "Interface",
        add = ArgValueCompleter::new(completion::palette_candidates))]
    palette: Option<String>,
    /// Remap a key chord to a default chord, e.g. ctrl+n=down; TARGET=none disables it
    ///
    /// Modifiers are ctrl+, alt+ and shift+; key names are enter, esc, tab, space,
    /// backspace, up, down, left, right, pageup, pagedown, f1, f5 or a single
    /// character. Letter case matters. Repeatable; later rules for the same
    /// SOURCE win.
    #[arg(long = "map-key", value_name = "SOURCE=TARGET", help_heading = "Interface",
        add = ArgValueCompleter::new(completion::map_key_candidates))]
    key_maps: Vec<String>,
    /// Hide Nerd Font filetype icons; overrides [ui] icons
    ///
    /// Rows then use the plain `▸`/`·` markers. Useful when the terminal font has
    /// no Nerd Font glyphs.
    #[arg(long, help_heading = "Interface")]
    no_icons: bool,
    /// Read this TOML configuration instead of the default file
    #[arg(long, value_name = "FILE", conflicts_with = "no_config",
        value_hint = clap::ValueHint::FilePath, help_heading = "Configuration")]
    config: Option<PathBuf>,
    /// Ignore the configuration file and use built-in defaults
    #[arg(long, help_heading = "Configuration")]
    no_config: bool,
    /// Validate the configuration (and --palette), report the result and exit
    #[arg(long, help_heading = "Configuration")]
    check_config: bool,
    /// Print the shell completion script for SHELL and exit
    ///
    /// Load it from your shell's startup file, e.g. `source <(picker --completions
    /// bash)`, so it is regenerated after upgrades. `COMPLETE=SHELL picker` prints
    /// the same script.
    #[arg(long, value_name = "SHELL", help_heading = "Shell integration",
        value_parser = clap::builder::PossibleValuesParser::new(completion::shells()))]
    completions: Option<String>,
}
fn main() -> ExitCode {
    // Answers `COMPLETE=<shell> picker ...` requests before anything is printed.
    CompleteEnv::with_factory(Args::command)
        .var(completion::ENV_VAR)
        .complete();
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(130),
        Err(e) => {
            eprintln!("picker: {e}");
            ExitCode::from(1)
        }
    }
}
fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(shell) = &args.completions {
        let program = std::env::args_os()
            .next()
            .unwrap_or_else(|| "picker".into());
        completion::write_registration(shell, &program, &mut io::stdout().lock())?;
        return Ok(true);
    }
    let settings = Settings::load(
        args.config.as_deref(),
        args.no_config,
        args.palette.as_deref(),
    )?;
    if args.check_config {
        eprintln!("picker: configuration valid");
        return Ok(true);
    }
    let watcher = picker::reload::PaletteWatcher::start(
        args.config.clone(),
        args.no_config,
        args.palette.clone(),
        picker::reload::PaletteState::from_settings(&settings),
    );
    let invocation = std::env::current_dir()?.canonicalize()?;
    let mut roots = Vec::new();
    for root in args.roots {
        let path = root.canonicalize()?;
        if !path.is_dir() {
            return Err(format!("Not a directory: {}", root.display()).into());
        }
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    let mut policy = Policy {
        hidden: args.hidden,
        ignore: !args.no_exclusions,
        ..Policy::default()
    };
    policy.set_rules(args.exclusions)?;
    policy.glob = args.glob.as_deref().map(glob).transpose()?;
    let keys = KeyMap::parse(&args.key_maps)?;
    let mut app = if roots.is_empty() {
        App::browse(invocation.clone(), args.depth as usize, policy)
    } else {
        App::new(roots, args.depth as usize, policy)
    };
    app.apply_palette(picker::reload::PaletteState::from_settings(&settings));
    app.bookmarks = settings.bookmarks;
    app.icons = settings.icons && !args.no_icons;
    if let Some(paths) = ui::run(
        app,
        keys,
        settings.cache,
        watcher,
        args.editor.or(settings.editor.backend),
    )? {
        write_paths(
            io::stdout().lock(),
            &paths,
            &invocation,
            args.relative,
            args.null,
        )?;
        Ok(true)
    } else {
        Ok(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::engine::complete;
    use std::{ffi::OsString, path::Path};

    /// Value-taking arguments whose values are free-form (numbers, globs) and
    /// deliberately have no completion. Every other one must complete.
    const FREE_FORM: &[&str] = &["depth", "glob", "exclusions"];

    fn candidates(line: &[&str], cwd: Option<&Path>) -> Vec<String> {
        let args: Vec<OsString> = line.iter().map(OsString::from).collect();
        let index = args.len() - 1;
        complete(&mut Args::command(), args, index, cwd)
            .unwrap()
            .iter()
            .map(|c| c.get_value().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn every_completable_argument_has_completion() {
        let command = Args::command();
        for arg in command.get_arguments() {
            let id = arg.get_id().as_str();
            if !arg.get_action().takes_values() || ["help", "version"].contains(&id) {
                continue;
            }
            let completes = arg.get::<ArgValueCompleter>().is_some()
                || !arg.get_possible_values().is_empty()
                || matches!(
                    arg.get_value_hint(),
                    clap::ValueHint::DirPath | clap::ValueHint::FilePath | clap::ValueHint::AnyPath
                );
            assert_eq!(completes, !FREE_FORM.contains(&id), "argument {id}");
        }
    }

    #[test]
    fn completion_offers_options_values_paths_and_palettes() {
        assert_eq!(candidates(&["picker", "--pal"], None), ["--palette"]);
        assert_eq!(
            candidates(&["picker", "--editor", ""], None),
            ["basic", "nvim"]
        );
        let shells = candidates(&["picker", "--completions", ""], None);
        for shell in ["bash", "zsh", "fish"] {
            assert!(shells.contains(&shell.to_string()), "{shell}");
        }
        // Custom palettes from the tester's own config may follow the built-ins.
        let palettes = candidates(&["picker", "--palette", "catppuccin-m"], None);
        assert_eq!(palettes[..2], ["catppuccin-macchiato", "catppuccin-mocha"]);
        assert_eq!(
            candidates(&["picker", "--map-key", "ctrl+n=pag"], None),
            ["ctrl+n=pageup", "ctrl+n=pagedown"]
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(candidates(&["picker", "do"], Some(root)), ["docs/"]);
        let configs = candidates(&["picker", "--config", "docs/con"], Some(root));
        assert!(
            configs.contains(&"docs/config.example.toml".to_string()),
            "{configs:?}"
        );
    }

    #[test]
    fn registration_scripts_call_back_into_picker() {
        for shell in ["bash", "zsh", "fish"] {
            let mut script = Vec::new();
            completion::write_registration(shell, "/usr/bin/picker".as_ref(), &mut script).unwrap();
            let script = String::from_utf8(script).unwrap();
            assert!(
                script.contains("COMPLETE") && script.contains("/usr/bin/picker"),
                "{shell}"
            );
            assert!(
                script.contains(&format!("COMPLETE=\"{shell}\""))
                    || script.contains(&format!("COMPLETE={shell}"))
            );
        }
        assert!(
            completion::write_registration("tcsh", "picker".as_ref(), &mut Vec::new()).is_err()
        );
    }
}
