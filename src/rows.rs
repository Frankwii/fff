//! The results' order and the rows kept in it. The order is one value that
//! every comparison of rows goes through, so that another order is a
//! variant here rather than a change to ranking and merging. The rows are
//! sorted runs, merged lazily: only as far as a reader looks, or all at
//! once for whatever needs every row in order (`Rows::all`).
use crate::{Candidate, sizes::Totals};
use std::{cell::OnceCell, cmp::Ordering, path::Path, sync::Arc, time::SystemTime};

/// How a program ranks its rows (`Program::rank`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Rank {
    /// Depth, directories first, best score, root order, then pathname
    /// bytes, which break every tie.
    #[default]
    Depth,
    /// The best score first, then as `Depth` (`fff pick` and `fff seek`).
    Score,
    /// The best score first, then the order a list read from stdin gave
    /// (`Candidate::order`), so that an empty query keeps it.
    Listed,
}
impl Rank {
    fn cmp(self, a: &Candidate, score_a: i64, b: &Candidate, score_b: i64) -> Ordering {
        let score = score_b.cmp(&score_a);
        let depth = a.depth.cmp(&b.depth).then(b.directory.cmp(&a.directory));
        let (first, second) = match self {
            Self::Depth => (depth, score),
            Self::Score => (score, depth),
            // Each listed path has its own place: no tie is left.
            Self::Listed => (score, a.order.cmp(&b.order)),
        };
        first
            .then(second)
            .then(a.root.cmp(&b.root))
            .then(a.path.as_os_str().cmp(b.path.as_os_str()))
    }
}

/// What the sorting picker sorts the matches by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criterion {
    /// Names alphabetically, numbers by value: `file2` before `file10`.
    Name,
    /// A file's size; a folder's, the files below it once added up.
    Size,
    Modified,
    Accessed,
    /// Where the filesystem records it.
    Created,
    /// The extension, then the name.
    Extension,
    /// Folders, then files, then links.
    Kind,
    /// The depth, then the name.
    Depth,
    /// The depth, folders first, then the best match: `fff roam`'s ranking.
    Shallowest,
    /// The match, less a penalty growing with the depth (`Criterion::merit`):
    /// in proportion to it,
    Linear,
    /// as its square root,
    Sqrt,
    /// as its logarithm,
    Log,
    /// or as its square.
    Quadratic,
}
impl Criterion {
    pub const ALL: [Self; 13] = [
        Self::Name,
        Self::Size,
        Self::Modified,
        Self::Accessed,
        Self::Created,
        Self::Extension,
        Self::Kind,
        Self::Depth,
        Self::Shallowest,
        Self::Linear,
        Self::Sqrt,
        Self::Log,
        Self::Quadratic,
    ];
    /// Its name in the picker and the configuration.
    pub fn name(self) -> &'static str {
        match self {
            Self::Name => "name",
            Self::Size => "size",
            Self::Modified => "modified",
            Self::Accessed => "accessed",
            Self::Created => "created",
            Self::Extension => "extension",
            Self::Kind => "kind",
            Self::Depth => "depth",
            Self::Shallowest => "shallowest",
            Self::Linear => "linear",
            Self::Sqrt => "sqrt",
            Self::Log => "log",
            Self::Quadratic => "quadratic",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Name => "Names alphabetically, numbers in them by value: file2 before file10.",
            Self::Size => {
                "Sizes; a folder's is the files below it, once added up (until then it comes last)."
            }
            Self::Modified => "When each entry was last modified.",
            Self::Accessed => "When each entry was last read.",
            Self::Created => {
                "When each entry was created, where the filesystem records it (others come last)."
            }
            Self::Extension => "Extensions alphabetically, then names.",
            Self::Kind => "Folders, then files, then links.",
            Self::Depth => "Depth below the folder searched, then names.",
            Self::Shallowest => "Depth, folders first, then the best match.",
            Self::Linear => "The best match, less a penalty in proportion to the depth.",
            Self::Sqrt => "The best match, less a penalty growing as the square root of the depth.",
            Self::Log => "The best match, less a penalty growing as the logarithm of the depth.",
            Self::Quadratic => "The best match, less a penalty growing as the square of the depth.",
        }
    }
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.name() == name)
    }
    /// Its rows need a value read from the filesystem before ranking.
    fn reads(self) -> bool {
        matches!(
            self,
            Self::Size | Self::Modified | Self::Accessed | Self::Created | Self::Kind
        )
    }
    /// A penalty, whose values are merits (`merit`).
    fn penalises(self) -> bool {
        matches!(
            self,
            Self::Linear | Self::Sqrt | Self::Log | Self::Quadratic
        )
    }
    /// What a match at `depth` costs, in points per letter of the query,
    /// at temperature 1. Each penalty is scaled so that all cost the same
    /// `ALIKE_AT` levels down: as many `LEVEL`s as levels. `None` for the
    /// criteria without one.
    fn penalty(self, depth: usize) -> Option<f64> {
        let shape = |d: f64| match self {
            Self::Linear => Some(d),
            Self::Sqrt => Some(d.sqrt()),
            Self::Log => Some(d.ln_1p()),
            Self::Quadratic => Some(d * d),
            _ => None,
        };
        Some(LEVEL * ALIKE_AT * shape(depth as f64)? / shape(ALIKE_AT)?)
    }
    /// A row's merit for a penalty: its score per letter of the query,
    /// less the penalty at `temperature`, in fixed point (the best match
    /// the highest). An empty query scores every
    /// row 0, so its rows rank by depth.
    fn merit(self, depth: usize, score: i64, letters: usize, temperature: f64) -> Option<i64> {
        let penalty = self.penalty(depth)?;
        let per_letter = score as f64 / letters.max(1) as f64;
        Some(((per_letter - temperature * penalty) * FIXED).round() as i64)
    }
}

