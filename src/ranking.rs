//! Ranking rows away from the input thread: a large listing is scored and
//! sorted from a snapshot, in parallel pieces, while the previous rows stay on
//! screen. Small ones are ranked in place, exactly the same way.
#[cfg(test)]
use crate::score;
use crate::{
    Candidate, Listing, Matcher, disambiguate,
    rows::{Key, Order, Run},
    sizes::Totals,
};
use std::{
    collections::HashMap,
    ffi::OsString,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

/// Listed entries by basename, for `fff pick` and `fff seek` labels.
pub(crate) type Names = HashMap<OsString, Vec<Arc<Candidate>>>;
pub(crate) type Labels = HashMap<PathBuf, PathBuf>;

/// Candidates from which ranking runs on its own thread.
pub(crate) const BACKGROUND_AT: usize = 16_384;
/// Rows with their keys, or none (every known path).
type Slice<'a> = (&'a [Arc<Candidate>], &'a [Key]);
/// Candidates checked between looks at the cancellation flag.
const CHECK_EVERY: usize = 4096;

pub(crate) struct Job {
    /// Every known path, filtered by `listing`, or rows that are already
    /// listed: shared runs, read in no particular order. Rows ranked in
    /// this order already keep their keys' values; others have no keys.
    pub base: Vec<Arc<Run>>,
    pub listing: Option<Listing>,
    /// The lowercased query.
    pub query: String,
    /// Recompute the labels; `Some(true)` when they are shown.
    pub labels: Option<bool>,
    /// The order to rank in.
    pub order: Order,
    /// Folders' sizes, for sorting by them.
    pub totals: Totals,
}
pub(crate) struct Ranked {
    pub run: Run,
    pub labels: Option<(Names, Labels)>,
}

impl Job {
    /// How many rows it reads.
    pub fn len(&self) -> usize {
        self.base.iter().map(|r| r.len()).sum()
    }
    /// The ranked rows, or `None` once `cancel` is set.
    pub fn run(self, cancel: &AtomicBool) -> Option<Ranked> {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, 8)
            .min(self.len() / CHECK_EVERY + 1);
        let piece = self.len().div_ceil(threads).max(1);
        let slices: Vec<Slice> = self
            .base
            .iter()
            .map(|r| (r.rows.as_slice(), r.keys.as_slice()))
            .collect();
        let keep_listed = self.labels == Some(true);
        // Each piece is filtered, scored and sorted on its own thread; a
        // single piece here, as starting a thread costs more than small work.
        let pieces: Vec<_> = if threads == 1 {
            vec![self.rank_piece(&slices, keep_listed, cancel)]
        } else {
            let job = &self;
            std::thread::scope(|scope| {
                let handles: Vec<_> = slices
                    .iter()
                    .flat_map(|&(rows, keys)| {
                        (0..rows.len()).step_by(piece).map(move |at| {
                            let end = (at + piece).min(rows.len());
                            (&rows[at..end], keys.get(at..end).unwrap_or_default())
                        })
                    })
                    .map(|chunk| scope.spawn(move || job.rank_piece(&[chunk], keep_listed, cancel)))
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().ok().flatten())
                    .collect()
            })
        };
        let mut sorted = Vec::with_capacity(pieces.len());
        let mut listed = Vec::new();
        for piece in pieces {
            let (rows, kept) = piece?;
            sorted.push(rows);
            listed.extend(kept);
        }
        let rows = merge_all(sorted, self.order, cancel)?;
        let labels = match self.labels {
            Some(shown) => Some(label(if shown { listed } else { Vec::new() }, cancel)?),
            None => None,
        };
        let (keys, rows) = rows.into_iter().unzip();
        Some(Ranked {
            run: Run { rows, keys },
            labels,
        })
    }
    /// One piece's listed rows, sorted, with every listed candidate when
    /// labels need them.
    #[allow(clippy::type_complexity)]
    fn rank_piece(
        &self,
        chunk: &[Slice],
        keep_listed: bool,
        cancel: &AtomicBool,
    ) -> Option<(Vec<(Key, Arc<Candidate>)>, Vec<Arc<Candidate>>)> {
        let matcher = Matcher::new(&self.query);
        let letters = matcher.letters();
        let mut ranked = Vec::new();
        let mut listed = Vec::new();
        let rows = chunk.iter().flat_map(|&(rows, keys)| {
            let keys = keys.iter().map(Some).chain(std::iter::repeat(None));
            rows.iter().zip(keys)
        });
        for (i, (c, key)) in rows.enumerate() {
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
                let key = match key {
                    Some(key) => self.order.rescore(*key, c, score, letters),
                    None => self.order.key(c, score, letters, &self.totals),
                };
                ranked.push((key, Arc::clone(c)));
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        // Pathnames break every tie, so an unstable sort yields the same order.
        ranked.sort_unstable_by(|(sa, a), (sb, b)| self.order.cmp(a, *sa, b, *sb));
        Some((ranked, listed))
    }
}

