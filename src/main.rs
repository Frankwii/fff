use clap::{CommandFactory, Parser, builder::styling};
use clap_complete::{ArgValueCompleter, CompleteEnv};
use fuzzy_file_finder::{
    App, Policy, completion, config::Settings, explorer::Dialog, keys::KeyMap, ui, write_paths,
};
use std::{
    io::{self, Write},
    path::{Path, PathBuf},
    process::ExitCode,
};

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
navigate with Enter, Alt+h and Alt+l, mark several paths with Space, and confirm.
The interface draws on /dev/tty. Without a command (fff roam), leaving fff
changes your shell's directory, with the shell function from fff setup --init;
fff pick prints the selected paths instead, for pipes and command substitution.
fff --resume continues the last session as it ended.

With no ROOTS, browsing starts in the invocation directory and follows
navigation. Each ROOT is pinned: its descendants stay in the results wherever
you navigate.";

const AFTER_HELP: &str = color_print::cstr!(
    "<bold><green>Examples:</green></bold>
  <cyan>fff</cyan>                             browse; ! or Enter on a file changes directory
  <cyan>eval \"$(fff setup --init zsh)\"</cyan>  shell integration line for ~/.zshrc
  <cyan>vim \"$(fff pick)\"</cyan>               open one picked file
  <cyan>fff --resume</cyan>                    continue the last session

Use <cyan>--help</cyan> for details, keys, exit codes and shell completions."
);