/// The temperature, `[ui] temperature`, unless the configuration sets one.
pub const TEMPERATURE: f64 = 1.0;
/// What one level down costs a match at temperature 1, in points per
/// letter of the query: nucleo's bonus for a letter starting a word. Per
/// letter, matches score about 3 to 31 (half of them over a home folder
/// above 13); a good match scores 6 to 14 more than a typical one.
const LEVEL: f64 = 8.0;
/// The depth at which every penalty costs the same.
const ALIKE_AT: f64 = 4.0;
/// Fractions of a point a merit keeps.
const FIXED: f64 = 1024.0;

/// A criterion and its direction, chosen in the sorting picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort {
    pub by: Criterion,
    pub descending: bool,
}
impl Sort {
    /// How the results' border names it: `name ↑`.
    pub fn label(self) -> String {
        format!("{} {}", self.by.name(), self.arrow())
    }
    /// Its direction: ↓ descending, the criterion's own order, ↑
    /// ascending, that order reversed.
    pub fn arrow(self) -> &'static str {
        if self.descending { "↓" } else { "↑" }
    }
    fn cmp(self, a: &Candidate, value_a: i64, b: &Candidate, value_b: i64) -> Ordering {
        // Values not known come last, whichever the direction.
        let unknown = (value_a == UNKNOWN).cmp(&(value_b == UNKNOWN));
        let name = || natural(&name(&a.path), &name(&b.path));
        // Each criterion's own order, which descending keeps and ascending
        // reverses: names from A, the shallowest, the largest, the newest
        // and the best match first.
        let by = match self.by {
            Criterion::Name => name(),
            Criterion::Extension => {
                natural(&extension(&a.path), &extension(&b.path)).then_with(name)
            }
            Criterion::Depth => a.depth.cmp(&b.depth).then_with(name),
            // The best match comes from the ranking breaking the tie.
            Criterion::Shallowest => a.depth.cmp(&b.depth).then(b.directory.cmp(&a.directory)),
            Criterion::Kind => value_a.cmp(&value_b),
            Criterion::Size
            | Criterion::Modified
            | Criterion::Accessed
            | Criterion::Created
            | Criterion::Linear
            | Criterion::Sqrt
            | Criterion::Log
            | Criterion::Quadratic => value_b.cmp(&value_a),
        };
        unknown.then(if self.descending { by } else { by.reverse() })
    }
}

/// A row's value that could not be read: it sorts last.
const UNKNOWN: i64 = i64::MIN;

