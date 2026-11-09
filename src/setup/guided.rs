//! `fff setup` without an option: a guided start. It reports which of the
//! programs fff uses are installed ([`programs`], which `--check-config`
//! reports too), asks whether the terminal shows Nerd Font icons, which
//! palette to use, which of the installed programs open documents, images,
//! media and office files (the desktop's own choice first) and, without
//! `$EDITOR`, which editor opens the rest; shows what it would add to the
//! configuration file and adds it once confirmed. What the file sets
//! already is never asked about or changed: `[ui] icons` and `palette`
//! once set, and the files its `[[open.rules]]` open; rules for the others
//! are added after the file's own. Then it offers what is not set up yet,
//! each shown and asked: the shell function and completions (a block
//! appended to the shell's startup file), the manual pages and, on a
//! freedesktop.org desktop, opening folders and file dialogs in fff.
use super::{Finding, Report, Section};
use std::{
    fs,
    io::{self, BufRead},
    path::{Path, PathBuf},
};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

/// What the guided setup knows of the system it runs on; tests make their own.
pub struct System {
    /// macOS: files open with `open` and programs install with Homebrew.
    pub macos: bool,
    /// Whether a program is on PATH.
    pub installed: Box<dyn Fn(&str) -> bool>,
    /// `$EDITOR`, when set.
    pub editor: Option<String>,
    /// `[open] terminal`, configured or detected.
    pub terminal: Option<String>,
    /// Whether a Nerd Font is installed.
    pub nerd_font: bool,
    /// `$SHELL`'s name: `zsh`, `bash`, `fish`.
    pub shell: Option<String>,
    /// The startup file of `shell`, when it is bash, zsh or fish.
    pub startup: Option<PathBuf>,
    /// Setup runs through the shell function, which passes `--cd-file`.
    pub shell_function: bool,
    /// fff's completions for `shell` are installed where it finds them
    /// without a startup line (the Arch package's, or saved by hand).
    pub completions: bool,
    /// The program the desktop opens a MIME type with, by its desktop
    /// entry's `Exec` line (Linux; None on macOS, where `open` is).
    pub default_app: DefaultApp,
}
/// The program the desktop opens a MIME type with.
pub type DefaultApp = Box<dyn Fn(&str) -> Option<String>>;
impl System {
    /// This system, with `terminal` from `[open]`.
    pub fn from_env(terminal: Option<String>) -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let home = var("HOME").map(PathBuf::from);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let shell =
            var("SHELL").and_then(|s| Some(Path::new(&s).file_name()?.to_str()?.to_owned()));
        let absolute = |name: &str| var(name).map(PathBuf::from).filter(|p| p.is_absolute());
        let config_home =
            absolute("XDG_CONFIG_HOME").or_else(|| Some(home.as_ref()?.join(".config")));
        let data_home =
            absolute("XDG_DATA_HOME").or_else(|| Some(home.as_ref()?.join(".local/share")));
        let startup = startup_path(
            shell.as_deref(),
            home.as_deref(),
            absolute("ZDOTDIR").as_deref(),
            config_home.as_deref(),
        );
        let data_dirs: Vec<PathBuf> = data_home
            .into_iter()
            .chain(
                var("XDG_DATA_DIRS")
                    .unwrap_or_else(|| "/usr/local/share:/usr/share".into())
                    .split(':')
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute()),
            )
            .collect();
        let mut completion_dirs = data_dirs.clone();
        completion_dirs.extend(config_home);
        let completions = completions_installed(shell.as_deref(), &completion_dirs);
        let macos = crate::platform::MACOS;
        Self {
            macos,
            installed: Box::new(move |name| crate::run::resolve(name, &cwd).is_ok()),
            editor: var("EDITOR"),
            terminal,
            nerd_font: nerd_font_installed(&crate::platform::font_dirs(home.as_deref())),
            shell,
            startup,
            shell_function: false,
            completions,
            default_app: Box::new(move |mime| {
                (!macos)
                    .then(|| desktop_default(mime, &data_dirs))
                    .flatten()
            }),
        }
    }
    fn has(&self, program: &str) -> bool {
        (self.installed)(program)
    }
}

/// The startup file of `shell`: `$ZDOTDIR/.zshrc` (else `~/.zshrc`),
/// `~/.bashrc`, or fish's `config.fish` in `config_home`.
pub fn startup_path(
    shell: Option<&str>,
    home: Option<&Path>,
    zdotdir: Option<&Path>,
    config_home: Option<&Path>,
) -> Option<PathBuf> {
    match shell? {
        "zsh" => Some(zdotdir.or(home)?.join(".zshrc")),
        "bash" => Some(home?.join(".bashrc")),
        "fish" => Some(config_home?.join("fish/config.fish")),
        _ => None,
    }
}
/// Whether fff's completions for `shell` are in a folder of `dirs` (the
/// XDG data and configuration folders) where the shell finds them itself.
fn completions_installed(shell: Option<&str>, dirs: &[PathBuf]) -> bool {
    let places: &[&str] = match shell {
        Some("zsh") => &["zsh/site-functions/_fff"],
        Some("bash") => &["bash-completion/completions/fff"],
        Some("fish") => &[
            "fish/vendor_completions.d/fff.fish",
            "fish/completions/fff.fish",
        ],
        _ => &[],
    };
    dirs.iter()
        .any(|dir| places.iter().any(|place| dir.join(place).is_file()))
}
/// The program the desktop opens `mime` with: `xdg-mime`'s default
/// application, by the first word of its entry's `Exec` line (after `env`
/// and its variables), looked up in the `applications` folder of `dirs`.
fn desktop_default(mime: &str, dirs: &[PathBuf]) -> Option<String> {
    let output = std::process::Command::new("xdg-mime")
        .args(["query", "default", mime])
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    let id = String::from_utf8(output.stdout).ok()?;
    let id = id.trim();
    if id.is_empty() || id.contains('/') {
        return None;
    }
    let text = dirs
        .iter()
        .find_map(|dir| fs::read_to_string(dir.join("applications").join(id)).ok())?;
    exec_program(&text)
}
/// The program a desktop entry's `[Desktop Entry]` `Exec` line runs.
fn exec_program(entry: &str) -> Option<String> {
    let mut section = "";
    let line = entry.lines().map(str::trim).find(|line| {
        if line.starts_with('[') {
            section = line;
        }
        section == "[Desktop Entry]" && line.starts_with("Exec=")
    })?;
    // Words, a quoted one (a path with spaces) kept whole.
    let mut rest = line["Exec=".len()..].trim_start();
    let mut word = || -> Option<&str> {
        let (word, after) = match rest.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"')?,
            None => rest.split_once(char::is_whitespace).unwrap_or((rest, "")),
        };
        rest = after.trim_start();
        (!word.is_empty()).then_some(word)
    };
    let mut first = word()?;
    if Path::new(first).file_name()? == "env" {
        first = std::iter::from_fn(&mut word).find(|w| !w.contains('='))?;
    }
    Some(Path::new(first).file_name()?.to_str()?.to_owned())
}

