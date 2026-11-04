//! The results' order and the rows kept in it. The order is one value that
//! every comparison of rows goes through, so that another order is a
//! variant here rather than a change to ranking and merging. The rows are
//! sorted runs, merged lazily: only as far as a reader looks, or all at
//! once for whatever needs every row in order (`Rows::all`).
use crate::Candidate;
use std::{cell::OnceCell, cmp::Ordering, sync::Arc};

/// How rows are ordered. Each program has one (`Program::order`, read as
/// `App::order`); a sorting picker would add variants and choose among them
/// there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// Depth, directories first, best score, root order, then pathname
    /// bytes, which break every tie.
    #[default]
    Depth,
    /// The best score first, then as `Depth` (`fff pick` and `fff seek`).
    Score,
}
impl Order {
    /// Compares two rows with their scores for the current query.
    pub fn cmp(self, a: &Candidate, score_a: i64, b: &Candidate, score_b: i64) -> Ordering {
        let score = score_b.cmp(&score_a);
        let depth = a.depth.cmp(&b.depth).then(b.directory.cmp(&a.directory));
        let (first, second) = match self {
            Self::Depth => (depth, score),
            Self::Score => (score, depth),
        };
        first
            .then(second)
            .then(a.root.cmp(&b.root))
            .then(a.path.as_os_str().cmp(b.path.as_os_str()))
    }
}

