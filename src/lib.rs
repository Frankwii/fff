//! Fuzzy file finder state, independent of terminal input and filesystem timing.
pub mod bookmarks;
mod cache;
pub mod completion;
pub mod config;
pub mod discovery;
pub mod editor;
pub mod icons;
pub mod keys;
pub mod normal_keys;
mod palettes;
mod prefetch;
pub mod reload;
pub mod ui;
mod view;

use globset::{GlobBuilder, GlobMatcher};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub path: PathBuf,
    pub directory: bool,
    pub depth: usize,
    pub root: usize,
}

#[derive(Clone, Default)]
pub struct Policy {
    pub hidden: bool,
    pub ignore: bool,
    pub rules: Vec<String>,
    pub exclusions: Vec<GlobMatcher>,
    pub glob: Option<GlobMatcher>,
}

pub fn glob(pattern: &str) -> Result<GlobMatcher, globset::Error> {
    Ok(GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()?
        .compile_matcher())
}

impl Policy {
    pub fn set_rules(&mut self, rules: Vec<String>) -> Result<(), globset::Error> {
        let compiled = rules
            .iter()
            .map(|s| glob(s))
            .collect::<Result<Vec<_>, _>>()?;
        self.rules = rules;
        self.exclusions = compiled;
        Ok(())
    }
    /// Byte-level `allows` for `path` below `root`, used on every refresh.
    /// `None` (use `allows`) when exclusion rules apply or the bytes do not
    /// show a plain `root/…` prefix, so Path's component rules decide instead.
    pub fn allows_below(&self, root: &Path, path: &Path) -> Option<bool> {
        if self.ignore && !self.exclusions.is_empty() {
            return None;
        }
        let root = root.as_os_str().as_encoded_bytes();
        let path = path.as_os_str().as_encoded_bytes();
        let rest = path.strip_prefix(root)?;
        let rest = if root.ends_with(b"/") {
            rest
        } else {
            rest.strip_prefix(b"/")?
        };
        // Empty, "." and ".." segments are normalized by Path; leave them to it.
        let mut hidden = false;
        for part in rest.split(|&b| b == b'/') {
            if matches!(part, b"" | b"." | b"..") {
                return None;
            }
            hidden |= part[0] == b'.';
        }
        Some(self.hidden || !hidden)
    }
    pub fn allows(&self, relative: &Path) -> bool {
        // Ancestor prefixes are only needed to match exclusion rules.
        let excluding = self.ignore && !self.exclusions.is_empty();
        let mut prefix = PathBuf::new();
        for part in relative.components() {
            if !self.hidden && part.as_os_str().as_encoded_bytes().first() == Some(&b'.') {
                return false;
            }
            if excluding {
                prefix.push(part);
                if self
                    .exclusions
                    .iter()
                    .any(|g| g.is_match(&prefix) || g.is_match(Path::new(part.as_os_str())))
                {
                    return false;
                }
            }
        }
        true
    }
}

/// Case-insensitive basename subsequence score: reward adjacency and word starts,
/// then prefer fewer gaps and earlier matches. Empty queries have equal score.
pub fn score(name: &str, query: &str) -> Option<i64> {
    Matcher::new(query).score(name)
}