/// Whether a font file below `dirs` (a few folders deep) is a Nerd Font,
/// by its name: `SymbolsNerdFont-Regular.ttf`, `Hack Nerd Font Complete.ttf`.
pub fn nerd_font_installed(dirs: &[PathBuf]) -> bool {
    fn below(dir: &Path, depth: usize, budget: &mut usize) -> bool {
        let Ok(entries) = fs::read_dir(dir) else {
            return false;
        };
        for entry in entries.flatten() {
            if *budget == 0 {
                return false;
            }
            *budget -= 1;
            let name = entry.file_name().to_string_lossy().to_lowercase();
            let squeezed: String = name.chars().filter(|c| c.is_alphanumeric()).collect();
            if squeezed.contains("nerdfont") {
                return true;
            }
            if depth > 0
                && entry.file_type().is_ok_and(|t| t.is_dir())
                && below(&entry.path(), depth - 1, budget)
            {
                return true;
            }
        }
        false
    }
    let mut budget = 50_000;
    dirs.iter().any(|dir| below(dir, 3, &mut budget))
}

/// How to install a package here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Manager {
    Pacman,
    Apt,
    Dnf,
    Brew,
}
impl Manager {
    fn find(system: &System) -> Option<Self> {
        if system.macos {
            return system.has("brew").then_some(Self::Brew);
        }
        [
            (Self::Pacman, "pacman"),
            (Self::Apt, "apt"),
            (Self::Dnf, "dnf"),
        ]
        .into_iter()
        .find(|(_, program)| system.has(program))
        .map(|(manager, _)| manager)
    }
    /// The command installing `package`, if this manager has it.
    fn install(self, package: &Package) -> Option<String> {
        let (command, name) = match self {
            Self::Pacman => ("sudo pacman -S", package.pacman),
            Self::Apt => ("sudo apt install", package.apt),
            Self::Dnf => ("sudo dnf install", package.dnf),
            Self::Brew => ("brew install", package.brew),
        };
        (!name.is_empty()).then(|| format!("{command} {name}"))
    }
}
/// A package's name for each manager; empty where it has none.
struct Package {
    pacman: &'static str,
    apt: &'static str,
    dnf: &'static str,
    brew: &'static str,
}
const fn named(name: &'static str) -> Package {
    Package {
        pacman: name,
        apt: name,
        dnf: name,
        brew: name,
    }
}

/// A program fff runs, and what for.
struct Tool {
    name: &'static str,
    programs: &'static [&'static str],
    gives: &'static str,
    package: Package,
    /// fff does not start without it.
    required: bool,
}
const TOOLS: &[Tool] = &[
    Tool {
        name: "Neovim",
        programs: &["nvim"],
        gives: "edits every text field and colors previews",
        package: named("neovim"),
        required: true,
    },
    Tool {
        name: "poppler",
        programs: &["pdftotext", "pdfinfo"],
        gives: "PDF previews and contents search",
        package: Package {
            pacman: "poppler",
            apt: "poppler-utils",
            dnf: "poppler-utils",
            brew: "poppler",
        },
        required: false,
    },
    Tool {
        name: "DjVuLibre",
        programs: &["djvutxt"],
        gives: "contents search in DjVu files",
        package: Package {
            pacman: "djvulibre",
            apt: "djvulibre-bin",
            dnf: "djvulibre",
            brew: "djvulibre",
        },
        required: false,
    },
    Tool {
        name: "FFmpeg",
        programs: &["ffprobe"],
        gives: "audio and video details in the preview",
        package: named("ffmpeg"),
        required: false,
    },
    Tool {
        name: "zoxide",
        programs: &["zoxide"],
        gives: "your frequent folders in the location prompt",
        package: named("zoxide"),
        required: false,
    },
];
/// A Nerd Font with only the symbols, which terminals use as a fallback.
const NERD_FONT: Package = Package {
    pacman: "ttf-nerd-fonts-symbols",
    apt: "",
    dnf: "",
    brew: "--cask font-symbols-only-nerd-font",
};
const NERD_FONTS_URL: &str = "https://www.nerdfonts.com/font-downloads";
/// Where every setting is described.
const SETTINGS_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/blob/main/docs/CONFIGURATION.md"
);

/// The programs fff uses, whether each is installed (with how to install
/// it), the terminal terminal programs open in and `$EDITOR`, or without it
/// `fallback`, the command of an `[[open.rules]]` entry every file matches.
pub fn programs(system: &System, fallback: Option<&str>) -> Section {
    let manager = Manager::find(system);
    let mut findings: Vec<Finding> = TOOLS
        .iter()
        .map(|tool| {
            let names = tool.programs.join(", ");
            if tool.programs.iter().all(|p| system.has(p)) {
                return Finding::Fine(format!("{} ({names}): {}", tool.name, tool.gives));
            }
            let fix = manager.and_then(|m| m.install(&tool.package));
            if tool.required {
                Finding::Problem(
                    format!(
                        "{} ({names}) is not installed: fff needs it to start",
                        tool.name
                    ),
                    fix,
                )
            } else {
                Finding::Optional(
                    format!(
                        "{} ({names}) is not installed, for {}",
                        tool.name, tool.gives
                    ),
                    fix,
                )
            }
        })
        .collect();
    findings.push(match &system.terminal {
        Some(terminal) => Finding::Fine(format!("terminal programs open in `{terminal}`")),
        None => Finding::Problem(
            "no terminal found to open terminal programs in; set [open] terminal, \
             e.g. \"xterm -e\""
                .into(),
            None,
        ),
    });
    let export = match system.shell.as_deref() {
        Some("fish") => "set -Ux EDITOR nvim".into(),
        shell => format!("export EDITOR=nvim   # in {}", startup_file(shell)),
    };
    findings.push(match (&system.editor, fallback) {
        (Some(editor), _) => Finding::Fine(format!(
            "$EDITOR ({editor}) opens files no opener rule matches"
        )),
        (None, Some(command)) => Finding::Optional(
            format!(
                "$EDITOR is unset: `{command}` opens the files no other opener rule matches, \
                 but editing a file or the configuration from fff needs $EDITOR"
            ),
            Some(export),
        ),
        (None, None) => Finding::Problem(
            "$EDITOR is unset: files no opener rule matches do not open".into(),
            Some(export),
        ),
    });
    Section {
        title: "Programs",
        findings,
    }
}
/// The startup file of a shell, for the lines setup suggests.
pub fn startup_file(shell: Option<&str>) -> &'static str {
    match shell {
        Some("zsh") => "~/.zshrc",
        Some("bash") => "~/.bashrc",
        Some("fish") => "~/.config/fish/config.fish",
        _ => "your shell's startup file",
    }
}

