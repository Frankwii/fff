//! TOML configuration. Relative workspace paths use the config directory.
use ratatui::style::Color;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub background: Color,
    pub panel: Color,
    pub text: Color,
    pub muted: Color,
    pub border: Color,
    pub accent: Color,
    pub marked: Color,
    pub success: Color,
    pub error: Color,
    pub selection: Color,
    pub current_directory: Color,
    pub current_directory_text: Color,
    /// Result names of symbolic links, executable files and empty files.
    pub link: Color,
    pub executable: Color,
    pub empty: Color,
    /// Function names, wherever keys are shown or changed.
    pub function: Color,
    /// Folders a copy or extraction is still writing (`.fff-stage-*` and
    /// those in them); such files use `marked`.
    pub staged_folder: Color,
}
impl Default for Palette {
    fn default() -> Self {
        Self::named("midnight").unwrap()
    }
}
impl Palette {
    pub fn names() -> &'static [&'static str] {
        crate::palettes::NAMES
    }
    pub fn named(name: &str) -> Option<Self> {
        let colors = match name {
            "midnight" => [
                0x0f1724, 0x141e2e, 0xdae4f0, 0x8b9db6, 0x34455d, 0x67dde8, 0xf5c670, 0x8edaa8,
                0xfa9191, 0x223e56, 0x23485a, 0xe3fafc,
            ],
            "light" => [
                0xf4f6fa, 0xffffff, 0x182638, 0x52647b, 0xa7b6c8, 0x006779, 0x835000, 0x236d42,
                0xb3283c, 0xdbeaf4, 0x145468, 0xffffff,
            ],
            "forest" => [
                0x101c18, 0x172820, 0xe0ede3, 0x9ab6a3, 0x365a47, 0x8bd7ad, 0xeac482, 0xa0dfa0,
                0xffa0a0, 0x2c4b3a, 0x315e45, 0xefffef,
            ],
            _ => crate::palettes::colors(name)?,
        };
        let rgb = |n: u32| Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8);
        let c = colors.map(rgb);
        Some(Self {
            background: c[0],
            panel: c[1],
            text: c[2],
            muted: c[3],
            border: c[4],
            accent: c[5],
            marked: c[6],
            success: c[7],
            error: c[8],
            selection: c[9],
            current_directory: c[10],
            current_directory_text: c[11],
            link: rgb(crate::palettes::link(name)),
            executable: c[7],
            empty: c[8],
            function: c[8],
            staged_folder: rgb(crate::palettes::staged_folder(name)),
        })
    }
    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let color = if value == "default" {
            Color::Reset
        } else {
            let digits = value
                .strip_prefix('#')
                .filter(|s| s.len() == 6 && s.bytes().all(|c| c.is_ascii_hexdigit()))
                .ok_or_else(|| {
                    format!("Color {key} must be #RRGGBB or 'default', got {value:?}")
                })?;
            let n = u32::from_str_radix(digits, 16).unwrap();
            Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8)
        };
        *match key {
            "background" => &mut self.background,
            "panel" => &mut self.panel,
            "text" => &mut self.text,
            "muted" => &mut self.muted,
            "border" => &mut self.border,
            "accent" => &mut self.accent,
            "marked" => &mut self.marked,
            "success" => &mut self.success,
            "error" => &mut self.error,
            "selection" => &mut self.selection,
            "current_directory" => &mut self.current_directory,
            "current_directory_text" => &mut self.current_directory_text,
            "link" => &mut self.link,
            "executable" => &mut self.executable,
            "empty" => &mut self.empty,
            "function" => &mut self.function,
            "staged_folder" => &mut self.staged_folder,
            _ => return Err(format!("Unknown palette color: {key}")),
        } = color;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    pub enabled: bool,
    pub max_entries: usize,
    pub max_candidates: usize,
    pub ttl_seconds: u64,
    /// Recent exhaustive listings may be used without an immediate rescan.
    pub fresh_seconds: u64,
    /// Speculatively read the highlighted child directory after a short dwell.
    pub prefetch: bool,
}
impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 16,
            max_candidates: 4096,
            ttl_seconds: 60,
            fresh_seconds: 3,
            prefetch: true,
        }
    }
}
impl CacheConfig {
    fn validate(&self) -> Result<(), String> {
        if !(1..=128).contains(&self.max_entries) {
            return Err("cache.max_entries must be 1..128".into());
        }
        if !(1..=100_000).contains(&self.max_candidates) {
            return Err("cache.max_candidates must be 1..100000".into());
        }
        if !(1..=86_400).contains(&self.ttl_seconds) {
            return Err("cache.ttl_seconds must be 1..86400".into());
        }
        if self.fresh_seconds > 60 {
            return Err("cache.fresh_seconds must be 0..60".into());
        }
        Ok(())
    }
}
/// `[performance]`: responsiveness trade-offs that do not affect results.
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PerformanceConfig {
    /// Minimum milliseconds between re-sorts of the result list while a scan
    /// streams in; 0 re-sorts after every drained batch.
    pub refresh_ms: u64,
    /// Shorter queries whose results are kept while typing, so deleting
    /// letters restores them without ranking every path again; 0 disables.
    pub cached_queries: usize,
    /// Most paths a view keeps: a scan stops once it knows more, so even a
    /// huge tree cannot exhaust memory.
    pub max_paths: usize,
}
impl Default for PerformanceConfig {
    fn default() -> Self {
        Self {
            refresh_ms: 50,
            cached_queries: 5,
            max_paths: 1_000_000,
        }
    }
}

