//! Read-only TOML configuration. Relative bookmark paths use the config directory.
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
        let c = colors.map(|n| Color::Rgb((n >> 16) as u8, (n >> 8) as u8, n as u8));
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
            _ => return Err(format!("Unknown palette color: {key}")),
        } = color;
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    pub enabled: bool,
    pub max_entries: usize,
    pub max_candidates: usize,
    pub ttl_seconds: u64,
}
impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_entries: 16,
            max_candidates: 4096,
            ttl_seconds: 60,
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
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Bookmark {
    pub name: String,
    pub path: PathBuf,
}
#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RawConfig {
    ui: UiConfig,
    palettes: BTreeMap<String, BTreeMap<String, String>>,
    bookmarks: Vec<RawBookmark>,
    cache: CacheConfig,
}
#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct UiConfig {
    palette: String,
}
impl Default for UiConfig {
    fn default() -> Self {
        Self {
            palette: "midnight".into(),
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawBookmark {
    name: String,
    path: String,
}
#[derive(Default)]
pub struct Settings {
    pub palette: Palette,
    pub bookmarks: Vec<Bookmark>,
    pub cache: CacheConfig,
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
        let palette=Palette::named(chosen).or_else(||palettes.get(chosen).copied()).ok_or_else(||format!("Unknown palette {chosen:?}; use midnight, light, forest, or define [palettes.{chosen}]"))?;
        let mut names = HashSet::new();
        let mut bookmarks = Vec::new();
        for bookmark in raw.bookmarks {
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
            let path = if bookmark.path == "~" {
                home.ok_or("Cannot expand ~ without HOME")?.to_path_buf()
            } else if let Some(suffix) = bookmark.path.strip_prefix("~/") {
                home.ok_or("Cannot expand ~/ without HOME")?.join(suffix)
            } else {
                let path = PathBuf::from(bookmark.path);
                if path.is_absolute() {
                    path
                } else {
                    base.join(path)
                }
            };
            bookmarks.push(Bookmark {
                name: bookmark.name,
                path,
            });
        }
        Ok(Self {
            palette,
            bookmarks,
            cache: raw.cache,
        })
    }
    pub fn load(
        explicit: Option<&Path>,
        disabled: bool,
        palette_override: Option<&str>,
    ) -> Result<Self, String> {
        let home = std::env::var_os("HOME")
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        let default = default_path(
            std::env::var_os("XDG_CONFIG_HOME")
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
                .as_deref(),
            home.as_deref(),
        );
        let path = if disabled {
            None
        } else {
            explicit.map(PathBuf::from).or(default)
        };
        if let Some(path) = path {
            let text = match fs::read_to_string(&path) {
                Ok(s) => s,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound && explicit.is_none() => {
                    String::new()
                }
                Err(e) => return Err(format!("{}: {e}", path.display())),
            };
            let absolute = if path.is_absolute() {
                path
            } else {
                std::env::current_dir()
                    .map_err(|e| e.to_string())?
                    .join(path)
            };
            Self::parse(
                &text,
                absolute.parent().unwrap(),
                home.as_deref(),
                palette_override,
            )
            .map_err(|e| format!("{}: {e}", absolute.display()))
        } else {
            Self::parse("", Path::new("."), home.as_deref(), palette_override)
        }
    }
}
pub fn default_path(xdg: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    xdg.filter(|p| p.is_absolute())
        .map(|p| p.join("picker/config.toml"))
        .or_else(|| home.map(|p| p.join(".config/picker/config.toml")))
}