/// The systems a viewer is suggested on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Systems {
    All,
    MacOs,
    NotMacOs,
}
/// A program that opens a kind of file.
struct Viewer {
    command: &'static str,
    note: &'static str,
    at_grep_match: &'static str,
    /// The extensions of the kind it opens; empty: all of them.
    only: &'static [&'static str],
    systems: Systems,
}
const fn viewer(command: &'static str, systems: Systems) -> Viewer {
    Viewer {
        command,
        note: "",
        at_grep_match: "",
        only: &[],
        systems,
    }
}
const OPEN: Viewer = Viewer {
    note: "the application macOS opens them with",
    ..viewer("open", Systems::MacOs)
};
const XDG_OPEN: Viewer = Viewer {
    note: "asks your desktop each time, so it follows the desktop's choice",
    ..viewer("xdg-open", Systems::NotMacOs)
};
const AT_PAGE: &str = "opens at a contents match's page";
/// A kind of file and the programs that open it, in order of preference.
struct Kind {
    title: &'static str,
    /// The MIME type whose default application the desktop's choice is.
    mime: &'static str,
    extensions: &'static [&'static str],
    viewers: &'static [Viewer],
}
const KINDS: &[Kind] = &[
    Kind {
        title: "Documents",
        mime: "application/pdf",
        extensions: &["pdf", "djvu", "epub"],
        viewers: &[
            OPEN,
            Viewer {
                note: AT_PAGE,
                at_grep_match: "--page {page} --find {query}",
                ..viewer("zathura", Systems::All)
            },
            Viewer {
                note: AT_PAGE,
                at_grep_match: "--page {page} --find {query}",
                ..viewer("okular", Systems::NotMacOs)
            },
            Viewer {
                only: &["pdf", "djvu"],
                ..viewer("evince", Systems::NotMacOs)
            },
            Viewer {
                note: AT_PAGE,
                at_grep_match: "{path} {page}",
                only: &["pdf", "epub"],
                ..viewer("mupdf", Systems::NotMacOs)
            },
            XDG_OPEN,
        ],
    },
    Kind {
        title: "Images",
        mime: "image/png",
        extensions: &["png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff"],
        viewers: &[
            OPEN,
            viewer("imv", Systems::NotMacOs),
            viewer("nsxiv", Systems::NotMacOs),
            viewer("sxiv", Systems::NotMacOs),
            viewer("feh", Systems::NotMacOs),
            viewer("eog", Systems::NotMacOs),
            viewer("gwenview", Systems::NotMacOs),
            viewer("ristretto", Systems::NotMacOs),
            XDG_OPEN,
        ],
    },
    Kind {
        title: "Audio and video",
        mime: "video/mp4",
        extensions: &[
            "mp4", "mkv", "webm", "avi", "mov", "m4v", "mp3", "flac", "ogg", "opus", "m4a", "wav",
        ],
        viewers: &[
            OPEN,
            // A window even for audio, so that it can be stopped.
            viewer("mpv --force-window", Systems::All),
            viewer("vlc", Systems::NotMacOs),
            XDG_OPEN,
        ],
    },
    Kind {
        title: "Office documents",
        mime: "application/vnd.oasis.opendocument.text",
        extensions: &[
            "odt", "ods", "odp", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "rtf",
        ],
        viewers: &[OPEN, viewer("libreoffice", Systems::NotMacOs), XDG_OPEN],
    },
];
impl Viewer {
    fn program(&self) -> &str {
        self.command.split(' ').next().unwrap_or(self.command)
    }
    fn offered(&self, system: &System) -> bool {
        let here = match self.systems {
            Systems::All => true,
            Systems::MacOs => system.macos,
            Systems::NotMacOs => !system.macos,
        };
        here && system.has(self.program())
    }
}

/// What setup adds to the configuration file.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    /// `[ui] icons`.
    pub icons: Option<bool>,
    /// `[ui] palette`.
    pub palette: Option<&'static str>,
    pub rules: Vec<PlannedRule>,
    /// The editor of a last rule, for every file, when `$EDITOR` is unset.
    pub fallback: Option<&'static str>,
}
/// An `[[open.rules]]` entry, with the kinds of file it is for.
#[derive(Debug, PartialEq)]
pub struct PlannedRule {
    pub kinds: Vec<&'static str>,
    pub command: String,
    pub extensions: Vec<String>,
    pub at_grep_match: String,
}
impl Plan {
    fn is_empty(&self) -> bool {
        self.icons.is_none()
            && self.palette.is_none()
            && self.rules.is_empty()
            && self.fallback.is_none()
    }
    /// `viewer` for `extensions`, of `kind`: one rule per program, so that
    /// kinds opening with the same one share it.
    fn open_with(&mut self, kind: &Kind, extensions: &[&str], viewer: &Viewer) {
        let extensions = extensions
            .iter()
            .filter(|e| viewer.only.is_empty() || viewer.only.contains(e))
            .map(|e| e.to_string());
        match self
            .rules
            .iter_mut()
            .find(|r| r.command == viewer.command && r.at_grep_match == viewer.at_grep_match)
        {
            Some(rule) => {
                rule.kinds.push(kind.title);
                rule.extensions.extend(extensions);
            }
            None => self.rules.push(PlannedRule {
                kinds: vec![kind.title],
                command: viewer.command.into(),
                extensions: extensions.collect(),
                at_grep_match: viewer.at_grep_match.into(),
            }),
        }
    }
}

/// What a new configuration file starts with.
fn header() -> String {
    format!(
        "# fff's configuration, started by fff setup. Every setting is described in\n\
         # {SETTINGS_URL}\n"
    )
}
/// `text` (a configuration file; None when there is none) with `plan`
/// added, everything else kept as it was.
pub fn apply(text: Option<&str>, plan: &Plan) -> Result<String, String> {
    let mut document: DocumentMut = text
        .unwrap_or_default()
        .parse()
        .map_err(|e| format!("{e}"))?;
    let table = |document: &mut DocumentMut, name: &str| -> Result<(), String> {
        match document.get(name) {
            None => {
                let mut table = Table::new();
                table.set_implicit(true);
                document.insert(name, Item::Table(table));
                Ok(())
            }
            Some(item) if item.is_table() => Ok(()),
            Some(_) => Err(format!(
                "[{name}] is not a table in the configuration file; add the settings by hand"
            )),
        }
    };
    if let Some(icons) = plan.icons {
        table(&mut document, "ui")?;
        document["ui"]["icons"] = value(icons);
    }
    if let Some(palette) = plan.palette {
        table(&mut document, "ui")?;
        document["ui"]["palette"] = value(palette);
    }
    if !plan.rules.is_empty() || plan.fallback.is_some() {
        table(&mut document, "open")?;
        // After the file's own rules, which match first.
        let mut rules = match document["open"].get_mut("rules").map(std::mem::take) {
            None | Some(Item::None) => ArrayOfTables::new(),
            Some(Item::ArrayOfTables(rules)) => rules,
            Some(_) => {
                return Err("open.rules is not a list of [[open.rules]] tables in the \
                            configuration file; add the rules by hand"
                    .into());
            }
        };
        for rule in &plan.rules {
            let mut table = Table::new();
            table
                .decor_mut()
                .set_prefix(format!("\n# {}\n", rule.kinds.join(", ")));
            table["command"] = value(&rule.command);
            table["extensions"] = value(
                rule.extensions
                    .iter()
                    .map(String::as_str)
                    .collect::<Array>(),
            );
            if !rule.at_grep_match.is_empty() {
                table["at_grep_match"] = value(&rule.at_grep_match);
            }
            rules.push(table);
        }
        // Last, as rules match in order.
        if let Some(editor) = plan.fallback {
            let mut table = Table::new();
            table
                .decor_mut()
                .set_prefix("\n# Everything else, as $EDITOR would (it is unset)\n");
            table["command"] = value(editor);
            table["patterns"] = value(Array::from_iter(["*"]));
            table["terminal"] = value(true);
            rules.push(table);
        }
        document["open"]["rules"] = Item::ArrayOfTables(rules);
    }
    let added = document.to_string();
    Ok(match text {
        Some(_) => added,
        None => format!("{}\n{}", header(), added.trim_start()),
    })
}

/// Reads an answer, asking again until `parse` takes it. The end of input
/// stops setup, leaving what was confirmed before.
fn ask<T>(
    report: &mut Report,
    input: &mut dyn BufRead,
    question: &str,
    parse: impl Fn(&str) -> Option<T>,
) -> io::Result<T> {
    loop {
        report.ask(question)?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            report.line("")?;
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "setup stopped; only what you confirmed was written",
            ));
        }
        if let Some(answer) = parse(line.trim()) {
            return Ok(answer);
        }
    }
}
/// Asks `question`, `yes` the answer when none is typed.
fn yes_or_no(
    report: &mut Report,
    input: &mut dyn BufRead,
    question: &str,
    yes: bool,
) -> io::Result<bool> {
    let choices = if yes { "[Y/n]" } else { "[y/N]" };
    ask(
        report,
        input,
        &format!("{question} {choices}"),
        |answer| match answer.to_lowercase().as_str() {
            "" => Some(yes),
            "y" | "yes" => Some(true),
            "n" | "no" => Some(false),
            _ => None,
        },
    )
}

