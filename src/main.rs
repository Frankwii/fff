use clap::{CommandFactory, Parser, builder::styling};
use clap_complete::{ArgValueCompleter, CompleteEnv};
use fuzzy_file_finder::{App, Policy, completion, config::Settings, keys::KeyMap, ui, write_paths};
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

Press i to type and fuzzy-filter file and directory basenames (Neovim default),
navigate with Enter, Alt+h and Alt+l, mark several paths with Tab, and confirm.
The interface draws on /dev/tty. By default, leaving fff changes your shell's
directory (with the --init shell function); --output-paths prints the selected
paths instead, for pipes and command substitution.

With no ROOTS, browsing starts in the invocation directory and follows
navigation. Each ROOT is pinned: its descendants stay in the results wherever
you navigate.";

const AFTER_HELP: &str = color_print::cstr!(
    "<bold><green>Examples:</green></bold>
  <cyan>fff</cyan>                          browse; ! or Enter on a file changes directory
  <cyan>eval \"$(fff --init zsh)\"</cyan>     shell integration line for ~/.zshrc
  <cyan>vim \"$(fff --output-paths)\"</cyan>  open one picked file

Use <cyan>--help</cyan> for details, keys, exit codes and shell completions."
);

const AFTER_LONG_HELP: &str = color_print::cstr!(
    "<bold><green>Examples:</green></bold>
  <cyan>fff</cyan>
      Browse from the current directory, two levels deep.
  <cyan>fff ~/notes ~/papers --depth 2 --glob '*.tex' --glob '*.pdf'</cyan>
      Search two pinned roots for TeX and PDF files.
  <cyan>eval \"$(fff --init zsh)\"</cyan>
      In ~/.zshrc: leaving fff changes the shell's directory (bash, fish too).
  <cyan>vim \"$(fff --output-paths --relative)\"</cyan>
      Open the picked file using a path relative to here.
  <cyan>fff --output-paths --exclude build -0 | xargs -0 ls -ld</cyan>
      Act on every marked path with NUL-separated records.
  <cyan>fff --map-key ctrl+n=down --map-key ctrl+p=up</cyan>
      Add Emacs-style movement keys.
  <cyan>fff --setup desktop; fff --setup file-chooser</cyan>
      Open folders from other programs, and Firefox's file dialogs, in fff.

<bold><green>Keys:</green></bold> (press <cyan>F1</cyan> in fff for the full guide; Tab there shows session details)
  <cyan>i, type</cyan>  filter       <cyan>Enter</cyan>  open directory / confirm   <cyan>Tab</cyan>  mark
  <cyan>Alt+h</cyan> parent          <cyan>Alt+l</cyan>  enter directory            <cyan>Alt+Space</cyan>  marked list
  <cyan>Alt+←/→</cyan> depth         <cyan>Alt+g</cyan>  location prompt            <cyan>Alt+b</cyan>  bookmarks
  <cyan>Alt+p</cyan> palette chooser <cyan>Alt+P</cyan>  re-read config palette     <cyan>Ctrl+c</cyan> cancel
  <cyan>Escape</cyan> leave Insert mode (Neovim), then clear query   <cyan>Ctrl+w</cyan> return marks
  In Neovim Normal mode, list views also take <cyan>h/j/k/l</cyan> (as Alt+h/j/k/l),
  results take <cyan>←/→</cyan> and <cyan>1-9</cyan> depth, <cyan>.</cyan> hidden, <cyan>/</cyan> location, <cyan>b</cyan> bookmarks,
  <cyan>↑/↓</cyan> (Alt+↑/↓ in any mode) next / previous of three tabs that share marks,
  Normal mode: <cyan>v/p/s</cyan> move/copy/symlink selected; <cyan>x</cyan> delete with size review.
  <cyan>r</cyan> batch rename; <cyan>e</cyan> extract archive; <cyan>n</cyan> new file/directory.
  <cyan>gx</cyan> run a program on current/marked paths and come back; <cyan>gX</cyan> run it and exit.
  <cyan>E</cyan> open current/marked paths in $EDITOR. Each folder remembers its cursor.
  <cyan>gg</cyan> first row, <cyan>G</cyan> last row and <cyan>Space</cyan> mark. <cyan>ZZ</cyan> saves buffer edits; <cyan>ZQ</cyan> discards.

<bold><green>Configuration file:</green></bold>
  Optional TOML at $XDG_CONFIG_HOME/fff/config.toml (~/.config/fff/
  config.toml when XDG_CONFIG_HOME is unset): palettes, bookmarks, editor, cache,
  file icons ([ui] icons), row details ([ui] columns), the mouse and buttons
  ([ui] mouse, buttons), live updates ([ui] watch), Normal-mode list keys
  ([keys.normal]) and chord remapping ([keys.map], applied before --map-key).
  <cyan>fff --check-config</cyan> validates it and prints which file was read.

<bold><green>Exit status:</green></bold>
  <cyan>0</cyan>    normal exit: a directory was chosen or paths printed (or the
       configuration is valid); after gX, the program's own status
  <cyan>1</cyan>    startup, configuration (including an unknown --palette) or I/O error
  <cyan>2</cyan>    invalid command-line usage: unknown options or malformed values
       (--depth, --map-key, --glob, --exclude, ...)
  <cyan>130</cyan>  cancelled with Ctrl+c; nothing printed, no directory change

<bold><green>Shell completions:</green></bold>
  bash  <cyan>source <<(fff --completions bash)</cyan>   in ~/.bashrc
  zsh   <cyan>source <<(fff --completions zsh)</cyan>    in ~/.zshrc
  fish  <cyan>fff --completions fish | source</cyan>    in ~/.config/fish/config.fish
  Completions cover options, root and config paths, palettes
  (including custom ones from the configuration) and --map-key chords."
);