const AFTER_LONG_HELP: &str = color_print::cstr!(
    "<bold><green>Examples:</green></bold>
  <cyan>fff</cyan>
      Browse from the current directory, two levels deep.
  <cyan>fff ~/notes ~/papers --depth 2 --glob '*.tex' --glob '*.pdf'</cyan>
      Search two pinned roots for TeX and PDF files.
  <cyan>eval \"$(fff setup --init zsh)\"</cyan>
      In ~/.zshrc: leaving fff changes the shell's directory (bash, fish too).
  <cyan>fff --resume</cyan>
      Continue the last session: folders, tabs, highlight, marks and query.
  <cyan>vim \"$(fff pick --relative)\"</cyan>
      Open the picked file using a path relative to here.
  <cyan>fff pick --exclude build -0 | xargs -0 ls -ld</cyan>
      Act on every marked path with NUL-separated records.
  <cyan>fff pick ~/papers --depth 4 --glob '*.pdf' -0 | xargs -0 zathura</cyan>
      Choose PDFs from anywhere below ~/papers and open them.
  <cyan>fff --map-key ctrl+n=down --map-key ctrl+p=up</cyan>
      Add Emacs-style movement keys.
  <cyan>fff setup --desktop; fff setup --file-chooser</cyan>
      Open folders from other programs, and Firefox's file dialogs, in fff.
  <cyan>fff explorer --save ~/Downloads/paper.pdf</cyan>
      Choose where to save a file, as a file dialog would (see fff explorer --help).

<bold><green>Keys:</green></bold> (press <cyan>F1</cyan> in fff for the full guide, <cyan>F2</cyan> for session details)
  <cyan>i, type</cyan>  filter       <cyan>Enter</cyan>  open directory / confirm   <cyan>Space</cyan>  mark
  <cyan>Alt+h</cyan> parent          <cyan>Alt+l</cyan>  enter directory            <cyan>Alt+Space</cyan>  marked list
  <cyan>Alt+←/→</cyan> depth         <cyan>Alt+g</cyan>  location prompt            <cyan>Alt+b</cyan>  bookmarks
  <cyan>Alt+p</cyan> palette chooser <cyan>Alt+P</cyan>  re-read config palette     <cyan>Ctrl+c</cyan> cancel
  <cyan>Escape</cyan> leave Insert mode, clear query, twice: marks   <cyan>Ctrl+w</cyan> return marks (pick)
  In Neovim Normal mode, list views also take <cyan>h/j/k/l</cyan> (as Alt+h/j/k/l),
  results take <cyan>-/+</cyan> and <cyan>1-9</cyan> depth, <cyan>.</cyan> hidden, <cyan>g/</cyan> location, <cyan>b</cyan> bookmarks,
  <cyan>↑/↓</cyan> (Alt+↑/↓ in any mode) next / previous of three tabs that share marks,
  Normal mode: <cyan>v/p/s</cyan> move/copy/symlink selected; <cyan>x</cyan> trash, <cyan>X</cyan> delete, <cyan>u</cyan> undo.
  <cyan>r</cyan> batch rename; <cyan>E</cyan> extract archive; <cyan>e</cyan> edit; <cyan>n</cyan> new file/directory.
  <cyan>Alt+v</cyan> (<cyan>P</cyan>) preview; <cyan>Alt+d</cyan> (<cyan>d</cyan>) hide folders, <cyan>Alt+D</cyan> (<cyan>D</cyan>) files;
  feature chords like these work while typing, their letters in Normal mode.
  <cyan>gx</cyan> run a program on current/marked paths and come back; <cyan>gX</cyan> run it and exit.
  <cyan>E</cyan> open current/marked paths in $EDITOR; <cyan>gc</cyan> edit the configuration and apply it.
  Each folder remembers its cursor.
  <cyan>gg</cyan> first row, <cyan>G</cyan> last row and <cyan>Space</cyan> mark. <cyan>ZZ</cyan> saves buffer edits; <cyan>ZQ</cyan> discards.

<bold><green>Configuration file:</green></bold>
  Optional TOML at $XDG_CONFIG_HOME/fff/config.toml (~/.config/fff/
  config.toml when XDG_CONFIG_HOME is unset): palettes, bookmarks, editor, cache,
  file icons ([ui] icons), row details ([ui] columns), the preview pane
  ([ui] preview) and its syntax colors ([ui] highlight), the mouse and buttons
  ([ui] mouse, buttons), live updates ([ui] watch), Normal-mode list keys
  ([keys.normal]) and chord remapping ([keys.map], applied before --map-key).
  <cyan>fff setup --check-config</cyan> validates it and prints which file was read.

<bold><green>Exit status:</green></bold>
  <cyan>0</cyan>    normal exit: a directory was chosen or paths printed (or the
       configuration is valid); after gX, the program's own status
  <cyan>1</cyan>    startup, configuration (including an unknown --palette) or I/O error
  <cyan>2</cyan>    invalid command-line usage: unknown options or malformed values
       (--depth, --map-key, --glob, --exclude, ...)
  <cyan>130</cyan>  cancelled with Ctrl+c; nothing printed, no directory change

<bold><green>Shell completions:</green></bold>
  bash  <cyan>source <<(fff setup --completions bash)</cyan>   in ~/.bashrc
  zsh   <cyan>source <<(fff setup --completions zsh)</cyan>    in ~/.zshrc
  fish  <cyan>fff setup --completions fish | source</cyan>    in ~/.config/fish/config.fish
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
    max_term_width = 100,
    subcommand_value_name = "COMMAND",
    subcommand_help_heading = "Commands (without one, fff roam)"
)]
struct Args {
    #[command(flatten)]
    roam: RoamArgs,
    /// Maximum discovery depth below each scope (immediate children = 1)
    ///
    /// Change it during the session with Alt+Left / Alt+Right, or - / + in
    /// Normal mode.
    ///
    /// [default: 1; in fff pick, 3; in fff seek, 6]
    #[arg(global = true, short, long, value_name = "N", help_heading = "Search",
        value_parser = clap::value_parser!(u32).range(1..))]
    depth: Option<u32>,
    /// Only list files whose basename matches GLOB; repeat to allow several
    ///
    /// A file is listed when it matches any GLOB; directories are hidden but
    /// still traversed. Uses globset syntax (*, ?, [abc], {a,b}); quote it so
    /// the shell does not expand it. Edit the filters during the session with
    /// Alt+f.
    #[arg(
        global = true,
        short,
        long,
        value_name = "GLOB",
        help_heading = "Search"
    )]
    glob: Vec<String>,
    /// Exclude a basename or root-relative glob; repeat for several rules
    ///
    /// A matching directory prunes its whole subtree. Edit the rules during the
    /// session with Alt+e and toggle enforcement with Alt+H.
    #[arg(
        global = true,
        long = "exclude",
        value_name = "GLOB",
        help_heading = "Search"
    )]
    exclusions: Vec<String>,
    /// Show hidden (dot) entries from the start; Alt+. toggles them
    #[arg(global = true, long, help_heading = "Search")]
    hidden: bool,
    /// Start with exclusion rules disabled; toggle with Alt+H
    #[arg(global = true, long, help_heading = "Search")]
    no_exclusions: bool,
    /// Color palette for this session; overrides [ui] palette
    ///
    /// Built-in names include midnight, light, forest, nord, gruvbox-dark,
    /// catppuccin-mocha and tokyonight-night; custom [palettes.NAME] tables from
    /// the configuration work too. Shell completion lists all of them.
    #[arg(global = true, long, value_name = "NAME", help_heading = "Interface",
        add = ArgValueCompleter::new(completion::palette_candidates))]
    palette: Option<String>,
    /// Remap a key chord to a default chord, e.g. ctrl+n=down; TARGET=none disables it
    ///
    /// Modifiers are ctrl+, alt+ and shift+; key names are enter, esc, tab, space,
    /// backspace, up, down, left, right, pageup, pagedown, f1 to f12 or a single
    /// character. Letter case matters. Repeatable; later rules for the same
    /// SOURCE win, and these override the configuration's [keys.map].
    #[arg(global = true, long = "map-key", value_name = "SOURCE=TARGET", help_heading = "Interface",
        add = ArgValueCompleter::new(completion::map_key_candidates))]
    key_maps: Vec<String>,
    /// Hide Nerd Font filetype icons; overrides [ui] icons
    ///
    /// Rows then use the plain `▸`/`·` markers. Useful when the terminal font has
    /// no Nerd Font glyphs.
    #[arg(global = true, long, help_heading = "Interface")]
    no_icons: bool,
    /// Read this TOML configuration instead of the default file
    #[arg(global = true, long, value_name = "FILE", conflicts_with = "no_config",
        value_hint = clap::ValueHint::FilePath, help_heading = "Configuration")]
    config: Option<PathBuf>,
    /// Ignore the configuration file and use built-in defaults
    #[arg(global = true, long, help_heading = "Configuration")]
    no_config: bool,
    /// Write the directory to change into to FILE instead of printing it
    ///
    /// Used by the shell function from fff setup --init, which passes it
    /// before any command. Nothing is written when the directory would not
    /// change.
    #[arg(long, value_name = "FILE", value_hint = clap::ValueHint::FilePath,
        help_heading = "Shell integration")]
    cd_file: Option<PathBuf>,
    /// Options that became commands, kept only to say so.
    #[command(flatten)]
    moved: Moved,
    #[command(subcommand)]
    command: Option<Command>,
}
/// fff roam, which is also fff without a command.
#[derive(clap::Args, Default)]
struct RoamArgs {
    /// Pinned search roots; without any, browse from the current directory
    ///
    /// Each ROOT's descendants stay in scope while you navigate. The current
    /// directory is searched in addition; duplicate roots are ignored.
    #[arg(value_hint = clap::ValueHint::DirPath, help_heading = "Search")]
    roots: Vec<PathBuf>,
    /// Browse from the one ROOT given instead of pinning it
    ///
    /// A directory is where browsing starts; a file starts in its folder with
    /// the file highlighted. Without a ROOT it is plain browsing. The desktop
    /// entry installed by fff setup --desktop starts fff this way.
    #[arg(long, help_heading = "Search")]
    browse: bool,
    /// Continue the last fff roam session where it ended
    ///
    /// Its folders and tabs, highlight, marks, query, depth, filters,
    /// exclusions and toggles come back as they were. Only the last session is
    /// kept; folders that are gone fall back to their nearest parent.
    #[arg(long, help_heading = "Search",
        conflicts_with_all = ["roots", "browse", "depth", "glob", "exclusions", "hidden", "no_exclusions"])]
    resume: bool,
    /// Run in a terminal window of its own, with no shell to change directory
    ///
    /// Normal-mode Enter on a folder enters it instead of leaving, ! replaces
    /// fff with $SHELL in the folder being browsed, and nothing is printed.
    /// The desktop entry installed by fff setup --desktop starts fff this way.
    #[arg(long, help_heading = "Interface")]
    window: bool,
    /// Accepted for compatibility: fff always starts here unless --resume.
    #[arg(long, hide = true)]
    here: bool,
}
impl RoamArgs {
    fn is_empty(&self) -> bool {
        self.roots.is_empty() && !self.browse && !self.resume && !self.window
    }
}
/// Options of earlier versions, each now part of a command.
#[derive(clap::Args)]
struct Moved {
    #[arg(long, hide = true)]
    no_dir: bool,
    #[arg(long, hide = true)]
    output_paths: bool,
    #[arg(long, hide = true, value_name = "SHELL")]
    init: Option<String>,
    #[arg(long, hide = true, value_name = "SHELL")]
    completions: Option<String>,
    #[arg(long, hide = true, value_name = "TARGET")]
    setup: Option<String>,
    #[arg(long, hide = true)]
    check_config: bool,
}
impl Moved {
    /// Where the option given went, if one was.
    fn instead(&self) -> Option<String> {
        let shell = |s: &Option<String>| s.clone().unwrap_or_default();
        Some(if self.no_dir {
            "--no-dir is now a command of its own: fff seek [ROOTS]".into()
        } else if self.output_paths {
            "--output-paths is now a command of its own: fff pick [ROOTS]".into()
        } else if self.init.is_some() {
            format!("--init is now fff setup --init {}", shell(&self.init))
        } else if self.completions.is_some() {
            format!(
                "--completions is now fff setup --completions {}",
                shell(&self.completions)
            )
        } else if self.setup.is_some() {
            format!("--setup is now fff setup --{}", shell(&self.setup))
        } else if self.check_config {
            "--check-config is now fff setup --check-config".into()
        } else {
            return None;
        })
    }
}
#[derive(clap::Subcommand)]
enum Command {
    /// Browse folders; leaving changes your shell's directory (the default)
    ///
    /// What fff does without a command. q leaves you where you started, !
    /// takes you to the folder you were browsing, and Normal-mode Enter goes
    /// into a folder or opens a file and leaves you in its folder (with the
    /// shell function from fff setup --init; without it, the folder is
    /// printed).
    Roam(RoamArgs),
    /// Browse and print the chosen paths, for scripts and $(...)
    ///
    /// Enter on a file prints it (while typing, the marks and it), Ctrl+number
    /// does the same for its row, Ctrl+w prints the marks and ! the browsing
    /// directory; q, like Ctrl+c, prints nothing.
    Pick(PickArgs),
    /// Choose files below the current directory or ROOTS, without navigating
    ///
    /// A static file choice: only files are listed, from every folder down to
    /// --depth; the current directory cannot change, and the query starts in
    /// Insert mode, ready to type. Rows show bare names; files sharing a name
    /// gain the shortest distinguishing parents. Enter leaves into the chosen
    /// file's folder. q and ! leave without choosing anything. To print files, use fff pick:
    /// with --glob it too lists only matching files.
    Seek(SeekArgs),
    /// Answer a file dialog: choose files or a folder, or where to save
    ///
    /// Prints the chosen paths, one per line, for the file chooser that
    /// fff setup --file-chooser installs. Enter on a file chooses it and on a
    /// folder enters it, in either mode; ! takes the folder you are in, and
    /// Ctrl+s does too while typing; q and Ctrl+c cancel. The query line says
    /// how to choose.
    Explorer(ExplorerArgs),
    /// Set fff up: the desktop, file dialogs, your shell, the configuration
    ///
    /// Does one of these and exits.
    Setup(SetupArgs),
    /// Answer org.freedesktop.FileManager1 ("Show in folder"), run by D-Bus
    ///
    /// The session bus starts it from the service file fff setup --desktop
    /// writes; each folder or file it is asked to show opens with LAUNCHER.
    /// It exits after ten minutes without calls.
    #[command(name = "dbus-file-manager", hide = true)]
    DbusFileManager(DbusArgs),
}
#[derive(clap::Args)]
struct DbusArgs {
    /// The open-folder launcher fff setup --desktop wrote
    #[arg(value_hint = clap::ValueHint::FilePath)]
    launcher: PathBuf,
}
#[derive(clap::Args)]
struct PickArgs {
    /// Pinned search roots; without any, browse from the current directory
    #[arg(value_hint = clap::ValueHint::DirPath, help_heading = "Search")]
    roots: Vec<PathBuf>,
    /// Browse from the one ROOT given instead of pinning it
    #[arg(long, help_heading = "Search")]
    browse: bool,
    #[command(flatten)]
    output: OutputArgs,
}
#[derive(clap::Args)]
struct OutputArgs {
    /// Print paths relative to the invocation directory instead of absolute
    #[arg(long, help_heading = "Output")]
    relative: bool,
    /// Terminate each output path with NUL instead of newline (for xargs -0)
    #[arg(short = '0', long, help_heading = "Output")]
    null: bool,
}
#[derive(clap::Args)]
struct SeekArgs {
    /// Where to look; without any, the current directory
    #[arg(value_hint = clap::ValueHint::DirPath, help_heading = "Search")]
    roots: Vec<PathBuf>,
}
#[derive(clap::Args)]
struct ExplorerArgs {
    /// Where to start: a folder, or a file to highlight in its folder; when
    /// saving, the suggested file, which need not exist
    #[arg(value_hint = clap::ValueHint::AnyPath)]
    path: Option<PathBuf>,
    /// Allow several files (Space marks them)
    #[arg(long)]
    multiple: bool,
    /// Choose folders instead of files: ! chooses the folder you are in
    #[arg(long, conflicts_with = "save")]
    directory: bool,
    /// Choose where to save: ! or Ctrl+s saves here under the typed name (the
    /// query) or the suggested one; replacing a file asks first
    #[arg(long)]
    save: bool,
}
#[derive(clap::Args)]
#[group(required = true, multiple = false)]
struct SetupArgs {
    /// Open folders from other programs in fff
    ///
    /// Writes an fff.desktop entry, run in [open] fff_terminal, and makes it
    /// the default for directories (xdg-open), and a D-Bus service that
    /// answers "Show in folder" with the file highlighted. Files fff
    /// generated before are replaced; other files are never overwritten.
    #[arg(long)]
    desktop: bool,
    /// Pick files with fff in Firefox and other applications' file dialogs
    ///
    /// Sets up xdg-desktop-portal-termfilechooser to run fff explorer, for
    /// every application using the desktop portal. Files fff generated before
    /// are replaced; other files are never overwritten.
    #[arg(long, visible_alias = "browser")]
    file_chooser: bool,
    /// Print a shell function that changes directory when fff exits
    ///
    /// Add `eval "$(fff setup --init zsh)"` (or bash) to your shell's startup
    /// file, or `fff setup --init fish | source` for fish. The function is
    /// named fff and calls the real binary with --cd-file.
    #[arg(long, value_enum, value_name = "SHELL")]
    init: Option<completion::InitShell>,
    /// Print the shell completion script for SHELL
    ///
    /// Load it from your shell's startup file, e.g. `source <(fff setup
    /// --completions bash)`, so it is regenerated after upgrades.
    /// `COMPLETE=SHELL fff` prints the same script.
    #[arg(long, value_name = "SHELL",
        value_parser = clap::builder::PossibleValuesParser::new(completion::shells()))]
    completions: Option<String>,
    /// Validate the configuration (and --palette) and report the result
    #[arg(long)]
    check_config: bool,
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
            // Not eprintln!, which panics when the terminal is gone.
            let _ = writeln!(io::stderr(), "fff: {e}");
            ExitCode::from(1)
        }
    }
}
/// What kind of session a command asks for, from its arguments.
#[derive(Default)]
struct Mode {
    roots: Vec<PathBuf>,
    browse: bool,
    resume: bool,
    window: bool,
    output_paths: bool,
    relative: bool,
    null: bool,
    no_dir: bool,
    explorer: Option<ExplorerArgs>,
}
/// Exits with a usage error (status 2) for arguments that cannot go together.
fn conflict(message: &str) -> ! {
    Args::command()
        .error(clap::error::ErrorKind::ArgumentConflict, message)
        .exit()
}
/// Exits with a usage error (status 2) for a malformed value, like clap's own.
fn usage(flag: &str, error: &dyn std::fmt::Display) -> ! {
    Args::command()
        .error(
            clap::error::ErrorKind::ValueValidation,
            format!("invalid value for {flag}: {error}"),
        )
        .exit()
}
fn run() -> Result<bool, Box<dyn std::error::Error>> {
    let mut args = Args::parse();
    if let Some(instead) = args.moved.instead() {
        Args::command()
            .error(clap::error::ErrorKind::UnknownArgument, instead)
            .exit()
    }
    // fff roam's options before another command would be ignored.
    let roam = std::mem::take(&mut args.roam);
    if !matches!(args.command, None | Some(Command::Roam(_))) && !roam.is_empty() {
        conflict("roots, --browse, --resume and --window belong to fff roam");
    }
    let mode = match args.command.take() {
        None => Mode {
            roots: roam.roots,
            browse: roam.browse,
            resume: roam.resume,
            window: roam.window,
            ..Mode::default()
        },
        Some(Command::Roam(mut more)) => {
            let mut roots = roam.roots;
            roots.append(&mut more.roots);
            Mode {
                roots,
                browse: roam.browse || more.browse,
                resume: roam.resume || more.resume,
                window: roam.window || more.window,
                ..Mode::default()
            }
        }
        Some(Command::Pick(pick)) => Mode {
            roots: pick.roots,
            browse: pick.browse,
            output_paths: true,
            relative: pick.output.relative,
            null: pick.output.null,
            ..Mode::default()
        },
        Some(Command::Seek(seek)) => Mode {
            roots: seek.roots,
            no_dir: true,
            ..Mode::default()
        },
        Some(Command::Explorer(explorer)) => Mode {
            output_paths: true,
            explorer: Some(explorer),
            ..Mode::default()
        },
        Some(Command::Setup(setup)) => return run_setup(setup, args),
        Some(Command::DbusFileManager(dbus)) => {
            fuzzy_file_finder::dbus::serve(None, &dbus.launcher, fuzzy_file_finder::dbus::IDLE)?;
            return Ok(true);
        }
    };
    if mode.resume && !mode.roots.is_empty() {
        conflict("--resume continues the last session's own roots; give no ROOTS");
    }
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
    // A configuration with errors left by gc gives way to its backup.
    let settings = Settings::load_or_backup(
        args.config.as_deref(),
        args.no_config,
        args.palette.as_deref(),
    )?;
    // Configured [keys.map] chords first, so --map-key rules override them.
    let keys = KeyMap::parse(&[settings.key_maps.clone(), args.key_maps.clone()].concat())?;
    let watcher = fuzzy_file_finder::reload::PaletteWatcher::start(
        args.config.clone(),
        args.no_config,
        args.palette.clone(),
        fuzzy_file_finder::reload::PaletteState::from_settings(&settings),
    );
    let invocation = std::env::current_dir()?.canonicalize()?;
    let (browse_from, dialog) = starting_place(&mode, &invocation)?;
    let roots = pinned_roots(&mode)?;
    let state = fuzzy_file_finder::state::path()
        .map(|p| fuzzy_file_finder::state::load(&p))
        .unwrap_or_default();
    // fff roam sessions are saved for --resume; the other commands never are.
    let roaming = !mode.output_paths && !mode.no_dir;
    let session = mode
        .resume
        .then(fuzzy_file_finder::state::session_path)
        .flatten()
        .and_then(|p| fuzzy_file_finder::state::load_session(&p));
    // Browsing shows a folder's own entries; picking looks a little deeper,
    // and seeking deeper still.
    let depth = args.depth.unwrap_or(if mode.no_dir {
        6
    } else if mode.output_paths && mode.explorer.is_none() {
        3
    } else {
        1
    }) as usize;
    let mut app = if let Some((start, _)) = &browse_from {
        App::browse(start.clone(), depth, policy)
    } else if roots.is_empty() {
        App::browse(invocation.clone(), depth, policy)
    } else {
        App::new(roots, depth, policy)
    };
    app.records_session = roaming;
    app.openers = settings.open.clone();
    app.output_paths = mode.output_paths;
    app.dialog = dialog.clone();
    app.window = mode.window;
    app.positions = state.positions;
    // The highlight starts on the first row, unless a remembered one is listed.
    app.highlight_top = true;
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
    app.key_map = keys.clone();
    app.no_dir = mode.no_dir;
    // Choosing paths shows names; the footer has the highlighted one's path.
    app.short_labels = mode.no_dir || (mode.output_paths && dialog.is_none());
    app.status_timeout = settings.message_timeout;
    app.columns = settings.columns;
    app.mouse = settings.mouse;
    app.buttons = settings.buttons;
    app.buttons_timeout = settings.buttons_timeout;
    app.watch = settings.watch;
    app.preview.shown = settings.preview;
    app.preview.highlight = settings.highlight;
    app.config_path = settings.bookmark_path.clone();
    app.cli_key_maps = args.key_maps;
    app.no_icons = args.no_icons;
    app.trash = settings
        .trash
        .enabled
        .then(|| {
            fuzzy_file_finder::trash::Trash::from_env(settings.trash.days, settings.trash.ask_above)
        })
        .flatten();
    app.refresh_trash();
    app.home = std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    if mode.resume {
        match session {
            Some(session) => {
                app.restore(session);
                app.status = match app.sequence(fuzzy_file_finder::normal_keys::ListAction::Quit) {
                    Some(quit) => {
                        format!("Resumed the last session; {quit} returns to where you started")
                    }
                    None => "Resumed the last session".into(),
                };
                app.status_success = app.status.clone();
            }
            None => app.status = "No earlier session to resume; starting here".into(),
        }
    }
    app.config_broken = settings.broken;
    if let Some(status) = app.config_errors_status() {
        app.status = status;
    }
    app.bookmark_store = fuzzy_file_finder::bookmarks::Store::new(
        settings.bookmark_path,
        app.bookmarks.clone(),
        app.home.clone(),
    );
    let finish = ui::run(app, keys, settings.cache, watcher, settings.performance)?;
    report(finish, &mode, dialog, args.cd_file.as_deref(), &invocation)
}
/// --browse and fff explorer: the folder to start in and the file to
/// highlight there, and explorer's dialog. `None` starts in the roots.
type Start = Option<(PathBuf, Option<PathBuf>)>;
fn starting_place(
    mode: &Mode,
    invocation: &Path,
) -> Result<(Start, Option<Dialog>), Box<dyn std::error::Error>> {
    if let Some(explorer) = &mode.explorer {
        let (asked, start, highlight) = Dialog::new(
            explorer.path.as_deref(),
            explorer.multiple,
            explorer.directory,
            explorer.save,
            invocation,
        );
        let start = start.canonicalize()?;
        let highlight = highlight.and_then(|h| h.file_name().map(|n| start.join(n)));
        return Ok((Some((start, highlight)), Some(asked)));
    }
    if !mode.browse {
        return Ok((None, None));
    }
    if mode.roots.len() > 1 {
        usage("--browse", &"give at most one path to browse from");
    }
    let Some(path) = mode.roots.first() else {
        return Ok((None, None));
    };
    let full = path
        .canonicalize()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let start = match full.parent() {
        Some(parent) if !full.is_dir() => (parent.to_owned(), Some(full.clone())),
        _ => (full, None),
    };
    Ok((Some(start), None))
}
/// The ROOTS of fff roam, pick and seek, resolved, each once.
fn pinned_roots(mode: &Mode) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let mut roots = Vec::new();
    for root in mode.roots.iter().filter(|_| !mode.browse) {
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
    Ok(roots)
}
/// What the session's end prints or changes: a dialog's answer, chosen
/// paths, or (fff roam) the directory the shell changes to; gX then
/// replaces fff with its program. False when nothing was chosen.
fn report(
    finish: ui::Finish,
    mode: &Mode,
    dialog: Option<Dialog>,
    cd_file: Option<&Path>,
    invocation: &Path,
) -> Result<bool, Box<dyn std::error::Error>> {
    // In the default mode the session chooses a directory: the folder of the
    // confirmed entry (a directory itself for directories).
    let change_to = |target: Option<&PathBuf>| -> io::Result<()> {
        // A window of its own has no shell to change.
        let Some(target) = target.filter(|_| !mode.window) else {
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
        match cd_file {
            Some(file) => std::fs::write(file, dir.as_os_str().as_encoded_bytes()),
            // Without the wrapper, `cd "$(command fff)"` still works.
            None => write_paths(
                io::stdout().lock(),
                &[dir.to_owned()],
                invocation,
                false,
                false,
            ),
        }
    };
    if let Some(dialog) = dialog {
        let chosen = match finish {
            ui::Finish::Paths(paths) => paths,
            ui::Finish::Folder(folder) => vec![folder],
            ui::Finish::Cancelled | ui::Finish::Exec(_) => vec![],
        };
        let answer = dialog.answer(chosen);
        let mut out = io::stdout().lock();
        for path in &answer {
            out.write_all(path.as_os_str().as_encoded_bytes())?;
            out.write_all(b"\n")?;
        }
        return Ok(!answer.is_empty());
    }
    match finish {
        ui::Finish::Cancelled => Ok(false),
        ui::Finish::Paths(paths) if mode.output_paths => {
            write_paths(
                io::stdout().lock(),
                &paths,
                invocation,
                mode.relative,
                mode.null,
            )?;
            Ok(true)
        }
        ui::Finish::Folder(folder) if mode.output_paths => {
            write_paths(
                io::stdout().lock(),
                &[folder],
                invocation,
                mode.relative,
                mode.null,
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
            if !mode.output_paths {
                change_to(Some(&plan.cwd))?;
            }
            let error = plan.exec();
            // It never started, so the shell stays where it was.
            if let Some(file) = cd_file.filter(|_| !mode.output_paths) {
                let _ = std::fs::write(file, b"");
            }
            Err(format!("Could not run {}: {error}", plan.name).into())
        }
    }
}

/// fff setup: one integration, printed script or check, then exit.
fn run_setup(setup: SetupArgs, args: Args) -> Result<bool, Box<dyn std::error::Error>> {
    if let Some(shell) = setup.init {
        print!("{}", completion::wrapper(shell));
        return Ok(true);
    }
    if let Some(shell) = &setup.completions {
        let program = std::env::args_os().next().unwrap_or_else(|| "fff".into());
        completion::write_registration(shell, &program, &mut io::stdout().lock())?;
        return Ok(true);
    }
    let settings = Settings::load(
        args.config.as_deref(),
        args.no_config,
        args.palette.as_deref(),
    )?;
    if setup.check_config {
        KeyMap::parse(&[settings.key_maps.clone(), args.key_maps].concat())?;
        // Openers name programs; report the ones that are not installed.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let missing = settings
            .open
            .missing(&std::env::current_dir()?, home.as_deref());
        if !missing.is_empty() {
            return Err(format!("[open] programs not found: {}", missing.join("; ")).into());
        }
        match &settings.source {
            Some(path) => eprintln!("fff: configuration valid: {}", path.display()),
            None => eprintln!("fff: no configuration file; using built-in settings"),
        }
        return Ok(true);
    }
    let target = if setup.desktop {
        fuzzy_file_finder::setup::Target::Desktop
    } else {
        fuzzy_file_finder::setup::Target::FileChooser
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let terminal = fuzzy_file_finder::run::split(&settings.open.fff_terminal, home.as_deref())
        .map_err(|e| format!("[open] fff_terminal: {e}"))?;
    let dirs = fuzzy_file_finder::setup::Dirs::from_env()?;
    let fff = std::env::current_exe()?;
    fuzzy_file_finder::setup::run(target, &dirs, &fff, &terminal, &mut io::stderr())?;
    Ok(true)
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
        let commands: Vec<_> = command.get_subcommands().collect();
        for arg in command
            .get_arguments()
            .chain(commands.iter().flat_map(|c| c.get_arguments()))
        {
            let id = arg.get_id().as_str();
            // Hidden options only point to their new place.
            if !arg.get_action().takes_values()
                || ["help", "version"].contains(&id)
                || arg.is_hide_set()
            {
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
        let shells = candidates(&["fff", "setup", "--completions", ""], None);
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
        assert_eq!(
            candidates(&["fff", "explorer", "--no-config", "do"], Some(root)),
            ["docs/"]
        );
        assert_eq!(candidates(&["fff", "seek", "do"], Some(root)), ["docs/"]);
        assert_eq!(candidates(&["fff", "pick", "do"], Some(root)), ["docs/"]);
        assert_eq!(candidates(&["fff", "roam", "do"], Some(root)), ["docs/"]);
        assert!(candidates(&["fff", "setup", "--in"], None).contains(&"--init".to_string()));
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
