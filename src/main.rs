use clap::{CommandFactory, Parser, builder::styling};
use clap_complete::{ArgValueCompleter, CompleteEnv};
use fuzzy_file_finder::{
    App, Policy, completion, config::Settings, explorer::Dialog, keymap, ui, write_paths,
};
use std::{
    io::{self, IsTerminal, Write},
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
  <cyan>fff setup --desktop; fff setup --browser</cyan>
      Open folders from other programs, and Firefox's file dialogs, in fff.
  <cyan>fff explorer save ~/Downloads/paper.pdf</cyan>
      Choose where to save a file, as a file dialog would (see fff explorer --help).

<bold><green>Keys:</green></bold> (press <cyan>F1</cyan> in fff to search and rebind them, <cyan>F2</cyan> for session details)
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
  ([ui] mouse, buttons) and live updates ([ui] watch). Keys live in keys.toml
  beside it, which the Keys screen (<cyan>F1</cyan>) edits.
  <cyan>fff setup --check-config</cyan> validates both and prints which file was read.

<bold><green>Exit status:</green></bold>
  <cyan>0</cyan>    normal exit: a directory was chosen or paths printed (or the
       configuration is valid); after gX, the program's own status
  <cyan>1</cyan>    startup, configuration (including an unknown --palette) or I/O error
  <cyan>2</cyan>    invalid command-line usage: unknown options or malformed values
       (--depth, --glob, --exclude, ...)
  <cyan>130</cyan>  cancelled with Ctrl+c; nothing printed, no directory change

<bold><green>Shell completions:</green></bold>
  bash  <cyan>source <<(fff setup --completions bash)</cyan>   in ~/.bashrc
  zsh   <cyan>source <<(fff setup --completions zsh)</cyan>    in ~/.zshrc, or once:
        <cyan>fff setup --completions zsh > DIR/_fff</cyan>   for a DIR in $fpath before compinit
  fish  <cyan>fff setup --completions fish | source</cyan>    in ~/.config/fish/config.fish
  Completions cover options, root and config paths, and palettes
  (including custom ones from the configuration)."
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
    /// A matching directory prunes its whole subtree; a glob starting with / or
    /// ~/ matches absolute paths. The rules follow the configuration's
    /// [exclusions] rules. Edit them during the session with Alt+e and toggle
    /// enforcement with Alt+H.
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
    /// the file highlighted. Without a ROOT it is plain browsing.
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
}
impl RoamArgs {
    fn is_empty(&self) -> bool {
        self.roots.is_empty() && !self.browse && !self.resume
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
    /// Browse in a terminal window of its own, with no shell to change directory
    ///
    /// Starts at PATH: in a folder, or in a file's folder with the file
    /// highlighted. Normal-mode Enter on a folder enters it instead of
    /// leaving, ! replaces fff with $SHELL in the folder being browsed, and
    /// nothing is printed. The desktop entry installed by fff setup --desktop
    /// starts fff this way.
    Window(WindowArgs),
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
    /// Insert mode, ready to type. Rows show their path below the folder
    /// searched; gd switches to bare names. Enter leaves into the chosen
    /// file's folder. q and ! leave without choosing anything. To print files, use fff pick:
    /// with --glob it too lists only matching files.
    Seek(SeekArgs),
    /// Answer a file dialog: open, open-multiple, folder or save
    ///
    /// Prints the chosen paths, one per line, for the file chooser that
    /// fff setup --browser installs. Enter on a file chooses it and on a
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
struct WindowArgs {
    /// Where to start: a folder, or a file to highlight in its folder;
    /// without it, the current directory
    #[arg(value_hint = clap::ValueHint::AnyPath)]
    path: Option<PathBuf>,
    /// Continue the last fff roam or fff window session where it ended
    #[arg(long, conflicts_with = "path")]
    resume: bool,
}
#[derive(clap::Args)]
struct ExplorerArgs {
    #[command(subcommand)]
    dialog: DialogCommand,
}
/// The kinds of file dialog, named as the Keys screen names their programs.
#[derive(clap::Subcommand)]
enum DialogCommand {
    /// Open one file
    Open(DialogPath),
    /// Open one or more files (Space marks them, Ctrl+w chooses the marks)
    OpenMultiple(DialogPath),
    /// Choose a folder: ! or Ctrl+s chooses the folder you are in
    Folder(FolderArgs),
    /// Choose where to save: ! or Ctrl+s saves here under the typed name (the
    /// query) or the suggested one; replacing a file asks first
    Save(SavePath),
}
#[derive(clap::Args)]
struct DialogPath {
    /// Where to start: a folder, or a file to highlight in its folder
    #[arg(value_hint = clap::ValueHint::AnyPath)]
    path: Option<PathBuf>,
}
#[derive(clap::Args)]
struct FolderArgs {
    /// Where to start: a folder, or a file to highlight in its folder
    #[arg(value_hint = clap::ValueHint::AnyPath)]
    path: Option<PathBuf>,
    /// Allow several folders (Space marks them, Ctrl+w chooses the marks)
    #[arg(long)]
    multiple: bool,
}
#[derive(clap::Args)]
struct SavePath {
    /// The suggested file, which need not exist: where to start and the name
    /// to save under; without it, the current directory and `untitled`
    #[arg(value_hint = clap::ValueHint::AnyPath)]
    path: Option<PathBuf>,
}
/// What a file dialog asks, from `fff explorer`'s command line.
struct Asked {
    path: Option<PathBuf>,
    multiple: bool,
    directory: bool,
    save: bool,
}
impl From<DialogCommand> for Asked {
    fn from(dialog: DialogCommand) -> Self {
        let asked = |path, multiple, directory, save| Self {
            path,
            multiple,
            directory,
            save,
        };
        match dialog {
            DialogCommand::Open(d) => asked(d.path, false, false, false),
            DialogCommand::OpenMultiple(d) => asked(d.path, true, false, false),
            DialogCommand::Folder(d) => asked(d.path, d.multiple, true, false),
            DialogCommand::Save(d) => asked(d.path, false, false, true),
        }
    }
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
    #[arg(long)]
    browser: bool,
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
    /// --completions bash)`. The script only calls fff back, so it can also
    /// be saved once: zsh's as `_fff` in a directory that is in $fpath
    /// when compinit runs, fish's as
    /// ~/.config/fish/completions/fff.fish. `COMPLETE=SHELL fff` prints the
    /// same script.
    #[arg(long, value_name = "SHELL",
        value_parser = clap::builder::PossibleValuesParser::new(completion::shells()))]
    completions: Option<String>,
    /// Write the manual pages: fff.1, and fff-COMMAND.1 for each command
    ///
    /// Without DIR, into the man1 folder of a directory man searches (as
    /// `manpath` lists them) that is in your home, else one you can write
    /// to: ~/.local/share/man when ~/.local/bin is on PATH. With DIR, into
    /// DIR itself. Then `man fff`, `man fff-pick`. Run it again after
    /// upgrading; the pages there are replaced.
    #[arg(long, value_name = "DIR", num_args = 0..=1,
        value_hint = clap::ValueHint::DirPath)]
    man: Option<Option<PathBuf>>,
    /// Validate the configuration (and --palette) and report the result
    ///
    /// Also warns about the desktop integration, if set up: files fff setup
    /// wrote that are missing, out of date or run another fff binary, folders
    /// that open in another application, and an environment in which Qt
    /// applications draw their own file dialog.
    #[arg(long)]
    check_config: bool,
}
fn main() -> ExitCode {
    // Answers `COMPLETE=<shell> fff ...` requests before anything is printed.
    CompleteEnv::with_factory(Args::command)
        .var(completion::ENV_VAR)
        .shells(completion::SHELLS)
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
struct Mode {
    /// The program it starts as; a file dialog's is set once its dialog is
    /// known, and the session may end as another (`switch_program`).
    program: fuzzy_file_finder::program::Program,
    roots: Vec<PathBuf>,
    browse: bool,
    resume: bool,
    relative: bool,
    null: bool,
    explorer: Option<Asked>,
}
impl Default for Mode {
    fn default() -> Self {
        Self {
            program: fuzzy_file_finder::program::Program::Roam,
            roots: Vec::new(),
            browse: false,
            resume: false,
            relative: false,
            null: false,
            explorer: None,
        }
    }
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
    // fff roam's options before another command would be ignored.
    let roam = std::mem::take(&mut args.roam);
    if !matches!(args.command, None | Some(Command::Roam(_))) && !roam.is_empty() {
        conflict("roots, --browse and --resume belong to fff roam");
    }
    let mut mode = match args.command.take() {
        None => Mode {
            roots: roam.roots,
            browse: roam.browse,
            resume: roam.resume,
            ..Mode::default()
        },
        Some(Command::Roam(mut more)) => {
            let mut roots = roam.roots;
            roots.append(&mut more.roots);
            Mode {
                roots,
                browse: roam.browse || more.browse,
                resume: roam.resume || more.resume,
                ..Mode::default()
            }
        }
        Some(Command::Window(window)) => Mode {
            roots: window.path.into_iter().collect(),
            browse: true,
            resume: window.resume,
            program: fuzzy_file_finder::program::Program::Window,
            ..Mode::default()
        },
        Some(Command::Pick(pick)) => Mode {
            roots: pick.roots,
            browse: pick.browse,
            program: fuzzy_file_finder::program::Program::Pick,
            relative: pick.output.relative,
            null: pick.output.null,
            ..Mode::default()
        },
        Some(Command::Seek(seek)) => Mode {
            roots: seek.roots,
            program: fuzzy_file_finder::program::Program::Seek,
            ..Mode::default()
        },
        Some(Command::Explorer(explorer)) => Mode {
            program: fuzzy_file_finder::program::Program::ExplorerOpen,
            explorer: Some(explorer.dialog.into()),
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
    let mut policy = Policy {
        hidden: args.hidden,
        ignore: !args.no_exclusions,
        ..Policy::default()
    };
    if let Err(e) = policy.set_filters(args.glob) {
        usage("--glob", &e);
    }
    // A configuration with errors left by gc gives way to its backup.
    let settings = Settings::load_or_backup(
        args.config.as_deref(),
        args.no_config,
        args.palette.as_deref(),
    )?;
    let watcher = fuzzy_file_finder::reload::PaletteWatcher::start(
        args.config.clone(),
        args.no_config,
        args.palette.clone(),
        fuzzy_file_finder::reload::PaletteState::from_settings(&settings),
    );
    // The configuration's exclusions come first, then those of --exclude.
    if let Err(e) = policy.set_rules(
        settings
            .exclusions
            .iter()
            .cloned()
            .chain(args.exclusions)
            .collect(),
    ) {
        usage("--exclude", &e);
    }
    let invocation = std::env::current_dir()?.canonicalize()?;
    let (browse_from, dialog) = starting_place(&mode, &invocation)?;
    let roots = pinned_roots(&mode)?;
    let state = fuzzy_file_finder::state::path()
        .map(|p| fuzzy_file_finder::state::load(&p))
        .unwrap_or_default();
    if let Some(dialog) = &dialog {
        mode.program = dialog.program();
    }
    let session = mode
        .resume
        .then(fuzzy_file_finder::state::session_path)
        .flatten()
        .and_then(|p| fuzzy_file_finder::state::load_session(&p));
    let depth = args
        .depth
        .map_or(mode.program.default_depth(), |depth| depth as usize);
    let mut app = if let Some((start, _)) = &browse_from {
        App::browse(start.clone(), depth, policy)
    } else if roots.is_empty() {
        App::browse(invocation.clone(), depth, policy)
    } else {
        App::new(roots, depth, policy)
    };
    app.openers = settings.open.clone();
    match &dialog {
        Some(dialog) => app.open_dialog(dialog.clone()),
        // Choosing paths shows names; the footer has the highlighted one's path.
        None => app.set_program(mode.program),
    }
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
    app.config_rules = settings.exclusions;
    app.icons = settings.icons && !args.no_icons;
    // keys.toml sits beside the configuration; one with errors gives way
    // to the defaults, and the Session tab says why.
    app.keys_path = settings.bookmark_path.as_deref().map(keymap::path_beside);
    if let Some(path) = &app.keys_path {
        match keymap::Keymap::load(path) {
            Ok(keys) => app.keymap = keys,
            Err(error) => app.keys_broken = Some(error),
        }
    }
    app.status_timeout = settings.message_timeout;
    app.columns = settings.columns;
    app.mouse = settings.mouse;
    app.buttons = settings.buttons;
    app.buttons_timeout = settings.buttons_timeout;
    app.watch = settings.watch;
    app.preview.shown = settings.preview;
    app.preview.highlight = settings.highlight;
    app.sort = settings.sort;
    app.config_path = settings.bookmark_path.clone();
    if let Some(path) = fuzzy_file_finder::sizes::path() {
        app.details.keep_sizes(path);
    }
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
                app.status = match app.bound_in(keymap::Function::Quit, keymap::Mode::Normal) {
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
    let (finish, program) = ui::run(app, settings.cache, watcher, settings.performance)?;
    // A session switched to another program leaves as that one does: fff
    // pick prints paths, roam and seek change directory.
    mode.program = program;
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
        let Some(target) =
            target.filter(|_| mode.program != fuzzy_file_finder::program::Program::Window)
        else {
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
        ui::Finish::Paths(paths) if mode.program.prints_paths() => {
            write_paths(
                io::stdout().lock(),
                &paths,
                invocation,
                mode.relative,
                mode.null,
            )?;
            Ok(true)
        }
        ui::Finish::Folder(folder) if mode.program.prints_paths() => {
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
            if !mode.program.prints_paths() {
                change_to(Some(&plan.cwd))?;
            }
            let error = plan.exec();
            // It never started, so the shell stays where it was.
            if let Some(file) = cd_file.filter(|_| !mode.program.prints_paths()) {
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
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let mut stderr = io::stderr();
    let color = stderr.is_terminal() && std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());
    let mut report = fuzzy_file_finder::setup::Report::new(&mut stderr, color, home.clone());
    if let Some(dir) = &setup.man {
        let dir = match dir {
            Some(dir) => dir.clone(),
            None => man_dir()?,
        };
        write_man_pages(&dir, &mut report)?;
        return Ok(true);
    }
    let settings = Settings::load(
        args.config.as_deref(),
        args.no_config,
        args.palette.as_deref(),
    )?;
    if setup.check_config {
        let keys = settings.bookmark_path.as_deref().map(keymap::path_beside);
        if let Some(path) = &keys {
            keymap::Keymap::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        // Openers name programs; report the ones that are not installed.
        let missing = settings
            .open
            .missing(&std::env::current_dir()?, home.as_deref());
        if !missing.is_empty() {
            return Err(format!("[open] programs not found: {}", missing.join("; ")).into());
        }
        report.heading("Configuration")?;
        match &settings.source {
            Some(path) => report.done(&format!("valid: {}", report.path(path)))?,
            None => report.same("no configuration file; using built-in settings")?,
        }
        if let Some(path) = keys.filter(|p| p.exists()) {
            report.done(&format!("valid: {}", report.path(&path)))?;
        }
        report.done("every program [open] names is installed")?;
        // The desktop integration, if set up: warnings, since the
        // configuration itself is valid.
        if let (Ok(terminal), Ok(dirs), Ok(fff)) = (
            fuzzy_file_finder::run::split(&settings.open.fff_terminal, home.as_deref()),
            fuzzy_file_finder::setup::Dirs::from_env(),
            std::env::current_exe(),
        ) {
            use fuzzy_file_finder::setup::{self, Finding};
            let var = |name: &str| std::env::var(name).ok();
            for section in setup::check(&dirs, &fff, &terminal, var, setup::folder_default) {
                report.heading(section.title)?;
                for finding in section.findings {
                    match finding {
                        Finding::Fine(text) => report.done(&text)?,
                        Finding::Problem(text, fix) => {
                            report.warn(&text)?;
                            if let Some(fix) = fix {
                                report.command(&fix)?;
                            }
                        }
                    }
                }
            }
        }
        return Ok(true);
    }
    let target = if setup.desktop {
        fuzzy_file_finder::setup::Target::Desktop
    } else {
        fuzzy_file_finder::setup::Target::FileChooser
    };
    let terminal = fuzzy_file_finder::run::split(&settings.open.fff_terminal, home.as_deref())
        .map_err(|e| format!("[open] fff_terminal: {e}"))?;
    let dirs = fuzzy_file_finder::setup::Dirs::from_env()?;
    let fff = std::env::current_exe()?;
    fuzzy_file_finder::setup::run(target, &dirs, &fff, &terminal, &mut report)?;
    Ok(true)
}

/// Where `fff setup --man` writes without a DIR: the man1 folder of a
/// directory man searches, or why there is none.
fn man_dir() -> Result<PathBuf, String> {
    let listed = std::process::Command::new("manpath")
        .arg("-q")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .or_else(|| std::env::var("MANPATH").ok())
        .unwrap_or_default();
    let search: Vec<PathBuf> = listed
        .split(':')
        .filter(|d| !d.is_empty())
        .map(PathBuf::from)
        .collect();
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let writable = |dir: &Path| {
        let man1 = dir.join("man1");
        let at = if man1.exists() { man1 } else { dir.to_owned() };
        rustix::fs::access(&at, rustix::fs::Access::WRITE_OK).is_ok()
    };
    choose_man_dir(&search, home.as_deref(), writable).ok_or_else(|| {
        let looked = if search.is_empty() {
            "fff could not tell where man looks for pages (manpath gave nothing and MANPATH \
             is unset)"
                .to_string()
        } else {
            format!(
                "none of the directories man searches ({}) is in your home or writable",
                listed
            )
        };
        format!(
            "{looked}; name the folder for the pages, e.g. \
             fff setup --man ~/.local/share/man/man1, and make man search \
             ~/.local/share/man (~/.local/bin on PATH does, or add it to MANPATH)"
        )
    })
}
/// The man1 folder of the first directory in `search` (man's search path,
/// in its order) that is in `home`, else of the first `writable` one.
fn choose_man_dir(
    search: &[PathBuf],
    home: Option<&Path>,
    writable: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let mine = search
        .iter()
        .find(|d| home.is_some_and(|h| d.starts_with(h)) && writable(d));
    mine.or_else(|| search.iter().find(|d| writable(d)))
        .map(|d| d.join("man1"))
}
/// fff setup --man: a page per command, as clap_mangen names them (fff.1,
/// fff-pick.1, ...), reported on stderr.
fn write_man_pages(dir: &Path, report: &mut fuzzy_file_finder::setup::Report) -> io::Result<()> {
    fn pages(command: &clap::Command, all: &mut Vec<clap::Command>) {
        for sub in command.get_subcommands().filter(|s| !s.is_hide_set()) {
            pages(sub, all);
        }
        all.push(command.clone());
    }
    std::fs::create_dir_all(dir)?;
    let mut command = Args::command().disable_help_subcommand(true);
    command.build();
    let mut all = Vec::new();
    pages(&command, &mut all);
    let count = all.len();
    for page in all.into_iter().rev() {
        clap_mangen::Man::new(page).generate_to(dir)?;
    }
    // Reports fail quietly: the pages are written, and stderr may be closed.
    let _ = report.heading("Manual pages");
    let _ = report.done(&format!("wrote {count} pages to {}", report.path(dir)));
    let _ = report.step("Read them with:");
    let _ = report.command("man fff; man fff-pick; man fff-setup");
    Ok(())
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
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        assert_eq!(candidates(&["fff", "do"], Some(root)), ["docs/"]);
        assert_eq!(
            candidates(
                &["fff", "explorer", "save", "--no-config", "do"],
                Some(root)
            ),
            ["docs/"]
        );
        assert_eq!(candidates(&["fff", "window", "do"], Some(root)), ["docs/"]);
        assert_eq!(
            candidates(&["fff", "explorer", "open-"], None),
            ["open-multiple"]
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
    fn man_pages_go_to_a_directory_man_searches_in_home_first() {
        let search: Vec<PathBuf> = [
            "/usr/local/man",
            "/usr/share/man",
            "/home/u/.local/share/man",
        ]
        .map(PathBuf::from)
        .to_vec();
        let all = |_: &Path| true;
        let home = Some(Path::new("/home/u"));
        assert_eq!(
            choose_man_dir(&search, home, all),
            Some("/home/u/.local/share/man/man1".into())
        );
        // As root, or with no man directory in home: the first writable one.
        let usr_local = |d: &Path| d.starts_with("/usr/local");
        assert_eq!(
            choose_man_dir(&search[..2], home, usr_local),
            Some("/usr/local/man/man1".into())
        );
        assert_eq!(choose_man_dir(&search[..2], home, |_| false), None);
        assert_eq!(choose_man_dir(&[], home, all), None);
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
        // zsh's also works saved as _fff in $fpath, completing on the first Tab.
        let mut zsh = Vec::new();
        completion::write_registration("zsh", "fff".as_ref(), &mut zsh).unwrap();
        let zsh = String::from_utf8(zsh).unwrap();
        assert!(zsh.starts_with("#compdef fff\n"), "{zsh}");
        assert!(
            zsh.contains("if [[ ${funcstack[1]} == _fff ]]; then\n    # Autoloaded")
                && zsh.contains("    compdef _clap_dynamic_completer_fff fff\nfi\n"),
            "{zsh}"
        );
    }
}
