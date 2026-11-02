//! Fuzzy file finder state, independent of terminal input and filesystem timing.
mod archive;
pub mod bookmarks;
mod cache;
pub mod completion;
pub mod config;
pub mod details;
pub mod discovery;
pub mod editor;
pub mod explorer;
pub mod icons;
pub mod keys;
pub mod location;
pub mod marks;
pub mod normal_keys;
pub mod notify;
pub mod open;
pub mod operations;
mod palettes;
mod prefetch;
pub mod preview;
pub mod reload;
pub mod run;
pub mod setup;
pub mod state;
pub mod ui;
mod view;
pub mod watch;

use globset::{GlobBuilder, GlobMatcher};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
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
    /// File filters (`--glob`, Alt+f): with any, only files whose basename
    /// matches one of them are listed; directories are still traversed.
    pub filters: Vec<String>,
    pub globs: Vec<GlobMatcher>,
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
    /// Like `set_rules`, atomic: every filter compiles before any replaces the list.
    pub fn set_filters(&mut self, filters: Vec<String>) -> Result<(), globset::Error> {
        let compiled = filters
            .iter()
            .map(|s| glob(s))
            .collect::<Result<Vec<_>, _>>()?;
        self.filters = filters;
        self.globs = compiled;
        Ok(())
    }
    /// Whether a file filter admits `name`, a basename; true when there are none.
    pub fn admits(&self, name: &Path) -> bool {
        self.globs.is_empty() || self.globs.iter().any(|g| g.is_match(name))
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
    filters: Vec<String>,
    no_dir: bool,
    files_only: bool,
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
            filters: app.policy.filters.clone(),
            no_dir: app.no_dir,
            files_only: app.files_only,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum EnterOutcome {
    Stay,
    Navigated,
    Selected(Vec<PathBuf>),
}

/// How many tabs a session has. Each starts in the session's first directory.
pub const TABS: usize = 3;
/// A tab's own directory and Ctrl+o / Ctrl+i history. Marks, the query, depth
/// and policies are shared by every tab.
#[derive(Clone, Debug, Default)]
pub struct Tab {
    pub cwd: PathBuf,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
}
pub struct App {
    pub roots: Vec<PathBuf>,
    pub scope: Vec<PathBuf>,
    pub cwd: PathBuf,
    pub depth: usize,
    pub query: String,
    pub policy: Policy,
    /// Every discovered path; rows share these candidates.
    pub known: HashMap<PathBuf, Arc<Candidate>>,
    /// Bumped whenever `known` changes; part of the refresh shortcut's basis.
    pub(crate) known_version: u64,
    /// What the current rows were computed from, and the lowercased query.
    refreshed: Option<(RefreshBasis, String)>,
    /// Rows of shorter queries the current one extends, shortest first, kept so
    /// that deleting letters restores them; they share candidates with `rows`.
    earlier: Vec<(String, Vec<Arc<Candidate>>)>,
    /// `[performance] cached_queries`: most entries kept in `earlier`.
    pub cached_queries: usize,
    pub rows: Vec<Arc<Candidate>>,
    pub highlight: Option<PathBuf>,
    /// Where `highlight` was last found in `rows`. Anything may change either,
    /// so it is checked before use; moving and drawing then skip the search.
    highlight_hint: Cell<usize>,
    /// After a bookmark jump, the highlight follows the first row while the
    /// scan streams in and the query changes, until the user moves it.
    pub highlight_top: bool,
    pub marks: marks::Marks,
    pub rendered: Vec<PathBuf>,
    pub offset: usize,
    /// The mouse wheel scrolled the results while this was the highlight: the
    /// view stays put, even past the highlight, until the highlight changes.
    pub scrolled: Option<Option<PathBuf>>,
    pub generation: u64,
    pub scanning: bool,
    pub status: String,
    /// The status text of the last success; it shows as success only while
    /// `status` still equals it, so any later message reverts to an error.
    pub status_success: String,
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
    /// `fff seek`: a static file choice. Directories are never listed, entered,
    /// or returned, and the current directory cannot change.
    pub no_dir: bool,
    /// Alt+d / `zd`: directories are hidden from the results for now; unlike
    /// `fff seek`, navigation still works.
    pub files_only: bool,
    /// `fff seek` row labels for files whose basename another listed file shares
    /// (presentation only); other rows show their bare name.
    pub labels: HashMap<PathBuf, PathBuf>,
    /// Home directory, shown as `~` in the location line (presentation only).
    pub home: Option<PathBuf>,
    /// Status messages clear after this long; `None` keeps them until replaced.
    pub status_timeout: Option<std::time::Duration>,
    /// Each directory's last highlighted entry, least recently left first.
    pub positions: Vec<(PathBuf, PathBuf)>,
    /// A highlight to restore as soon as its row is listed (scans stream in),
    /// unless the user moves first.
    pub wanted: Option<PathBuf>,
    /// Whether this session's directory is where the next one resumes.
    pub records_location: bool,
    /// `[open]`: which program opens a file.
    pub openers: open::OpenConfig,
    /// `--output-paths`: Insert-mode Enter on a file returns it instead of
    /// revealing it.
    pub output_paths: bool,
    /// Pop-up notifications and when each arrived.
    pub notices: Vec<(String, std::time::Instant)>,
    /// Ctrl+o / Ctrl+i: directories left this session, and those jumped back from.
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
    /// The active tab. Its live state is `cwd`, `back` and `forward`; its slot
    /// in `tabs` is only refreshed when another tab becomes active.
    pub tab: usize,
    pub tabs: Vec<Tab>,
    /// `[ui] columns`: details on the right of each result row.
    pub columns: Vec<details::Column>,
    /// Visible rows' details, read as they are drawn.
    pub details: details::Details,
    /// `[ui] mouse`: clicks and the wheel act on the results.
    pub mouse: bool,
    /// `[ui] buttons`, and whether the mouse has been used this session.
    pub buttons: config::Buttons,
    pub mouse_used: bool,
    /// The preview pane (Alt+v, `zp`) and what it shows.
    pub preview: preview::Pane,
    /// `[ui] watch`: follow changes to the current directory as they happen.
    pub watch: bool,
    /// `fff explorer`: the file dialog being answered. Enter chooses files in
    /// either mode and q cancels.
    pub dialog: Option<explorer::Dialog>,
    /// `--window`: fff has a terminal window of its own and no shell to leave
    /// into, so Enter enters folders and `!` starts a shell in this one.
    pub window: bool,
}

impl App {
    /// Browse freely when the caller supplied no fixed search roots.
    pub fn browse(cwd: PathBuf, depth: usize, policy: Policy) -> Self {
        let mut app = Self::new(vec![cwd], depth, policy);
        app.roots.clear();
        app
    }
    pub fn new(roots: Vec<PathBuf>, depth: usize, policy: Policy) -> Self {
        let start = roots[0].clone();
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
            earlier: Vec::new(),
            cached_queries: config::PerformanceConfig::default().cached_queries,
            rows: Vec::new(),
            highlight: None,
            highlight_top: false,
            highlight_hint: Cell::new(0),
            marks: marks::Marks::default(),
            rendered: Vec::new(),
            offset: 0,
            scrolled: None,
            generation: 0,
            scanning: true,
            status: String::new(),
            status_success: String::new(),
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
            files_only: false,
            labels: HashMap::new(),
            home: None,
            status_timeout: Some(std::time::Duration::from_secs(4)),
            positions: Vec::new(),
            wanted: None,
            records_location: false,
            openers: open::OpenConfig::default(),
            output_paths: false,
            notices: Vec::new(),
            back: Vec::new(),
            forward: Vec::new(),
            tab: 0,
            tabs: vec![
                Tab {
                    cwd: start.clone(),
                    ..Tab::default()
                };
                TABS
            ],
            columns: details::DEFAULT_COLUMNS.to_vec(),
            details: details::Details::default(),
            mouse: true,
            buttons: config::Buttons::Off,
            mouse_used: false,
            watch: true,
            preview: preview::Pane::default(),
            window: false,
            dialog: None,
        }
    }
    /// The button bar is shown: always, or once the mouse has been used.
    pub fn buttons_shown(&self) -> bool {
        match self.buttons {
            config::Buttons::Off => false,
            config::Buttons::Mouse => self.mouse_used,
            config::Buttons::Always => true,
        }
    }
    /// Records the current directory's highlight, as most recently used.
    pub fn remember(&mut self) {
        let Some(highlight) = self.highlight.clone() else {
            return;
        };
        self.positions.retain(|(dir, _)| *dir != self.cwd);
        self.positions.push((self.cwd.clone(), highlight));
        let excess = self.positions.len().saturating_sub(state::CAPACITY);
        self.positions.drain(..excess);
    }
    pub fn remembered(&self, dir: &Path) -> Option<PathBuf> {
        self.positions
            .iter()
            .rev()
            .find(|(d, _)| d == dir)
            .map(|(_, entry)| entry.clone())
    }
    /// Changes directory, recording the old one for Ctrl+o.
    pub fn navigate(&mut self, path: PathBuf, retain_query: bool) {
        if path != self.cwd {
            self.back.push(self.cwd.clone());
            self.forward.clear();
        }
        self.go(path, retain_query);
    }
    /// Ctrl+o (back) / Ctrl+i (forward) through this session's directories,
    /// skipping any that no longer exist. Returns whether the directory changed.
    pub fn jump(&mut self, back: bool) -> bool {
        loop {
            let target = if back {
                self.back.pop()
            } else {
                self.forward.pop()
            };
            let Some(target) = target else {
                return false;
            };
            if !target.is_dir() || target == self.cwd {
                continue;
            }
            let from = self.cwd.clone();
            if back {
                self.forward.push(from);
            } else {
                self.back.push(from);
            }
            self.go(target, false);
            return true;
        }
    }
    /// Alt+Up / Alt+Down: make the next or previous tab active, wrapping
    /// around. Its directory is listed again (a vanished one falls back to its
    /// nearest existing ancestor). Returns whether the directory changed.
    pub fn switch_tab(&mut self, next: bool) -> bool {
        self.select_tab(if next {
            (self.tab + 1) % TABS
        } else {
            (self.tab + TABS - 1) % TABS
        })
    }
    /// Make tab `index` (from 0) the active one; like `switch_tab`, true when
    /// the current directory changed.
    pub fn select_tab(&mut self, index: usize) -> bool {
        if index == self.tab || index >= TABS {
            return false;
        }
        self.tabs[self.tab] = Tab {
            cwd: self.cwd.clone(),
            back: std::mem::take(&mut self.back),
            forward: std::mem::take(&mut self.forward),
        };
        self.tab = index;
        let Tab {
            mut cwd,
            back,
            forward,
        } = std::mem::take(&mut self.tabs[self.tab]);
        self.back = back;
        self.forward = forward;
        while !cwd.is_dir() {
            match cwd.parent() {
                Some(parent) => cwd = parent.to_path_buf(),
                None => break,
            }
        }
        if cwd == self.cwd || !cwd.is_dir() {
            return false;
        }
        self.go(cwd, false);
        true
    }
    /// Every tab's directory, the active one live.
    pub fn tab_dirs(&self) -> Vec<&Path> {
        (0..TABS)
            .map(|i| {
                if i == self.tab {
                    self.cwd.as_path()
                } else {
                    self.tabs[i].cwd.as_path()
                }
            })
            .collect()
    }
    fn go(&mut self, path: PathBuf, retain_query: bool) {
        self.remember();
        let from = std::mem::replace(&mut self.cwd, path);
        self.scope = self.roots.clone();
        if !self.scope.contains(&self.cwd) {
            self.scope.push(self.cwd.clone());
        }
        if !retain_query {
            self.query.clear();
        }
        self.offset = 0;
        self.highlight = None;
        self.highlight_top = false;
        // Going up highlights the folder we came from; otherwise restore what
        // was highlighted here last time.
        self.wanted = from
            .strip_prefix(&self.cwd)
            .ok()
            .and_then(|rest| rest.components().next())
            .map(|child| self.cwd.join(child))
            .or_else(|| self.remembered(&self.cwd));
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
            self.known
                .insert(candidate.path.clone(), Arc::new(candidate));
            self.known_version += 1;
        }
    }
    /// Rows an empty query would list: eligible, and passing `fff seek` (files only) and the
    /// file filters, which hide directories whenever any is set.
    fn listed(&self, c: &Candidate) -> bool {
        self.eligible(c)
            && !((self.no_dir || self.files_only) && c.directory)
            && (self.policy.globs.is_empty()
                || !c.directory
                    && c.path.file_name().is_some_and(|name| {
                        self.policy
                            .admits(Path::new(name.to_string_lossy().as_ref()))
                    }))
    }
    /// The query rows are filtered by. A save dialog's query is the file
    /// name being typed: matches are only highlighted, nothing is hidden.
    pub fn filter(&self) -> &str {
        match &self.dialog {
            Some(dialog) if dialog.save.is_some() => "",
            _ => &self.query,
        }
    }
    pub fn refresh(&mut self) {
        let basis = RefreshBasis::of(self);
        let folded = self.filter().to_lowercase();
        // Labels depend on the whole listing, not the query, so they stay put
        // while typing.
        if self
            .refreshed
            .as_ref()
            .is_none_or(|(previous, _)| *previous != basis)
        {
            self.labels = if self.no_dir {
                disambiguate(
                    self.known
                        .values()
                        .filter(|c| self.listed(c))
                        .map(|c| c.path.as_path()),
                )
            } else {
                HashMap::new()
            };
        }
        // Rows already match the previous query under the same basis. A query
        // that only appends letters (after lowercasing, as `score` does) can only
        // remove matches, so re-score those rows instead of every known path.
        // Deleting letters returns to a shorter query's kept rows, or narrows
        // from the longest kept query the new one still extends.
        let mut base = None;
        match self.refreshed.take() {
            Some((previous, query)) if previous == basis => {
                if query == folded {
                    self.refreshed = Some((previous, query));
                    self.keep_highlight();
                    return;
                }
                if folded.starts_with(query.as_str()) {
                    if self.cached_queries > 0 {
                        self.earlier.push((query, self.rows.clone()));
                        if self.earlier.len() > self.cached_queries {
                            self.earlier.remove(0);
                        }
                    }
                    base = Some(std::mem::take(&mut self.rows));
                } else {
                    while self
                        .earlier
                        .last()
                        .is_some_and(|(q, _)| !folded.starts_with(q.as_str()))
                    {
                        self.earlier.pop();
                    }
                    if let Some((q, _)) = self.earlier.last() {
                        if *q == folded {
                            self.rows = self.earlier.pop().expect("checked").1;
                            self.refreshed = Some((basis, folded));
                            self.keep_highlight();
                            return;
                        }
                        base = self.earlier.last().map(|(_, rows)| rows.clone());
                    }
                }
            }
            _ => self.earlier.clear(),
        }
        let order = |(a, sa): &(&Arc<Candidate>, i64), (b, sb): &(&Arc<Candidate>, i64)| {
            a.depth
                .cmp(&b.depth)
                .then(b.directory.cmp(&a.directory))
                .then(sb.cmp(sa))
                .then(a.root.cmp(&b.root))
                .then(a.path.as_os_str().cmp(b.path.as_os_str()))
        };
        let matcher = Matcher::new(self.filter());
        let score_of = |c: &Candidate| matcher.score(&c.path.file_name()?.to_string_lossy());
        // Pathnames break every tie, so an unstable sort yields the same order.
        let rows = if let Some(base) = base {
            // Move the surviving rows rather than cloning them.
            let mut ranked: Vec<_> = base
                .into_iter()
                .filter_map(|c| {
                    let score = score_of(&c)?;
                    Some((c, score))
                })
                .collect();
            ranked.sort_unstable_by(|(a, sa), (b, sb)| order(&(a, *sa), &(b, *sb)));
            ranked.into_iter().map(|(c, _)| c).collect()
        } else {
            // Sort references and share only the result.
            let mut ranked: Vec<_> = self
                .known
                .values()
                .filter(|c| self.listed(c))
                .filter_map(|c| Some((c, score_of(c)?)))
                .collect();
            ranked.sort_unstable_by(order);
            ranked.into_iter().map(|(c, _)| Arc::clone(c)).collect()
        };
        self.rows = rows;
        self.refreshed = Some((basis, folded));
        self.keep_highlight();
    }
    fn keep_highlight(&mut self) {
        if let Some(wanted) = &self.wanted
            && !self.highlight_top
            && self.rows.iter().any(|c| &c.path == wanted)
        {
            self.highlight = self.wanted.take();
            return;
        }
        if self.highlight_top || self.highlighted().is_none() {
            self.highlight = self.rows.first().map(|c| c.path.clone());
        }
    }
    pub fn move_by(&mut self, amount: isize) {
        // Moving, even before rows arrive, means the user chose a position.
        self.wanted = None;
        if self.rows.is_empty() {
            return;
        }
        let i = self.highlighted().unwrap_or(0);
        let i = i.saturating_add_signed(amount).min(self.rows.len() - 1);
        self.highlight = Some(self.rows[i].path.clone());
        self.highlight_hint.set(i);
        self.highlight_top = false;
    }
    /// The highlighted row's index, when it is listed.
    pub fn highlighted(&self) -> Option<usize> {
        let path = self.highlight.as_ref()?;
        let hint = self.highlight_hint.get();
        if self.rows.get(hint).is_some_and(|c| &c.path == path) {
            return Some(hint);
        }
        let i = self.rows.iter().position(|c| &c.path == path)?;
        self.highlight_hint.set(i);
        Some(i)
    }
    pub fn highlighted_row(&self) -> Option<&Arc<Candidate>> {
        self.highlighted().map(|i| &self.rows[i])
    }
    /// One row down or up, wrapping from the last row to the first and back.
    pub fn step(&mut self, forward: bool) {
        let last = self.rows.len().saturating_sub(1);
        match self.highlighted() {
            Some(i) if forward && i == last => self.move_by(-(last as isize)),
            Some(0) if !forward => self.move_by(last as isize),
            _ => self.move_by(if forward { 1 } else { -1 }),
        }
    }
    pub fn toggle(&mut self, path: PathBuf) {
        if let Some(i) = self.marks.iter().position(|p| p == &path) {
            self.marks.remove(i);
        } else {
            self.marks.push(path);
        }
    }
    /// Mark every one of `rows`, or with `invert` toggle each; marks on other
    /// paths stay.
    pub fn mark_rows(&mut self, rows: &[PathBuf], invert: bool) {
        let listed: HashSet<&PathBuf> = rows.iter().collect();
        let added: Vec<_> = rows
            .iter()
            .filter(|p| !self.marks.contains(p))
            .cloned()
            .collect();
        if invert {
            self.marks.retain(|p| !listed.contains(p));
        }
        self.marks.extend(added);
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
    /// Whether the current directory may change; false in `fff seek`.
    pub fn can_navigate(&mut self) -> bool {
        if self.no_dir {
            self.status = "Navigation is disabled in fff seek".into();
        }
        !self.no_dir
    }
    pub fn navigate_highlighted(&mut self) -> bool {
        let target = self
            .highlighted_row()
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
    /// Insert-mode Enter on a file (without --output-paths): show it in its
    /// folder with the query cleared. Returns whether the directory changed.
    pub fn reveal(&mut self, file: PathBuf) -> bool {
        let folder = file.parent().map(Path::to_path_buf);
        self.query.clear();
        let moved = match folder {
            Some(folder) if folder != self.cwd && self.can_navigate() => {
                self.navigate(folder, false);
                true
            }
            _ => false,
        };
        self.wanted = Some(file);
        // After moving, the old folder's rows must not consume the wanted
        // highlight; the new scan applies it as rows arrive.
        if !moved {
            self.refresh();
        }
        moved
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
                self.highlight_top = true;
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
            self.highlight_top = false;
            self.wanted = None;
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
            self.nothing_to_enter();
            EnterOutcome::Stay
        }
    }
    /// Says why Enter (or Ctrl+number) with no row did nothing.
    pub fn nothing_to_enter(&mut self) {
        self.status = if self.scanning {
            "Nothing listed yet; still searching".into()
        } else if !self.filter().is_empty() {
            format!("Nothing matches {:?}; Esc clears the query", self.query)
        } else {
            "Nothing listed here".into()
        };
    }
    pub fn enter_numbered(&mut self, digit: u32) -> EnterOutcome {
        if self.rendered.is_empty() {
            return self.enter(None);
        }
        let path = self.numbered(digit);
        if let Some(target) = self.rows.iter().find(|c| c.path == path) {
            self.enter(Some(Candidate::clone(target)))
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
        let mut paths = self.marks.to_vec();
        if let Some(path) = target
            && !paths.contains(&path)
        {
            paths.push(path);
        }
        (!paths.is_empty()).then_some(paths)
    }
}

/// For each path sharing its basename with another, the shortest trailing path
/// that no other path with that basename ends in; unique basenames get no entry.
/// A path whose whole length still collides keeps all of it.
pub fn disambiguate<'a>(paths: impl Iterator<Item = &'a Path>) -> HashMap<PathBuf, PathBuf> {
    let mut groups: HashMap<_, Vec<_>> = HashMap::new();
    for path in paths {
        if let Some(name) = path.file_name() {
            groups.entry(name).or_default().push(path);
        }
    }
    let mut labels = HashMap::new();
    for group in groups.into_values().filter(|g| g.len() > 1) {
        let parts: Vec<Vec<_>> = group
            .iter()
            .map(|p| p.components().rev().collect())
            .collect();
        // Once a suffix is unique, every longer one is too, so resolved paths
        // drop out and only the still-colliding ones are counted at the next length.
        let mut pending: Vec<usize> = (0..group.len()).collect();
        let mut length = 2;
        while !pending.is_empty() {
            let mut counts: HashMap<&[_], usize> = HashMap::new();
            for &i in &pending {
                *counts
                    .entry(&parts[i][..length.min(parts[i].len())])
                    .or_default() += 1;
            }
            pending.retain(|&i| {
                let suffix = &parts[i][..length.min(parts[i].len())];
                if counts[suffix] > 1 && length < parts[i].len() {
                    return true;
                }
                labels.insert(
                    group[i].to_path_buf(),
                    suffix.iter().rev().collect::<PathBuf>(),
                );
                false
            });
            length += 1;
        }
    }
    labels
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