/// The palettes setup offers, fff's default first; `gP` in fff shows them all.
const PALETTES: &[&str] = &[
    "midnight",
    "light",
    "catppuccin-mocha",
    "gruvbox-dark",
    "tokyonight-night",
    "nord",
    "rose-pine-dawn",
];
/// A few rows of results in `palette`'s colors: (foreground, background, text).
fn sample(palette: &crate::config::Palette) -> Vec<(Rgb, Rgb, &'static str)> {
    let rgb = |color: ratatui::style::Color| match color {
        ratatui::style::Color::Rgb(r, g, b) => (r, g, b),
        _ => (0, 0, 0),
    };
    let (background, selection) = (rgb(palette.background), rgb(palette.selection));
    vec![
        (rgb(palette.accent), background, " ▸ folder/ "),
        (rgb(palette.text), background, " notes.pdf "),
        (rgb(palette.marked), background, " ● marked "),
        (rgb(palette.muted), background, " 2 days ago "),
        (rgb(palette.text), selection, " highlighted "),
    ]
}
/// An RGB color, for [`Report::swatch`].
pub type Rgb = (u8, u8, u8);

/// Editors for the files no other rule opens when `$EDITOR` is unset, in
/// order of preference: Neovim, which fff needs, first.
const EDITORS: &[(&str, &str)] = &[
    ("nvim", "Neovim"),
    ("vim", "Vim"),
    ("hx", "Helix"),
    ("kak", "Kakoune"),
    ("micro", "micro"),
    ("nano", "nano"),
    ("emacs -nw", "Emacs"),
];
/// The command of the `[[open.rules]]` entry that opens any file, if one does.
pub fn fallback(open: &crate::open::OpenConfig) -> Option<&str> {
    // A name no extension rule matches; a `*` pattern does.
    open.rule(Path::new("any file"))
        .map(|rule| rule.command.as_str())
}

/// What setup can set up besides the configuration file and the shell
/// (whose state is in [`System`]), and how; tests make their own.
#[derive(Default)]
pub struct Integrations<'a> {
    /// Writes the manual pages, reporting where; None when `man` finds them.
    pub man: Option<SetUp<'a>>,
    /// The freedesktop.org desktop's; None without one (macOS).
    pub desktop: Option<Desktop<'a>>,
}
/// Sets something up, reporting what it wrote, or says why it could not.
pub type SetUp<'a> = Box<dyn FnOnce(&mut Report) -> Result<(), String> + 'a>;
/// Sets up one desktop integration, as [`SetUp`] does.
pub type SetUpTarget<'a> =
    Box<dyn FnMut(crate::setup::Target, &mut Report) -> Result<(), String> + 'a>;
pub struct Desktop<'a> {
    /// Folders open in fff already.
    pub folders: bool,
    /// The file chooser portal fff works with: None when it is not
    /// installed, else whether fff is its chooser already.
    pub chooser: Option<bool>,
    /// `fff setup --desktop` and `--browser`.
    pub set_up: SetUpTarget<'a>,
}