/// Merge sorted pieces pairwise, each level's pairs in parallel.
fn merge_all(
    mut pieces: Vec<Vec<(Key, Arc<Candidate>)>>,
    order: Order,
    cancel: &AtomicBool,
) -> Option<Vec<(Key, Arc<Candidate>)>> {
    while pieces.len() > 1 {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let mut pairs = Vec::new();
        let mut rest = pieces.into_iter();
        while let Some(a) = rest.next() {
            pairs.push((a, rest.next()));
        }
        let merge = |(a, b): (_, Option<_>)| match b {
            Some(b) => merge_two(a, b, order, cancel),
            None => Some(a),
        };
        // The last pair is merged on this thread, which would only wait.
        let last = pairs.pop().expect("at least two pieces");
        pieces = std::thread::scope(|scope| {
            let handles: Vec<_> = pairs
                .into_iter()
                .map(|pair| scope.spawn(move || merge(pair)))
                .collect();
            let last = merge(last);
            handles
                .into_iter()
                .map(|h| h.join().expect("merging sorted rows does not panic"))
                .chain([last])
                .collect::<Option<_>>()
        })?;
    }
    Some(pieces.pop().unwrap_or_default())
}
/// `None` once `cancel` is set.
fn merge_two(
    a: Vec<(Key, Arc<Candidate>)>,
    b: Vec<(Key, Arc<Candidate>)>,
    order: Order,
    cancel: &AtomicBool,
) -> Option<Vec<(Key, Arc<Candidate>)>> {
    let mut merged = Vec::with_capacity(a.len() + b.len());
    let mut a = a.into_iter().peekable();
    let mut b = b.into_iter().peekable();
    while let (Some((sa, ca)), Some((sb, cb))) = (a.peek(), b.peek()) {
        if merged.len() % CHECK_EVERY == 0 && cancel.load(Ordering::Relaxed) {
            return None;
        }
        if order.cmp(cb, *sb, ca, *sa).is_lt() {
            merged.extend(b.next());
        } else {
            merged.extend(a.next());
        }
    }
    merged.extend(a);
    merged.extend(b);
    Some(merged)
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

#[cfg(test)]
#[test]
fn typing_over_a_pending_ranking_narrows_the_rows_shown() {
    use crate::{App, Policy};
    let mut app = App::new(vec!["/fixture".into()], 1, Policy::default());
    for i in 0..BACKGROUND_AT {
        app.insert(
            0,
            Candidate {
                path: format!("/fixture/f{i}").into(),
                directory: false,
                depth: 1,
                root: 0,
                order: 0,
            },
        );
    }
    let (waker, _woken) = std::sync::mpsc::sync_channel(1);
    app.set_waker(waker);
    app.refresh();
    app.finish_ranking();
    app.query = "f1".into();
    app.refresh();
    assert!(app.ranking_pending());
    // Typed before "f1" is ranked: "f12" narrows the rows of "" on screen
    // instead of ranking and labelling every known path again.
    app.query = "f12".into();
    app.refresh();
    let pending = app.ranking.as_ref().expect("ranked in the background");
    assert_eq!(pending.shown.as_ref().map(|(_, q)| q.as_str()), Some(""));
    app.finish_ranking();
    assert!(!app.rows.is_empty());
    assert!(
        app.rows
            .iter()
            .all(|c| score(&c.path.to_string_lossy(), "f12").is_some())
    );
}