/// A query lowercased once, for scoring many names.
pub struct Matcher {
    letters: Vec<char>,
}
impl Matcher {
    pub fn new(query: &str) -> Self {
        Self {
            letters: query.to_lowercase().chars().collect(),
        }
    }
    pub fn score(&self, name: &str) -> Option<i64> {
        if self.letters.is_empty() {
            return Some(0);
        }
        if name.is_ascii() {
            return self.score_ascii(name.as_bytes());
        }
        // Whole-string lowercasing is context-sensitive (e.g. final sigma).
        let name: Vec<char> = name.to_lowercase().chars().collect();
        self.score_chars(&name)
    }
    fn score_chars(&self, name: &[char]) -> Option<i64> {
        let mut position = 0;
        let mut previous = None;
        let mut score = 0;
        for &letter in &self.letters {
            let found = (position..name.len()).find(|&i| name[i] == letter)?;
            score += 20 - found as i64;
            if previous == Some(found.wrapping_sub(1)) {
                score += 30;
            }
            if found == 0 || !name[found - 1].is_alphanumeric() {
                score += 20;
            }
            previous = Some(found);
            position = found + 1;
        }
        Some(score)
    }
    /// Same rules as `score_chars`, without allocating, for ASCII names.
    fn score_ascii(&self, name: &[u8]) -> Option<i64> {
        let mut position = 0;
        let mut previous = None;
        let mut score = 0;
        for &letter in &self.letters {
            if !letter.is_ascii() {
                return None;
            }
            let letter = letter as u8;
            let found = (position..name.len()).find(|&i| name[i].to_ascii_lowercase() == letter)?;
            score += 20 - found as i64;
            if previous == Some(found.wrapping_sub(1)) {
                score += 30;
            }
            if found == 0 || !name[found - 1].is_ascii_alphanumeric() {
                score += 20;
            }
            previous = Some(found);
            position = found + 1;
        }
        Some(score)
    }
}

