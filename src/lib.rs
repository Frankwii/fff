//! Fuzzy file finder state, independent of terminal input and filesystem timing.
mod archive;
pub mod bookmarks;
mod cache;
pub mod completion;
pub mod config;
pub mod dbus;
pub mod details;
pub mod discovery;
pub mod editor;
pub mod explorer;
pub mod icons;
pub mod keys;
pub mod known;
pub mod location;
pub mod marks;
pub mod normal_keys;
pub mod notify;
pub mod open;
pub mod operations;
mod palettes;
mod prefetch;
pub mod preview;
mod ranking;
pub mod reload;
pub mod run;
pub mod setup;
pub mod state;
pub mod trash;
pub mod ui;
mod view;
pub mod watch;

use globset::{GlobBuilder, GlobMatcher};
use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
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

/// Path-aware matching: bonus for a query part naming a whole component.
const WHOLE: i64 = 40;
/// Path-aware matching: bonus for consecutive parts matching consecutive
/// components (the last folder part next to the basename included).
const NEXT: i64 = 20;

/// A query lowercased once, for scoring many names.
pub struct Matcher {
    letters: Vec<char>,
    /// A query containing `/` matches the path below a result's root: the
    /// parts before the last (empty ones dropped) match ancestors in order,
    /// the last matches the basename.
    path: Option<(Vec<Matcher>, Box<Matcher>)>,
}
/// Lowercase for matching. Whole-string lowercasing turns a word-final Σ
/// into ς but a lone one into σ: both become σ, so either spelling matches.
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase().replace('ς', "σ")
}
impl Matcher {
    pub fn new(query: &str) -> Self {
        let folded = fold(query);
        let plain = |part: &str| Matcher {
            letters: part.chars().collect(),
            path: None,
        };
        let path = folded.rsplit_once('/').map(|(folders, last)| {
            let folders = folders
                .split('/')
                .filter(|part| !part.is_empty())
                .map(plain)
                .collect();
            (folders, Box::new(plain(last)))
        });
        Self {
            letters: folded.chars().collect(),
            path,
        }
    }
    /// Whether the query matches paths rather than basenames.
    pub fn paths(&self) -> bool {
        self.path.is_some()
    }
    /// Score `name` as a whole: a basename, a bookmark or palette name.
    pub fn score(&self, name: &str) -> Option<i64> {
        if self.letters.is_empty() {
            return Some(0);
        }
        if name.is_ascii() {
            return self.score_ascii(name.as_bytes());
        }
        // Whole-string lowercasing is context-sensitive (e.g. final sigma).
        let name: Vec<char> = fold(name).chars().collect();
        self.score_chars(&name)
    }
    /// Score a result: its basename, or with a `/` in the query, its path
    /// below its root.
    pub fn score_candidate(&self, c: &Candidate) -> Option<i64> {
        if self.path.is_none() {
            return self.score(&c.path.file_name()?.to_string_lossy());
        }
        self.score_path(&relative_parts(c))
    }
    /// Score a path's components below its root, the basename last.
    pub fn score_path<S: AsRef<str>>(&self, parts: &[S]) -> Option<i64> {
        match &self.path {
            None => self.score(parts.last()?.as_ref()),
            Some(_) => self.assign(parts).map(|(score, _)| score),
        }
    }
    /// The letters of `parts` (a path's components below its root) a path
    /// query matches, as char indices within each component; `None` when it
    /// does not match.
    pub fn path_positions<S: AsRef<str>>(&self, parts: &[S]) -> Option<Vec<Vec<usize>>> {
        let (folders, last) = self.path.as_ref()?;
        let (_, chosen) = self.assign(parts)?;
        let mut positions = vec![Vec::new(); parts.len()];
        for (folder, &at) in folders.iter().zip(&chosen) {
            positions[at] = greedy(&folder.letters, parts[at].as_ref());
        }
        positions[parts.len() - 1] = greedy(&last.letters, parts[parts.len() - 1].as_ref());
        Some(positions)
    }
    /// The best path score and, for each folder part, the component it
    /// matches: parts match components in order, the last part the basename.
    fn assign<S: AsRef<str>>(&self, parts: &[S]) -> Option<(i64, Vec<usize>)> {
        let (folders, last) = self.path.as_ref()?;
        let (basename, ancestors) = parts.split_last()?;
        let basename = basename.as_ref();
        let mut total = last.score(basename)? + last.whole(basename);
        if folders.is_empty() {
            return Some((total, Vec::new()));
        }
        if folders.len() > ancestors.len() {
            return None;
        }
        // best[i][j]: the best score of folder parts 0..=i with part i on
        // ancestor j, and the ancestor part i - 1 is on.
        let mut best: Vec<Vec<Option<(i64, usize)>>> = Vec::with_capacity(folders.len());
        for (i, folder) in folders.iter().enumerate() {
            let mut row = vec![None; ancestors.len()];
            for (j, part) in ancestors.iter().enumerate() {
                // Leave room for the parts still to place.
                if j < i || ancestors.len() - j < folders.len() - i {
                    continue;
                }
                let Some(own) = folder.score(part.as_ref()) else {
                    continue;
                };
                let own = own + folder.whole(part.as_ref());
                row[j] = if i == 0 {
                    Some((own, 0))
                } else {
                    (0..j)
                        .filter_map(|k| {
                            let (score, _) = best[i - 1][k]?;
                            Some((score + own + if k + 1 == j { NEXT } else { 0 }, k))
                        })
                        .max_by_key(|&(score, k)| (score, std::cmp::Reverse(k)))
                };
            }
            best.push(row);
        }
        let (end, score) = best[folders.len() - 1]
            .iter()
            .enumerate()
            .filter_map(|(j, cell)| {
                let (score, _) = (*cell)?;
                Some((j, score + if j + 1 == ancestors.len() { NEXT } else { 0 }))
            })
            .max_by_key(|&(j, score)| (score, std::cmp::Reverse(j)))?;
        total += score;
        let mut chosen = vec![end; folders.len()];
        for i in (1..folders.len()).rev() {
            chosen[i - 1] = best[i][chosen[i]].expect("on the best path").1;
        }
        Some((total, chosen))
    }
    /// `WHOLE` when this part names all of `name`, ignoring case.
    fn whole(&self, name: &str) -> i64 {
        let same = if name.is_ascii() {
            name.len() == self.letters.len()
                && name
                    .bytes()
                    .zip(&self.letters)
                    .all(|(b, &l)| b.to_ascii_lowercase() as u32 == l as u32)
        } else {
            fold(name).chars().eq(self.letters.iter().copied())
        };
        if same && !self.letters.is_empty() {
            WHOLE
        } else {
            0
        }
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

/// A ranking running on another thread, for this basis and query.
struct Pending {
    basis: RefreshBasis,
    query: String,
    result: mpsc::Receiver<ranking::Ranked>,
    cancel: Arc<AtomicBool>,
}

/// Whether `c` is within `depth` of its root, allowed by `policy`, and not `cwd`.
fn eligible(depth: usize, scope: &[PathBuf], cwd: &Path, policy: &Policy, c: &Candidate) -> bool {
    if c.depth > depth {
        return false;
    }
    let Some(root) = scope.get(c.root) else {
        return false;
    };
    if let Some(allowed) = policy.allows_below(root, &c.path) {
        return allowed && c.path.as_os_str() != cwd.as_os_str();
    }
    c.path != cwd && c.path.strip_prefix(root).is_ok_and(|p| policy.allows(p))
}
/// Whether an eligible `c` passes `fff seek` (files only) and the file
/// filters, which hide directories whenever any is set.
/// Which kinds of entry the results list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kinds {
    #[default]
    All,
    /// Alt+d / `d`, and always in `fff seek`.
    Files,
    /// Alt+D / `D`: folders only. File filters do not apply.
    Directories,
}
impl Kinds {
    /// `kinds` after toggling `only` on or off.
    pub fn toggled(self, only: Kinds) -> Kinds {
        if self == only { Kinds::All } else { only }
    }
}
fn shown(kinds: Kinds, policy: &Policy, c: &Candidate) -> bool {
    match kinds {
        Kinds::Directories => c.directory,
        Kinds::Files if c.directory => false,
        _ => {
            policy.globs.is_empty()
                || !c.directory
                    && c.path.file_name().is_some_and(|name| {
                        policy.admits(Path::new(name.to_string_lossy().as_ref()))
                    })
        }
    }
}
/// `App::listed` with its own copy of what it depends on.
struct Listing {
    depth: usize,
    scope: Vec<PathBuf>,
    cwd: PathBuf,
    policy: Policy,
    kinds: Kinds,
}
impl Listing {
    fn listed(&self, c: &Candidate) -> bool {
        eligible(self.depth, &self.scope, &self.cwd, &self.policy, c)
            && shown(self.kinds, &self.policy, c)
    }
}
/// A result's components below its root, the basename last: its last
/// `depth` components.
pub fn relative_parts(c: &Candidate) -> Vec<std::borrow::Cow<'_, str>> {
    let mut parts: Vec<_> = c
        .path
        .components()
        .rev()
        .take(c.depth.max(1))
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    parts.reverse();
    parts
}
/// Char indices of `name` matching `letters` (lowercase) in order, earliest
/// first, as drawn.
pub(crate) fn greedy(letters: &[char], name: &str) -> Vec<usize> {
    let mut wanted = letters.iter().peekable();
    name.chars()
        .enumerate()
        .filter(|(_, c)| {
            let matched = wanted.peek().is_some_and(|&&q| {
                c.to_lowercase()
                    .any(|lower| lower == q || (lower == 'ς' && q == 'σ'))
            });
            if matched {
                wanted.next();
            }
            matched
        })
        .map(|(i, _)| i)
        .collect()
}
#[cfg(test)]
#[test]
fn final_sigma_is_highlighted_for_either_sigma() {
    assert_eq!(greedy(&['σ'], "ΟΔΟΣ"), [3]);
    assert_eq!(greedy(&['ο', 'σ'], "οδος"), [0, 3]);
}
/// Whether every match of the (lowercased) query `longer` is among those of
/// `shorter`: it appends letters, but no `/`, which turns the part before it
/// into a folder (or a basename query into a path query).
fn narrows(shorter: &str, longer: &str) -> bool {
    longer
        .strip_prefix(shorter)
        .is_some_and(|added| !added.contains('/'))
}

