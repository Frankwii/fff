//! `fff setup` without an option: a guided start. It reports which of the
//! programs fff uses are installed ([`programs`], which `--check-config`
//! reports too), asks whether the terminal shows Nerd Font icons and which
//! of the installed programs open documents, images, media and office files,
//! shows what it would add to the configuration file and adds it once
//! confirmed. What the file sets already is never asked about or changed:
//! `[ui] icons` once set, and the files its `[[open.rules]]` open; rules
//! for the others are added after the file's own.
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
}
impl System {
    /// This system, with `terminal` from `[open]`.
    pub fn from_env(terminal: Option<String>) -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        let home = var("HOME").map(PathBuf::from);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        Self {
            macos: crate::platform::MACOS,
            installed: Box::new(move |name| crate::run::resolve(name, &cwd).is_ok()),
            editor: var("EDITOR"),
            terminal,
            nerd_font: nerd_font_installed(&crate::platform::font_dirs(home.as_deref())),
            shell: var("SHELL").and_then(|s| Some(Path::new(&s).file_name()?.to_str()?.to_owned())),
        }
    }
    fn has(&self, program: &str) -> bool {
        (self.installed)(program)
    }
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
/// it), the terminal terminal programs open in and `$EDITOR`.
pub fn programs(system: &System) -> Section {
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
    findings.push(match &system.editor {
        Some(editor) => Finding::Fine(format!(
            "$EDITOR ({editor}) opens files no opener rule matches"
        )),
        None => Finding::Problem(
            "$EDITOR is unset: files no opener rule matches do not open".into(),
            Some(match system.shell.as_deref() {
                Some("fish") => "set -Ux EDITOR nvim".into(),
                shell => format!("export EDITOR=nvim   # in {}", startup_file(shell)),
            }),
        ),
    });
    Section {
        title: "Programs",
        findings,
    }
}
/// The startup file of a shell, for the lines setup suggests.
fn startup_file(shell: Option<&str>) -> &'static str {
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
    note: "the desktop's default application",
    ..viewer("xdg-open", Systems::NotMacOs)
};
const AT_PAGE: &str = "opens at a contents match's page";
/// A kind of file and the programs that open it, in order of preference.
struct Kind {
    title: &'static str,
    extensions: &'static [&'static str],
    viewers: &'static [Viewer],
}
const KINDS: &[Kind] = &[
    Kind {
        title: "Documents",
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
    pub rules: Vec<PlannedRule>,
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
        self.icons.is_none() && self.rules.is_empty()
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
    if !plan.rules.is_empty() {
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
        document["open"]["rules"] = Item::ArrayOfTables(rules);
    }
    let added = document.to_string();
    Ok(match text {
        Some(_) => added,
        None => format!("{}\n{}", header(), added.trim_start()),
    })
}

/// Reads an answer, asking again until `parse` takes it. The end of input
/// stops setup, before anything is written.
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
                "setup stopped; nothing was written",
            ));
        }
        if let Some(answer) = parse(line.trim()) {
            return Ok(answer);
        }
    }
}
fn yes_or_no(report: &mut Report, input: &mut dyn BufRead, question: &str) -> io::Result<bool> {
    ask(
        report,
        input,
        &format!("{question} [Y/n]"),
        |answer| match answer.to_lowercase().as_str() {
            "" | "y" | "yes" => Some(true),
            "n" | "no" => Some(false),
            _ => None,
        },
    )
}

/// The guided setup: the programs, the questions, then the file at
/// `path` (holding `before`, None when there is none) with the answers
/// added. Questions and the report go to `report`, answers come from `input`.
pub fn run(
    system: &System,
    path: &Path,
    before: Option<&str>,
    home: Option<&Path>,
    input: &mut dyn BufRead,
    report: &mut Report,
) -> io::Result<()> {
    let invalid = |e: String| io::Error::new(io::ErrorKind::InvalidData, e);
    report.section(&programs(system))?;
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
        let viewers: Vec<&Viewer> = kind.viewers.iter().filter(|v| v.offered(system)).collect();
        let what = format!("{} ({})", kind.title, extensions.join(", "));
        if viewers.is_empty() {
            report.same(&format!("{what}: no program for them is installed"))?;
            continue;
        }
        report.line("")?;
        report.line(&format!("{what}:"))?;
        let width = viewers.iter().map(|v| v.command.len()).max().unwrap_or(0);
        for (i, viewer) in viewers.iter().enumerate() {
            let line = format!("  {}  {:width$}  {}", i + 1, viewer.command, viewer.note);
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
        if yes_or_no(report, input, &question)? {
            crate::workspaces::atomic_write(path, before, &text).map_err(invalid)?;
            report.done(&match before {
                Some(_) => format!("added to {shown}"),
                None => format!("wrote {shown}"),
            })?;
        } else {
            report.same("nothing written")?;
        }
    }

    for (text, fix) in next {
        report.step(&text)?;
        if let Some(fix) = fix {
            report.command(&fix)?;
        }
    }
    let shell = system.shell.as_deref();
    if let Some(shell @ ("zsh" | "bash" | "fish")) = shell {
        report.step(&format!(
            "For leaving fff to change your shell's directory, and for completions, add to {}:",
            startup_file(Some(shell))
        ))?;
        if shell == "fish" {
            report.command("fff setup --init fish | source")?;
            report.command("fff setup --completions fish | source")?;
        } else {
            report.command(&format!("eval \"$(fff setup --init {shell})\""))?;
            report.command(&format!("source <(fff setup --completions {shell})"))?;
        }
    }
    // The freedesktop.org desktop's, which macOS has not.
    if !system.macos {
        report.step("To open folders and file dialogs (Firefox's, among others) in fff:")?;
        report.command("fff setup --desktop; fff setup --browser")?;
    }
    report.step(&format!("Every setting is described in {SETTINGS_URL}"))
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
        let before = fs::read_to_string(path).ok();
        let mut out = vec![];
        let result = run(
            system,
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
        let findings = programs(&arch).findings;
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
        let debian = programs(&system(false, &["apt"])).findings;
        assert!(
            matches!(&debian[1], Finding::Optional(_, Some(c)) if c == "sudo apt install poppler-utils")
        );
        let mac = programs(&system(true, &["brew", "apt"])).findings;
        assert!(matches!(&mac[0], Finding::Problem(_, Some(c)) if c == "brew install neovim"));
        let bare = programs(&system(true, &[])).findings;
        assert!(matches!(&bare[0], Finding::Problem(_, None)));
    }

    #[test]
    fn macos_defaults_open_everything_with_open() {
        let dir = scratch("macos");
        let path = dir.join("fff/config.toml");
        let mac = system(true, &["open", "nvim"]);
        // Icons seen, every default taken, then the file written.
        let (result, out) = guided(&mac, &path, "y\n\n\n\n\n\n");
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
        // Run again, it asks only about icons, which the file leaves on:
        // the end of the answers would stop it.
        let (result, out) = guided(&mac, &path, "y\n");
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
        // Documents with zathura again, images with feh; then write.
        let (result, out) = guided(&linux, &path, "\n\ny\n");
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
        let (result, out) = guided(&linux, &path, "n\n\nn\n");
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