/// `[trash]`: where `x` sends entries, and for how long.
#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawTrash {
    enabled: bool,
    days: u64,
    ask_above: Size,
}
/// A size in bytes, or a number with a unit such as "10 GiB".
#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
enum Size {
    Bytes(u64),
    Text(String),
}
impl Default for RawTrash {
    fn default() -> Self {
        Self {
            enabled: true,
            days: 30,
            ask_above: Size::Text("10 GiB".into()),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TrashConfig {
    pub enabled: bool,
    /// Entries older than this are deleted at startup.
    pub days: u64,
    /// Larger entries ask before going to the trash.
    pub ask_above: u64,
}
impl Default for TrashConfig {
    fn default() -> Self {
        RawTrash::default().validate().unwrap()
    }
}
impl RawTrash {
    fn validate(&self) -> Result<TrashConfig, String> {
        if !(1..=36_500).contains(&self.days) {
            return Err("trash.days must be 1..36500".into());
        }
        let ask_above = match &self.ask_above {
            Size::Bytes(n) => *n,
            Size::Text(text) => {
                crate::trash::parse_size(text).map_err(|e| format!("trash.ask_above: {e}"))?
            }
        };
        Ok(TrashConfig {
            enabled: self.enabled,
            days: self.days,
            ask_above,
        })
    }
}

/// A saved name for some of a session's settings: opening it applies the
/// ones it has and leaves the others as they are. One with only a folder is
/// a place to go to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Workspace {
    pub name: String,
    /// Optional label drawn before the name in the chooser, e.g. a Nerd Font glyph.
    pub icon: Option<String>,
    /// The folder to browse.
    pub path: Option<PathBuf>,
    /// `fff roam`, `pick` or `seek`, as `switch_program` changes it.
    pub program: Option<crate::program::Program>,
    /// The pinned roots, replacing the session's (none unpins them).
    pub roots: Option<Vec<PathBuf>>,
    pub query: Option<String>,
    /// The contents query.
    pub contents: Option<String>,
    pub depth: Option<usize>,
    pub filters: Option<Vec<String>>,
    /// The whole exclusion list, the configuration's rules included.
    pub exclusions: Option<Vec<String>>,
    /// `Some(None)` ranks as the program does.
    pub sort: Option<Option<crate::rows::Sort>>,
    pub hidden: Option<bool>,
    pub kinds: Option<crate::Kinds>,
    pub preview: Option<bool>,
    /// Exclusions enforced (Alt+H).
    pub exclusions_on: Option<bool>,
    /// Filters applied (gf).
    pub filters_on: Option<bool>,
}
impl Workspace {
    /// Its settings other than the program: what a session that cannot
    /// change program (a window of its own, a file dialog) still applies.
    pub fn applies_beside_program(&self) -> bool {
        Setting::ALL
            .iter()
            .any(|&s| s != Setting::Program && s.is_in(self))
            || self.path.is_some()
    }
}
/// The settings a workspace may keep, as its form ticks them: some are
/// several keys that go together (both queries, the toggles).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    Program,
    Roots,
    Query,
    Depth,
    Filters,
    Exclusions,
    Sorting,
    Toggles,
}
impl Setting {
    pub const ALL: [Setting; 8] = [
        Self::Program,
        Self::Roots,
        Self::Query,
        Self::Depth,
        Self::Filters,
        Self::Exclusions,
        Self::Sorting,
        Self::Toggles,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Roots => "roots",
            Self::Query => "queries",
            Self::Depth => "depth",
            Self::Filters => "filters",
            Self::Exclusions => "exclusions",
            Self::Sorting => "sorting",
            Self::Toggles => "toggles",
        }
    }
    /// `workspace` keeps any of its keys.
    pub fn is_in(self, w: &Workspace) -> bool {
        match self {
            Self::Program => w.program.is_some(),
            Self::Roots => w.roots.is_some(),
            Self::Query => w.query.is_some() || w.contents.is_some(),
            Self::Depth => w.depth.is_some(),
            Self::Filters => w.filters.is_some(),
            Self::Exclusions => w.exclusions.is_some(),
            Self::Sorting => w.sort.is_some(),
            Self::Toggles => {
                w.hidden.is_some()
                    || w.kinds.is_some()
                    || w.preview.is_some()
                    || w.exclusions_on.is_some()
                    || w.filters_on.is_some()
            }
        }
    }
    /// Its keys in `to` become those of `from` (cleared when `from` has none).
    pub fn copy(self, from: &Workspace, to: &mut Workspace) {
        match self {
            Self::Program => to.program = from.program,
            Self::Roots => to.roots = from.roots.clone(),
            Self::Query => {
                to.query = from.query.clone();
                to.contents = from.contents.clone();
            }
            Self::Depth => to.depth = from.depth,
            Self::Filters => to.filters = from.filters.clone(),
            Self::Exclusions => to.exclusions = from.exclusions.clone(),
            Self::Sorting => to.sort = from.sort,
            Self::Toggles => {
                to.hidden = from.hidden;
                to.kinds = from.kinds;
                to.preview = from.preview;
                to.exclusions_on = from.exclusions_on;
                to.filters_on = from.filters_on;
            }
        }
    }
    pub fn clear(self, w: &mut Workspace) {
        self.copy(&Workspace::default(), w);
    }
    /// Its value in `w`, in a few words, paths shown by `show`; empty when
    /// `w` has none.
    pub fn describe(self, w: &Workspace, show: impl Fn(&Path) -> String) -> String {
        let list = |items: &[String]| {
            if items.is_empty() {
                "none".to_string()
            } else {
                items.join(", ")
            }
        };
        let paths = |paths: &[PathBuf]| list(&paths.iter().map(|p| show(p)).collect::<Vec<_>>());
        let flag = |on: Option<bool>, yes: &'static str, no: &'static str| {
            on.map(|on| if on { yes } else { no })
        };
        match self {
            Self::Program => w.program.map(|p| p.title().to_string()).unwrap_or_default(),
            Self::Roots => w.roots.as_deref().map(paths).unwrap_or_default(),
            Self::Query => [
                w.query.as_ref().map(|q| format!("names {q:?}")),
                w.contents.as_ref().map(|q| format!("contents {q:?}")),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
            Self::Depth => w.depth.map(|d| d.to_string()).unwrap_or_default(),
            Self::Filters => w.filters.as_deref().map(list).unwrap_or_default(),
            Self::Exclusions => w.exclusions.as_deref().map(list).unwrap_or_default(),
            Self::Sorting => match w.sort {
                Some(Some(sort)) => sort.label(),
                Some(None) => "ranking".into(),
                None => String::new(),
            },
            Self::Toggles => [
                flag(w.hidden, "hidden shown", "hidden off"),
                w.kinds.map(|k| match k {
                    crate::Kinds::All => "all kinds",
                    crate::Kinds::Files => "files only",
                    crate::Kinds::Directories => "folders only",
                }),
                flag(w.preview, "preview", "no preview"),
                flag(w.exclusions_on, "exclusions on", "exclusions off"),
                flag(w.filters_on, "filters on", "filters off"),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" · "),
        }
    }
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    ui: UiConfig,
    palettes: BTreeMap<String, BTreeMap<String, String>>,
    workspaces: Vec<RawWorkspace>,
    cache: CacheConfig,
    performance: PerformanceConfig,
    open: crate::open::OpenConfig,
    trash: RawTrash,
    exclusions: RawExclusions,
}
/// `[exclusions]`: rules every session starts with.
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawExclusions {
    rules: Vec<String>,
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UiConfig {
    palette: String,
    icons: bool,
    message_seconds: u64,
    columns: Vec<crate::details::Column>,
    mouse: bool,
    buttons: Buttons,
    buttons_seconds: u64,
    watch: bool,
    preview: bool,
    highlight: bool,
    sort: String,
    sort_descending: bool,
}
/// `[ui] buttons`: when the button bar is shown (opt-in).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Buttons {
    #[default]
    Off,
    /// While the mouse is in use: from its first move until it rests
    /// `[ui] buttons_seconds`.
    Mouse,
    Always,
}
impl Default for UiConfig {
    fn default() -> Self {
        Self {
            palette: "midnight".into(),
            icons: true,
            message_seconds: 4,
            columns: crate::details::DEFAULT_COLUMNS.to_vec(),
            mouse: true,
            buttons: Buttons::Off,
            buttons_seconds: 5,
            watch: true,
            preview: true,
            highlight: true,
            sort: "ranking".into(),
            sort_descending: false,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWorkspace {
    name: String,
    icon: Option<String>,
    path: Option<String>,
    program: Option<String>,
    roots: Option<Vec<String>>,
    query: Option<String>,
    contents: Option<String>,
    depth: Option<usize>,
    filters: Option<Vec<String>>,
    exclusions: Option<Vec<String>>,
    sort: Option<String>,
    sort_descending: Option<bool>,
    hidden: Option<bool>,
    kinds: Option<String>,
    preview: Option<bool>,
    exclusions_on: Option<bool>,
    filters_on: Option<bool>,
}
#[derive(Default)]
pub struct Settings {
    pub palette: Palette,
    /// Name of the active palette (the CLI pin when given, else `[ui] palette`).
    pub palette_name: String,
    /// Every selectable palette: built-ins in documented order, then custom ones.
    pub palettes: Vec<(String, Palette)>,
    pub workspaces: Vec<Workspace>,
    pub cache: CacheConfig,
    pub performance: PerformanceConfig,
    /// Nerd Font filetype icons (`[ui] icons`); presentation only.
    pub icons: bool,
    /// How long status messages stay visible (`[ui] message_seconds`); `None`
    /// keeps them until replaced.
    pub message_timeout: Option<std::time::Duration>,
    /// `[ui] columns`, `[ui] mouse` and `[ui] watch`.
    pub columns: Vec<crate::details::Column>,
    pub mouse: bool,
    /// `[ui] buttons`.
    pub buttons: Buttons,
    /// `[ui] buttons_seconds`: how long a resting mouse keeps the bar of
    /// `buttons = "mouse"`; `None` keeps it for the session.
    pub buttons_timeout: Option<std::time::Duration>,
    pub watch: bool,
    /// `[ui] preview`: start with the preview pane shown.
    pub preview: bool,
    /// `[ui] highlight`: color the syntax of text previews.
    pub highlight: bool,
    /// `[ui] sort` and `sort_descending`: the order sessions start in;
    /// `None` ranks as the program does.
    pub sort: Option<crate::rows::Sort>,
    /// `[open]`: openers by file type and the terminal for terminal openers.
    pub open: crate::open::OpenConfig,
    pub trash: TrashConfig,
    /// `[exclusions] rules`: the exclusion rules sessions start with, before
    /// those of `--exclude`.
    pub exclusions: Vec<String>,
    /// The file that was read; `None` without a config file.
    pub source: Option<PathBuf>,
    /// Active workspace save destination, including a missing default config.
    pub workspace_path: Option<PathBuf>,
    /// The configuration file's errors, when these settings come from the
    /// backup beside it (`load_or_backup`).
    pub broken: Option<String>,
}
impl Settings {
    pub fn parse(
        text: &str,
        base: &Path,
        home: Option<&Path>,
        palette_override: Option<&str>,
    ) -> Result<Self, String> {
        let raw: RawConfig = toml::from_str(text).map_err(|e| e.to_string())?;
        raw.cache.validate()?;
        if raw.performance.refresh_ms > 1000 {
            return Err("performance.refresh_ms must be 0..1000".into());
        }
        if raw.performance.cached_queries > 64 {
            return Err("performance.cached_queries must be 0..64".into());
        }
        if !(1_000..=100_000_000).contains(&raw.performance.max_paths) {
            return Err("performance.max_paths must be 1000..100000000".into());
        }
        if raw.ui.message_seconds > 3600 {
            return Err("ui.message_seconds must be 0..3600 (0 keeps messages)".into());
        }
        if raw.ui.buttons_seconds > 3600 {
            return Err("ui.buttons_seconds must be 0..3600 (0 keeps the buttons)".into());
        }
        let sort = sort_named(&raw.ui.sort, raw.ui.sort_descending)
            .map_err(|names| format!("ui.sort must be {names}, got {:?}", raw.ui.sort))?;
        if raw.ui.buttons != Buttons::Off && !raw.ui.mouse {
            return Err("ui.buttons needs ui.mouse = true: buttons are clicked".into());
        }
        raw.open.validate()?;
        let trash = raw.trash.validate()?;
        if let Some(rule) = raw.exclusions.rules.iter().find(|r| r.is_empty()) {
            return Err(format!("exclusions.rules: {rule:?} is empty"));
        }
        crate::Policy::default()
            .set_rules(raw.exclusions.rules.clone())
            .map_err(|e| format!("exclusions.rules: {e}"))?;
        let mut palettes = BTreeMap::new();
        for (name, values) in &raw.palettes {
            if Palette::named(name).is_some() {
                return Err(format!(
                    "Custom palette {name:?} shadows a built-in palette"
                ));
            }
            if name.trim().is_empty() {
                return Err("Palette name must not be empty".into());
            }
            let base = values
                .get("extends")
                .map(String::as_str)
                .unwrap_or("midnight");
            let mut palette = Palette::named(base)
                .ok_or_else(|| format!("Palette {name:?}: extends must name a built-in palette"))?;
            for (key, value) in values {
                if key != "extends" {
                    palette
                        .set(key, value)
                        .map_err(|e| format!("Palette {name:?}: {e}"))?;
                }
            }
            palettes.insert(name.as_str(), palette);
        }
        let chosen = palette_override.unwrap_or(&raw.ui.palette);
        let palette = Palette::named(chosen)
            .or_else(|| palettes.get(chosen).copied())
            .ok_or_else(|| {
                format!(
                    "Unknown palette {chosen:?}; use one of {} or define [palettes.{chosen}]",
                    Palette::names().join(", ")
                )
            })?;
        let workspaces = resolve_workspaces(raw.workspaces, base, home)?;
        let available = Palette::names()
            .iter()
            .map(|name| (name.to_string(), Palette::named(name).unwrap()))
            .chain(palettes.iter().map(|(name, p)| (name.to_string(), *p)))
            .collect();
        Ok(Self {
            palette,
            palette_name: chosen.to_string(),
            palettes: available,
            workspaces,
            cache: raw.cache,
            performance: raw.performance,
            icons: raw.ui.icons,
            message_timeout: (raw.ui.message_seconds > 0)
                .then(|| std::time::Duration::from_secs(raw.ui.message_seconds)),
            columns: raw.ui.columns,
            mouse: raw.ui.mouse,
            buttons: raw.ui.buttons,
            buttons_timeout: (raw.ui.buttons_seconds > 0)
                .then(|| std::time::Duration::from_secs(raw.ui.buttons_seconds)),
            watch: raw.ui.watch,
            preview: raw.ui.preview,
            highlight: raw.ui.highlight,
            sort,
            open: raw.open,
            trash,
            exclusions: raw.exclusions.rules,
            source: None,
            workspace_path: None,
            broken: None,
        })
    }
    pub fn load(
        explicit: Option<&Path>,
        disabled: bool,
        palette_override: Option<&str>,
    ) -> Result<Self, String> {
        let home = home();
        if let Some(path) = chosen_path(explicit, disabled, home.as_deref()) {
            let (text, found) = match fs::read_to_string(&path) {
                Ok(s) => (s, true),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
                    (String::new(), false)
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            };
            let absolute = absolute(path)?;
            let mut settings = Self::parse(
                &text,
                absolute.parent().unwrap(),
                home.as_deref(),
                palette_override,
            )
            .map_err(|e| format!("{}: {e}", absolute.display()))?;
            settings.workspace_path = Some(absolute.clone());
            settings.source = found.then_some(absolute);
            Ok(settings)
        } else {
            Self::parse("", Path::new("."), home.as_deref(), palette_override)
        }
    }
    /// `load`, except that a configuration file with errors gives way to the
    /// backup `gc` keeps beside it ([`backup_path`]) while that one loads;
    /// `broken` then holds the file's errors.
    pub fn load_or_backup(
        explicit: Option<&Path>,
        disabled: bool,
        palette_override: Option<&str>,
    ) -> Result<Self, String> {
        let error = match Self::load(explicit, disabled, palette_override) {
            Ok(settings) => return Ok(settings),
            Err(error) => error,
        };
        let home = home();
        let Some(path) = chosen_path(explicit, disabled, home.as_deref()) else {
            return Err(error);
        };
        let path = absolute(path)?;
        let Ok(text) = fs::read_to_string(backup_path(&path)) else {
            return Err(error);
        };
        let mut settings = Self::parse(
            &text,
            path.parent().unwrap(),
            home.as_deref(),
            palette_override,
        )
        .map_err(|_| error.clone())?;
        settings.workspace_path = Some(path.clone());
        settings.source = Some(path);
        settings.broken = Some(error);
        Ok(settings)
    }
}
fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}
/// `--config`, else the default location; `None` with `--no-config`.
fn chosen_path(explicit: Option<&Path>, disabled: bool, home: Option<&Path>) -> Option<PathBuf> {
    if disabled {
        return None;
    }
    explicit.map(PathBuf::from).or_else(|| {
        default_path(
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .as_deref(),
            home,
        )
    })
}
fn absolute(path: PathBuf) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path))
    }
}
/// Where `gc` keeps a copy of the configuration file at `path` while it is
/// edited: `config.toml.bak` beside `config.toml`.
pub fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".bak");
    path.with_file_name(name)
}
/// What the session says while it reads the backup of the configuration
/// file at `path`, which has errors.
pub fn reading_backup(path: &Path) -> String {
    format!(
        "{} has errors, so fff reads its backup {} until they are fixed",
        path.display(),
        backup_path(path).display(),
    )
}
/// `error` in the configuration file at `path`, without the path it starts with.
pub fn config_error<'a>(path: &Path, error: &'a str) -> &'a str {
    error
        .strip_prefix(&format!("{}: ", path.display()))
        .unwrap_or(error)
}
/// A configuration error on one line: TOML's excerpt of the file, with its
/// line numbers and carets, is left out.
pub fn brief(error: &str) -> String {
    error
        .lines()
        .map(str::trim)
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with('|')
                && !line
                    .split_once('|')
                    .is_some_and(|(number, _)| number.trim().parse::<u64>().is_ok())
        })
        .collect::<Vec<_>>()
        .join(": ")
}
#[derive(Deserialize)]
struct WorkspaceDocument {
    #[serde(default)]
    workspaces: Vec<RawWorkspace>,
}
pub(crate) fn workspaces_from_text(
    text: &str,
    base: &Path,
    home: Option<&Path>,
) -> Result<Vec<Workspace>, String> {
    let raw: WorkspaceDocument = toml::from_str(text).map_err(|e| e.to_string())?;
    resolve_workspaces(raw.workspaces, base, home)
}
/// `[ui] sort` and a workspace's `sort`: `None` for the program's ranking.
/// The error lists the names allowed.
fn sort_named(name: &str, descending: bool) -> Result<Option<crate::rows::Sort>, String> {
    if name == "ranking" {
        return Ok(None);
    }
    match crate::rows::Criterion::named(name) {
        Some(by) => Ok(Some(crate::rows::Sort { by, descending })),
        None => {
            let names: Vec<_> = crate::rows::Criterion::ALL.map(|c| c.name()).into();
            Err(format!("\"ranking\" or one of {}", names.join(", ")))
        }
    }
}
fn resolve_workspaces(
    raw: Vec<RawWorkspace>,
    base: &Path,
    home: Option<&Path>,
) -> Result<Vec<Workspace>, String> {
    let mut names = HashSet::new();
    let mut workspaces = Vec::new();
    for raw in raw {
        let name = raw.name;
        if name.trim().is_empty() || !names.insert(name.clone()) {
            return Err(format!(
                "Workspace names must be nonempty and unique: {name:?}"
            ));
        }
        let error = |message: &str| format!("Workspace {name:?}: {message}");
        // One or two terminal columns keep the chooser's names aligned.
        let icon = raw.icon.map(|icon| icon.trim().to_string());
        if let Some(icon) = &icon
            && !(1..=2).contains(&unicode_width::UnicodeWidthStr::width(icon.as_str()))
        {
            return Err(error(&format!(
                "icon must be one or two terminal columns wide, got {icon:?}"
            )));
        }
        let path_of = |text: &String, what: &str| {
            if text.is_empty() {
                return Err(error(&format!("{what} must not be empty")));
            }
            expand_path(text, base, home)
        };
        let path = raw.path.as_ref().map(|p| path_of(p, "path")).transpose()?;
        let roots = raw
            .roots
            .map(|roots| roots.iter().map(|r| path_of(r, "a root")).collect())
            .transpose()?;
        let program = raw
            .program
            .map(|text| {
                crate::program::Program::SWITCHABLE
                    .into_iter()
                    .map(|(p, _)| p)
                    .find(|p| p.title() == text)
                    .ok_or_else(|| {
                        error(&format!(
                            "program must be \"roam\", \"pick\" or \"seek\", got {text:?}"
                        ))
                    })
            })
            .transpose()?;
        if raw.depth == Some(0) {
            return Err(error("depth must be at least 1"));
        }
        if let Some(filters) = &raw.filters {
            crate::Policy::default()
                .set_filters(filters.clone())
                .map_err(|e| error(&format!("filters: {e}")))?;
        }
        if let Some(rules) = &raw.exclusions {
            if rules.iter().any(String::is_empty) {
                return Err(error("an exclusion is empty"));
            }
            crate::Policy::default()
                .set_rules(rules.clone())
                .map_err(|e| error(&format!("exclusions: {e}")))?;
        }
        let sort = match (&raw.sort, raw.sort_descending) {
            (Some(sort), descending) => Some(
                sort_named(sort, descending.unwrap_or(false))
                    .map_err(|names| error(&format!("sort must be {names}, got {sort:?}")))?,
            ),
            (None, Some(_)) => return Err(error("sort_descending needs a sort")),
            (None, None) => None,
        };
        let kinds = raw
            .kinds
            .map(|kinds| match kinds.as_str() {
                "all" => Ok(crate::Kinds::All),
                "files" => Ok(crate::Kinds::Files),
                "directories" => Ok(crate::Kinds::Directories),
                _ => Err(error(&format!(
                    "kinds must be \"all\", \"files\" or \"directories\", got {kinds:?}"
                ))),
            })
            .transpose()?;
        let workspace = Workspace {
            name: name.clone(),
            icon,
            path,
            program,
            roots,
            query: raw.query,
            contents: raw.contents,
            depth: raw.depth,
            filters: raw.filters,
            exclusions: raw.exclusions,
            sort,
            hidden: raw.hidden,
            kinds,
            preview: raw.preview,
            exclusions_on: raw.exclusions_on,
            filters_on: raw.filters_on,
        };
        if workspace.path.is_none() && !Setting::ALL.iter().any(|s| s.is_in(&workspace)) {
            return Err(error("it keeps no setting; give it a path or another one"));
        }
        workspaces.push(workspace);
    }
    Ok(workspaces)
}
/// A workspace's path as written: `~` and `~/…` are in the home folder, and
/// a relative path is below `base`.
pub fn expand_path(text: &str, base: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
    let home = || home.ok_or_else(|| format!("Cannot expand {text:?} without HOME"));
    if text == "~" {
        return Ok(home()?.to_path_buf());
    }
    if let Some(rest) = text.strip_prefix("~/") {
        return Ok(home()?.join(rest));
    }
    Ok(base.join(text))
}
pub fn default_path(xdg: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let base = xdg
        .filter(|p| p.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|p| p.join(".config")))?;
    // Keep the application directory stable across crate and executable renames.
    Some(base.join("fff/config.toml"))
}