/// How rows are ordered: the program's ranking (`Program::rank`), or a
/// sort chosen in the picker with the ranking breaking its ties. Read as
/// `App::order`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Order {
    pub rank: Rank,
    pub sort: Option<Sort>,
    /// How much depth costs a match in a sort by a penalty: 0 not at all,
    /// a large one ranks as `Criterion::Shallowest` (`[ui] temperature`).
    /// An `f32` keeps the order to 8 bytes, copied into every comparison.
    pub temperature: f32,
}
impl Default for Order {
    fn default() -> Self {
        Rank::default().into()
    }
}
impl From<Rank> for Order {
    fn from(rank: Rank) -> Self {
        Self {
            rank,
            sort: None,
            temperature: TEMPERATURE as f32,
        }
    }
}
impl Order {
    /// Compares two rows with their keys for the current query.
    pub fn cmp(self, a: &Candidate, key_a: Key, b: &Candidate, key_b: Key) -> Ordering {
        let ranked = || self.rank.cmp(a, key_a.score, b, key_b.score);
        match self.sort {
            Some(sort) => sort.cmp(a, key_a.value, b, key_b.value).then_with(ranked),
            None => ranked(),
        }
    }
    /// A row's key for its `score` for a query of `letters`. For a sort by
    /// what the filesystem records (sizes, dates, links), its value is read
    /// here, before ranking: comparisons never touch the filesystem. For a
    /// penalty, it is the row's merit.
    pub fn key(self, c: &Candidate, score: i64, letters: usize, totals: &Totals) -> Key {
        let value = match self.sort {
            Some(sort) if sort.by.reads() => value(sort.by, c, totals),
            Some(sort) => sort
                .by
                .merit(c.depth, score, letters, f64::from(self.temperature))
                .unwrap_or(0),
            None => 0,
        };
        Key { score, value }
    }
    /// Its keys' values are merits, which another query changes: rows
    /// ranked again take a new `key` rather than keeping their values.
    pub fn penalises(self) -> bool {
        self.sort.is_some_and(|sort| sort.by.penalises())
    }
}

/// What a row is ranked by: its match score and, for a sort by size,
/// dates or kind, the value read for it, for a penalty its merit
/// (`Order::key`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Key {
    pub score: i64,
    pub value: i64,
}

/// `c`'s value for `by`, `UNKNOWN` when it cannot be read.
fn value(by: Criterion, c: &Candidate, totals: &Totals) -> i64 {
    let path = c.path.as_path();
    let stamp = |time: std::io::Result<SystemTime>| {
        let Ok(time) = time else {
            return UNKNOWN;
        };
        match time.duration_since(SystemTime::UNIX_EPOCH) {
            Ok(after) => i64::try_from(after.as_nanos()).unwrap_or(i64::MAX),
            Err(before) => i64::try_from(before.duration().as_nanos()).map_or(UNKNOWN + 1, |n| -n),
        }
    };
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return UNKNOWN;
    };
    let bytes = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
    match by {
        Criterion::Size if c.directory => totals.bytes(path).map_or(UNKNOWN, bytes),
        // A link's size is its target's, as the size column shows it.
        Criterion::Size if meta.is_symlink() => {
            std::fs::metadata(path).map_or(UNKNOWN, |m| bytes(m.len()))
        }
        Criterion::Size => bytes(meta.len()),
        Criterion::Modified => stamp(meta.modified()),
        Criterion::Accessed => stamp(meta.accessed()),
        Criterion::Created => stamp(meta.created()),
        Criterion::Kind if meta.is_symlink() => 2,
        Criterion::Kind if meta.is_dir() => 0,
        Criterion::Kind => 1,
        _ => 0,
    }
}
fn name(path: &Path) -> std::borrow::Cow<'_, str> {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
}
fn extension(path: &Path) -> std::borrow::Cow<'_, str> {
    path.extension().unwrap_or_default().to_string_lossy()
}
/// Compares names alphabetically, ignoring case, and the numbers in them
/// by value (`file2` before `file10`); names equal so are told apart by
/// their bytes.
pub fn natural(a: &str, b: &str) -> Ordering {
    let (mut x, mut y) = (a, b);
    loop {
        let (Some(c), Some(d)) = (x.chars().next(), y.chars().next()) else {
            return (!x.is_empty()).cmp(&!y.is_empty()).then_with(|| a.cmp(b));
        };
        if c.is_ascii_digit() && d.is_ascii_digit() {
            let n = x.bytes().take_while(u8::is_ascii_digit).count();
            let m = y.bytes().take_while(u8::is_ascii_digit).count();
            let (p, q) = (
                x[..n].trim_start_matches('0'),
                y[..m].trim_start_matches('0'),
            );
            let by = p.len().cmp(&q.len()).then_with(|| p.cmp(q));
            if by.is_ne() {
                return by;
            }
            (x, y) = (&x[n..], &y[m..]);
            continue;
        }
        let by = c.to_lowercase().cmp(d.to_lowercase());
        if by.is_ne() {
            return by;
        }
        (x, y) = (&x[c.len_utf8()..], &y[d.len_utf8()..]);
    }
}

