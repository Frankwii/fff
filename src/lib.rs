//! Fuzzy file finder state, independent of terminal input and filesystem timing.
mod archive;
mod cache;
pub mod completion;
pub mod config;
pub mod dbus;
pub mod details;
pub mod discovery;
pub mod editor;
pub mod explorer;
pub mod grep;
pub mod highlight;
pub mod icons;
mod index;
pub mod keymap;
pub mod known;
pub mod location;
pub mod marks;
mod matching;
pub mod mounts;
pub mod notify;
pub mod open;
pub mod operations;
mod palettes;
pub mod piped;
mod platform;
mod policy;
mod prefetch;
pub mod preview;
pub mod program;
mod ranking;
pub mod reload;
pub mod rows;
pub mod run;
pub mod setup;
pub mod sizes;
pub mod state;
pub mod terminal;
pub mod trash;
pub mod ui;
mod view;
// inotify is Linux's own; elsewhere fff works without live updates.
#[cfg_attr(not(target_os = "linux"), path = "platform/watch_none.rs")]
pub mod watch;
pub mod workspaces;

use matching::narrows;
pub use matching::{Matcher, relative_parts, score};
pub(crate) use policy::Listing;
pub use policy::{Kinds, Policy, glob};
use policy::{eligible, shown};
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
    /// Its place in a list read from stdin (`Program::Piped`), whose order
    /// ranks the rows a query scores alike; 0 elsewhere.
    pub order: u32,
}

/// A ranking running on another thread, for this basis and query.
struct Pending {
    basis: RefreshBasis,
    query: String,
    result: mpsc::Receiver<ranking::Ranked>,
    cancel: Arc<AtomicBool>,
    /// What the rows on screen were ranked for, restored on cancelling so
    /// that the next query narrows them instead of ranking (and labelling)
    /// every known path again.
    shown: Option<(RefreshBasis, String)>,
}

/// How long the progress marker stays after discovery completes.
pub const PROGRESS_SHOWN: std::time::Duration = std::time::Duration::from_secs(3);

/// New paths filtered, scored and sorted per budgeted round; each round
/// becomes a run of the rows.
const MERGE_ROUND: usize = 1024;

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
    filters_off: bool,
    navigates: bool,
    kinds: Kinds,
    short_labels: bool,
    order: rows::Order,
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
            filters_off: app.policy.filters_off,
            navigates: app.program().navigates(),
            kinds: app.kinds,
            short_labels: app.short_labels,
            order: app.order(),
        }
    }
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
/// The row of `rows` drawn ones that Alt+number (or Ctrl+number) `digit`
/// reaches: 1 the first, 0 the last, a number past them the last too.
pub fn numbered_row(digit: u32, rows: usize) -> usize {
    (digit as usize)
        .checked_sub(1)
        .unwrap_or(usize::MAX)
        .min(rows.saturating_sub(1))
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
    earlier: Vec<(String, rows::Rows)>,
    /// Rows being ranked on another thread, and what wakes the UI when done.
    ranking: Option<Pending>,
    waker: Option<mpsc::SyncSender<()>>,
    /// Input events waiting to be read: budgeted merges stop early for them.
    input: Option<Arc<AtomicUsize>>,
    /// `[performance] cached_queries`: most entries kept in `earlier`.
    pub cached_queries: usize,
    /// The ranked rows with their scores, as sorted runs merged lazily
    /// (`rows::Rows`): read every row in order through `shown` (or
    /// `Rows::all`).
    pub rows: rows::Rows,
    pub highlight: Option<PathBuf>,
    /// Where `highlight` was last found in `rows`. Anything may change either,
    /// so it is checked before use; moving and drawing then skip the search.
    highlight_hint: Cell<usize>,
    /// In a newly opened location the highlight stays on the first row while
    /// the scan streams in and the query changes, until the user moves it or
    /// a row to restore (`wanted`) is listed.
    pub highlight_top: bool,
    /// The (lowercased) query the highlight was placed under: rows for
    /// another one, as typing changes it, highlight the best match.
    highlight_query: String,
    /// The program the session runs as, with what only it keeps: a file
    /// dialog's question. It decides what is listed, what Enter finishes
    /// with and what is printed (`program::Program`); `set_program` and
    /// `open_dialog` change it.
    pub running: program::Running,
    /// The contents query, its search and the rows it shows: one per match
    /// while it is set. Every program but the dialogs has it, and it stays
    /// when `switch_program` changes program.
    pub grep: grep::Grep,
    /// Counts changes to `rows`, for what is built from them.
    rows_version: u64,
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
    /// fff explorer save: the path whose extension the status line warned
    /// about; saving it again takes it.
    pub save_warned: Option<PathBuf>,
    pub physical_empty: bool,
    /// The scan stopped at `[performance] max_paths`: the rows are partial.
    pub truncated: bool,
    /// The scan running is a quiet refresh that keeps the rows (periodic or
    /// after a change on disk): the progress marker stays hidden for it.
    pub quiet_scan: bool,
    /// When discovery and sorting last finished; `None` while they run. The
    /// progress marker shows until `PROGRESS_SHOWN` after it.
    pub settled_at: Option<std::time::Instant>,
    pub cache_pending: bool,
    pub edit_view: Option<editor::EditView>,
    pub palette: config::Palette,
    pub workspaces: Vec<config::Workspace>,
    pub workspace_store: workspaces::Store,
    /// Draw Nerd Font filetype icons before result and marked names.
    pub icons: bool,
    /// Active palette name and the palettes offered by the Alt+p chooser.
    pub palette_name: String,
    pub palettes: Vec<(String, config::Palette)>,
    /// With a contents query, the highlighted match's file as lines, once
    /// read, for its preview to scroll past the match's context.
    pub match_text: Option<(PathBuf, std::sync::Arc<grep::Lines>)>,
    /// Every key binding: the defaults with `keys.toml` applied.
    pub keymap: keymap::Keymap,
    /// The keys of a sequence begun and not finished yet.
    pub pending: Vec<crossterm::event::KeyEvent>,
    /// `keys.toml`, which the Keys screen writes; `None` with `--no-config`.
    pub keys_path: Option<PathBuf>,
    /// Why `keys.toml` was not read, while the defaults stand in for it.
    pub keys_broken: Option<String>,
    /// Alt+d / `d` hide directories and Alt+D / `D` files for now; unlike
    /// `fff seek`, navigation still works. See `listed_kinds` for seek.
    pub kinds: Kinds,
    /// Rows show bare names, with only the parents that tell apart entries
    /// sharing one (`labels`), rather than their path below the folder
    /// searched: `Program::short_labels` to start, `toggle_labels` after.
    pub short_labels: bool,
    /// The sorting picker's choice (`[ui] sort` to start): the matches
    /// sorted by it rather than as the program ranks them (`order`).
    pub sort: Option<rows::Sort>,
    /// What depth costs a match in a sort by a penalty (`[ui] temperature`).
    pub temperature: f64,
    /// What `short_labels` was when the rows on screen were last labelled:
    /// while another ranking labels them again, they keep their labels.
    shown_labels: Option<bool>,
    /// `toggle_labels` chose the labels: `switch_program` keeps them.
    pub labels_chosen: bool,
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
    /// `[open]`: which program opens a file.
    pub openers: open::OpenConfig,
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
    /// `[ui] buttons` and `[ui] buttons_seconds`, and when the mouse last
    /// moved or clicked.
    pub buttons: config::Buttons,
    pub buttons_timeout: Option<std::time::Duration>,
    pub mouse_seen: Option<std::time::Instant>,
    /// The preview pane (Alt+v, `P`) and what it shows.
    pub preview: preview::Pane,
    /// `[ui] watch`: follow changes to the current directory as they happen.
    pub watch: bool,
    /// `[trash]`: where `x` sends entries; `None` deletes them permanently.
    pub trash: Option<trash::Trash>,
    /// The trash's `files/` folders, home bin first, as last read
    /// (`refresh_trash`): where R restores.
    pub trash_folders: Vec<PathBuf>,
    /// This session's file operations that `u` can undo, in the order they
    /// started.
    pub undo: Vec<operations::undo::Record>,
    /// File operations and undos running on threads of their own, in the
    /// order they started (the jobs list).
    pub(crate) jobs: Vec<ui::Job>,
    /// How many operations this session started: each one's `order`.
    pub(crate) started: u64,
    /// The configuration file `gc` edits; `None` with `--no-config`.
    pub config_path: Option<PathBuf>,
    /// The configuration file's errors while its backup is read instead.
    pub config_broken: Option<String>,
    /// `--no-icons`, which outranks the configuration each time it is read
    /// again.
    pub no_icons: bool,
    /// The configuration's exclusion rules (`[exclusions]`), which lead the
    /// session's; `gc` replaces them when they change there.
    pub config_rules: Vec<String>,
}