#[derive(Parser)]
#[command(
    name = "fff",
    bin_name = "fff",
    version,
    about = "Interactive fuzzy file browser and picker for the terminal",
    long_about = LONG_ABOUT,
    after_help = AFTER_HELP,
    after_long_help = AFTER_LONG_HELP,
    styles = STYLES,
    max_term_width = 100
)]
struct Args {
    /// Pinned search roots; without any, browse from the current directory
    ///
    /// Each ROOT's descendants stay in scope while you navigate. The current
    /// directory is searched in addition; duplicate roots are ignored.
    #[arg(value_hint = clap::ValueHint::DirPath, help_heading = "Search")]
    roots: Vec<PathBuf>,
    /// Maximum discovery depth below each scope (immediate children = 1)
    ///
    /// Change it during the session with Alt+Left / Alt+Right.
    #[arg(short, long, default_value_t = 2, value_name = "N", help_heading = "Search",
        value_parser = clap::value_parser!(u32).range(1..))]
    depth: u32,
    /// Only list files whose basename matches GLOB; repeat to allow several
    ///
    /// A file is listed when it matches any GLOB; directories are hidden but
    /// still traversed. Uses globset syntax (*, ?, [abc], {a,b}); quote it so
    /// the shell does not expand it. Edit the filters during the session with
    /// Alt+f.
    #[arg(short, long, value_name = "GLOB", help_heading = "Search")]
    glob: Vec<String>,
    /// Exclude a basename or root-relative glob; repeat for several rules
    ///
    /// A matching directory prunes its whole subtree. Edit the rules during the
    /// session with Alt+e and toggle enforcement with Alt+H.
    #[arg(long = "exclude", value_name = "GLOB", help_heading = "Search")]
    exclusions: Vec<String>,
    /// Static file choice: hide directories and disable navigation
    ///
    /// Only files below the current directory (or the pinned ROOTS) are listed,
    /// down to --depth. Directory keys (Enter on a directory, Alt+h/Alt+l, the
    /// Alt+g location prompt, Alt+b bookmarks) are disabled, and the current
    /// directory is never returned. Mark files with Tab, then confirm. Rows show
    /// bare names; files sharing a name gain the shortest distinguishing parents.
    #[arg(long, help_heading = "Search")]
    no_dir: bool,
    /// Start in the current directory instead of where the last session ended
    ///
    /// Without ROOTS and without --output-paths, fff resumes in the directory
    /// you were last browsing ([ui] resume, on by default).
    #[arg(long, help_heading = "Search")]
    here: bool,
    /// Browse from the one ROOT given instead of pinning it
    ///
    /// A directory is where browsing starts; a file starts in its folder with
    /// the file highlighted. Without a ROOT it is plain browsing. The desktop
    /// entry and file chooser installed by --setup start fff this way.
    #[arg(long, help_heading = "Search")]
    browse: bool,
    /// Show hidden (dot) entries from the start; Alt+. toggles them
    #[arg(long, help_heading = "Search")]
    hidden: bool,
    /// Start with exclusion rules disabled; toggle with Alt+H
    #[arg(long, help_heading = "Search")]
    no_exclusions: bool,
    /// Print the selected paths to stdout instead of changing directory
    ///
    /// The picker behavior for scripts and command substitution: Enter on a
    /// file prints the marks and that file, Ctrl+w the marks, q the launch
    /// directory and ! the browsing directory. Without it, fff chooses a
    /// directory to change into (see --init).
    #[arg(long, help_heading = "Output")]
    output_paths: bool,
    /// Print paths relative to the invocation directory instead of absolute
    #[arg(long, requires = "output_paths", help_heading = "Output")]
    relative: bool,
    /// Terminate each output path with NUL instead of newline (for xargs -0)
    #[arg(short = '0', long, requires = "output_paths", help_heading = "Output")]
    null: bool,
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
    /// SOURCE win, and these override the configuration's [keys.map].
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
    /// Load it from your shell's startup file, e.g. `source <(fff --completions
    /// bash)`, so it is regenerated after upgrades. `COMPLETE=SHELL fff` prints
    /// the same script.
    #[arg(long, value_name = "SHELL", help_heading = "Shell integration",
        value_parser = clap::builder::PossibleValuesParser::new(completion::shells()))]
    completions: Option<String>,
    /// Print a shell function that changes directory when fff exits, and exit
    ///
    /// Add `eval "$(fff --init zsh)"` (or bash) to your shell's startup file,
    /// or `fff --init fish | source` for fish. The function is named fff and
    /// calls the real binary with --cd-file.
    #[arg(
        long,
        value_enum,
        value_name = "SHELL",
        help_heading = "Shell integration"
    )]
    init: Option<completion::InitShell>,
    /// Write the directory to change into to FILE instead of printing it
    ///
    /// Used by the --init function. Nothing is written when the directory
    /// would not change.
    #[arg(long, value_name = "FILE", value_hint = clap::ValueHint::FilePath,
        help_heading = "Shell integration")]
    cd_file: Option<PathBuf>,
    /// Install fff as the desktop's folder handler or file chooser, and exit
    ///
    /// desktop writes an fff.desktop entry, run in [open] terminal, and makes
    /// it the default for directories (xdg-open, "Show in folder").
    /// file-chooser sets up xdg-desktop-portal-termfilechooser to pick files
    /// with fff, for Firefox and other portal users. Files fff generated
    /// before are replaced; other files are never overwritten.
    #[arg(
        long,
        value_enum,
        value_name = "TARGET",
        help_heading = "Shell integration"
    )]
    setup: Option<fuzzy_file_finder::setup::Target>,
}
fn main() -> ExitCode {
    // Answers `COMPLETE=<shell> fff ...` requests before anything is printed.
    CompleteEnv::with_factory(Args::command)
        .var(completion::ENV_VAR)
        .complete();
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(130),
        Err(e) => {
            eprintln!("fff: {e}");
            ExitCode::from(1)
        }
    }
}
fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(shell) = args.init {
        print!("{}", completion::wrapper(shell));
        return Ok(true);
    }
    if let Some(shell) = &args.completions {
        let program = std::env::args_os().next().unwrap_or_else(|| "fff".into());
        completion::write_registration(shell, &program, &mut io::stdout().lock())?;
        return Ok(true);
    }
    // Malformed argument values are usage errors (exit 2), like clap's own.
    let usage = |flag: &str, error: &dyn std::fmt::Display| -> ! {
        Args::command()
            .error(
                clap::error::ErrorKind::ValueValidation,
                format!("invalid value for {flag}: {error}"),
            )
            .exit()
    };
    if let Err(e) = KeyMap::parse(&args.key_maps) {
        usage("--map-key", &e);
    }
    let mut policy = Policy {
        hidden: args.hidden,
        ignore: !args.no_exclusions,
        ..Policy::default()
    };
    if let Err(e) = policy.set_rules(args.exclusions) {
        usage("--exclude", &e);
    }
    if let Err(e) = policy.set_filters(args.glob) {
        usage("--glob", &e);
    }
    let settings = Settings::load(
        args.config.as_deref(),
        args.no_config,
        args.palette.as_deref(),
    )?;
    // Configured [keys.map] chords first, so --map-key rules override them.
    let keys = KeyMap::parse(&[settings.key_maps.clone(), args.key_maps].concat())?;
    if let Some(target) = args.setup {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let terminal = fuzzy_file_finder::run::split(&settings.open.terminal, home.as_deref())
            .map_err(|e| format!("[open] terminal: {e}"))?;
        let dirs = fuzzy_file_finder::setup::Dirs::from_env()?;
        let fff = std::env::current_exe()?;
        fuzzy_file_finder::setup::run(target, &dirs, &fff, &terminal, &mut io::stderr())?;
        return Ok(true);
    }
    if args.check_config {
        // Openers name programs; report the ones that are not installed.
        let missing = settings.open.missing(&std::env::current_dir()?);
        if !missing.is_empty() {
            return Err(format!("[open] programs not found: {}", missing.join("; ")).into());
        }
        match &settings.source {
            Some(path) => eprintln!("fff: configuration valid: {}", path.display()),
            None => eprintln!("fff: no configuration file; using built-in settings"),
        }
        return Ok(true);
    }
    let watcher = fuzzy_file_finder::reload::PaletteWatcher::start(
        args.config.clone(),
        args.no_config,
        args.palette.clone(),
        fuzzy_file_finder::reload::PaletteState::from_settings(&settings),
    );
    let invocation = std::env::current_dir()?.canonicalize()?;
    // --browse: where to start, and the file to highlight there.
    let mut browse_from = None;
    if args.browse {
        if args.roots.len() > 1 {
            usage("--browse", &"give at most one path to browse from");
        }
        if let Some(path) = args.roots.first() {
            let full = path
                .canonicalize()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            browse_from = Some(match full.parent() {
                Some(parent) if !full.is_dir() => (parent.to_owned(), Some(full.clone())),
                _ => (full, None),
            });
        }
    }
    let mut roots = Vec::new();
    for root in args.roots.into_iter().filter(|_| !args.browse) {
        let path = root
            .canonicalize()
            .map_err(|e| format!("{}: {e}", root.display()))?;
        if !path.is_dir() {
            return Err(format!("Not a directory: {}", root.display()).into());
        }
        if !roots.contains(&path) {
            roots.push(path);
        }
    }
    // Interactive sessions without roots resume where the last one ended;
    // scripts (--output-paths) and pinned roots always start as asked.
    let state = fuzzy_file_finder::state::path()
        .map(|p| fuzzy_file_finder::state::load(&p))
        .unwrap_or_default();
    let records_location = roots.is_empty() && !args.output_paths;
    let resume = (records_location && settings.resume && !args.here)
        .then_some(state.last.as_ref())
        .flatten()
        .filter(|last| last.is_dir() && **last != invocation)
        .filter(|_| browse_from.is_none());
    let mut app = if let Some((start, _)) = &browse_from {
        App::browse(start.clone(), args.depth as usize, policy)
    } else if let Some(last) = resume {
        App::browse(last.clone(), args.depth as usize, policy)
    } else if roots.is_empty() {
        App::browse(invocation.clone(), args.depth as usize, policy)
    } else {
        App::new(roots, args.depth as usize, policy)
    };
    app.records_location = records_location;
    app.openers = settings.open.clone();
    app.output_paths = args.output_paths;
    app.positions = state.positions;
    app.wanted = match &browse_from {
        Some((_, Some(file))) => Some(file.clone()),
        _ => app.remembered(&app.cwd.clone()),
    };
    app.apply_palette(fuzzy_file_finder::reload::PaletteState::from_settings(
        &settings,
    ));
    app.bookmarks = settings.bookmarks;
    app.icons = settings.icons && !args.no_icons;
    app.normal_keys = settings.normal_keys;
    app.no_dir = args.no_dir;
    app.status_timeout = settings.message_timeout;
    app.columns = settings.columns;
    app.mouse = settings.mouse;
    app.buttons = settings.buttons;
    app.watch = settings.watch;
    app.home = std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    if resume.is_some() {
        app.status =
            "Resumed where you left off; q returns to where you started, --here starts there"
                .into();
        app.status_success = app.status.clone();
    }
    app.bookmark_store = fuzzy_file_finder::bookmarks::Store::new(
        settings.bookmark_path,
        app.bookmarks.clone(),
        app.home.clone(),
    );
    let finish = ui::run(app, keys, settings.cache, watcher, settings.performance)?;
    // In the default mode the session chooses a directory: the folder of the
    // confirmed entry (a directory itself for directories).
    let change_to = |target: Option<&PathBuf>| -> io::Result<()> {
        let Some(target) = target else {
            return Ok(());
        };
        let dir = if target.is_dir() {
            target.as_path()
        } else {
            target.parent().unwrap_or(target)
        };
        if dir == invocation {
            return Ok(());
        }
        match &args.cd_file {
            Some(file) => std::fs::write(file, dir.as_os_str().as_encoded_bytes()),
            // Without the wrapper, `cd "$(command fff)"` still works.
            None => write_paths(
                io::stdout().lock(),
                &[dir.to_owned()],
                &invocation,
                false,
                false,
            ),
        }
    };
    match finish {
        ui::Finish::Cancelled => Ok(false),
        ui::Finish::Paths(paths) if args.output_paths => {
            write_paths(
                io::stdout().lock(),
                &paths,
                &invocation,
                args.relative,
                args.null,
            )?;
            Ok(true)
        }
        ui::Finish::Folder(folder) if args.output_paths => {
            write_paths(
                io::stdout().lock(),
                &[folder],
                &invocation,
                args.relative,
                args.null,
            )?;
            Ok(true)
        }
        ui::Finish::Folder(folder) => {
            change_to(Some(&folder))?;
            Ok(true)
        }
        ui::Finish::Paths(paths) => {
            change_to(paths.last())?;
            Ok(true)
        }
        ui::Finish::Exec(plan) => {
            // The wrapper changes to the browsing directory once the program ends.
            if !args.output_paths {
                change_to(Some(&plan.cwd))?;
            }
            Err(format!("Could not run {}: {}", plan.name, plan.exec()).into())
        }
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
        assert_eq!(candidates(&["fff", "--pal"], None), ["--palette"]);
        let shells = candidates(&["fff", "--completions", ""], None);
        for shell in ["bash", "zsh", "fish"] {
            assert!(shells.contains(&shell.to_string()), "{shell}");
        }
        // Custom palettes from the tester's own config may follow the built-ins.
        let palettes = candidates(&["fff", "--palette", "catppuccin-m"], None);
        assert_eq!(palettes[..2], ["catppuccin-macchiato", "catppuccin-mocha"]);
        assert_eq!(
            candidates(&["fff", "--map-key", "ctrl+n=pag"], None),
            ["ctrl+n=pageup", "ctrl+n=pagedown"]
        );
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(candidates(&["fff", "do"], Some(root)), ["docs/"]);
        let configs = candidates(&["fff", "--config", "docs/con"], Some(root));
        assert!(
            configs.contains(&"docs/config.example.toml".to_string()),
            "{configs:?}"
        );
    }

    #[test]
    fn registration_scripts_call_back_into_fff() {
        for shell in ["bash", "zsh", "fish"] {
            let mut script = Vec::new();
            completion::write_registration(shell, "/usr/bin/fff".as_ref(), &mut script).unwrap();
            let script = String::from_utf8(script).unwrap();
            assert!(
                script.contains("COMPLETE") && script.contains("/usr/bin/fff"),
                "{shell}"
            );
            assert!(
                script.contains(&format!("COMPLETE=\"{shell}\""))
                    || script.contains(&format!("COMPLETE={shell}"))
            );
        }
        assert!(completion::write_registration("tcsh", "fff".as_ref(), &mut Vec::new()).is_err());
    }
}