/// Rows sorted in one order, with their keys for the query they were
/// ranked by, in the same order.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub rows: Vec<Arc<Candidate>>,
    pub keys: Vec<Key>,
}
impl Run {
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// How many rows from the front are kept merged as the runs change: what a
/// frame reads (the rows in view, the highlight) is there. Reading past them
/// merges every run once (`Rows::all`).
pub const LAZY: usize = 2048;

/// The ranked rows: sorted runs, the largest first, each at least twice as
/// large as the next. A ranking makes one run; each round of a scan's new
/// paths is pushed as another, so no row already ranked moves, and only the
/// first `LAZY` rows are kept merged (`head`). Once something reads every
/// row, they stay merged: new rounds are inserted among them in place.
/// Clones share the runs (kept queries, rankings on another thread).
#[derive(Clone, Default)]
pub struct Rows {
    order: Order,
    runs: Vec<Arc<Run>>,
    len: usize,
    /// The first `LAZY` rows (or every one, when fewer) in order.
    head: Run,
    /// Every row in order, once something needed them all; it is then the
    /// only run, and stays merged as rows are pushed.
    all: OnceCell<Arc<Run>>,
}
impl Rows {
    /// Rows ranked all at once.
    pub(crate) fn sorted(order: Order, run: Run) -> Self {
        let len = run.len();
        Self {
            order,
            head: front(&run),
            runs: if len > 0 {
                vec![Arc::new(run)]
            } else {
                Vec::new()
            },
            len,
            all: OnceCell::new(),
        }
    }
    /// How many rows there are; nothing is merged to count them.
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Every row, in order: the one way to read them all, which merges the
    /// runs first (once: they stay merged after).
    pub fn all(&self) -> &[Arc<Candidate>] {
        &self.merged().rows
    }
    fn merged(&self) -> &Arc<Run> {
        self.all.get_or_init(|| match self.runs.as_slice() {
            [] => Arc::default(),
            [run] => Arc::clone(run),
            runs => {
                // Smallest first, so that each row moves about twice.
                let mut runs = runs.iter().rev().map(|r| Run::clone(r));
                let first = runs.next().expect("several runs");
                Arc::new(runs.fold(first, |merged, run| merge_two(self.order, run, merged)))
            }
        })
    }
    /// Row `i` in order.
    pub fn row(&self, i: usize) -> Option<&Arc<Candidate>> {
        if i >= self.len {
            return None;
        }
        match self.all.get() {
            Some(all) => all.rows.get(i),
            None => self.head.rows.get(i).or_else(|| self.all().get(i)),
        }
    }
    /// The rows in order from row `from` on: the first `LAZY` as they are
    /// kept, every run merged only when read past them.
    pub fn iter_from(&self, from: usize) -> Box<dyn Iterator<Item = &Arc<Candidate>> + '_> {
        let kept = self.head.len();
        if from >= self.len {
            return Box::new(std::iter::empty());
        }
        match self.all.get() {
            Some(all) => Box::new(all.rows.iter().skip(from)),
            None if from < kept => Box::new(self.head.rows[from..].iter().chain(
                std::iter::once(()).flat_map(move |()| {
                    let rest: &[Arc<Candidate>] = if self.len > kept {
                        &self.all()[kept..]
                    } else {
                        &[]
                    };
                    rest.iter()
                }),
            )),
            None => Box::new(self.all().iter().skip(from)),
        }
    }
    /// Where the first row `found` holds for is, in order.
    pub fn position(&self, mut found: impl FnMut(&Arc<Candidate>) -> bool) -> Option<usize> {
        if self.all.get().is_none() {
            if let Some(i) = self.head.rows.iter().position(&mut found) {
                return Some(i);
            }
            if self.len <= self.head.len() {
                return None;
            }
        }
        self.all().iter().position(found)
    }
    /// Whether any row, in whatever order, is one `found` holds for.
    pub fn contains(&self, mut found: impl FnMut(&Arc<Candidate>) -> bool) -> bool {
        match self.all.get() {
            Some(all) => all.rows.iter().any(found),
            None => self.runs.iter().any(|r| r.rows.iter().any(&mut found)),
        }
    }
    /// The runs as they are, in no particular order between them: what a
    /// ranking reads every row from.
    pub(crate) fn runs(&self) -> Vec<Arc<Run>> {
        match self.all.get() {
            Some(all) => vec![Arc::clone(all)],
            None => self.runs.clone(),
        }
    }
    /// The rows are about to change: every row merged becomes the only run.
    /// Whether they were.
    fn thaw(&mut self) -> bool {
        let Some(all) = self.all.take() else {
            return false;
        };
        let runs = std::mem::take(&mut self.runs);
        if runs.len() > 1 {
            crate::known::discard(runs, self.len);
        }
        if !all.is_empty() {
            self.runs.push(all);
        }
        true
    }
    /// Add `run`, sorted in `order`. While every row is merged, it is
    /// inserted among them; otherwise it becomes a run of its own, merged
    /// with the last ones while the one before it is not twice as large.
    pub(crate) fn push(&mut self, order: Order, run: Run) {
        if run.is_empty() {
            return;
        }
        if self.len == 0 {
            self.order = order;
        }
        debug_assert_eq!(self.order, order, "rows are kept in one order");
        let whole = self.thaw();
        self.len += run.len();
        // The first rows: what was first, or the new ones ranked before.
        let late = self.head.len() == LAZY
            && self
                .head
                .rows
                .last()
                .zip(self.head.keys.last())
                .is_some_and(|(c, s)| order.cmp(&run.rows[0], run.keys[0], c, *s).is_gt());
        if !late {
            let head = std::mem::take(&mut self.head);
            let mut merged = merge_two(order, head, front(&run));
            merged.rows.truncate(LAZY);
            merged.keys.truncate(LAZY);
            self.head = merged;
        }
        if whole {
            match self.runs.last_mut() {
                Some(all) => insert(order, Arc::make_mut(all), run),
                None => self.runs.push(Arc::new(run)),
            }
            let all = Arc::clone(&self.runs[0]);
            let _ = self.all.set(all);
            return;
        }
        self.runs.push(Arc::new(run));
        while let [.., before, last] = self.runs.as_slice()
            && before.len() <= 2 * last.len()
        {
            let last = owned(self.runs.pop().expect("two runs"));
            let before = owned(self.runs.pop().expect("two runs"));
            self.runs
                .push(Arc::new(merge_two(self.order, before, last)));
        }
    }
    /// Keep only the rows `keep` holds for, in order.
    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&Arc<Candidate>) -> bool) {
        let whole = self.thaw();
        for run in &mut self.runs {
            let kept: Vec<bool> = run.rows.iter().map(&mut keep).collect();
            if kept.iter().all(|k| *k) {
                continue;
            }
            let run = Arc::make_mut(run);
            let mut each = kept.iter();
            run.rows.retain(|_| *each.next().expect("one per row"));
            let mut each = kept.iter();
            run.keys.retain(|_| *each.next().expect("one per row"));
        }
        self.runs.retain(|r| !r.is_empty());
        self.len = self.runs.iter().map(|r| r.len()).sum();
        // The first rows again, from each run's.
        self.head = self
            .runs
            .iter()
            .map(|r| front(r))
            .reduce(|merged, run| merge_two(self.order, merged, run))
            .unwrap_or_default();
        self.head.rows.truncate(LAZY);
        self.head.keys.truncate(LAZY);
        if whole && let [all] = self.runs.as_slice() {
            let _ = self.all.set(Arc::clone(all));
        }
    }
}
/// Every row in order.
impl std::ops::Deref for Rows {
    type Target = [Arc<Candidate>];
    fn deref(&self) -> &Self::Target {
        self.all()
    }
}
impl PartialEq for Rows {
    fn eq(&self, other: &Self) -> bool {
        self.all() == other.all()
    }
}
impl std::fmt::Debug for Rows {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.all()).finish()
    }
}