/// Rows sorted in one order, with their scores for the query they were
/// ranked by, in the same order.
#[derive(Clone, Debug, Default)]
pub struct Run {
    pub rows: Vec<Arc<Candidate>>,
    pub scores: Vec<i64>,
}
impl Run {
    pub fn len(&self) -> usize {
        self.rows.len()
    }
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

/// Rows read from the front, up to this many, are merged from the runs as
/// they are read; reading past them merges every run once (`Rows::all`).
pub const LAZY: usize = 2048;

/// The ranked rows: sorted runs, the largest first, each at least twice as
/// large as the next, merged lazily. A ranking makes one run; each round of
/// a scan's new paths is pushed as another, so no row already ranked moves.
/// Clones share the runs (kept queries, rankings on another thread).
#[derive(Clone, Default)]
pub struct Rows {
    order: Order,
    runs: Vec<Arc<Run>>,
    len: usize,
    /// Every row in order, once something needed them all; until the rows
    /// change, when it becomes their only run.
    all: OnceCell<Arc<Run>>,
}
impl Rows {
    /// Rows ranked all at once.
    pub(crate) fn sorted(order: Order, run: Run) -> Self {
        let len = run.len();
        Self {
            order,
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
    /// runs first (once, until they change).
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
        self.iter_from(i).next()
    }
    /// The rows in order from row `from` on. Near the front they are merged
    /// as they are read; past `LAZY`, every run is merged first.
    pub fn iter_from(&self, from: usize) -> Box<dyn Iterator<Item = &Arc<Candidate>> + '_> {
        match self.all.get() {
            Some(all) => Box::new(all.rows.iter().skip(from)),
            None if from >= LAZY || self.runs.len() < 2 => Box::new(self.all().iter().skip(from)),
            None => Box::new(self.lazy().skip(from)),
        }
    }
    /// Where the first row `found` holds for is, in order.
    pub fn position(&self, mut found: impl FnMut(&Arc<Candidate>) -> bool) -> Option<usize> {
        if self.all.get().is_none() && self.runs.len() > 1 {
            if let Some(i) = self.lazy().take(LAZY).position(&mut found) {
                return Some(i);
            }
            if self.len <= LAZY {
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
    /// Merges the runs' fronts as they are read, leaving them as they are.
    fn lazy(&self) -> Lazy<'_> {
        Lazy {
            order: self.order,
            runs: &self.runs,
            at: vec![0; self.runs.len()],
        }
    }
    /// The rows are about to change: every row merged becomes the only run.
    fn thaw(&mut self) {
        if let Some(all) = self.all.take() {
            let runs = std::mem::take(&mut self.runs);
            if runs.len() > 1 {
                crate::known::discard(runs, self.len);
            }
            if !all.is_empty() {
                self.runs.push(all);
            }
        }
    }
    /// Add `run`, sorted in `order`: it becomes a run of its own, merged
    /// with the last ones while the one before it is not twice as large.
    pub(crate) fn push(&mut self, order: Order, run: Run) {
        if run.is_empty() {
            return;
        }
        if self.len == 0 {
            self.order = order;
        }
        debug_assert_eq!(self.order, order, "rows are kept in one order");
        self.thaw();
        self.len += run.len();
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
        self.thaw();
        for run in &mut self.runs {
            let kept: Vec<bool> = run.rows.iter().map(&mut keep).collect();
            if kept.iter().all(|k| *k) {
                continue;
            }
            let run = Arc::make_mut(run);
            let mut each = kept.iter();
            run.rows.retain(|_| *each.next().expect("one per row"));
            let mut each = kept.iter();
            run.scores.retain(|_| *each.next().expect("one per row"));
        }
        self.runs.retain(|r| !r.is_empty());
        self.len = self.runs.iter().map(|r| r.len()).sum();
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

/// The runs' rows in order, merged as they are read.
struct Lazy<'a> {
    order: Order,
    runs: &'a [Arc<Run>],
    /// Each run's next row.
    at: Vec<usize>,
}
impl<'a> Iterator for Lazy<'a> {
    type Item = &'a Arc<Candidate>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut best: Option<usize> = None;
        for (j, run) in self.runs.iter().enumerate() {
            let i = self.at[j];
            if i >= run.len() {
                continue;
            }
            best = match best {
                Some(b) => {
                    let (other, k) = (&self.runs[b], self.at[b]);
                    let before = self
                        .order
                        .cmp(&run.rows[i], run.scores[i], &other.rows[k], other.scores[k])
                        .is_lt();
                    Some(if before { j } else { b })
                }
                None => Some(j),
            };
        }
        let j = best?;
        self.at[j] += 1;
        Some(&self.runs[j].rows[self.at[j] - 1])
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
        scores: Vec::with_capacity(a.len() + b.len()),
    };
    let (mut a_rows, mut a_scores) = (a.rows.into_iter(), a.scores.into_iter());
    let (mut b_rows, mut b_scores) = (b.rows.into_iter(), b.scores.into_iter());
    while let (Some(head), Some(&score)) = (b_rows.as_slice().first(), b_scores.as_slice().first())
    {
        let (rows, scores) = (a_rows.as_slice(), a_scores.as_slice());
        let n = leading(rows.len(), |i| {
            order.cmp(&rows[i], scores[i], head, score).is_le()
        });
        merged.rows.extend(a_rows.by_ref().take(n));
        merged.scores.extend(a_scores.by_ref().take(n));
        let (Some(head), Some(&score)) = (a_rows.as_slice().first(), a_scores.as_slice().first())
        else {
            break;
        };
        let (rows, scores) = (b_rows.as_slice(), b_scores.as_slice());
        let n = leading(rows.len(), |i| {
            order.cmp(&rows[i], scores[i], head, score).is_lt()
        });
        merged.rows.extend(b_rows.by_ref().take(n));
        merged.scores.extend(b_scores.by_ref().take(n));
    }
    merged.rows.extend(a_rows.chain(b_rows));
    merged.scores.extend(a_scores.chain(b_scores));
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
    fn run(order: Order, mut rows: Vec<(i64, Candidate)>) -> Run {
        rows.sort_by(|(sa, a), (sb, b)| order.cmp(a, *sa, b, *sb));
        let (scores, rows) = rows.into_iter().map(|(s, c)| (s, Arc::new(c))).unzip();
        Run { rows, scores }
    }
    fn candidate(i: u64) -> (i64, Candidate) {
        let n = i.wrapping_mul(2_654_435_761) % 10_007;
        (
            (n % 5) as i64,
            Candidate {
                path: format!("/r/d{}/f{n:05}", n % 13).into(),
                directory: n.is_multiple_of(3),
                depth: 1 + (n % 4) as usize,
                root: (n % 2) as usize,
            },
        )
    }
    #[test]
    fn runs_pushed_one_by_one_read_as_one_sorted_run() {
        for order in [Order::Depth, Order::Score] {
            let every: Vec<_> = (0..6000).map(candidate).collect();
            let expected = run(order, every.clone());
            let mut rows = Rows::default();
            let mut pushed = 0;
            for size in [1, 7, 300, 2, 900, 33, 1500, 5, 1200, 2052] {
                rows.push(order, run(order, every[pushed..pushed + size].to_vec()));
                pushed += size;
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
            assert_eq!(rows.merged().scores, expected.scores);
            // Rows dropped leave the rest in order, whether merged or not.
            rows.push(order, run(order, (6000..6100).map(candidate).collect()));
            rows.retain(|c| !c.path.to_string_lossy().ends_with('7'));
            let kept: Vec<_> = (0..6100)
                .map(candidate)
                .filter(|(_, c)| !c.path.to_string_lossy().ends_with('7'))
                .collect();
            let kept = run(order, kept);
            assert_eq!(rows.all(), kept.rows.as_slice());
            assert_eq!(rows.merged().scores, kept.scores);
        }
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