/// The guided setup: the programs, the questions, then the file at
/// `path` (holding `before`, None when there is none) with the answers
/// added, then what `integrations` and the shell leave to set up.
/// Questions and the report go to `report`, answers come from `input`.
pub fn run(
    system: &System,
    integrations: Integrations,
    path: &Path,
    before: Option<&str>,
    home: Option<&Path>,
    input: &mut dyn BufRead,
    report: &mut Report,
) -> io::Result<()> {
    let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
    let document: DocumentMut = before
        .unwrap_or_default()
        .parse()
        .map_err(|e| invalid(format!("{}: {e}", path.display())))?;
    let sets = |table: &str, key: &str| document.get(table).and_then(|t| t.get(key)).is_some();
    // The file's own openers, read as fff reads them.
    let mine: crate::open::OpenConfig = match before {
        Some(text) => toml::from_str::<toml::Table>(text)
            .ok()
            .and_then(|t| t.get("open").cloned())
            .map(|open| open.try_into())
            .transpose()
            .map_err(|e| invalid(format!("{}: [open]: {e}", path.display())))?
            .unwrap_or_default(),
        None => Default::default(),
    };
    report.section(&programs(system, fallback(&mine)))?;
    let opened = |extension: &str| mine.rule(Path::new(&format!("file.{extension}"))).is_some();
    let mut plan = Plan::default();
    let mut next: Vec<(String, Option<String>)> = vec![];

    report.heading("Icons")?;
    if sets("ui", "icons") {
        report.same("[ui] icons is set already")?;
    } else {
        let glyphs: Vec<String> = ["folder/", "notes.pdf", "photo.jpg", "main.rs", "song.mp3"]
            .iter()
            .map(|name| {
                crate::icons::icon(Path::new(name.trim_end_matches('/')), name.ends_with('/'))
                    .glyph
                    .to_string()
            })
            .collect();
        report.line(&format!(
            "fff marks files with Nerd Font icons:   {}",
            glyphs.join("  ")
        ))?;
        if yes_or_no(
            report,
            input,
            "Do you see five small icons, not boxes or blanks?",
            true,
        )? {
            report.done("icons stay on")?;
        } else {
            plan.icons = Some(false);
            report.done("icons off: folders are marked ▸ and files ·")?;
            let choose = "choose it as your terminal's font (or its fallback font), then set \
                          [ui] icons = true";
            next.push(if system.nerd_font {
                (format!("A Nerd Font is installed: {choose}."), None)
            } else {
                match Manager::find(system).and_then(|m| m.install(&NERD_FONT)) {
                    Some(install) => (
                        format!("For icons, install a Nerd Font, {choose}:"),
                        Some(install),
                    ),
                    None => (
                        format!("For icons, install a Nerd Font ({NERD_FONTS_URL}), {choose}."),
                        None,
                    ),
                }
            });
        }
    }

    report.heading("Colors")?;
    if sets("ui", "palette") {
        report.same("[ui] palette is set already")?;
    } else {
        report.line(&format!(
            "Which palette? A few of fff's {}; gP in fff tries each of them on the whole screen.",
            crate::config::Palette::names().len()
        ))?;
        let label = |i: usize, name: &str| match i {
            0 => format!("{name} (default)"),
            _ => name.to_owned(),
        };
        let width = PALETTES
            .iter()
            .enumerate()
            .map(|(i, name)| label(i, name).len())
            .max()
            .unwrap_or(0);
        for (i, name) in PALETTES.iter().enumerate() {
            let palette = crate::config::Palette::named(name).unwrap_or_default();
            let label = label(i, name);
            report.swatch(&format!("{}  {label:width$}  ", i + 1), &sample(&palette))?;
        }
        let chosen = ask(report, input, "Choice [1]:", |answer| match answer {
            "" => Some(1),
            _ => answer
                .parse::<usize>()
                .ok()
                .filter(|n| (1..=PALETTES.len()).contains(n)),
        })?;
        // The default needs no line.
        if chosen > 1 {
            plan.palette = Some(PALETTES[chosen - 1]);
        }
        report.done(&format!("{} colors", PALETTES[chosen - 1]))?;
    }

    report.heading("Opening files")?;
    report.line("Which program opens each kind of file? Files no rule matches open in $EDITOR.")?;
    for kind in KINDS {
        // Only what the file's rules leave out, so that its own stay first.
        let extensions: Vec<&str> = kind
            .extensions
            .iter()
            .copied()
            .filter(|e| !opened(e))
            .collect();
        if extensions.is_empty() {
            report.same(&format!("{}: your [[open.rules]] open them", kind.title))?;
            continue;
        }
        let mut viewers: Vec<&Viewer> = kind.viewers.iter().filter(|v| v.offered(system)).collect();
        let what = format!("{} ({})", kind.title, extensions.join(", "));
        if viewers.is_empty() {
            report.same(&format!("{what}: no program for them is installed"))?;
            continue;
        }
        // What the desktop opens them with comes first, the default answer.
        let desktop = (system.default_app)(kind.mime);
        let chosen_by_desktop = |viewer: &Viewer| desktop.as_deref() == Some(viewer.program());
        viewers.sort_by_key(|viewer| !chosen_by_desktop(viewer));
        report.line("")?;
        report.line(&format!("{what}:"))?;
        let width = viewers.iter().map(|v| v.command.len()).max().unwrap_or(0);
        for (i, viewer) in viewers.iter().enumerate() {
            let note = match (viewer.note, chosen_by_desktop(viewer)) {
                (note, false) => note.to_owned(),
                ("", true) => "your desktop's current choice".to_owned(),
                (note, true) => format!("{note}; your desktop's current choice"),
            };
            let line = format!("  {}  {:width$}  {note}", i + 1, viewer.command);
            report.line(line.trim_end())?;
        }
        report.line(&format!("  0  {:width$}  no rule", "none"))?;
        let chosen = ask(report, input, "Choice [1]:", |answer| match answer {
            "" => Some(1),
            _ => answer.parse::<usize>().ok().filter(|&n| n <= viewers.len()),
        })?;
        if chosen > 0 {
            plan.open_with(kind, &extensions, viewers[chosen - 1]);
        }
    }
    // Without $EDITOR, the rest opens nowhere: a last rule can open it.
    if system.editor.is_none() && fallback(&mine).is_none() {
        let editors: Vec<&(&str, &str)> = EDITORS
            .iter()
            .filter(|(command, _)| system.has(command.split(' ').next().unwrap_or(command)))
            .collect();
        if !editors.is_empty() {
            report.line("")?;
            report.line("Everything else, which $EDITOR would open but it is unset:")?;
            let width = editors.iter().map(|(c, _)| c.len()).max().unwrap_or(0);
            for (i, (command, name)) in editors.iter().enumerate() {
                report.line(&format!("  {}  {command:width$}  {name}", i + 1))?;
            }
            report.line(&format!("  0  {:width$}  no rule", "none"))?;
            let chosen = ask(report, input, "Choice [1]:", |answer| match answer {
                "" => Some(1),
                _ => answer.parse::<usize>().ok().filter(|&n| n <= editors.len()),
            })?;
            if chosen > 0 {
                plan.fallback = Some(editors[chosen - 1].0);
            }
        }
    }

    report.heading("Configuration")?;
    let shown = report.path(path);
    if plan.is_empty() {
        report.same(&format!("nothing to add to {shown}"))?;
    } else {
        let text = apply(before, &plan).map_err(invalid)?;
        // Checked as fff reads it, so that setup never writes a file fff refuses.
        let base = path.parent().unwrap_or(Path::new("/"));
        crate::config::Settings::parse(&text, base, home, None)
            .map_err(|e| invalid(format!("the settings would not load: {e}")))?;
        let added = apply(Some(""), &plan).map_err(invalid)?;
        for line in added.trim().lines() {
            report.command(line)?;
        }
        let question = match before {
            Some(_) => format!("Add this to {shown}?"),
            None => format!("Write this to a new {shown}?"),
        };
        if yes_or_no(report, input, &question, true)? {
            crate::workspaces::atomic_write(path, before, &text).map_err(invalid)?;
            report.done(&match before {
                Some(_) => format!("added to {shown}"),
                None => format!("wrote {shown}"),
            })?;
        } else {
            report.same("nothing written")?;
        }
    }

    shell(system, input, report, &mut next)?;
    report.heading("Manual pages")?;
    match integrations.man {
        None => report.done("man finds fff's pages: man fff")?,
        Some(write) => {
            if yes_or_no(report, input, "Write fff's manual pages (man fff)?", true)? {
                if let Err(error) = write(report) {
                    report.warn(&error)?;
                }
            } else {
                report.same("not written; fff setup --man writes them")?;
            }
        }
    }
    if let Some(desktop) = integrations.desktop {
        desktop_integration(desktop, input, report)?;
    }

    for (text, fix) in next {
        report.step(&text)?;
        if let Some(fix) = fix {
            report.command(&fix)?;
        }
    }
    report.step(&format!("Every setting is described in {SETTINGS_URL}"))
}