/// The first `LAZY` rows of `run`.
fn front(run: &Run) -> Run {
    let n = run.len().min(LAZY);
    Run {
        rows: run.rows[..n].to_vec(),
        keys: run.keys[..n].to_vec(),
    }
}
/// Insert `added` among the rows of `into`, both sorted, in place: rows
/// ranked before every added one stay where they are, and as scans deliver
/// shallow paths first, insertions mostly land near the end.
fn insert(order: Order, into: &mut Run, added: Run) {
    let Run { rows, keys } = into;
    // Where an insertion goes: searched back from the end in growing
    // steps, as each lands near the one after it, then halving.
    let before = |rows: &[Arc<Candidate>], keys: &[Key], c: &Candidate, s: Key| {
        let lt = |i: usize| order.cmp(&rows[i], keys[i], c, s).is_lt();
        let (mut lo, mut hi, mut step) = (rows.len(), rows.len(), 1);
        while lo > 0 && !lt(lo - 1) {
            hi = lo - 1;
            lo = lo.saturating_sub(step);
            step *= 2;
        }
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if lt(mid) {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        lo
    };
    // From the last insertion back, each opens a gap where it goes by
    // moving the rows after it up once.
    let mut end = rows.len();
    let k = added.len();
    rows.resize(end + k, Arc::clone(&added.rows[0]));
    keys.resize(end + k, Key::default());
    for (i, (c, score)) in added.rows.into_iter().zip(added.keys).enumerate().rev() {
        let at = before(&rows[..end], &keys[..end], &c, score);
        // Rows at..end move up by the i + 1 insertions left.
        rows[at..end + i + 1].rotate_right(i + 1);
        keys.copy_within(at..end, at + i + 1);
        rows[at + i] = c;
        keys[at + i] = score;
        end = at;
    }
}
fn owned(run: Arc<Run>) -> Run {
    Arc::try_unwrap(run).unwrap_or_else(|run| Run::clone(&run))
}
/// Merge two sorted runs, `a`'s rows first on ties. Each step takes the
/// whole stretch of one run that ranks before the other's next row, found
/// by galloping, so runs that barely interleave (a folder's paths among
/// another's) cost a few comparisons and a copy.
fn merge_two(order: Order, a: Run, b: Run) -> Run {
    let mut merged = Run {
        rows: Vec::with_capacity(a.len() + b.len()),
        keys: Vec::with_capacity(a.len() + b.len()),
    };
    let (mut a_rows, mut a_keys) = (a.rows.into_iter(), a.keys.into_iter());
    let (mut b_rows, mut b_keys) = (b.rows.into_iter(), b.keys.into_iter());
    while let (Some(head), Some(&score)) = (b_rows.as_slice().first(), b_keys.as_slice().first()) {
        let (rows, keys) = (a_rows.as_slice(), a_keys.as_slice());
        let n = leading(rows.len(), |i| {
            order.cmp(&rows[i], keys[i], head, score).is_le()
        });
        merged.rows.extend(a_rows.by_ref().take(n));
        merged.keys.extend(a_keys.by_ref().take(n));
        let (Some(head), Some(&score)) = (a_rows.as_slice().first(), a_keys.as_slice().first())
        else {
            break;
        };
        let (rows, keys) = (b_rows.as_slice(), b_keys.as_slice());
        let n = leading(rows.len(), |i| {
            order.cmp(&rows[i], keys[i], head, score).is_lt()
        });
        merged.rows.extend(b_rows.by_ref().take(n));
        merged.keys.extend(b_keys.by_ref().take(n));
    }
    merged.rows.extend(a_rows.chain(b_rows));
    merged.keys.extend(a_keys.chain(b_keys));
    merged
}
/// How many of the first of `len` indices `holds` is true for, given that
/// it is true up to some index and false after: probed at 1, 2, 4... from
/// the start, then halving.
fn leading(len: usize, holds: impl Fn(usize) -> bool) -> usize {
    let (mut lo, mut hi, mut step) = (0, len, 1);
    while lo < len {
        let probe = (lo + step - 1).min(len - 1);
        if holds(probe) {
            lo = probe + 1;
            step *= 2;
        } else {
            hi = probe;
            break;
        }
    }
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if holds(mid) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(order: Order, mut rows: Vec<(Key, Candidate)>) -> Run {
        rows.sort_by(|(sa, a), (sb, b)| order.cmp(a, *sa, b, *sb));
        let (keys, rows) = rows.into_iter().map(|(s, c)| (s, Arc::new(c))).unzip();
        Run { rows, keys }
    }
    fn candidate(i: u64) -> (Key, Candidate) {
        let n = i.wrapping_mul(2_654_435_761) % 10_007;
        (
            Key {
                score: (n % 5) as i64,
                value: if n.is_multiple_of(11) {
                    UNKNOWN
                } else {
                    (n % 7) as i64
                },
            },
            Candidate {
                path: format!("/r/d{}/f{n:05}", n % 13).into(),
                directory: n.is_multiple_of(3),
                depth: 1 + (n % 4) as usize,
                root: (n % 2) as usize,
                order: 0,
            },
        )
    }
    /// Both rankings, and sorts by a name and by a value read, each way.
    fn orders() -> Vec<Order> {
        let mut orders = vec![Rank::Depth.into(), Rank::Score.into()];
        for by in [Criterion::Name, Criterion::Size, Criterion::Depth] {
            for descending in [false, true] {
                orders.push(Order {
                    sort: Some(Sort { by, descending }),
                    ..Rank::Depth.into()
                });
            }
        }
        orders
    }
    #[test]
    fn a_list_keeps_its_order_among_matches_as_good() {
        let row = |path: &str, order| Candidate {
            path: path.into(),
            directory: path.ends_with('/'),
            depth: 1,
            root: 0,
            order,
        };
        let key = |score| Key { score, value: 0 };
        let listed = Order::from(Rank::Listed);
        // Neither the name, the depth nor being a folder comes before it.
        let (z, a, folder) = (row("/r/z", 0), row("/r/a", 1), row("/r/d/", 2));
        assert!(listed.cmp(&z, key(0), &a, key(0)).is_lt());
        assert!(listed.cmp(&a, key(0), &folder, key(0)).is_lt());
        // The best match first.
        assert!(listed.cmp(&folder, key(9), &z, key(1)).is_lt());
    }
    #[test]
    fn every_penalty_costs_the_same_four_levels_down() {
        let penalties = [
            Criterion::Linear,
            Criterion::Sqrt,
            Criterion::Log,
            Criterion::Quadratic,
        ];
        for by in penalties {
            assert_eq!(by.penalty(0), Some(0.0), "{by:?}");
            let alike = by.penalty(4).unwrap();
            assert!((alike - 4.0 * LEVEL).abs() < 1e-9, "{by:?} {alike}");
            for depth in 1..12 {
                assert!(by.penalty(depth) < by.penalty(depth + 1), "{by:?}");
            }
        }
        // Above it, the root and the logarithm count the first levels most,
        // the square the last ones.
        let at = |by: Criterion, depth| by.penalty(depth).unwrap();
        for by in [Criterion::Sqrt, Criterion::Log] {
            assert!(at(by, 1) > at(Criterion::Linear, 1));
            assert!(at(by, 9) < at(Criterion::Linear, 9));
        }
        assert!(at(Criterion::Quadratic, 1) < at(Criterion::Linear, 1));
        assert!(at(Criterion::Quadratic, 9) > at(Criterion::Linear, 9));
        assert_eq!(Criterion::Name.penalty(3), None);
        // Taken per letter: a query twice as long scoring twice as much
        // costs the same.
        assert_eq!(
            Criterion::Log.merit(2, 40, 2, 1.0),
            Criterion::Log.merit(2, 80, 4, 1.0)
        );
    }
    #[test]
    fn names_compare_naturally() {
        let mut names = ["file10", "File2", "file2", "a", "file02", "B", "file1b", ""];
        names.sort_by(|a, b| natural(a, b));
        assert_eq!(
            names,
            ["", "a", "B", "file1b", "File2", "file02", "file2", "file10"]
        );
    }
    #[test]
    fn sorts_put_unknown_values_last_and_break_ties_by_the_ranking() {
        let row = |path: &str, depth: usize| Candidate {
            path: path.into(),
            directory: false,
            depth,
            root: 0,
            order: 0,
        };
        let key = |score: i64, value: i64| Key { score, value };
        let (a, b, c) = (row("/r/a", 2), row("/r/b", 1), row("/r/c", 1));
        let sort = |by, descending| Order {
            sort: Some(Sort { by, descending }),
            ..Rank::Score.into()
        };
        for descending in [false, true] {
            let size = sort(Criterion::Size, descending);
            assert!(size.cmp(&a, key(0, 5), &b, key(0, UNKNOWN)).is_lt());
            assert!(size.cmp(&b, key(0, UNKNOWN), &a, key(0, 5)).is_gt());
        }
        let size = sort(Criterion::Size, true);
        assert!(size.cmp(&a, key(0, 9), &b, key(0, 5)).is_lt());
        // Equal sizes: the best match first.
        assert!(size.cmp(&a, key(1, 5), &b, key(9, 5)).is_gt());
        let depth = sort(Criterion::Depth, true);
        assert!(depth.cmp(&b, key(0, 0), &c, key(9, 0)).is_lt());
        assert!(depth.cmp(&b, key(0, 0), &a, key(9, 0)).is_lt());
        // Descending keeps a criterion's own order: names from A.
        assert!(
            sort(Criterion::Name, true)
                .cmp(&a, key(0, 0), &b, key(0, 0))
                .is_lt()
        );
        assert!(
            sort(Criterion::Name, false)
                .cmp(&a, key(0, 0), &b, key(0, 0))
                .is_gt()
        );
    }
    #[test]
    fn runs_pushed_one_by_one_read_as_one_sorted_run() {
        for order in orders() {
            let every: Vec<_> = (0..6000).map(candidate).collect();
            let expected = run(order, every.clone());
            let mut rows = Rows::default();
            // Read only from the front until the end: runs merged lazily.
            let mut lazy = Rows::default();
            let mut pushed = 0;
            for size in [1, 7, 300, 2, 900, 33, 1500, 5, 1200, 2052] {
                rows.push(order, run(order, every[pushed..pushed + size].to_vec()));
                lazy.push(order, run(order, every[pushed..pushed + size].to_vec()));
                pushed += size;
                assert!(lazy.all.get().is_none());
                assert!(lazy.runs.len() <= 1 + (pushed as f64).log2() as usize);
                let front = run(order, every[..pushed].to_vec());
                let n = pushed.min(LAZY);
                let read: Vec<_> = lazy.iter_from(0).take(n).cloned().collect();
                assert_eq!(read, front.rows[..n]);
                assert_eq!(lazy.head.keys, front.keys[..n]);
                assert_eq!(lazy.row(n / 2), front.rows.get(n / 2));
                assert_eq!(lazy.position(|c| *c == front.rows[n - 1]), Some(n - 1));
                // Never more runs than doublings.
                assert!(rows.runs.len() <= 1 + (pushed as f64).log2() as usize);
                let sorted = run(order, every[..pushed].to_vec());
                let paths = |rows: &mut dyn Iterator<Item = &Arc<Candidate>>| {
                    rows.map(|c| c.path.clone()).collect::<Vec<_>>()
                };
                let want = paths(&mut sorted.rows.iter());
                // Read lazily from the front, from the middle, and whole.
                assert_eq!(
                    paths(&mut rows.iter_from(0).take(50)),
                    want[..50.min(pushed)]
                );
                let mid = pushed / 2;
                assert_eq!(
                    rows.row(mid).map(|c| c.path.clone()),
                    want.get(mid).cloned()
                );
                let found = rows.position(|c| c.path == want[pushed - 1]);
                assert_eq!(found, Some(pushed - 1));
                assert_eq!(paths(&mut rows.all().iter()), want);
                assert_eq!(rows.len(), pushed);
            }
            assert_eq!(pushed, every.len());
            assert_eq!(lazy.all(), expected.rows.as_slice());
            assert_eq!(rows.merged().keys, expected.keys);
            // Rows dropped leave the rest in order, whether merged or not.
            rows.push(order, run(order, (6000..6100).map(candidate).collect()));
            rows.retain(|c| !c.path.to_string_lossy().ends_with('7'));
            let kept: Vec<_> = (0..6100)
                .map(candidate)
                .filter(|(_, c)| !c.path.to_string_lossy().ends_with('7'))
                .collect();
            let kept = run(order, kept);
            assert_eq!(rows.all(), kept.rows.as_slice());
            assert_eq!(rows.merged().keys, kept.keys);
            // And in runs not merged yet, the first rows again.
            let mut lazy = Rows::default();
            for part in (0..6100).collect::<Vec<u64>>().chunks(700) {
                lazy.push(
                    order,
                    run(order, part.iter().map(|&i| candidate(i)).collect()),
                );
            }
            lazy.retain(|c| !c.path.to_string_lossy().ends_with('7'));
            assert!(lazy.all.get().is_none() && lazy.runs.len() > 1);
            let read: Vec<_> = lazy.iter_from(0).take(LAZY).cloned().collect();
            assert_eq!(read, kept.rows[..LAZY]);
            assert_eq!(lazy.len(), kept.len());
            assert_eq!(lazy.all(), kept.rows.as_slice());
        }
    }
    #[test]
    fn reading_a_short_list_to_its_end_leaves_it_in_runs() {
        // What a frame reads (a page past the last row, the row after the
        // highlight) when every row is among the first ones.
        let mut rows = Rows::default();
        for part in (0..3000).collect::<Vec<u64>>().chunks(100) {
            assert_eq!(rows.iter_from(rows.len() + 30).count(), 0);
            assert!(rows.row(rows.len()).is_none());
            if rows.len() <= LAZY {
                assert_eq!(rows.position(|_| false), None);
            }
            assert!(rows.all.get().is_none(), "after {} rows", rows.len());
            rows.push(
                Order::default(),
                run(
                    Order::default(),
                    part.iter().map(|&i| candidate(i)).collect(),
                ),
            );
        }
        assert!(rows.runs.len() > 1);
    }
    #[test]
    fn galloping_counts_what_holds() {
        for len in 0..40 {
            for count in 0..=len {
                assert_eq!(leading(len, |i| i < count), count);
            }
        }
    }
}
