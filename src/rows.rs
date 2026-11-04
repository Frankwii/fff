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
                .zip(self.head.scores.last())
                .is_some_and(|(c, s)| order.cmp(&run.rows[0], run.scores[0], c, *s).is_gt());
        if !late {
            let head = std::mem::take(&mut self.head);
            let mut merged = merge_two(order, head, front(&run));
            merged.rows.truncate(LAZY);
            merged.scores.truncate(LAZY);
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
            run.scores.retain(|_| *each.next().expect("one per row"));
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
        self.head.scores.truncate(LAZY);
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
        scores: run.scores[..n].to_vec(),
    }
}
/// Insert `added` among the rows of `into`, both sorted, in place: rows
/// ranked before every added one stay where they are, and as scans deliver
/// shallow paths first, insertions mostly land near the end.
fn insert(order: Order, into: &mut Run, added: Run) {
    let Run { rows, scores } = into;
    // Where an insertion goes: searched back from the end in growing
    // steps, as each lands near the one after it, then halving.
    let before = |rows: &[Arc<Candidate>], scores: &[i64], c: &Candidate, s: i64| {
        let lt = |i: usize| order.cmp(&rows[i], scores[i], c, s).is_lt();
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
    scores.resize(end + k, 0);
    for (i, (c, score)) in added.rows.into_iter().zip(added.scores).enumerate().rev() {
        let at = before(&rows[..end], &scores[..end], &c, score);
        // Rows at..end move up by the i + 1 insertions left.
        rows[at..end + i + 1].rotate_right(i + 1);
        scores.copy_within(at..end, at + i + 1);
        rows[at + i] = c;
        scores[at + i] = score;
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
                assert_eq!(lazy.head.scores, front.scores[..n]);
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
                Order::Depth,
                run(Order::Depth, part.iter().map(|&i| candidate(i)).collect()),
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