/// The shell function and completions: the lines that load them, appended
/// to the shell's startup file once confirmed, unless something loads them
/// already. What is left to do by hand goes to `next`.
fn shell(
    system: &System,
    input: &mut dyn BufRead,
    report: &mut Report,
    next: &mut Vec<(String, Option<String>)>,
) -> io::Result<()> {
    let (Some(shell), Some(startup)) = (system.shell.as_deref(), system.startup.as_deref()) else {
        return Ok(());
    };
    report.heading("Shell")?;
    let shown = report.path(startup);
    let text = match fs::read_to_string(startup) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return report.warn(&format!("{shown}: {e}")),
    };
    let fish = shell == "fish";
    let mut lines = vec![];
    let mut wanted = vec![];
    if system.shell_function {
        report.done("the shell function is loaded: leaving fff changes the shell's folder")?;
    } else if text.contains("fff setup --init") {
        report.same(&format!(
            "{shown} loads the shell function; new shells have it"
        ))?;
    } else {
        lines.push(match fish {
            true => "fff setup --init fish | source".to_owned(),
            false => format!("eval \"$(fff setup --init {shell})\""),
        });
        wanted.push("leaving fff to change the shell's folder");
    }
    let completions = match fish {
        true => "fff setup --completions fish | source".to_owned(),
        false => format!("source <(fff setup --completions {shell})"),
    };
    if system.completions {
        report.done("completions are installed")?;
    } else if text.contains("fff setup --completions") {
        report.same(&format!("{shown} loads completions"))?;
    } else if shell == "zsh" && !runs_compinit(&text) {
        // compdef, which the script calls, needs it.
        report.same(&format!(
            "completions need compinit, which {shown} does not seem to run"
        ))?;
        next.push((
            format!("For completions, run compinit in {shown}, then load them after it:"),
            Some(completions),
        ));
    } else {
        lines.push(completions);
        wanted.push("completions");
    }
    if lines.is_empty() {
        return Ok(());
    }
    report.line(&format!("For {}, add to {shown}:", wanted.join(" and ")))?;
    for line in &lines {
        report.command(line)?;
    }
    let these = if lines.len() == 1 {
        "this line"
    } else {
        "these lines"
    };
    if !yes_or_no(report, input, &format!("Add {these} to {shown}?"), true)? {
        return report.same("nothing written");
    }
    match append_block(startup, &text, &lines) {
        Ok(()) => report.done(&format!("added to {shown}: new shells load it")),
        Err(e) => report.warn(&format!("{shown}: {e}")),
    }
}
/// Whether a `.zshrc` runs compinit, itself or through a framework.
fn runs_compinit(text: &str) -> bool {
    [
        "compinit",
        "oh-my-zsh",
        "prezto",
        "zim",
        "zinit",
        "antidote",
        "antigen",
        "zplug",
        "zgen",
        "sheldon",
    ]
    .iter()
    .any(|word| text.contains(word))
}
/// Appends `lines` to the startup file at `path` (holding `text`) under a
/// comment naming fff setup. Appending writes through a link to the file it
/// points to, as dotfiles managers make them, rather than replacing it.
fn append_block(path: &Path, text: &str, lines: &[String]) -> io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut block = String::new();
    if !text.is_empty() {
        block.push_str(if text.ends_with('\n') { "\n" } else { "\n\n" });
    }
    block.push_str("# Added by fff setup\n");
    for line in lines {
        block.push_str(line);
        block.push('\n');
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?
        .write_all(block.as_bytes())
}
/// Opening folders and file dialogs in fff: asked first, each defaulting
/// to no as it changes other applications, then set up.
fn desktop_integration(
    mut desktop: Desktop,
    input: &mut dyn BufRead,
    report: &mut Report,
) -> io::Result<()> {
    use crate::setup::Target;
    report.heading("Desktop")?;
    let mut targets = vec![];
    if desktop.folders {
        report.done("folders open in fff")?;
    } else if yes_or_no(
        report,
        input,
        "Open folders in fff from other applications (as your file manager)?",
        false,
    )? {
        targets.push(Target::Desktop);
    } else {
        report.same("folders: left as they are; fff setup --desktop does it")?;
    }
    match desktop.chooser {
        Some(true) => report.done("file dialogs open in fff")?,
        None => report.same(
            "file dialogs: with xdg-desktop-portal-termfilechooser installed, \
             fff setup --browser opens them in fff",
        )?,
        Some(false) => {
            if yes_or_no(
                report,
                input,
                "Choose files in fff in Firefox's file dialogs (and other applications')?",
                false,
            )? {
                targets.push(Target::FileChooser);
            } else {
                report.same("file dialogs: left as they are; fff setup --browser does it")?;
            }
        }
    }
    for target in targets {
        if let Err(error) = (desktop.set_up)(target, report) {
            report.warn(&error)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system(macos: bool, installed: &'static [&'static str]) -> System {
        System {
            macos,
            installed: Box::new(move |name| installed.contains(&name)),
            editor: Some("nvim".into()),
            terminal: Some("st -e".into()),
            nerd_font: false,
            shell: Some("zsh".into()),
            startup: None,
            shell_function: false,
            completions: false,
            default_app: Box::new(|_| None),
        }
    }
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fff-guided-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    /// Runs the guided setup on `path` with `answers`, one per line: the
    /// result and what it printed.
    fn guided(system: &System, path: &Path, answers: &str) -> (io::Result<()>, String) {
        with(system, Integrations::default(), path, answers)
    }
    fn with(
        system: &System,
        integrations: Integrations,
        path: &Path,
        answers: &str,
    ) -> (io::Result<()>, String) {
        let before = fs::read_to_string(path).ok();
        let mut out = vec![];
        let result = run(
            system,
            integrations,
            path,
            before.as_deref(),
            None,
            &mut answers.as_bytes(),
            &mut Report::new(&mut out, false, None),
        );
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn programs_say_what_is_missing_and_how_to_install_it() {
        let mut arch = system(false, &["pacman", "pdftotext", "pdfinfo", "zoxide"]);
        arch.editor = None;
        let findings = programs(&arch, None).findings;
        assert_eq!(
            findings[0],
            Finding::Problem(
                "Neovim (nvim) is not installed: fff needs it to start".into(),
                Some("sudo pacman -S neovim".into())
            )
        );
        assert!(matches!(&findings[1], Finding::Fine(t) if t.starts_with("poppler")));
        assert_eq!(
            findings[2],
            Finding::Optional(
                "DjVuLibre (djvutxt) is not installed, for contents search in DjVu files".into(),
                Some("sudo pacman -S djvulibre".into())
            )
        );
        assert!(findings.contains(&Finding::Problem(
            "$EDITOR is unset: files no opener rule matches do not open".into(),
            Some("export EDITOR=nvim   # in ~/.zshrc".into())
        )));
        // Each system its own manager and names; none known, no command.
        let debian = programs(&system(false, &["apt"]), None).findings;
        assert!(
            matches!(&debian[1], Finding::Optional(_, Some(c)) if c == "sudo apt install poppler-utils")
        );
        let mac = programs(&system(true, &["brew", "apt"]), None).findings;
        assert!(matches!(&mac[0], Finding::Problem(_, Some(c)) if c == "brew install neovim"));
        let bare = programs(&system(true, &[]), None).findings;
        assert!(matches!(&bare[0], Finding::Problem(_, None)));
    }

    #[test]
    fn macos_defaults_open_everything_with_open() {
        let dir = scratch("macos");
        let path = dir.join("fff/config.toml");
        let mac = system(true, &["open", "nvim"]);
        // Icons seen, the default palette, every default taken, then the
        // file written.
        let (result, out) = guided(&mac, &path, "y\n\n\n\n\n\n\n");
        result.unwrap();
        assert!(
            out.contains("1  open  the application macOS opens them with"),
            "{out}"
        );
        assert!(
            !out.contains("xdg-open") && !out.contains("--desktop"),
            "{out}"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("# fff's configuration, started by fff setup."),
            "{text}"
        );
        assert!(!text.contains("[ui]"), "icons are on by default: {text}");
        // One rule for every kind, as one program opens them all.
        assert_eq!(text.matches("[[open.rules]]").count(), 1, "{text}");
        assert!(text.contains("# Documents, Images, Audio and video, Office documents\n"));
        let settings = crate::config::Settings::parse(&text, &dir, None, None).unwrap();
        let opener = settings.open.opener(Path::new("a.PDF")).unwrap();
        assert_eq!(opener.line, "open");
        assert_eq!(
            settings.open.opener(Path::new("b.docx")).unwrap().line,
            "open"
        );
        // Run again, it asks only about icons and colors, which the file
        // leaves to their defaults: the end of the answers would stop it.
        let (result, out) = guided(&mac, &path, "y\n\n");
        result.unwrap();
        assert!(
            out.contains("Documents: your [[open.rules]] open them"),
            "{out}"
        );
        assert!(
            out.contains("Office documents: your [[open.rules]] open them"),
            "{out}"
        );
        assert!(out.contains("nothing to add to"), "{out}");
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }

    #[test]
    fn linux_offers_installed_viewers_and_keeps_the_rest_of_the_file() {
        let dir = scratch("linux");
        let path = dir.join("config.toml");
        let mine = "# mine\n[ui]\npalette = \"nord\" # kept\n\n[cache]\nenabled = false\n";
        fs::write(&path, mine).unwrap();
        let linux = system(false, &["zathura", "evince", "feh", "xdg-open", "pacman"]);
        // No icons; evince for documents (asked again after a bad answer),
        // nothing for images, xdg-open for media and office files.
        let (result, out) = guided(&linux, &path, "n\n7\n2\n0\n\n\ny\n");
        result.unwrap();
        assert!(
            out.contains("1  zathura   opens at a contents match's page"),
            "{out}"
        );
        assert!(!out.contains("okular") && !out.contains("  open "), "{out}");
        assert_eq!(out.matches("Choice [1]:").count(), 5, "{out}");
        assert!(
            out.contains("sudo pacman -S ttf-nerd-fonts-symbols"),
            "{out}"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.starts_with("# mine\n[ui]\npalette = \"nord\" # kept\nicons = false\n"),
            "{text}"
        );
        assert!(text.contains("[cache]\nenabled = false\n"), "{text}");
        let settings = crate::config::Settings::parse(&text, &dir, None, None).unwrap();
        assert!(!settings.icons);
        let open = |name: &str| settings.open.opener(Path::new(name)).map(|o| o.line);
        // evince opens no EPUB, so EPUB is left to $EDITOR.
        assert_eq!(open("a.djvu").unwrap(), "evince");
        assert_ne!(open("a.epub").ok().as_deref(), Some("evince"));
        assert_ne!(open("a.png").ok().as_deref(), Some("feh"));
        assert_eq!(open("a.mkv").unwrap(), "xdg-open");
        assert_eq!(open("a.odt").unwrap(), "xdg-open");
        assert_eq!(text.matches("[[open.rules]]").count(), 2, "{text}");
    }

    #[test]
    fn rules_are_added_after_the_files_own_for_what_they_leave_out() {
        let dir = scratch("appended");
        let path = dir.join("config.toml");
        let mine = "[ui]\nicons = true\n\n[open]\nterminal = \"st -e\" # mine\n\n\
                    # my viewer\n[[open.rules]]\ncommand = \"zathura\"\nextensions = [\"pdf\"]\n\n\
                    [[open.rules]]\ncommand = \"nvim\"\npatterns = [\"*.jpg\", \"*.jpeg\"]\nterminal = true\n\n\
                    [trash]\ndays = 7\n";
        fs::write(&path, mine).unwrap();
        let linux = system(false, &["zathura", "feh"]);
        // The default palette, documents with zathura again, images with
        // feh; then write.
        let (result, out) = guided(&linux, &path, "\n\n\ny\n");
        result.unwrap();
        assert!(out.contains("Documents (djvu, epub):"), "{out}");
        assert!(
            out.contains("Images (png, gif, webp, bmp, tif, tiff):"),
            "{out}"
        );
        assert!(!out.contains("Icons\n  ? "), "icons are set: {out}");
        let text = fs::read_to_string(&path).unwrap();
        // The file as it was, then the new rules after its own.
        let (kept, added) = text.split_at(mine.len() - "\n[trash]\ndays = 7\n".len());
        assert_eq!(kept, &mine[..kept.len()], "{text}");
        assert!(text.contains("[trash]\ndays = 7\n"), "{text}");
        assert!(added.contains("# Documents\n[[open.rules]]\ncommand = \"zathura\"\nextensions = [\"djvu\", \"epub\"]"), "{text}");
        let settings = crate::config::Settings::parse(&text, &dir, None, None).unwrap();
        let open = |name: &str| settings.open.opener(Path::new(name)).unwrap();
        assert_eq!(settings.open.rules.len(), 4);
        assert_eq!(open("a.pdf").line, "zathura");
        assert!(
            open("a.pdf").at_grep_match.is_empty(),
            "the file's own rule wins"
        );
        assert_eq!(open("a.epub").at_grep_match, "--page {page} --find {query}");
        assert!(open("a.jpg").terminal);
        assert_eq!(open("a.png").line, "feh");
    }

    #[test]
    fn nothing_is_written_unless_confirmed_or_when_stopped() {
        let dir = scratch("stopped");
        let path = dir.join("config.toml");
        let linux = system(false, &["zathura"]);
        let (result, out) = guided(&linux, &path, "n\n\n\nn\n");
        result.unwrap();
        assert!(out.contains("nothing written"), "{out}");
        let (result, _) = guided(&linux, &path, "n\n");
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
        assert!(!path.exists());
    }

    #[test]
    fn apply_refuses_tables_it_cannot_add_to() {
        let plan = Plan {
            icons: None,
            palette: None,
            fallback: None,
            rules: vec![PlannedRule {
                kinds: vec!["Images"],
                command: "feh".into(),
                extensions: vec!["png".into()],
                at_grep_match: String::new(),
            }],
        };
        assert!(apply(Some("open = { terminal = \"st -e\" }\n"), &plan).is_err());
        let text = apply(Some("[open]\nterminal = \"st -e\"\n"), &plan).unwrap();
        assert_eq!(
            text,
            "[open]\nterminal = \"st -e\"\n\n# Images\n[[open.rules]]\ncommand = \"feh\"\nextensions = [\"png\"]\n"
        );
    }

    #[test]
    fn a_chosen_palette_is_written_and_the_default_is_not() {
        let dir = scratch("palette");
        let path = dir.join("config.toml");
        let linux = system(false, &[]);
        // Icons seen, catppuccin-mocha, then write.
        let (result, out) = guided(&linux, &path, "y\n9\n3\ny\n");
        result.unwrap();
        assert!(out.contains("1  midnight (default)\n"), "{out}");
        assert!(out.contains("3  catppuccin-mocha\n"), "{out}");
        assert_eq!(out.matches("Choice [1]:").count(), 2, "asked again: {out}");
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("[ui]\npalette = \"catppuccin-mocha\"\n"),
            "{text}"
        );
        let settings = crate::config::Settings::parse(&text, &dir, None, None).unwrap();
        assert_eq!(settings.palette_name, "catppuccin-mocha");
        // Set, it is not asked again.
        let (result, out) = guided(&linux, &path, "y\n");
        result.unwrap();
        assert!(out.contains("[ui] palette is set already"), "{out}");
        // Colors on, each choice shows a sample in its own colors.
        let mut out = vec![];
        let mut report = Report::new(&mut out, true, None);
        let palette = crate::config::Palette::named("nord").unwrap();
        report.swatch("6  nord  ", &sample(&palette)).unwrap();
        let out = String::from_utf8(out).unwrap();
        assert!(out.starts_with("  6  nord  \x1b[38;2;"), "{out:?}");
        assert!(
            out.contains(" highlighted ") && out.ends_with("\x1b[0m\n"),
            "{out:?}"
        );
    }

    #[test]
    fn the_desktops_own_viewer_comes_first() {
        let dir = scratch("desktop-default");
        let path = dir.join("config.toml");
        let mut linux = system(false, &["zathura", "evince", "feh", "imv"]);
        linux.default_app = Box::new(|mime| match mime {
            "application/pdf" => Some("evince".into()),
            // Not offered: the list keeps its order.
            "image/png" => Some("gimp".into()),
            _ => None,
        });
        let (result, out) = guided(&linux, &path, "y\n\n\n\ny\n");
        result.unwrap();
        assert!(
            out.contains("1  evince   your desktop's current choice\n    2  zathura  opens at"),
            "{out}"
        );
        assert!(out.contains("1  imv\n    2  feh\n"), "{out}");
        let text = fs::read_to_string(&path).unwrap();
        let settings = crate::config::Settings::parse(&text, &dir, None, None).unwrap();
        assert_eq!(
            settings.open.opener(Path::new("a.pdf")).unwrap().line,
            "evince"
        );
    }

    #[test]
    fn without_editor_a_last_rule_opens_everything_else() {
        let dir = scratch("editor");
        let path = dir.join("config.toml");
        let mut linux = system(false, &["nvim", "nano", "feh"]);
        linux.editor = None;
        // Icons, colors, feh for images, then nano for everything else.
        let (result, out) = guided(&linux, &path, "y\n\n\n2\ny\n");
        result.unwrap();
        assert!(
            out.contains("1  nvim  Neovim\n    2  nano  nano\n"),
            "{out}"
        );
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.ends_with(
                "# Everything else, as $EDITOR would (it is unset)\n[[open.rules]]\n\
                 command = \"nano\"\npatterns = [\"*\"]\nterminal = true\n"
            ),
            "{text}"
        );
        let settings = crate::config::Settings::parse(&text, &dir, None, None).unwrap();
        let open = |name: &str| settings.open.opener(Path::new(name)).unwrap();
        assert_eq!(open("a.png").line, "feh");
        assert_eq!(
            (open("notes.txt").line, open("Makefile").terminal),
            ("nano".into(), true)
        );
        assert_eq!(fallback(&settings.open), Some("nano"));
        // The programs say so; with the rule there, it is not asked again.
        let findings = programs(&linux, fallback(&settings.open)).findings;
        assert!(
            findings
                .iter()
                .any(|f| matches!(f, Finding::Optional(t, _) if t.contains("`nano` opens"))),
            "{findings:?}"
        );
        let (result, out) = guided(&linux, &path, "y\n\n");
        result.unwrap();
        assert!(!out.contains("Everything else"), "{out}");
    }

    #[test]
    fn the_shell_lines_are_appended_once_confirmed() {
        let dir = scratch("shell");
        let path = dir.join("config.toml");
        let zshrc = dir.join("zdot/.zshrc");
        fs::create_dir_all(dir.join("zdot")).unwrap();
        fs::write(&zshrc, "autoload -Uz compinit && compinit").unwrap();
        let mut zsh = system(false, &[]);
        zsh.startup = Some(zshrc.clone());
        // Icons, colors, then no to the lines: nothing changes.
        let (result, out) = guided(&zsh, &path, "y\n\nn\n");
        result.unwrap();
        assert!(
            out.contains("For leaving fff to change the shell's folder and completions, add to"),
            "{out}"
        );
        assert_eq!(
            fs::read_to_string(&zshrc).unwrap(),
            "autoload -Uz compinit && compinit"
        );
        let (result, out) = guided(&zsh, &path, "y\n\ny\n");
        result.unwrap();
        assert!(out.contains("added to"), "{out}");
        assert_eq!(
            fs::read_to_string(&zshrc).unwrap(),
            "autoload -Uz compinit && compinit\n\n# Added by fff setup\n\
             eval \"$(fff setup --init zsh)\"\nsource <(fff setup --completions zsh)\n"
        );
        // Run again, the file loads both: nothing is asked.
        let (result, out) = guided(&zsh, &path, "y\n\n");
        result.unwrap();
        assert!(out.contains("loads the shell function") && out.contains("loads completions"));
        // Without compinit, completions are left as a step; with the
        // function loaded and fish's completions installed, nothing to add.
        fs::write(&zshrc, "").unwrap();
        let (result, out) = guided(&zsh, &path, "y\n\ny\n");
        result.unwrap();
        assert!(out.contains("completions need compinit"), "{out}");
        assert_eq!(
            fs::read_to_string(&zshrc).unwrap(),
            "# Added by fff setup\neval \"$(fff setup --init zsh)\"\n"
        );
        let mut fish = system(false, &[]);
        let config = dir.join("fish/config.fish");
        fish.shell = Some("fish".into());
        fish.startup = Some(config.clone());
        fish.shell_function = true;
        fish.completions = true;
        let (result, out) = guided(&fish, &path, "y\n\n");
        result.unwrap();
        assert!(out.contains("the shell function is loaded"), "{out}");
        assert!(out.contains("completions are installed"), "{out}");
        assert!(!config.exists());
    }

    #[test]
    fn man_pages_and_the_desktop_are_set_up_when_asked() {
        let dir = scratch("integrations");
        let path = dir.join("config.toml");
        let linux = system(false, &[]);
        let done = std::cell::RefCell::new(vec![]);
        let integrations = || Integrations {
            man: Some(Box::new(|_: &mut Report| {
                done.borrow_mut().push("man".to_string());
                Ok(())
            }) as SetUp),
            desktop: Some(Desktop {
                folders: false,
                chooser: Some(false),
                set_up: Box::new(|target, _: &mut Report| {
                    done.borrow_mut().push(format!("{target:?}"));
                    Err("no bus".into())
                }),
            }),
        };
        // Icons, colors; no man pages; the desktop questions default to no.
        let (result, out) = with(&linux, integrations(), &path, "y\n\nn\n\n\n");
        result.unwrap();
        assert!(
            out.contains(
                "Open folders in fff from other applications (as your file manager)? [y/N]"
            )
        );
        assert!(done.borrow().is_empty(), "{out}");
        let (result, out) = with(&linux, integrations(), &path, "y\n\ny\ny\ny\n");
        result.unwrap();
        assert_eq!(*done.borrow(), ["man", "Desktop", "FileChooser"]);
        assert_eq!(out.matches("! no bus").count(), 2, "{out}");
        // Set up already, nothing is asked.
        let ready = Integrations {
            man: None,
            desktop: Some(Desktop {
                folders: true,
                chooser: None,
                set_up: Box::new(|_, _: &mut Report| unreachable!()),
            }),
        };
        let (result, out) = with(&linux, ready, &path, "y\n\n");
        result.unwrap();
        assert!(out.contains("man finds fff's pages") && out.contains("folders open in fff"));
        assert!(
            out.contains("with xdg-desktop-portal-termfilechooser installed"),
            "{out}"
        );
    }

    #[test]
    fn startup_files_and_desktop_entries_are_read_as_shells_and_launchers_do() {
        let home = Path::new("/home/u");
        let config = Path::new("/home/u/.config");
        let path = |shell, zdotdir: Option<&str>| {
            startup_path(
                Some(shell),
                Some(home),
                zdotdir.map(Path::new),
                Some(config),
            )
        };
        assert_eq!(path("zsh", None).unwrap(), Path::new("/home/u/.zshrc"));
        assert_eq!(path("zsh", Some("/z")).unwrap(), Path::new("/z/.zshrc"));
        assert_eq!(path("bash", None).unwrap(), Path::new("/home/u/.bashrc"));
        assert_eq!(path("fish", None).unwrap(), config.join("fish/config.fish"));
        assert_eq!(path("tcsh", None), None);
        let entry = |exec: &str| {
            exec_program(&format!(
                "[Desktop Entry]\nName=x\n{exec}\n[Desktop Action new]\nExec=other\n"
            ))
        };
        assert_eq!(
            entry("Exec=/usr/bin/zathura %U").as_deref(),
            Some("zathura")
        );
        assert_eq!(
            entry("Exec=env GDK_BACKEND=x11 evince %U").as_deref(),
            Some("evince")
        );
        assert_eq!(
            entry("Exec=\"/opt/my viewer/imv\" %f").as_deref(),
            Some("imv")
        );
        assert_eq!(entry("TryExec=x").as_deref(), None);
        let dir = scratch("completions");
        assert!(!completions_installed(
            Some("zsh"),
            std::slice::from_ref(&dir)
        ));
        fs::create_dir_all(dir.join("zsh/site-functions")).unwrap();
        fs::write(dir.join("zsh/site-functions/_fff"), "").unwrap();
        assert!(completions_installed(
            Some("zsh"),
            std::slice::from_ref(&dir)
        ));
        assert!(!completions_installed(Some("bash"), &[dir]));
    }

    #[test]
    fn nerd_fonts_are_found_by_their_file_names() {
        let dir = scratch("fonts");
        fs::create_dir_all(dir.join("TTF")).unwrap();
        fs::write(dir.join("TTF/DejaVuSans.ttf"), "").unwrap();
        assert!(!nerd_font_installed(std::slice::from_ref(&dir)));
        fs::write(dir.join("TTF/SymbolsNerdFont-Regular.ttf"), "").unwrap();
        assert!(nerd_font_installed(std::slice::from_ref(&dir)));
        assert!(!nerd_font_installed(&[dir.join("missing")]));
    }
}
