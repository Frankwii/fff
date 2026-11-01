use clap::Parser;
use picker::{App, Policy, config::Settings, glob, keys::KeyMap, ui, write_paths};
use std::{io, path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(
    version,
    about = "Interactive fuzzy file picker. UI uses /dev/tty; selected paths use stdout."
)]
struct Args {
    /// Experimental text editor backend (also configurable in TOML)
    #[arg(long, value_enum)]
    editor: Option<picker::config::EditorBackend>,
    /// Read this TOML configuration instead of the default config file
    #[arg(long, conflicts_with = "no_config")]
    config: Option<PathBuf>,
    /// Validate configuration and exit without starting the interface
    #[arg(long)]
    check_config: bool,
    /// Ignore the default configuration file
    #[arg(long)]
    no_config: bool,
    /// Override the configured color palette
    #[arg(long)]
    palette: Option<String>,
    /// Hide Nerd Font filetype icons (overrides [ui] icons = true)
    #[arg(long)]
    no_icons: bool,
    /// Remap a chord to a default chord, e.g. ctrl+n=alt+j; TARGET=none disables it
    #[arg(long = "map-key")]
    key_maps: Vec<String>,
    /// Optional fixed search roots; with none, browse from the invocation directory
    roots: Vec<PathBuf>,
    /// Maximum discovery depth (immediate children = 1)
    #[arg(short, long, default_value_t = 3, value_parser = clap::value_parser!(u32).range(1..))]
    depth: u32,
    /// Only show files whose basename matches this glob
    #[arg(short, long)]
    glob: Option<String>,
    /// Exclude a basename or root-relative glob; repeat for multiple rules
    #[arg(long = "exclude")]
    exclusions: Vec<String>,
    /// Show hidden entries
    #[arg(long)]
    hidden: bool,
    /// Initially disable exclusion enforcement
    #[arg(long)]
    no_exclusions: bool,
    /// Return paths relative to the invocation directory
    #[arg(long)]
    relative: bool,
    /// Terminate each output path with NUL instead of newline
    #[arg(short = '0', long)]
    null: bool,
}
fn main() -> ExitCode {
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
        settings.palette,
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
    app.palette = settings.palette;
    app.bookmarks = settings.bookmarks;
    app.icons = settings.icons && !args.no_icons;
    if let Some(paths) = ui::run(
        app,
        keys,
        settings.cache,
        watcher,
        args.editor.unwrap_or(settings.editor.backend),
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
