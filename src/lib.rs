//! Picker state, independent of terminal input and filesystem timing.
mod cache;
pub mod config;
pub mod discovery;
pub mod keys;
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
    pub fn allows(&self, relative: &Path) -> bool {
        let mut prefix = PathBuf::new();
        for part in relative.components() {
            prefix.push(part);
            if !self.hidden && part.as_os_str().to_string_lossy().starts_with('.') {
                return false;
            }
            if self.ignore
                && self
                    .exclusions
                    .iter()
                    .any(|g| g.is_match(&prefix) || g.is_match(Path::new(part.as_os_str())))
            {
                return false;
            }
        }
        true
    }
}

/// Case-insensitive basename subsequence score: reward adjacency and word starts,
/// then prefer fewer gaps and earlier matches. Empty queries have equal score.
pub fn score(name: &str, query: &str) -> Option<i64> {
    if query.is_empty() {
        return Some(0);
    }
    let name: Vec<char> = name.to_lowercase().chars().collect();
    let mut position = 0;
    let mut previous = None;
    let mut score = 0;
    for letter in query.to_lowercase().chars() {
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
    pub palette: config::Palette,
    pub bookmarks: Vec<config::Bookmark>,
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
            palette: config::Palette::default(),
            bookmarks: Vec::new(),
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
        c.path != self.cwd
            && c.depth <= self.depth
            && self
                .scope
                .get(c.root)
                .and_then(|r| c.path.strip_prefix(r).ok())
                .is_some_and(|p| self.policy.allows(p))
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
        }
    }
    pub fn refresh(&mut self) {
        let mut ranked: Vec<_> = self
            .known
            .values()
            .filter(|c| self.eligible(c))
            .filter_map(|c| {
                let name = c.path.file_name()?.to_string_lossy();
                if self
                    .policy
                    .glob
                    .as_ref()
                    .is_some_and(|g| c.directory || !g.is_match(Path::new(name.as_ref())))
                {
                    return None;
                }
                Some((c.clone(), score(&name, &self.query)?))
            })
            .collect();
        ranked.sort_by(|(a, sa), (b, sb)| {
            a.depth
                .cmp(&b.depth)
                .then(b.directory.cmp(&a.directory))
                .then(sb.cmp(sa))
                .then(a.root.cmp(&b.root))
                .then(a.path.as_os_str().cmp(b.path.as_os_str()))
        });
        self.rows = ranked.into_iter().map(|(c, _)| c).collect();
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
    pub fn navigate_highlighted(&mut self) -> bool {
        let target = self
            .rows
            .iter()
            .find(|c| Some(&c.path) == self.highlight.as_ref())
            .filter(|c| c.directory)
            .map(|c| c.path.clone());
        if let Some(path) = target {
            if !self.ready_path(&path) {
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
    pub fn open_bookmark(&mut self, index: usize) -> bool {
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
                self.navigate(candidate.path, false);
                EnterOutcome::Navigated
            } else {
                self.confirm(Some(candidate.path))
                    .map(EnterOutcome::Selected)
                    .unwrap_or(EnterOutcome::Stay)
            }
        } else if !self.scanning && self.physical_empty {
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
