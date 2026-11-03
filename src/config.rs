//! TOML configuration. Relative bookmark paths use the config directory.
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
            _ => return Err(format!("Unknown palette color: {key}")),
        } = color;
        Ok(())
    }
}

/// Neovim is the only text engine; `[editor] backend = "nvim"` is still accepted.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EditorBackend {
    #[default]
    Nvim,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EditorConfig {
    pub backend: Option<EditorBackend>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bookmark {
    pub name: String,
    pub path: PathBuf,
    /// Optional label drawn before the name in the chooser, e.g. a Nerd Font glyph.
    pub icon: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    editor: EditorConfig,
    ui: UiConfig,
    palettes: BTreeMap<String, BTreeMap<String, String>>,
    bookmarks: Vec<RawBookmark>,
    cache: CacheConfig,
    performance: PerformanceConfig,
    keys: KeysConfig,
    open: crate::open::OpenConfig,
    trash: RawTrash,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct KeysConfig {
    normal: crate::normal_keys::NormalKeysConfig,
    /// Chord remapping like --map-key: source chord = default chord or "none".
    map: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UiConfig {
    palette: String,
    icons: bool,
    message_seconds: u64,
    /// Accepted for configuration compatibility: sessions resume only with
    /// `fff --resume`.
    #[allow(dead_code)]
    resume: bool,
    columns: Vec<crate::details::Column>,
    mouse: bool,
    buttons: Buttons,
    watch: bool,
    preview: bool,
    highlight: bool,
}
/// `[ui] buttons`: when the button bar is shown (opt-in).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Buttons {
    #[default]
    Off,
    /// Once the mouse has been used in the session.
    Mouse,
    Always,
}
impl Default for UiConfig {
    fn default() -> Self {
        Self {
            palette: "midnight".into(),
            icons: true,
            message_seconds: 4,
            resume: true,
            columns: crate::details::DEFAULT_COLUMNS.to_vec(),
            mouse: true,
            buttons: Buttons::Off,
            watch: true,
            preview: true,
            highlight: true,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBookmark {
    name: String,
    path: String,
    icon: Option<String>,
}
#[derive(Default)]
pub struct Settings {
    pub editor: EditorConfig,
    pub palette: Palette,
    /// Name of the active palette (the CLI pin when given, else `[ui] palette`).
    pub palette_name: String,
    /// Every selectable palette: built-ins in documented order, then custom ones.
    pub palettes: Vec<(String, Palette)>,
    pub bookmarks: Vec<Bookmark>,
    pub cache: CacheConfig,
    pub performance: PerformanceConfig,
    /// Nerd Font filetype icons (`[ui] icons`); presentation only.
    pub icons: bool,
    pub normal_keys: crate::normal_keys::NormalKeys,
    /// `[keys.map]` as `SOURCE=TARGET` rules, applied before `--map-key`.
    pub key_maps: Vec<String>,
    /// How long status messages stay visible (`[ui] message_seconds`); `None`
    /// keeps them until replaced.
    pub message_timeout: Option<std::time::Duration>,
    /// `[ui] columns`, `[ui] mouse` and `[ui] watch`.
    pub columns: Vec<crate::details::Column>,
    pub mouse: bool,
    /// `[ui] buttons`.
    pub buttons: Buttons,
    pub watch: bool,
    /// `[ui] preview`: start with the preview pane shown.
    pub preview: bool,
    /// `[ui] highlight`: color the syntax of text previews.
    pub highlight: bool,
    /// `[open]`: openers by file type and the terminal for terminal openers.
    pub open: crate::open::OpenConfig,
    pub trash: TrashConfig,
    /// The file that was read; `None` without a config file.
    pub source: Option<PathBuf>,
    /// Active bookmark save destination, including a missing default config.
    pub bookmark_path: Option<PathBuf>,
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
        if raw.ui.buttons != Buttons::Off && !raw.ui.mouse {
            return Err("ui.buttons needs ui.mouse = true: buttons are clicked".into());
        }
        let normal_keys = raw.keys.normal.build()?;
        raw.open.validate()?;
        let trash = raw.trash.validate()?;
        let key_maps: Vec<String> = raw
            .keys
            .map
            .iter()
            .map(|(from, to)| format!("{from}={to}"))
            .collect();
        crate::keys::KeyMap::parse(&key_maps).map_err(|e| format!("keys.map: {e}"))?;
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
        let bookmarks = resolve_bookmarks(raw.bookmarks, base, home)?;
        let available = Palette::names()
            .iter()
            .map(|name| (name.to_string(), Palette::named(name).unwrap()))
            .chain(palettes.iter().map(|(name, p)| (name.to_string(), *p)))
            .collect();
        Ok(Self {
            editor: raw.editor,
            palette,
            palette_name: chosen.to_string(),
            palettes: available,
            bookmarks,
            cache: raw.cache,
            performance: raw.performance,
            icons: raw.ui.icons,
            normal_keys,
            key_maps,
            message_timeout: (raw.ui.message_seconds > 0)
                .then(|| std::time::Duration::from_secs(raw.ui.message_seconds)),
            columns: raw.ui.columns,
            mouse: raw.ui.mouse,
            buttons: raw.ui.buttons,
            watch: raw.ui.watch,
            preview: raw.ui.preview,
            highlight: raw.ui.highlight,
            open: raw.open,
            trash,
            source: None,
            bookmark_path: None,
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
            settings.bookmark_path = Some(absolute.clone());
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
        settings.bookmark_path = Some(path.clone());
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
/// file at `path`, which has `error`.
pub fn reading_backup(path: &Path, error: &str) -> String {
    format!(
        "{} has errors, so fff reads its backup {} until they are fixed: {}",
        path.display(),
        backup_path(path).display(),
        brief(
            error
                .strip_prefix(&format!("{}: ", path.display()))
                .unwrap_or(error)
        )
    )
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
struct BookmarkDocument {
    #[serde(default)]
    bookmarks: Vec<RawBookmark>,
}
pub(crate) fn bookmarks_from_text(
    text: &str,
    base: &Path,
    home: Option<&Path>,
) -> Result<Vec<Bookmark>, String> {
    let raw: BookmarkDocument = toml::from_str(text).map_err(|e| e.to_string())?;
    resolve_bookmarks(raw.bookmarks, base, home)
}
fn resolve_bookmarks(
    raw: Vec<RawBookmark>,
    base: &Path,
    home: Option<&Path>,
) -> Result<Vec<Bookmark>, String> {
    let mut names = HashSet::new();
    let mut bookmarks = Vec::new();
    for bookmark in raw {
        if bookmark.name.trim().is_empty() || !names.insert(bookmark.name.clone()) {
            return Err(format!(
                "Bookmark names must be nonempty and unique: {:?}",
                bookmark.name
            ));
        }
        if bookmark.path.is_empty() {
            return Err(format!(
                "Bookmark {:?}: path must not be empty",
                bookmark.name
            ));
        }
        // One or two terminal columns keep the chooser's names aligned.
        let icon = bookmark.icon.map(|icon| icon.trim().to_string());
        if let Some(icon) = &icon
            && !(1..=2).contains(&unicode_width::UnicodeWidthStr::width(icon.as_str()))
        {
            return Err(format!(
                "Bookmark {:?}: icon must be one or two terminal columns wide, got {icon:?}",
                bookmark.name
            ));
        }
        bookmarks.push(Bookmark {
            path: expand_path(&bookmark.path, base, home)?,
            name: bookmark.name,
            icon,
        });
    }
    Ok(bookmarks)
}
/// A bookmark's path as written: `~` and `~/…` are in the home folder, and
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
