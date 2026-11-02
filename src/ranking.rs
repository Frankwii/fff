//! Ranking rows away from the input thread: a large listing is scored and
//! sorted from a snapshot, in parallel pieces, while the previous rows stay on
//! screen. Small ones are ranked in place, exactly the same way.
use crate::{Candidate, Listing, Matcher, disambiguate, rank};
use std::{
    collections::HashMap,
    ffi::OsString,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Ranked rows, shared with kept queries and running rankings.
pub type Rows = Arc<Vec<Arc<Candidate>>>;
/// Listed entries by basename, for `fff pick` and `fff seek` labels.
pub(crate) type Names = HashMap<OsString, Vec<Arc<Candidate>>>;
pub(crate) type Labels = HashMap<PathBuf, PathBuf>;

/// Candidates from which ranking runs on its own thread.
pub(crate) const BACKGROUND_AT: usize = 16_384;
/// Candidates checked between looks at the cancellation flag.
const CHECK_EVERY: usize = 4096;

pub(crate) struct Job {
    /// Every known path, filtered by `listing`, or rows that are already listed.
    pub base: Rows,
    pub listing: Option<Listing>,
    /// The lowercased query.
    pub query: String,
    /// Recompute the labels; `Some(true)` when they are shown.
    pub labels: Option<bool>,
}
pub(crate) struct Ranked {
    pub rows: Vec<Arc<Candidate>>,
    pub labels: Option<(Names, Labels)>,
}

impl Job {
    /// The ranked rows, or `None` once `cancel` is set.
    pub fn run(self, cancel: &AtomicBool) -> Option<Ranked> {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, 8)
            .min(self.base.len() / CHECK_EVERY + 1);
        let piece = self.base.len().div_ceil(threads).max(1);
        let keep_listed = self.labels == Some(true);
        // Each piece is filtered, scored and sorted on its own thread.
        let pieces: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = self
                .base
                .chunks(piece)
                .map(|chunk| scope.spawn(|| self.rank_piece(chunk, keep_listed, cancel)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().ok().flatten())
                .collect()
        });
        let mut sorted = Vec::with_capacity(pieces.len());
        let mut listed = Vec::new();
        for piece in pieces {
            let (rows, kept) = piece?;
            sorted.push(rows);
            listed.extend(kept);
        }
        let rows = merge_all(sorted, cancel)?;
        let labels = match self.labels {
            Some(shown) => Some(label(if shown { listed } else { Vec::new() }, cancel)?),
            None => None,
        };
        Some(Ranked {
            rows: rows.into_iter().map(|(_, c)| c).collect(),
            labels,
        })
    }
    /// One piece's listed rows, sorted, with every listed candidate when
    /// labels need them.
    #[allow(clippy::type_complexity)]
    fn rank_piece(
        &self,
        chunk: &[Arc<Candidate>],
        keep_listed: bool,
        cancel: &AtomicBool,
    ) -> Option<(Vec<(i64, Arc<Candidate>)>, Vec<Arc<Candidate>>)> {
        let matcher = Matcher::new(&self.query);
        let mut ranked = Vec::new();
        let mut listed = Vec::new();
        for (i, c) in chunk.iter().enumerate() {
            if i % CHECK_EVERY == 0 && cancel.load(Ordering::Relaxed) {
                return None;
            }
            if self.listing.as_ref().is_some_and(|l| !l.listed(c)) {
                continue;
            }
            if keep_listed {
                listed.push(Arc::clone(c));
            }
            if let Some(score) = matcher.score_candidate(c) {
                ranked.push((score, Arc::clone(c)));
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        // Pathnames break every tie, so an unstable sort yields the same order.
        ranked.sort_unstable_by(|(sa, a), (sb, b)| rank(a, *sa, b, *sb));
        Some((ranked, listed))
    }
}

/// Merge sorted pieces pairwise, each level's pairs in parallel.
fn merge_all(
    mut pieces: Vec<Vec<(i64, Arc<Candidate>)>>,
    cancel: &AtomicBool,
) -> Option<Vec<(i64, Arc<Candidate>)>> {
    while pieces.len() > 1 {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let mut pairs = Vec::new();
        let mut rest = pieces.into_iter();
        while let Some(a) = rest.next() {
            pairs.push((a, rest.next()));
        }
        pieces = std::thread::scope(|scope| {
            let handles: Vec<_> = pairs
                .into_iter()
                .map(|(a, b)| {
                    scope.spawn(move || match b {
                        Some(b) => merge_two(a, b),
                        None => a,
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("merging sorted rows does not panic"))
                .collect()
        });
    }
    Some(pieces.pop().unwrap_or_default())
}
fn merge_two(
    a: Vec<(i64, Arc<Candidate>)>,
    b: Vec<(i64, Arc<Candidate>)>,
) -> Vec<(i64, Arc<Candidate>)> {
    let mut merged = Vec::with_capacity(a.len() + b.len());
    let mut a = a.into_iter().peekable();
    let mut b = b.into_iter().peekable();
    while let (Some((sa, ca)), Some((sb, cb))) = (a.peek(), b.peek()) {
        if rank(cb, *sb, ca, *sa).is_lt() {
            merged.extend(b.next());
        } else {
            merged.extend(a.next());
        }
    }
    merged.extend(a);
    merged.extend(b);
    merged
}
/// Group `listed` by basename and label the groups that share one.
fn label(listed: Vec<Arc<Candidate>>, cancel: &AtomicBool) -> Option<(Names, Labels)> {
    let mut names: Names = HashMap::new();
    for c in listed {
        if let Some(name) = c.path.file_name() {
            names.entry(name.to_os_string()).or_default().push(c);
        }
    }
    let mut labels = HashMap::new();
    for (i, group) in names.values().filter(|g| g.len() > 1).enumerate() {
        if i % 256 == 0 && cancel.load(Ordering::Relaxed) {
            return None;
        }
        labels.extend(disambiguate(group.iter().map(|c| c.path.as_path())));
    }
    Some((names, labels))
}