/// Insertions merged into the rows per budgeted round.
const MERGE_ROUND: usize = 512;

/// Result order: depth, directories first, best score, root order, then
/// pathname bytes, which break every tie.
fn rank(a: &Candidate, score_a: i64, b: &Candidate, score_b: i64) -> std::cmp::Ordering {
    a.depth
        .cmp(&b.depth)
        .then(b.directory.cmp(&a.directory))
        .then(score_b.cmp(&score_a))
        .then(a.root.cmp(&b.root))
        .then(a.path.as_os_str().cmp(b.path.as_os_str()))
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
    kinds: Kinds,
    short_labels: bool,
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
            kinds: app.kinds,
            short_labels: app.short_labels,
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
#[derive(Clone, Debug, Default, PartialEq, Eq)]
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
    pub known: known::Known,
    /// Bumped when `known` is replaced wholesale; part of the refresh
    /// shortcut's basis. Insertions and forgotten paths are merged instead.
    pub(crate) known_version: u64,
    /// Candidates inserted since the last refresh, which merges them into the
    /// ranked rows instead of ranking every known path again.
    added: Vec<Arc<Candidate>>,
    /// Candidates `known` replaced or forgot since the last refresh, whose
    /// rows it drops; holding them keeps their addresses from being reused.
    outdated: Vec<Arc<Candidate>>,
    /// What the current rows were computed from, and the lowercased query.
    refreshed: Option<(RefreshBasis, String)>,
    /// Rows of shorter queries the current one extends, shortest first, kept so
    /// that deleting letters restores them; they share candidates with `rows`.
    earlier: Vec<(String, ranking::Rows)>,
    /// Rows being ranked on another thread, and what wakes the UI when done.
    ranking: Option<Pending>,
    waker: Option<mpsc::SyncSender<()>>,
    /// Input events waiting to be read: budgeted merges stop early for them.
    input: Option<Arc<AtomicUsize>>,
    /// `[performance] cached_queries`: most entries kept in `earlier`.
    pub cached_queries: usize,
    pub rows: ranking::Rows,
    pub highlight: Option<PathBuf>,
    /// Where `highlight` was last found in `rows`. Anything may change either,
    /// so it is checked before use; moving and drawing then skip the search.
    highlight_hint: Cell<usize>,
    /// In a newly opened location the highlight stays on the first row while
    /// the scan streams in and the query changes, until the user moves it or
    /// a row to restore (`wanted`) is listed.
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
    /// The scan stopped at `[performance] max_paths`: the rows are partial.
    pub truncated: bool,
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
    /// Alt+d / `d` hide directories and Alt+D / `D` files for now; unlike
    /// `fff seek`, navigation still works. See `listed_kinds` for seek.
    pub kinds: Kinds,
    /// `fff pick` and `fff seek`: rows show bare names, with only the parents
    /// that tell apart entries sharing one (`labels`).
    pub short_labels: bool,
    /// Short row labels for entries whose basename another listed entry shares
    /// (presentation only); other rows show their bare name.
    pub labels: HashMap<PathBuf, PathBuf>,
    /// With labels shown: listed entries by basename. Labels only depend on the
    /// entries sharing a basename, so a change relabels just the groups it touches.
    names: HashMap<OsString, Vec<Arc<Candidate>>>,
    /// Basenames whose groups changed and wait to be relabelled, in order
    /// and as a set, so a group changed again meanwhile is queued once.
    relabel: Vec<OsString>,
    relabelling: HashSet<OsString>,
    /// Home directory, shown as `~` in the location line (presentation only).
    pub home: Option<PathBuf>,
    /// Status messages clear after this long; `None` keeps them until replaced.
    pub status_timeout: Option<std::time::Duration>,
    /// Each directory's last highlighted entry, least recently left first.
    pub positions: Vec<(PathBuf, PathBuf)>,
    /// A highlight to restore as soon as its row is listed (scans stream in),
    /// unless the user moves first.
    pub wanted: Option<PathBuf>,
    /// `fff roam`: the session is saved when it ends, for `--resume`.
    pub records_session: bool,
    /// `[open]`: which program opens a file.
    pub openers: open::OpenConfig,
    /// `fff pick`: Insert-mode Enter on a file returns it instead of
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
    /// The preview pane (Alt+v, `P`) and what it shows.
    pub preview: preview::Pane,
    /// `[ui] watch`: follow changes to the current directory as they happen.
    pub watch: bool,
    /// `fff explorer`: the file dialog being answered. Enter chooses files in
    /// either mode and q cancels.
    pub dialog: Option<explorer::Dialog>,
    /// `--window`: fff has a terminal window of its own and no shell to leave
    /// into, so Enter enters folders and `!` starts a shell in this one.
    pub window: bool,
    /// `[trash]`: where `x` sends entries; `None` deletes them permanently.
    pub trash: Option<trash::Trash>,
    /// This session's file operations that `u` can undo, oldest first.
    pub undo: Vec<operations::undo::Record>,
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
            known: known::Known::default(),
            known_version: 0,
            added: Vec::new(),
            outdated: Vec::new(),
            refreshed: None,
            earlier: Vec::new(),
            ranking: None,
            waker: None,
            input: None,
            cached_queries: config::PerformanceConfig::default().cached_queries,
            rows: Arc::default(),
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
            truncated: false,
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
            kinds: Kinds::All,
            short_labels: false,
            labels: HashMap::new(),
            names: HashMap::new(),
            relabel: Vec::new(),
            relabelling: HashSet::new(),
            home: None,
            status_timeout: Some(std::time::Duration::from_secs(4)),
            positions: Vec::new(),
            wanted: None,
            records_session: false,
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
            trash: None,
            undo: Vec::new(),
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
    /// The session as it is, for `--resume`.
    pub fn session(&self) -> state::Session {
        let mut tabs = self.tabs.clone();
        tabs[self.tab] = Tab {
            cwd: self.cwd.clone(),
            back: self.back.clone(),
            forward: self.forward.clone(),
        };
        state::Session {
            roots: self.roots.clone(),
            tabs,
            tab: self.tab,
            highlight: self.highlight.clone(),
            marks: self.marks.to_vec(),
            query: self.query.clone(),
            depth: self.depth,
            filters: self.policy.filters.clone(),
            rules: self.policy.rules.clone(),
            ignore: self.policy.ignore,
            hidden: self.policy.hidden,
            kinds: self.kinds,
            preview: self.preview.shown,
        }
    }
    /// `--resume`: continues a saved session. Vanished directories fall back
    /// to their nearest existing parent and vanished roots are dropped; marks
    /// are kept, missing ones flagged as usual.
    pub fn restore(&mut self, session: state::Session) {
        let existing = |mut dir: PathBuf| {
            while !dir.is_dir() {
                match dir.parent() {
                    Some(parent) => dir = parent.to_path_buf(),
                    None => break,
                }
            }
            dir
        };
        self.roots = session.roots.into_iter().filter(|r| r.is_dir()).collect();
        self.tab = session.tab.min(TABS - 1);
        let first = session.tabs[0].cwd.clone();
        for (i, slot) in self.tabs.iter_mut().enumerate() {
            *slot = session.tabs.get(i).cloned().unwrap_or(Tab {
                cwd: first.clone(),
                ..Tab::default()
            });
        }
        let Tab { cwd, back, forward } = std::mem::take(&mut self.tabs[self.tab]);
        self.go(existing(cwd), false);
        self.back = back;
        self.forward = forward;
        self.depth = session.depth.max(1);
        self.policy.hidden = session.hidden;
        self.policy.ignore = session.ignore;
        // Globs were valid when saved; one that no longer is drops its list.
        let _ = self.policy.set_rules(session.rules);
        let _ = self.policy.set_filters(session.filters);
        self.kinds = session.kinds;
        self.preview.shown = session.preview;
        self.query = session.query;
        self.marks = session.marks.into();
        self.wanted = session.highlight;
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
        // Rows stream in: the highlight stays on the first one until moved.
        self.highlight_top = true;
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
        eligible(self.depth, &self.scope, &self.cwd, &self.policy, c)
    }
    /// A path the scan of `generation` delivered.
    pub fn insert(&mut self, generation: u64, candidate: Candidate) {
        if generation == self.generation {
            self.place(candidate, Some(generation));
        }
    }
    /// A path replayed from earlier results, until a scan delivers it again.
    pub(crate) fn replay(&mut self, candidate: Candidate) {
        self.place(candidate, None);
    }
    fn place(&mut self, mut candidate: Candidate, delivered: Option<u64>) {
        if candidate.path == self.cwd {
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
            // Rescans deliver mostly what is already known: keep those rows.
            if self
                .known
                .get(&candidate.path)
                .is_some_and(|c| **c == candidate)
            {
                if let Some(generation) = delivered {
                    self.known.confirm(&candidate.path, generation);
                }
                return;
            }
            let candidate = Arc::new(candidate);
            // Replays are unconfirmed: no scan has generation 0.
            let stamp = delivered.unwrap_or(0);
            if let Some(old) = self.known.insert(Arc::clone(&candidate), stamp) {
                self.outdated.push(old);
            }
            self.added.push(candidate);
        }
    }
    /// The scan of `generation` completed: forget the paths it did not
    /// deliver. The next refresh drops their rows.
    pub(crate) fn reconcile(&mut self, generation: u64) {
        let forgotten = self.known.keep_stamped(generation);
        self.outdated.extend(forgotten);
    }
    /// Rows an empty query would list: eligible, and passing `fff seek` (files only) and the
    /// file filters, which hide directories whenever any is set.
    fn listed(&self, c: &Candidate) -> bool {
        self.eligible(c) && shown(self.listed_kinds(), &self.policy, c)
    }
    /// The kinds listed: `fff seek` lists only files whatever `kinds` says.
    pub fn listed_kinds(&self) -> Kinds {
        if self.no_dir {
            Kinds::Files
        } else {
            self.kinds
        }
    }
    /// What decides which known paths are listed, for ranking elsewhere.
    fn listing(&self) -> Listing {
        Listing {
            depth: self.depth,
            scope: self.scope.clone(),
            cwd: self.cwd.clone(),
            policy: self.policy.clone(),
            kinds: self.listed_kinds(),
        }
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
        self.refresh_within(None);
    }
    /// Like `refresh`, but merge insertions only for about `budget` when the
    /// query and everything else the rows depend on are unchanged; the rest
    /// wait for the next refresh (see `unmerged`). With a waker set, ranking a
    /// large listing again happens on its own thread: the previous rows stay
    /// until it is done (see `ranking_pending` and `finish_ranking`).
    pub fn refresh_within(&mut self, budget: Option<Duration>) {
        let basis = RefreshBasis::of(self);
        let folded = self.filter().to_lowercase();
        if let Some(pending) = &self.ranking {
            if pending.basis == basis && pending.query == folded {
                match pending.result.try_recv() {
                    Ok(ranked) => {
                        let pending = self.ranking.take().expect("checked");
                        self.install(ranked, pending.basis, pending.query);
                    }
                    Err(mpsc::TryRecvError::Empty) => return,
                    // The ranking thread failed: rank here instead.
                    Err(mpsc::TryRecvError::Disconnected) => {
                        self.ranking = None;
                        self.refreshed = None;
                        let waker = self.waker.take();
                        self.refresh_within(budget);
                        self.waker = waker;
                        return;
                    }
                }
            } else {
                self.cancel_ranking();
            }
        }
        let listing_changed =
            !self.added.is_empty() || !self.outdated.is_empty() || !self.relabel.is_empty();
        // Rows already match the previous query under the same basis. A query
        // that only appends letters other than `/` (after lowercasing, as `score`
        // does; see `narrows`) can only remove matches, so re-score those rows
        // instead of every known path.
        // Deleting letters returns to a shorter query's kept rows, or narrows
        // from the longest kept query the new one still extends.
        let base = match self.refreshed.take() {
            Some((previous, query)) if previous == basis => {
                if listing_changed {
                    // Kept queries' rows predate the change; only the current
                    // ones are brought up to date.
                    let kept = self.earlier.iter().map(|(_, rows)| rows.len()).sum();
                    known::discard(std::mem::take(&mut self.earlier), kept);
                    let budget = budget.filter(|_| query == folded);
                    self.merge(&query, budget);
                }
                if query == folded {
                    self.refreshed = Some((previous, query));
                    self.keep_highlight();
                    return;
                }
                if narrows(&query, &folded) {
                    if self.cached_queries > 0 {
                        self.earlier.push((query, Arc::clone(&self.rows)));
                        if self.earlier.len() > self.cached_queries {
                            self.earlier.remove(0);
                        }
                    }
                    Some(Arc::clone(&self.rows))
                } else {
                    while self
                        .earlier
                        .last()
                        .is_some_and(|(q, _)| !narrows(q, &folded))
                    {
                        self.earlier.pop();
                    }
                    if let Some((q, _)) = self.earlier.last()
                        && *q == folded
                    {
                        self.rows = self.earlier.pop().expect("checked").1;
                        self.refreshed = Some((basis, folded));
                        self.keep_highlight();
                        return;
                    }
                    self.earlier.last().map(|(_, rows)| Arc::clone(rows))
                }
            }
            _ => {
                // Ranking every known path covers what was added or forgotten,
                // and relabels it all.
                let kept = self.earlier.iter().map(|(_, rows)| rows.len()).sum();
                known::discard(std::mem::take(&mut self.earlier), kept);
                let added = self.added.len();
                known::discard(std::mem::take(&mut self.added), added);
                let outdated = self.outdated.len();
                known::discard(std::mem::take(&mut self.outdated), outdated);
                self.relabel.clear();
                self.relabelling.clear();
                let job = ranking::Job {
                    base: Arc::new(self.known.values().cloned().collect()),
                    listing: Some(self.listing()),
                    query: folded.clone(),
                    labels: Some(self.no_dir || self.short_labels),
                };
                self.start_ranking(job, basis, folded);
                return;
            }
        };
        let job = match base {
            Some(base) => ranking::Job {
                base,
                listing: None,
                query: folded.clone(),
                labels: None,
            },
            None => ranking::Job {
                base: Arc::new(self.known.values().cloned().collect()),
                listing: Some(self.listing()),
                query: folded.clone(),
                labels: None,
            },
        };
        self.start_ranking(job, basis, folded);
    }
    /// Run `job` here when it is small (or nothing can be woken when it is
    /// done), otherwise on its own thread.
    fn start_ranking(&mut self, job: ranking::Job, basis: RefreshBasis, query: String) {
        if job.base.len() >= ranking::BACKGROUND_AT
            && let Some(waker) = self.waker.clone()
        {
            let cancel = Arc::new(AtomicBool::new(false));
            let cancelled = Arc::clone(&cancel);
            let (sender, result) = mpsc::channel();
            let spawned = std::thread::Builder::new().spawn(move || {
                if let Some(ranked) = job.run(&cancelled) {
                    let _ = sender.send(ranked);
                    let _ = waker.try_send(());
                }
            });
            if spawned.is_ok() {
                self.ranking = Some(Pending {
                    basis,
                    query,
                    result,
                    cancel,
                });
                return;
            }
            // Without a thread, rank here after all.
            self.refreshed = None;
            let waker = self.waker.take();
            self.refresh();
            self.waker = waker;
            return;
        }
        let ranked = job
            .run(&AtomicBool::new(false))
            .expect("an uncancelled ranking completes");
        self.install(ranked, basis, query);
    }
    /// Show the rows (and labels) a ranking produced.
    fn install(&mut self, ranked: ranking::Ranked, basis: RefreshBasis, query: String) {
        let old = std::mem::replace(&mut self.rows, Arc::new(ranked.rows));
        let len = old.len();
        known::discard(old, len);
        if let Some((names, labels)) = ranked.labels {
            let old = std::mem::replace(&mut self.names, names);
            let len = old.values().map(Vec::len).sum();
            known::discard(old, len);
            let old = std::mem::replace(&mut self.labels, labels);
            let len = old.len();
            known::discard(old, len);
        }
        self.refreshed = Some((basis, query));
        self.keep_highlight();
    }
    fn cancel_ranking(&mut self) {
        if let Some(pending) = self.ranking.take() {
            pending.cancel.store(true, Ordering::Relaxed);
        }
    }
    /// The rows are being ranked again on another thread; meanwhile they are
    /// those of the previous query or settings.
    pub fn ranking_pending(&self) -> bool {
        self.ranking.is_some()
    }
    /// Wait for a pending ranking, so that what acts on rows sees current ones.
    pub fn finish_ranking(&mut self) {
        let Some(pending) = self.ranking.take() else {
            return;
        };
        match pending.result.recv() {
            Ok(ranked) => self.install(ranked, pending.basis, pending.query),
            Err(_) => {
                self.refreshed = None;
                let waker = self.waker.take();
                self.refresh();
                self.waker = waker;
            }
        }
    }
    /// Install a ranking that is done; whether it was.
    pub fn poll_ranking(&mut self) -> bool {
        if self.ranking.is_none() {
            return false;
        }
        self.refresh_within(Some(Duration::from_millis(4)));
        self.ranking.is_none()
    }
    /// Budgeted merges stop early while `waiting` counts unread input.
    pub fn yield_to(&mut self, waiting: Arc<AtomicUsize>) {
        self.input = Some(waiting);
    }
    /// A budgeted merge should stop: its time is up or input waits.
    fn yield_now(&self, budget: Option<Duration>, start: Instant) -> bool {
        budget.is_some_and(|b| {
            start.elapsed() >= b
                || self
                    .input
                    .as_ref()
                    .is_some_and(|waiting| waiting.load(Ordering::Relaxed) > 0)
        })
    }
    /// Wake `waker` when a ranking on another thread is done.
    pub fn set_waker(&mut self, waker: mpsc::SyncSender<()>) {
        self.waker = Some(waker);
    }
    /// Wakes the session loop, e.g. when background work finishes.
    pub fn waker(&self) -> Option<mpsc::SyncSender<()>> {
        self.waker.clone()
    }
    /// Bring the rows ranked for `query` up to date with the candidates
    /// forgotten and, within `budget`, those inserted since: work in proportion
    /// to the change plus one pass over the row pointers, rather than ranking
    /// every known path. Insertions left over wait for the next refresh.
    fn merge(&mut self, query: &str, budget: Option<Duration>) {
        let start = Instant::now();
        // Candidates replaced or forgotten; `outdated` keeps their addresses
        // unique until the rows are rid of them.
        let stale: HashSet<_> = self.outdated.iter().map(Arc::as_ptr).collect();
        if !stale.is_empty() {
            Arc::make_mut(&mut self.rows).retain(|c| !stale.contains(&Arc::as_ptr(c)));
            if self.no_dir || self.short_labels {
                for c in &self.outdated {
                    if let Some(name) = c.path.file_name()
                        && let Some(group) = self.names.get_mut(name)
                    {
                        group.retain(|g| !Arc::ptr_eq(g, c));
                        self.labels.remove(&c.path);
                        if self.relabelling.insert(name.to_os_string()) {
                            self.relabel.push(name.to_os_string());
                        }
                    }
                }
            }
        }
        // Only the latest candidate inserted for a path is still known.
        self.added.retain(|c| !stale.contains(&Arc::as_ptr(c)));
        let stale = std::mem::take(&mut self.outdated);
        let len = stale.len();
        known::discard(stale, len);
        let matcher = Matcher::new(query);
        let score_of = |c: &Candidate| matcher.score_candidate(c);
        // In budgeted rounds, each about a millisecond, at least one per call.
        while !self.added.is_empty() {
            let take = if budget.is_some() {
                self.added.len().min(MERGE_ROUND)
            } else {
                self.added.len()
            };
            // Oldest first: scans deliver shallow paths before deeper ones,
            // and the rows fill in that order.
            let round: Vec<_> = self.added.drain(..take).collect();
            let listed: Vec<_> = round.into_iter().filter(|c| self.listed(c)).collect();
            if self.no_dir || self.short_labels {
                for c in &listed {
                    if let Some(name) = c.path.file_name() {
                        let group = self.names.entry(name.to_os_string()).or_default();
                        group.push(Arc::clone(c));
                        if self.relabelling.insert(name.to_os_string()) {
                            self.relabel.push(name.to_os_string());
                        }
                    }
                }
            }
            let mut added: Vec<_> = listed
                .into_iter()
                .filter_map(|c| Some((score_of(&c)?, c)))
                .collect();
            added.sort_unstable_by(|(sa, a), (sb, b)| rank(a, *sa, b, *sb));
            let mut rows = Vec::with_capacity(self.rows.len() + added.len());
            let old = std::mem::take(&mut self.rows);
            let mut old = Arc::try_unwrap(old)
                .unwrap_or_else(|shared| Vec::clone(&shared))
                .into_iter();
            for (score, c) in added {
                // Rows all match the query; rank them by their own scores.
                let before = old.as_slice().partition_point(|r| {
                    rank(r, score_of(r).unwrap_or_default(), &c, score).is_lt()
                });
                rows.extend(old.by_ref().take(before));
                rows.push(c);
            }
            rows.extend(old);
            self.rows = Arc::new(rows);
            if self.yield_now(budget, start) {
                break;
            }
        }
        // Relabel with what is left of the budget, at least one group per
        // call.
        while let Some(name) = self.relabel.pop() {
            self.relabelling.remove(&name);
            if let Some(group) = self.names.get(&name) {
                for c in group {
                    self.labels.remove(&c.path);
                }
                if group.is_empty() {
                    self.names.remove(&name);
                } else if group.len() > 1 {
                    self.labels
                        .extend(disambiguate(group.iter().map(|c| c.path.as_path())));
                }
            }
            if self.yield_now(budget, start) {
                break;
            }
        }
    }
    /// Inserted candidates the rows do not reflect yet.
    pub fn unmerged(&self) -> usize {
        self.added.len()
    }
    /// The rows and their labels reflect every known path.
    pub fn settled(&self) -> bool {
        self.added.is_empty() && self.relabel.is_empty() && self.ranking.is_none()
    }
    fn keep_highlight(&mut self) {
        // Rows share the known candidates: look for the wanted one's address.
        // A row to restore takes the highlight off the first row once listed.
        if let Some(wanted) = self.wanted.as_ref().and_then(|w| self.known.get(w))
            && self.rows.iter().any(|c| Arc::ptr_eq(c, wanted))
        {
            self.highlight = self.wanted.take();
            self.highlight_top = false;
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
        // Comparing addresses avoids reading every row's path.
        let i = match self.known.get(path) {
            Some(known) => self.rows.iter().position(|c| Arc::ptr_eq(c, known)),
            None => None,
        }
        .or_else(|| self.rows.iter().position(|c| &c.path == path))?;
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
    /// Insert-mode Enter on a file (outside fff pick): show it in its
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
                self.wanted = None;
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