impl App {
    /// Choose among the paths `piped` reads, relative ones below `cwd`.
    /// Every path is below `/`, the one root, and none is hidden or
    /// excluded: the list is listed as given.
    pub fn piped(cwd: PathBuf, piped: piped::Piped) -> Self {
        let policy = Policy {
            hidden: true,
            ..Policy::default()
        };
        let mut app = Self::browse(cwd, program::Program::Piped.default_depth(), policy);
        app.scope = vec![PathBuf::from("/")];
        app.run(program::Running::Piped(Box::new(piped)));
        app
    }
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
            rows: rows::Rows::default(),
            highlight: None,
            highlight_top: false,
            highlight_query: String::new(),
            running: program::Running::Roam,
            grep: grep::Grep::new(),
            rows_version: 0,
            highlight_hint: Cell::new(0),
            marks: marks::Marks::default(),
            rendered: Vec::new(),
            offset: 0,
            scrolled: None,
            generation: 0,
            scanning: true,
            status: String::new(),
            status_success: String::new(),
            save_warned: None,
            physical_empty: false,
            truncated: false,
            quiet_scan: false,
            settled_at: None,
            cache_pending: false,
            edit_view: None,
            palette: config::Palette::default(),
            workspaces: Vec::new(),
            workspace_store: workspaces::Store::default(),
            icons: true,
            palette_name: "midnight".into(),
            palettes: config::Palette::names()
                .iter()
                .map(|n| (n.to_string(), config::Palette::named(n).unwrap()))
                .collect(),
            keymap: keymap::Keymap::default(),
            match_text: None,
            pending: Vec::new(),
            keys_path: None,
            keys_broken: None,
            kinds: Kinds::All,
            short_labels: false,
            sort: None,
            temperature: rows::TEMPERATURE,
            shown_labels: None,
            labels_chosen: false,
            labels: HashMap::new(),
            names: HashMap::new(),
            relabel: Vec::new(),
            relabelling: HashSet::new(),
            home: None,
            status_timeout: Some(std::time::Duration::from_secs(4)),
            positions: Vec::new(),
            wanted: None,
            openers: open::OpenConfig::default(),
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
            buttons_timeout: Some(std::time::Duration::from_secs(5)),
            mouse_seen: None,
            watch: true,
            preview: preview::Pane::default(),
            trash: None,
            trash_folders: Vec::new(),
            undo: Vec::new(),
            jobs: Vec::new(),
            started: 0,
            config_path: None,
            config_broken: None,
            no_icons: false,
            config_rules: Vec::new(),
        }
    }
    /// The button bar is shown: always, or while the mouse is in use.
    pub fn buttons_shown(&self) -> bool {
        self.buttons_shown_at(std::time::Instant::now())
    }
    pub fn buttons_shown_at(&self, now: std::time::Instant) -> bool {
        match self.buttons {
            config::Buttons::Off => false,
            config::Buttons::Mouse => self
                .mouse_seen
                .is_some_and(|seen| self.buttons_timeout.is_none_or(|t| now < seen + t)),
            config::Buttons::Always => true,
        }
    }
    /// When a resting mouse hides the bar of `buttons = "mouse"`.
    pub fn buttons_hide(&self) -> Option<std::time::Instant> {
        (self.buttons == config::Buttons::Mouse)
            .then(|| Some(self.mouse_seen? + self.buttons_timeout?))
            .flatten()
    }
    /// Records the current directory's highlight, as most recently used.
    /// A list's rows are not the directory's, so its highlight is not.
    pub fn remember(&mut self) {
        let Some(highlight) = self.highlight.clone() else {
            return;
        };
        if self.program() == program::Program::Piped {
            return;
        }
        self.positions.retain(|(dir, _)| *dir != self.cwd);
        self.positions.push((self.cwd.clone(), highlight));
        let excess = self.positions.len().saturating_sub(state::CAPACITY);
        self.positions.drain(..excess);
    }
    pub fn remembered(&self, dir: &Path) -> Option<PathBuf> {
        if self.program() == program::Program::Piped {
            return None;
        }
        self.positions
            .iter()
            .rev()
            .find(|(d, _)| d == dir)
            .map(|(_, entry)| entry.clone())
    }
    /// Run as `program`, with its labels (a file dialog is `open_dialog`).
    pub fn set_program(&mut self, program: crate::program::Program) {
        if program == self.program() {
            return;
        }
        self.run(program::Running::new(program));
    }
    /// Run as `running`. What the program before kept goes with it.
    fn run(&mut self, running: program::Running) {
        if !self.labels_chosen {
            self.short_labels = running.program().short_labels();
        }
        self.running = running;
    }
    /// The contents query's bar is drawn: while it is typed into or set.
    pub fn contents_shown(&self) -> bool {
        self.grep.editing || !self.grep.text.is_empty()
    }
    /// Rows show short labels (`labels`) rather than paths: `short_labels`,
    /// once a ranking has labelled the rows for it.
    pub fn labels_shown(&self) -> bool {
        self.shown_labels.unwrap_or(self.short_labels)
    }
    /// Typing goes into the contents query, not the names query.
    pub fn typing_contents(&self) -> bool {
        self.grep.editing
    }
    /// The program the session runs as.
    pub fn program(&self) -> crate::program::Program {
        self.running.program()
    }
    /// `toggle_labels`: rows labelled by name (with the parents that tell
    /// same-named entries apart) or by their path below the folder searched.
    /// The choice outlasts `switch_program`.
    pub fn toggle_labels(&mut self) {
        self.short_labels = !self.short_labels;
        self.labels_chosen = true;
        self.refresh();
        self.status = if self.short_labels {
            "Rows show their names".into()
        } else {
            "Rows show their paths".into()
        };
        self.status_success = self.status.clone();
    }
    /// Answer `dialog`, as `fff explorer` does.
    pub fn open_dialog(&mut self, dialog: explorer::Dialog) {
        self.run(program::Running::Dialog(dialog));
    }
    /// `switch_program`: go on as `to` (one of `Program::SWITCHABLE`). The
    /// folders, query, highlight, marks and policies stay; the depth
    /// follows the new program's default unless it was set to another, and
    /// the contents query stays. True when
    /// the depth changed, which needs a scan; else the rows are re-ranked.
    pub fn switch_program(&mut self, to: crate::program::Program) -> bool {
        use keymap::Dimension;
        let from = self.program();
        if from == to {
            return false;
        }
        self.set_program(to);
        // The highlight stays, or comes back once listed again (a folder
        // after `fff seek`, which lists only files), unless the user moves.
        self.wanted = self.highlight.clone().or(self.wanted.take());
        self.highlight_top = false;
        self.offset = 0;
        self.status = format!("Now fff {}", to.name());
        self.status_success = self.status.clone();
        if self.depth == from.default_depth() && self.depth != to.default_depth() {
            self.depth = to.default_depth();
            return true;
        }
        self.refresh();
        self.poll_grep();
        false
    }
    /// The order rows are ranked in: the program's ranking, or the sort
    /// chosen with the ranking breaking its ties.
    pub fn order(&self) -> rows::Order {
        rows::Order {
            rank: self.program().rank(),
            sort: self.sort,
            temperature: self.temperature as f32,
        }
    }
    /// Sort the matches by `sort`, or rank them as the program does with
    /// `None`. Each choice reads the values again (a folder's size added
    /// up since), even when it was the order already.
    pub fn set_sort(&mut self, sort: Option<rows::Sort>) {
        self.sort = sort;
        self.cancel_ranking();
        self.refreshed = None;
        self.refresh();
    }
    /// Every row on screen, in order: the ranked rows, or with a contents
    /// query, one row per match in their files' order. The one way to read
    /// them all, which merges the rows first (`Rows::all`); reading a few
    /// (`shown_row`, `shown_from`, `shown_position`) or counting them
    /// (`shown_len`) does not.
    pub fn shown(&self) -> &[Arc<Candidate>] {
        if self.grep.active() {
            &self.grep.shown
        } else {
            self.rows.all()
        }
    }
    /// How many rows are on screen.
    pub fn shown_len(&self) -> usize {
        if self.grep.active() {
            self.grep.shown.len()
        } else {
            self.rows.len()
        }
    }
    /// Row `i` on screen.
    pub fn shown_row(&self, i: usize) -> Option<&Arc<Candidate>> {
        if self.grep.active() {
            self.grep.shown.get(i)
        } else {
            self.rows.row(i)
        }
    }
    /// The rows on screen in order from row `from` on.
    pub fn shown_from(&self, from: usize) -> Box<dyn Iterator<Item = &Arc<Candidate>> + '_> {
        if self.grep.active() {
            Box::new(self.grep.shown.iter().skip(from))
        } else {
            self.rows.iter_from(from)
        }
    }
    /// The first row on screen `found` holds for.
    pub fn shown_position(&self, found: impl FnMut(&Arc<Candidate>) -> bool) -> Option<usize> {
        if self.grep.active() {
            self.grep.shown.iter().position(found)
        } else {
            self.rows.position(found)
        }
    }
    /// The shown rows' paths, each once (a contents query shows a file once
    /// per match).
    pub fn shown_paths(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        self.shown()
            .iter()
            .filter(|c| seen.insert(&c.path))
            .map(|c| c.path.clone())
            .collect()
    }
    /// The match shown at row `i`, with a contents query.
    pub fn hit(&self, i: usize) -> Option<&grep::Hit> {
        if !self.grep.active() {
            return None;
        }
        self.grep.hit(i)
    }
    /// The highlighted row's match, with a contents query.
    pub fn highlighted_hit(&self) -> Option<&grep::Hit> {
        self.hit(self.highlighted()?)
    }
    /// Catch up with the contents query's search. A new contents query,
    /// like a new names query, highlights the first match until the user
    /// moves.
    pub fn poll_grep(&mut self) -> bool {
        // The highlighted match, by its file's results and its place among
        // them: rows arriving above it move it down.
        let held = self
            .highlighted()
            .and_then(|i| self.grep.shown_hits.get(i))
            .map(|(hits, at)| (Arc::clone(hits), *at));
        if !self.grep.poll(&self.rows, self.rows_version) {
            return false;
        }
        let first = self.shown_row(0).map(|c| c.path.clone());
        if self.highlight_top || self.highlighted().is_none() {
            self.highlight = first;
            self.highlight_hint.set(0);
        } else if let Some((hits, at)) = held
            && let Some(i) = self
                .grep
                .shown_hits
                .iter()
                .position(|(h, a)| *a == at && Arc::ptr_eq(h, &hits))
        {
            self.highlight_hint.set(i);
        }
        true
    }
    /// The contents query changed to `text`.
    pub fn set_contents(&mut self, text: String) {
        self.grep.text = text;
        self.highlight_top = true;
        self.offset = 0;
        self.poll_grep();
    }
    /// The mode keys are read in: Normal in the query's Normal mode (also
    /// while a sequence begun there waits), else Insert.
    pub fn mode(&self) -> keymap::Mode {
        if self.edit_view.as_ref().is_some_and(|v| v.mode == "n") {
            keymap::Mode::Normal
        } else {
            keymap::Mode::Insert
        }
    }
    /// Where a key pressed in the results would apply.
    pub fn cell(&self) -> keymap::Cell {
        use keymap::Highlight;
        keymap::Cell {
            program: self.program(),
            place: keymap::Place::Results,
            highlight: match self.highlighted_row() {
                Some(row) if row.directory => Highlight::Dir,
                Some(_) => Highlight::File,
                None => Highlight::None,
            },
        }
    }
    /// The key the interface names for `function`: one bound in the
    /// current mode that applies here, else in the other mode, else any;
    /// `None` when no key does it.
    pub fn bound(&self, function: keymap::Function) -> Option<String> {
        let mode = self.mode();
        let cell = Some(self.cell());
        self.keymap
            .first(function, mode, cell)
            .or_else(|| self.keymap.first(function, mode.other(), cell))
            .map(|b| b.display())
    }
    /// `bound`, or `—` when no key does it, for prose that names one.
    pub fn bound_or_none(&self, function: keymap::Function) -> String {
        self.bound(function).unwrap_or_else(|| "—".into())
    }
    /// A fixed key (`"enter"`, `"esc"`, `"up"`) as the interface names it.
    pub fn key(&self, name: &str) -> Option<String> {
        Some(keymap::named(name))
    }
    pub fn key_or_none(&self, name: &str) -> String {
        keymap::named(name)
    }
    /// The key `function` has in `mode` that applies here, if any.
    pub fn bound_in(&self, function: keymap::Function, mode: keymap::Mode) -> Option<String> {
        let cell = self.cell();
        self.keymap
            .of(function, mode)
            .into_iter()
            .find(|b| b.applies(cell))
            .map(|b| b.display())
    }
    /// The status while the configuration file has errors, short enough for
    /// the query border: the key that shows them on the Session tab, else
    /// the errors in brief.
    pub fn config_errors_status(&self) -> Option<String> {
        let (path, error) = (self.config_path.as_ref()?, self.config_broken.as_ref()?);
        let name = |path: &std::path::Path| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            )
        };
        let reading = format!(
            "{} has errors, so fff reads {}",
            name(path),
            name(&config::backup_path(path))
        );
        Some(match self.bound(keymap::Function::ShowSession) {
            Some(key) => format!("{reading}; {key} shows them"),
            None => format!(
                "{reading}: {}",
                config::brief(config::config_error(path, error))
            ),
        })
    }
    /// Reads which folders the trash shows again: a bin on another
    /// filesystem appears with its first entry.
    pub fn refresh_trash(&mut self) {
        self.trash_folders = self.trash.as_ref().map_or_else(Vec::new, |trash| {
            trash.bins().into_iter().map(|b| b.join("files")).collect()
        });
    }
    /// Whether the browsing directory is the top of a trash.
    pub fn in_trash(&self) -> bool {
        self.trash_folders.contains(&self.cwd)
    }
    /// Goes to `path`, unless a running operation moves it away or deletes
    /// it (then says so). Returns whether it went.
    pub fn navigate(&mut self, path: PathBuf, retain_query: bool) -> bool {
        if self.leaving(&path) {
            return false;
        }
        if path != self.cwd {
            self.back.push(self.cwd.clone());
            self.forward.clear();
        }
        self.go(path, retain_query);
        true
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
            // Folders a running operation moves away or deletes are skipped too.
            if !target.is_dir() || target == self.cwd || self.leaving_job(&target).is_some() {
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
                (depth > 0 && depth <= self.depth && self.policy.allows(base, relative))
                    .then_some((depth, root))
            })
            .min();
        if let Some((depth, root)) = best {
            candidate.depth = depth;
            candidate.root = root;
            self.store(candidate, delivered);
        }
    }
    /// A path the walker of `generation` found and attributed itself, as
    /// `place` would: to the root it is shallowest below, within the depth
    /// and allowed by the policy, which a new scan follows when they
    /// change.
    pub(crate) fn insert_attributed(&mut self, generation: u64, candidate: Candidate) {
        if generation == self.generation {
            self.store(candidate, Some(generation));
        }
    }
    fn store(&mut self, candidate: Candidate, delivered: Option<u64>) {
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
    /// The scan of `generation` completed: forget the paths it did not
    /// deliver. The next refresh drops their rows.
    pub(crate) fn reconcile(&mut self, generation: u64) {
        let forgotten = self.known.keep_stamped(generation);
        self.outdated.extend(forgotten);
    }
    /// Rows an empty query would list: eligible and `shown`.
    fn listed(&self, c: &Candidate) -> bool {
        self.eligible(c) && shown(self.listed_kinds(), &self.policy, c)
    }
    /// The kinds listed: `fff seek` lists only files whatever `kinds` says.
    pub fn listed_kinds(&self) -> Kinds {
        if self.program().files_only() {
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
        match self.running.dialog() {
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
        let shown = self.refreshed.take();
        let base = match shown.clone() {
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
                    self.keep_highlight(&folded);
                    return;
                }
                if narrows(&query, &folded) {
                    // A cancelled ranking leaves these rows kept already.
                    if self.cached_queries > 0
                        && self.earlier.last().is_none_or(|(q, _)| *q != query)
                    {
                        self.earlier.push((query, self.rows.clone()));
                        if self.earlier.len() > self.cached_queries {
                            self.earlier.remove(0);
                        }
                    }
                    Some(self.rows.clone())
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
                        self.rows_version += 1;
                        self.keep_highlight(&folded);
                        self.refreshed = Some((basis, folded));
                        return;
                    }
                    self.earlier.last().map(|(_, rows)| rows.clone())
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
                    base: vec![Arc::new(rows::Run {
                        rows: self.known.values().cloned().collect(),
                        keys: Vec::new(),
                    })],
                    listing: Some(self.listing()),
                    query: folded.clone(),
                    labels: Some(self.short_labels),
                    order: self.order(),
                    totals: self.details.totals(),
                };
                // This job takes over the additions and removals discarded
                // above, so the rows on screen cannot be narrowed after it.
                self.start_ranking(job, basis, folded, None);
                return;
            }
        };
        let job = match base {
            Some(base) => ranking::Job {
                base: base.runs(),
                listing: None,
                query: folded.clone(),
                labels: None,
                order: self.order(),
                totals: self.details.totals(),
            },
            None => ranking::Job {
                base: vec![Arc::new(rows::Run {
                    rows: self.known.values().cloned().collect(),
                    keys: Vec::new(),
                })],
                listing: Some(self.listing()),
                query: folded.clone(),
                labels: None,
                order: self.order(),
                totals: self.details.totals(),
            },
        };
        self.start_ranking(job, basis, folded, shown);
    }
    /// Run `job` here when it is small (or nothing can be woken when it is
    /// done), otherwise on its own thread.
    fn start_ranking(
        &mut self,
        job: ranking::Job,
        basis: RefreshBasis,
        query: String,
        shown: Option<(RefreshBasis, String)>,
    ) {
        if job.len() >= ranking::BACKGROUND_AT
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
                    shown,
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
        let old = std::mem::replace(&mut self.rows, rows::Rows::sorted(basis.order, ranked.run));
        self.rows_version += 1;
        let len = old.len();
        known::discard(old, len);
        if let Some((names, labels)) = ranked.labels {
            let old = std::mem::replace(&mut self.names, names);
            let len = old.values().map(Vec::len).sum();
            known::discard(old, len);
            let old = std::mem::replace(&mut self.labels, labels);
            let len = old.len();
            known::discard(old, len);
            self.shown_labels = Some(basis.short_labels);
        }
        self.keep_highlight(&query);
        self.refreshed = Some((basis, query));
    }
    fn cancel_ranking(&mut self) {
        if let Some(pending) = self.ranking.take() {
            pending.cancel.store(true, Ordering::Relaxed);
            self.refreshed = self.refreshed.take().or(pending.shown);
        }
    }
    /// The rows are being ranked again on another thread; meanwhile they are
    /// those of the previous query or settings.
    pub fn ranking_pending(&self) -> bool {
        self.ranking.is_some()
    }
    /// Discovery or sorting the user waits for, which the progress marker
    /// reports; quiet refreshes do not count.
    pub fn working(&self) -> bool {
        (self.scanning && !self.quiet_scan) || self.ranking_pending()
    }
    /// The progress marker shows while working, while the rows are partial
    /// and for `PROGRESS_SHOWN` after work settles.
    pub fn progress_shown(&self, now: std::time::Instant) -> bool {
        self.truncated
            || self
                .settled_at
                .is_none_or(|at| now.saturating_duration_since(at) < PROGRESS_SHOWN)
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
        self.grep.set_waker(waker.clone());
        self.waker = Some(waker);
    }
    /// Wakes the session loop, e.g. when background work finishes.
    pub fn waker(&self) -> Option<mpsc::SyncSender<()>> {
        self.waker.clone()
    }
    /// Bring the rows ranked for `query` up to date with the candidates
    /// forgotten and, within `budget`, those inserted since: each round of
    /// insertions is scored, sorted and pushed as a run of the rows
    /// (`Rows::push`), so its cost is about sorting it, rather than ranking
    /// every known path. Forgotten ones cost a pass over the rows.
    /// Insertions left over wait for the next refresh.
    fn merge(&mut self, query: &str, budget: Option<Duration>) {
        let start = Instant::now();
        // Candidates replaced or forgotten; `outdated` keeps their addresses
        // unique until the rows are rid of them.
        let stale: HashSet<_> = self.outdated.iter().map(Arc::as_ptr).collect();
        if !stale.is_empty() {
            self.rows_version += 1;
            self.rows.retain(|c| !stale.contains(&Arc::as_ptr(c)));
            if self.short_labels {
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
        let letters = matcher.letters();
        let order = self.order();
        let totals = self.details.totals();
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
            if self.short_labels {
                for c in &listed {
                    if let Some(name) = c.path.file_name() {
                        // A name is copied only for a group or a queue it
                        // is not in yet. A new group of one has no label to
                        // make (removing a path removes its label).
                        match self.names.get_mut(name) {
                            Some(group) => group.push(Arc::clone(c)),
                            None => {
                                self.names.insert(name.to_os_string(), vec![Arc::clone(c)]);
                                continue;
                            }
                        }
                        if !self.relabelling.contains(name) {
                            self.relabelling.insert(name.to_os_string());
                            self.relabel.push(name.to_os_string());
                        }
                    }
                }
            }
            let mut added: Vec<_> = listed
                .into_iter()
                .filter_map(|c| Some((order.key(&c, score_of(&c)?, letters, &totals), c)))
                .collect();
            added.sort_unstable_by(|(sa, a), (sb, b)| order.cmp(a, *sa, b, *sb));
            if !added.is_empty() {
                self.rows_version += 1;
                // A run of its own: rows ranked already stay where they are.
                let (keys, rows) = added.into_iter().unzip();
                self.rows.push(order, rows::Run { rows, keys });
            }
            if self.yield_now(budget, start) {
                break;
            }
        }
        // Relabel with what is left of the budget, at least one group per
        // call.
        while let Some(name) = self.relabel.pop() {
            self.relabelling.remove(&name);
            if let Some(group) = self.names.get(&name) {
                if group.len() > 1 {
                    // Most labels stay as they were: only those that change
                    // are made again.
                    let paths: Vec<&Path> = group.iter().map(|c| c.path.as_path()).collect();
                    for (path, length) in paths.iter().zip(label_lengths(&paths)) {
                        match self.labels.get_mut(*path) {
                            Some(label) if label.components().count() == length => {}
                            Some(label) => *label = label_of(path, length),
                            None => {
                                self.labels
                                    .insert(path.to_path_buf(), label_of(path, length));
                            }
                        }
                    }
                } else {
                    for c in group {
                        self.labels.remove(&c.path);
                    }
                    if group.is_empty() {
                        self.names.remove(&name);
                    }
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
    /// After the rows change, for `query`: a row to restore once listed,
    /// else the best match when the query changed (typing), else the same
    /// entry (a scan, a new depth), or the first row when it is gone.
    fn keep_highlight(&mut self, query: &str) {
        let typed = self.highlight_query != query;
        if typed {
            self.highlight_query = query.to_owned();
            // A contents query's matches stream in after the files: follow
            // the first.
            self.highlight_top |= self.grep.active();
        }
        // Rows share the known candidates: look for the wanted one's address.
        // A row to restore takes the highlight off the first row once listed.
        if let Some(wanted) = self.wanted.as_ref().and_then(|w| self.known.get(w))
            && self.listed_row(|c| Arc::ptr_eq(c, wanted))
        {
            self.highlight = self.wanted.take();
            self.highlight_top = false;
            return;
        }
        if typed || self.highlight_top || self.highlighted().is_none() {
            self.highlight = self.shown_row(0).map(|c| c.path.clone());
            self.highlight_hint.set(0);
        }
    }
    /// Whether any row on screen, wherever it is, is one `found` holds for.
    fn listed_row(&self, found: impl FnMut(&Arc<Candidate>) -> bool) -> bool {
        if self.grep.active() {
            self.grep.shown.iter().any(found)
        } else {
            self.rows.contains(found)
        }
    }
    pub fn move_by(&mut self, amount: isize) {
        // Moving, even before rows arrive, means the user chose a position.
        self.wanted = None;
        let len = self.shown_len();
        if len == 0 {
            return;
        }
        let i = self.highlighted().unwrap_or(0);
        let i = i.saturating_add_signed(amount).min(len - 1);
        self.highlight = self.shown_row(i).map(|c| c.path.clone());
        self.highlight_hint.set(i);
        self.highlight_top = false;
    }
    /// Moves down to the next row of another path than `path`, or to the
    /// last row: a contents query lists a file once per match.
    pub fn move_past(&mut self, path: &Path) {
        let i = self.highlighted().unwrap_or(0);
        let step = self
            .shown_from(i + 1)
            .position(|c| c.path != path)
            .map_or(self.shown_len().saturating_sub(i + 1), |n| n + 1);
        self.move_by(step as isize);
    }
    /// The highlighted row's index, when it is listed.
    pub fn highlighted(&self) -> Option<usize> {
        let path = self.highlight.as_ref()?;
        let hint = self.highlight_hint.get();
        // A contents query shows a file once per match: the hint tells them
        // apart.
        if self.shown_row(hint).is_some_and(|c| &c.path == path) {
            return Some(hint);
        }
        // Comparing addresses avoids reading every row's path.
        let i = match self.known.get(path) {
            Some(known) => self.shown_position(|c| Arc::ptr_eq(c, known)),
            None => None,
        }
        .or_else(|| self.shown_position(|c| &c.path == path))?;
        self.highlight_hint.set(i);
        Some(i)
    }
    pub fn highlighted_row(&self) -> Option<&Arc<Candidate>> {
        self.highlighted().and_then(|i| self.shown_row(i))
    }
    /// Highlight row `i` of the shown rows; a file shown once per match
    /// keeps which one.
    pub fn highlight_row(&mut self, i: usize) {
        if let Some(c) = self.shown_row(i) {
            self.highlight = Some(c.path.clone());
            self.highlight_hint.set(i);
        }
    }
    /// One row down or up, wrapping from the last row to the first and back.
    pub fn step(&mut self, forward: bool) {
        let last = self.shown_len().saturating_sub(1);
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
    /// Highlight the row numbered `digit` on screen, by row: a contents
    /// query shows a file once per match.
    pub fn highlight_numbered(&mut self, digit: u32) {
        let i = numbered_row(digit, self.rendered.len());
        let path = self.numbered(digit);
        self.highlight_row(self.offset + i);
        // The drawn path rules, should the rows have moved since.
        if self.highlight.as_ref() != Some(&path) {
            self.highlight = Some(path);
        }
    }
    pub fn numbered(&self, digit: u32) -> PathBuf {
        self.rendered
            .get(numbered_row(digit, self.rendered.len()))
            .cloned()
            .unwrap_or_else(|| self.cwd.clone())
    }
    /// Whether the current directory may change; false in `fff seek`.
    pub fn can_navigate(&mut self) -> bool {
        let navigates = self.program().navigates();
        if !navigates {
            self.status = "Navigation is disabled in fff seek".into();
        }
        navigates
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
            self.navigate(path, false)
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
                self.navigate(folder, false)
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
    /// Whether Enter may act on `entry`: it is not a cached row that is
    /// gone, or that turned from a file into a folder or back, which the
    /// function chosen for its kind would mistake.
    pub fn ready_entry(&mut self, entry: &Candidate) -> bool {
        if !self.ready_path(&entry.path) {
            return false;
        }
        if self.cache_pending && entry.path.is_dir() != entry.directory {
            self.status = "Cached entry changed; waiting for refresh".into();
            return false;
        }
        true
    }
    pub fn ready_path(&mut self, path: &Path) -> bool {
        if self.cache_pending && !path.exists() {
            self.status = "Cached path no longer exists; waiting for refresh".into();
            false
        } else {
            true
        }
    }
    /// The chooser's workspaces matching `query`. A session that cannot
    /// change program leaves out those that would only change it.
    pub fn matching_workspaces(&self, query: &str) -> Vec<usize> {
        let switches = self.switches_program();
        self.workspaces
            .iter()
            .enumerate()
            .filter(|(_, w)| switches || w.applies_beside_program())
            .filter(|(_, w)| score(&w.name, query).is_some())
            .map(|(i, _)| i)
            .collect()
    }
    /// `switch_program` works here: not in a window of its own or a file
    /// dialog.
    pub fn switches_program(&self) -> bool {
        program::Program::SWITCHABLE
            .iter()
            .any(|&(p, _)| p == self.program())
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
    /// The session's settings, as a workspace saved now would keep them:
    /// every one this program has (a window or file dialog has no program
    /// to switch to, a dialog no contents query).
    pub fn snapshot(&self) -> config::Workspace {
        config::Workspace {
            path: Some(self.cwd.clone()),
            program: self.switches_program().then(|| self.program()),
            roots: Some(self.roots.clone()),
            query: Some(self.query.clone()),
            contents: (!self.program().dialog()).then(|| self.grep.text.clone()),
            depth: Some(self.depth),
            filters: Some(self.policy.filters.clone()),
            exclusions: Some(self.policy.rules.clone()),
            sort: Some(self.sort),
            hidden: Some(self.policy.hidden),
            kinds: Some(self.kinds),
            preview: Some(self.preview.shown),
            exclusions_on: Some(self.policy.ignore),
            filters_on: Some(!self.policy.filters_off),
            ..config::Workspace::default()
        }
    }
    pub fn open_workspace(&mut self, index: usize) -> bool {
        match self.workspaces.get(index).cloned() {
            Some(workspace) => self.apply_workspace(&workspace),
            None => false,
        }
    }
    /// Applies the settings `workspace` keeps and leaves the others, the
    /// query included. Its folder (else its first root) becomes the current
    /// directory; roots that are gone are left out. A program that cannot
    /// change (`switches_program`) stays. Nothing changes when the folder is
    /// not a directory or none of the roots is; the status says why. True
    /// when it was applied, and the caller scans.
    pub fn apply_workspace(&mut self, workspace: &config::Workspace) -> bool {
        let name = &workspace.name;
        let folder = match &workspace.path {
            Some(path) => match path.canonicalize() {
                Ok(path) if path.is_dir() => Some(path),
                _ => {
                    self.status = format!(
                        "Workspace {name:?}: {} is not an accessible directory",
                        path.display()
                    );
                    return false;
                }
            },
            None => None,
        };
        let mut notes = Vec::new();
        let roots = workspace.roots.as_ref().map(|roots| {
            let (found, missing): (Vec<_>, Vec<_>) = roots
                .iter()
                .map(|root| root.canonicalize().ok().filter(|r| r.is_dir()).ok_or(root))
                .partition(Result::is_ok);
            if !missing.is_empty() {
                let missing: Vec<_> = missing
                    .into_iter()
                    .map(|r| r.unwrap_err().display().to_string())
                    .collect();
                notes.push(format!("left out missing roots {}", missing.join(", ")));
            }
            let mut kept: Vec<PathBuf> = found.into_iter().map(Result::unwrap).collect();
            let mut seen = std::collections::HashSet::new();
            kept.retain(|r| seen.insert(r.clone()));
            kept
        });
        if let Some(kept) = &roots
            && kept.is_empty()
            && folder.is_none()
            && workspace.roots.as_ref().is_some_and(|r| !r.is_empty())
        {
            self.status = format!("Workspace {name:?}: none of its roots exists");
            return false;
        }
        let target = folder
            .or_else(|| roots.as_ref().and_then(|r| r.first().cloned()))
            .unwrap_or_else(|| self.cwd.clone());
        if self.leaving(&target) {
            return false;
        }
        if let Some(to) = workspace.program {
            let from = self.program();
            if !self.switches_program() {
                notes.push(format!(
                    "fff {} stays, as it cannot become fff {}",
                    from.title(),
                    to.title()
                ));
            } else if to != from {
                self.set_program(to);
                if workspace.depth.is_none() && self.depth == from.default_depth() {
                    self.depth = to.default_depth();
                }
            }
        }
        if let Some(roots) = roots {
            self.roots = roots;
        }
        if let Some(depth) = workspace.depth {
            self.depth = depth.max(1);
        }
        // Globs were checked when the configuration was read.
        if let Some(filters) = &workspace.filters {
            let _ = self.policy.set_filters(filters.clone());
        }
        if let Some(rules) = &workspace.exclusions {
            let _ = self.policy.set_rules(rules.clone());
        }
        if let Some(hidden) = workspace.hidden {
            self.policy.hidden = hidden;
        }
        if let Some(on) = workspace.exclusions_on {
            self.policy.ignore = on;
        }
        if let Some(on) = workspace.filters_on {
            self.policy.filters_off = !on;
        }
        if let Some(kinds) = workspace.kinds {
            self.kinds = kinds;
        }
        if let Some(shown) = workspace.preview {
            self.preview.shown = shown;
        }
        if let Some(contents) = &workspace.contents
            && !self.program().dialog()
        {
            self.grep.text = contents.clone();
        }
        // The scope follows the roots even when the folder stays.
        self.navigate(target, true);
        if let Some(query) = &workspace.query {
            self.query = query.clone();
        }
        if let Some(sort) = workspace.sort {
            self.set_sort(sort);
        }
        self.wanted = None;
        self.status = std::iter::once(format!("Opened workspace {name:?}"))
            .chain(notes)
            .collect::<Vec<_>>()
            .join("; ");
        self.status_success = self.status.clone();
        true
    }
    /// Enter with nothing listed while typing: in a folder verified empty,
    /// where a folder is an answer, the marks and the folder. Otherwise
    /// `None`, the status saying why.
    pub fn empty_folder(&mut self) -> Option<Vec<PathBuf>> {
        if self.scanning || !self.physical_empty || !self.returns_folder() {
            self.nothing_to_enter();
            return None;
        }
        self.confirm(Some(self.cwd.clone()))
    }
    /// Whether Enter in a verified empty folder may answer with the folder:
    /// not in `fff seek`, with no shell to leave into (`fff window`), or in a
    /// dialog choosing files, whose answer has no room for a folder.
    fn returns_folder(&self) -> bool {
        use crate::program::Program::*;
        matches!(self.program(), Roam | Pick | ExplorerFolder)
    }
    /// Says why Enter (or Ctrl+number) with no row did nothing.
    pub fn nothing_to_enter(&mut self) {
        self.status = if self.scanning {
            "Nothing listed yet; still searching".into()
        } else if !self.filter().is_empty() {
            match self.bound(keymap::Function::ClearQuery) {
                Some(key) => format!("Nothing matches {:?}; {key} clears the query", self.query),
                None => format!("Nothing matches {:?}", self.query),
            }
        } else {
            "Nothing listed here".into()
        };
    }
    /// Whether Ctrl+w may finish with the marks: only where the answer can be
    /// several paths, `fff pick` and the dialogs choosing several (`fff explorer open-multiple`, `folder --multiple`).
    pub fn returns_marks(&self) -> bool {
        self.running.returns_marks()
    }
    pub fn confirm(&mut self, target: Option<PathBuf>) -> Option<Vec<PathBuf>> {
        // A link counts as the entry it is, even when its target is gone.
        if self.marks.iter().any(|p| p.symlink_metadata().is_err()) {
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
        for (path, length) in group.iter().zip(label_lengths(&group)) {
            labels.insert(path.to_path_buf(), label_of(path, length));
        }
    }
    labels
}

/// How many trailing parts each path of `group` (two or more sharing a
/// basename) keeps in its label: one past the most it ends in common with
/// another, and at least its parent, short of the whole path.
fn label_lengths(group: &[&Path]) -> Vec<usize> {
    // Each path's parts from its name up, sorted: the path sharing the
    // longest ending with one is beside it.
    let mut parts: Vec<(Vec<_>, usize)> = group
        .iter()
        .enumerate()
        .map(|(i, p)| (p.components().rev().collect(), i))
        .collect();
    parts.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    let shared = |a: &[std::path::Component], b: &[std::path::Component]| {
        a.iter().zip(b).take_while(|(a, b)| a == b).count()
    };
    let mut lengths = vec![0; group.len()];
    for (i, (own, at)) in parts.iter().enumerate() {
        let before = i.checked_sub(1).map_or(0, |j| shared(&parts[j].0, own));
        let after = parts.get(i + 1).map_or(0, |next| shared(&next.0, own));
        lengths[*at] = (before.max(after) + 1).max(2).min(own.len());
    }
    lengths
}
/// The last `length` parts of `path`.
fn label_of(path: &Path, length: usize) -> PathBuf {
    let parts: Vec<_> = path.components().collect();
    parts[parts.len() - length..].iter().collect()
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