/// Everything except the query that decides which known paths become rows.
#[derive(Clone, PartialEq)]
struct RefreshBasis {
    known: u64,
    depth: usize,
    cwd: PathBuf,
    scope: Vec<PathBuf>,
    hidden: bool,
    ignore: bool,
    rules: Vec<String>,
    glob: Option<String>,
    no_dir: bool,
}
impl RefreshBasis {
    fn of(app: &App) -> Self {
        Self {
            known: app.known_version,
            depth: app.depth,
            cwd: app.cwd.clone(),
            scope: app.scope.clone(),
            hidden: app.policy.hidden,
            ignore: app.policy.ignore,
            rules: app.policy.rules.clone(),
            glob: app.policy.glob.as_ref().map(|g| g.glob().glob().to_owned()),
            no_dir: app.no_dir,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnterOutcome {
    Stay,
    Navigated,
    Selected(Vec<PathBuf>),
}

pub struct App {
    pub roots: Vec<PathBuf>,
    pub scope: Vec<PathBuf>,
    pub cwd: PathBuf,
    pub depth: usize,
    pub query: String,
    pub policy: Policy,
    pub known: HashMap<PathBuf, Candidate>,
    /// Bumped whenever `known` changes; part of the refresh shortcut's basis.
    pub(crate) known_version: u64,
    /// What the current rows were computed from, and the lowercased query.
    refreshed: Option<(RefreshBasis, String)>,
    pub rows: Vec<Candidate>,
    pub highlight: Option<PathBuf>,
    pub marks: Vec<PathBuf>,
    pub rendered: Vec<PathBuf>,
    pub offset: usize,
    pub generation: u64,
    pub scanning: bool,
    pub status: String,
    pub physical_empty: bool,
    pub cache_pending: bool,
    pub edit_view: Option<editor::EditView>,
    pub palette: config::Palette,
    pub bookmarks: Vec<config::Bookmark>,
    pub bookmark_store: bookmarks::Store,
    /// Draw Nerd Font filetype icons before result and marked names.
    pub icons: bool,
    /// Active palette name and the palettes offered by the Alt+p chooser.
    pub palette_name: String,
    pub palettes: Vec<(String, config::Palette)>,
    pub normal_keys: normal_keys::NormalKeys,
    /// `--no-dir`: a static file choice. Directories are never listed, entered,
    /// or returned, and the current directory cannot change.
    pub no_dir: bool,
    /// Home directory, shown as `~` in the location line (presentation only).
    pub home: Option<PathBuf>,
    /// Status messages clear after this long; `None` keeps them until replaced.
    pub status_timeout: Option<std::time::Duration>,
}

impl App {
    /// Browse freely when the caller supplied no fixed search roots.
    pub fn browse(cwd: PathBuf, depth: usize, policy: Policy) -> Self {
        let mut app = Self::new(vec![cwd], depth, policy);
        app.roots.clear();
        app
    }
    pub fn new(roots: Vec<PathBuf>, depth: usize, policy: Policy) -> Self {
        Self {
            cwd: roots[0].clone(),
            scope: roots.clone(),
            roots,
            depth,
            policy,
            query: String::new(),
            known: HashMap::new(),
            known_version: 0,
            refreshed: None,
            rows: Vec::new(),
            highlight: None,
            marks: Vec::new(),
            rendered: Vec::new(),
            offset: 0,
            generation: 0,
            scanning: true,
            status: String::new(),
            physical_empty: false,
            cache_pending: false,
            edit_view: None,
            palette: config::Palette::default(),
            bookmarks: Vec::new(),
            bookmark_store: bookmarks::Store::default(),
            icons: true,
            palette_name: "midnight".into(),
            palettes: config::Palette::names()
                .iter()
                .map(|n| (n.to_string(), config::Palette::named(n).unwrap()))
                .collect(),
            normal_keys: normal_keys::NormalKeys::default(),
            no_dir: false,
            home: None,
            status_timeout: Some(std::time::Duration::from_secs(4)),
        }
    }
    pub fn navigate(&mut self, path: PathBuf, retain_query: bool) {
        self.cwd = path;
        self.scope = self.roots.clone();
        if !self.scope.contains(&self.cwd) {
            self.scope.push(self.cwd.clone());
        }
        if !retain_query {
            self.query.clear();
        }
        self.offset = 0;
        self.highlight = None;
    }
    pub fn eligible(&self, c: &Candidate) -> bool {
        if c.depth > self.depth {
            return false;
        }
        let Some(root) = self.scope.get(c.root) else {
            return false;
        };
        if let Some(allowed) = self.policy.allows_below(root, &c.path) {
            return allowed && c.path.as_os_str() != self.cwd.as_os_str();
        }
        c.path != self.cwd
            && c.path
                .strip_prefix(root)
                .is_ok_and(|p| self.policy.allows(p))
    }
    pub fn insert(&mut self, generation: u64, mut candidate: Candidate) {
        if generation != self.generation || candidate.path == self.cwd {
            return;
        }
        // Attribute immediately across every scope, even if another root's
        // traversal has not yet delivered its duplicate event.
        let best = self
            .scope
            .iter()
            .enumerate()
            .filter_map(|(root, base)| {
                let relative = candidate.path.strip_prefix(base).ok()?;
                let depth = relative.components().count();
                (depth > 0 && depth <= self.depth && self.policy.allows(relative))
                    .then_some((depth, root))
            })
            .min();
        if let Some((depth, root)) = best {
            candidate.depth = depth;
            candidate.root = root;
            self.known.insert(candidate.path.clone(), candidate);
            self.known_version += 1;
        }
    }
    pub fn refresh(&mut self) {
        let basis = RefreshBasis::of(self);
        let folded = self.query.to_lowercase();
        // Rows already match the previous query under the same basis. A query
        // that only appends letters (after lowercasing, as `score` does) can only
        // remove matches, so re-score those rows instead of every known path.
        let narrowing = match &self.refreshed {
            Some((previous, query)) if *previous == basis => {
                if *query == folded {
                    self.keep_highlight();
                    return;
                }
                folded.starts_with(query.as_str())
            }
            _ => false,
        };
        let order = |(a, sa): &(&Candidate, i64), (b, sb): &(&Candidate, i64)| {
            a.depth
                .cmp(&b.depth)
                .then(b.directory.cmp(&a.directory))
                .then(sb.cmp(sa))
                .then(a.root.cmp(&b.root))
                .then(a.path.as_os_str().cmp(b.path.as_os_str()))
        };
        let matcher = Matcher::new(&self.query);
        let score_of = |c: &Candidate| matcher.score(&c.path.file_name()?.to_string_lossy());
        // Pathnames break every tie, so an unstable sort yields the same order.
        let rows =
            if narrowing {
                // Move the surviving rows rather than cloning them.
                let mut ranked: Vec<_> = std::mem::take(&mut self.rows)
                    .into_iter()
                    .filter_map(|c| {
                        let score = score_of(&c)?;
                        Some((c, score))
                    })
                    .collect();
                ranked.sort_unstable_by(|(a, sa), (b, sb)| order(&(a, *sa), &(b, *sb)));
                ranked.into_iter().map(|(c, _)| c).collect()
            } else {
                // Sort references and clone only the result.
                let mut ranked: Vec<_> =
                    self.known
                        .values()
                        .filter(|c| self.eligible(c) && !(self.no_dir && c.directory))
                        .filter_map(|c| {
                            let name = c.path.file_name()?.to_string_lossy();
                            if self.policy.glob.as_ref().is_some_and(|g| {
                                c.directory || !g.is_match(Path::new(name.as_ref()))
                            }) {
                                return None;
                            }
                            Some((c, matcher.score(&name)?))
                        })
                        .collect();
                ranked.sort_unstable_by(order);
                ranked.into_iter().map(|(c, _)| c.clone()).collect()
            };
        self.rows = rows;
        self.refreshed = Some((basis, folded));
        self.keep_highlight();
    }
    fn keep_highlight(&mut self) {
        if !self
            .rows
            .iter()
            .any(|c| Some(&c.path) == self.highlight.as_ref())
        {
            self.highlight = self.rows.first().map(|c| c.path.clone());
        }
    }
    pub fn move_by(&mut self, amount: isize) {
        if self.rows.is_empty() {
            return;
        }
        let i = self
            .rows
            .iter()
            .position(|c| Some(&c.path) == self.highlight.as_ref())
            .unwrap_or(0);
        let i = i.saturating_add_signed(amount).min(self.rows.len() - 1);
        self.highlight = Some(self.rows[i].path.clone());
    }
    pub fn toggle(&mut self, path: PathBuf) {
        if let Some(i) = self.marks.iter().position(|p| p == &path) {
            self.marks.remove(i);
        } else {
            self.marks.push(path);
        }
    }
    pub fn numbered(&self, digit: u32) -> PathBuf {
        let i = if digit == 0 {
            usize::MAX
        } else {
            digit as usize - 1
        };
        self.rendered
            .get(i.min(self.rendered.len().saturating_sub(1)))
            .cloned()
            .unwrap_or_else(|| self.cwd.clone())
    }
    /// Whether the current directory may change; false with `--no-dir`.
    pub fn can_navigate(&mut self) -> bool {
        if self.no_dir {
            self.status = "Navigation is disabled with --no-dir".into();
        }
        !self.no_dir
    }
    pub fn navigate_highlighted(&mut self) -> bool {
        let target = self
            .rows
            .iter()
            .find(|c| Some(&c.path) == self.highlight.as_ref())
            .filter(|c| c.directory)
            .map(|c| c.path.clone());
        if let Some(path) = target {
            if !self.ready_path(&path) || !self.can_navigate() {
                return false;
            }
            if !path.is_dir() {
                self.status = "Directory is no longer available".into();
                return false;
            }
            self.navigate(path, false);
            true
        } else {
            false
        }
    }
    pub fn ready_path(&mut self, path: &Path) -> bool {
        if self.cache_pending && !path.exists() {
            self.status = "Cached path no longer exists; waiting for refresh".into();
            false
        } else {
            true
        }
    }
    pub fn matching_bookmarks(&self, query: &str) -> Vec<usize> {
        self.bookmarks
            .iter()
            .enumerate()
            .filter(|(_, b)| score(&b.name, query).is_some())
            .map(|(i, _)| i)
            .collect()
    }
    /// Palette chooser rows in list order, fuzzy-filtered by name.
    pub fn matching_palettes(&self, query: &str) -> Vec<usize> {
        self.palettes
            .iter()
            .enumerate()
            .filter(|(_, (name, _))| score(name, query).is_some())
            .map(|(i, _)| i)
            .collect()
    }
    /// Install a loaded palette state without touching session data.
    pub fn apply_palette(&mut self, state: reload::PaletteState) {
        self.palette = state.palette;
        self.palette_name = state.name;
        self.palettes = state.available;
    }
    pub fn open_bookmark(&mut self, index: usize) -> bool {
        if !self.can_navigate() {
            return false;
        }
        let Some(bookmark) = self.bookmarks.get(index) else {
            return false;
        };
        match bookmark.path.canonicalize() {
            Ok(path) if path.is_dir() => {
                self.navigate(path, false);
                true
            }
            _ => {
                self.status = format!(
                    "Bookmark {:?} is not an accessible directory",
                    bookmark.name
                );
                false
            }
        }
    }
    /// Activate a displayed target with exactly the same rules as Enter.
    pub fn enter(&mut self, target: Option<Candidate>) -> EnterOutcome {
        if let Some(mut candidate) = target {
            if !self.ready_path(&candidate.path) {
                return EnterOutcome::Stay;
            }
            if self.cache_pending {
                candidate.directory = candidate.path.is_dir();
            }
            self.highlight = Some(candidate.path.clone());
            if candidate.directory {
                // A cached file that became a directory is not entered either.
                if !self.can_navigate() {
                    return EnterOutcome::Stay;
                }
                self.navigate(candidate.path, false);
                EnterOutcome::Navigated
            } else {
                self.confirm(Some(candidate.path))
                    .map(EnterOutcome::Selected)
                    .unwrap_or(EnterOutcome::Stay)
            }
        } else if !self.scanning && self.physical_empty && !self.no_dir {
            self.confirm(Some(self.cwd.clone()))
                .map(EnterOutcome::Selected)
                .unwrap_or(EnterOutcome::Stay)
        } else {
            EnterOutcome::Stay
        }
    }
    pub fn enter_numbered(&mut self, digit: u32) -> EnterOutcome {
        if self.rendered.is_empty() {
            return self.enter(None);
        }
        let path = self.numbered(digit);
        if let Some(target) = self.rows.iter().find(|c| c.path == path).cloned() {
            self.enter(Some(target))
        } else {
            // Never substitute another candidate for a previously rendered path.
            EnterOutcome::Stay
        }
    }
    pub fn confirm(&mut self, target: Option<PathBuf>) -> Option<Vec<PathBuf>> {
        if self.marks.iter().any(|p| !p.exists()) {
            self.status =
                "Missing marked paths: unmark them in the marked list before confirming".into();
            return None;
        }
        let mut paths = self.marks.clone();
        if let Some(path) = target
            && !paths.contains(&path)
        {
            paths.push(path);
        }
        (!paths.is_empty()).then_some(paths)
    }
}

/// Relative output is always based on the invocation directory, including `..`.
pub fn relative_to(path: &Path, base: &Path) -> PathBuf {
    let a: Vec<_> = path.components().collect();
    let b: Vec<_> = base.components().collect();
    let shared = a.iter().zip(&b).take_while(|(a, b)| a == b).count();
    if shared == 0 {
        return path.to_path_buf();
    }
    let mut result = PathBuf::new();
    for _ in shared..b.len() {
        result.push("..");
    }
    for p in &a[shared..] {
        result.push(p.as_os_str());
    }
    if result.as_os_str().is_empty() {
        result.push(".");
    }
    result
}

pub fn write_paths(
    mut out: impl std::io::Write,
    paths: &[PathBuf],
    base: &Path,
    relative: bool,
    nul: bool,
) -> std::io::Result<()> {
    for path in paths {
        let path = if relative {
            relative_to(path, base)
        } else {
            path.clone()
        };
        out.write_all(path.as_os_str().as_encoded_bytes())?;
        out.write_all(if nul { b"\0" } else { b"\n" })?;
    }
    out.flush()
}
