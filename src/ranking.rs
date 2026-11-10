//! Ranking rows away from the input thread: a large listing is scored and
//! sorted from a snapshot, in parallel pieces, while the previous rows stay on
//! screen. Small ones are ranked in place, exactly the same way.
#[cfg(test)]
use crate::score;
use crate::{
    Candidate, Listing, Matcher,
    labels::{Names, Reader},
    rows::{FolderSizes, Key, Order, Run},
    sizes::Totals,
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

/// Candidates from which ranking runs on its own thread.
pub(crate) const BACKGROUND_AT: usize = 16_384;
/// The most entries rows are labelled by name for: past it, labelling every
/// listed entry, and matching their labels, would slow typing.
pub const LABELLED_AT: usize = 10_000;
/// Rows with their keys, or none (every known path).
type Slice<'a> = (&'a [Arc<Candidate>], &'a [Key]);
/// Candidates checked between looks at the cancellation flag.
const CHECK_EVERY: usize = 4096;
/// The most threads a ranking uses, fewer when the machine has fewer: past
/// about 16, efficiency cores and second threads of a core slow it down.
const MAX_THREADS: usize = 16;

pub(crate) struct Job {
    /// Every known path, filtered by `listing`, or rows that are already
    /// listed: shared runs, read in no particular order. Rows ranked in
    /// this order already keep their keys' values; others have no keys.
    pub base: Vec<Arc<Run>>,
    pub listing: Option<Listing>,
    /// The lowercased query.
    pub query: String,
    /// Group the listed entries for their labels again; `Some(true)` when
    /// they are wanted: then, unless more than `LABELLED_AT` are listed,
    /// each is matched by its label (else the groups are left empty).
    pub labels: Option<bool>,
    /// Rows already listed are matched by the labels these names give
    /// them, as they are shown.
    pub labelled: Option<Reader>,
    /// The order to rank in.
    pub order: Order,
    /// Folders' sizes, for sorting by them.
    pub totals: Totals,
    /// Rows with keys take their folders' sizes from `totals` again: those
    /// added up since they were ranked (`App::follow_sizes`).
    pub resize: bool,
}
pub(crate) struct Ranked {
    pub run: Run,
    pub names: Option<Names>,
    /// The rows show labels and match by them: labels were wanted and few
    /// enough entries are listed.
    pub labelled: bool,
    /// How many entries are listed, from a ranking of every known path.
    pub listed: Option<usize>,
}

impl Job {
    /// How many rows it reads.
    pub fn len(&self) -> usize {
        self.base.iter().map(|r| r.len()).sum()
    }
    /// The ranked rows, or `None` once `cancel` is set.
    pub fn run(self, cancel: &AtomicBool) -> Option<Ranked> {
        if self.labels == Some(true) && self.listing.is_some() {
            return self.run_labelled(cancel);
        }
        if let Some(reader) = &self.labelled {
            let (run, _, _) = reader.with(|names| self.rank(Pass::Labels(names), cancel))?;
            return Some(Ranked {
                run,
                names: None,
                labelled: true,
                listed: None,
            });
        }
        let (run, listed, count) = self.rank(Pass::Paths, cancel)?;
        let names = self.labels.map(|_| Names::new(listed));
        Some(Ranked {
            run,
            names,
            labelled: false,
            listed: self.listing.is_some().then_some(count),
        })
    }
    /// Every listed entry, grouped for labels when few enough are, then
    /// ranked: by its label then, else by its path.
    fn run_labelled(mut self, cancel: &AtomicBool) -> Option<Ranked> {
        let (_, listed, count) = self.rank(Pass::List, cancel)?;
        let labelled = count <= LABELLED_AT;
        let names = Names::new(if labelled { listed.clone() } else { Vec::new() });
        self.base = vec![Arc::new(Run {
            rows: listed,
            keys: Vec::new(),
        })];
        self.listing = None;
        self.labels = None;
        let pass = if labelled {
            Pass::Labels(&names)
        } else {
            Pass::Paths
        };
        let (run, _, _) = self.rank(pass, cancel)?;
        Some(Ranked {
            run,
            names: Some(names),
            labelled,
            listed: Some(count),
        })
    }
    /// The rows ranked as `pass` says, with every listed candidate when
    /// labels are wanted, and how many are listed.
    #[allow(clippy::type_complexity)]
    fn rank(&self, pass: Pass, cancel: &AtomicBool) -> Option<(Run, Vec<Arc<Candidate>>, usize)> {
        let threads = std::thread::available_parallelism()
            .map_or(1, |n| n.get())
            .clamp(1, MAX_THREADS)
            .min(self.len() / CHECK_EVERY + 1);
        let piece = self.len().div_ceil(threads).max(1);
        let slices: Vec<Slice> = self
            .base
            .iter()
            .map(|r| (r.rows.as_slice(), r.keys.as_slice()))
            .collect();
        let keep_listed = self.labels == Some(true);
        // Folders' totals read here, on one thread, rather than by each
        // piece's: drawing reads them too.
        let sizes = if self.resize {
            let rows = self.base.iter().flat_map(|r| r.rows.iter());
            FolderSizes::read(rows, &self.totals)
        } else {
            FolderSizes::default()
        };
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let sizes = &sizes;
        // Each piece is filtered, scored and sorted on its own thread; a
        // single piece here, as starting a thread costs more than small work.
        let pieces: Vec<_> = if threads == 1 {
            vec![self.rank_piece(&slices, keep_listed, pass, sizes, cancel)]
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
                    .map(|chunk| {
                        scope.spawn(move || {
                            job.rank_piece(&[chunk], keep_listed, pass, sizes, cancel)
                        })
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().ok().flatten())
                    .collect()
            })
        };
        let mut sorted = Vec::with_capacity(pieces.len());
        let mut listed = Vec::new();
        let mut count = 0;
        for piece in pieces {
            let (rows, kept, counted) = piece?;
            sorted.push(rows);
            listed.extend(kept);
            count += counted;
        }
        let rows = merge_all(sorted, self.order, cancel)?;
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        let (keys, rows) = rows.into_iter().unzip();
        Some((Run { rows, keys }, listed, count))
    }
    /// One piece's listed rows, sorted, with every listed candidate when
    /// labels need them, and how many are listed.
    #[allow(clippy::type_complexity)]
    fn rank_piece(
        &self,
        chunk: &[Slice],
        keep_listed: bool,
        pass: Pass,
        sizes: &FolderSizes,
        cancel: &AtomicBool,
    ) -> Option<(Vec<(Key, Arc<Candidate>)>, Vec<Arc<Candidate>>, usize)> {
        let matcher = Matcher::new(&self.query);
        let letters = matcher.letters();
        let mut ranked = Vec::new();
        let mut listed = Vec::new();
        let mut count = 0;
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
            count += 1;
            if keep_listed {
                listed.push(Arc::clone(c));
            }
            let score = match pass {
                Pass::List => continue,
                Pass::Labels(names) => matcher.score_path(&names.text(c)),
                Pass::Paths => matcher.score_candidate(c),
            };
            if let Some(score) = score {
                let key = match key {
                    Some(key) if self.resize => {
                        let key = self.order.resize(*key, c, sizes);
                        self.order.rescore(key, c, score, letters)
                    }
                    Some(key) => self.order.rescore(*key, c, score, letters),
                    None => self.order.key(c, score, letters, &self.totals, None),
                };
                ranked.push((key, Arc::clone(c)));
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        // Pathnames break every tie, so an unstable sort yields the same order.
        ranked.sort_unstable_by(|(sa, a), (sb, b)| self.order.cmp(a, *sa, b, *sb));
        Some((ranked, listed, count))
    }
}

/// What a ranking's pass over the rows does.
#[derive(Clone, Copy)]
enum Pass<'a> {
    /// Only list them, for labels.
    List,
    /// Match each by its label from these names.
    Labels(&'a Names),
    /// Match each by its path below its root.
    Paths,
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
#[cfg(test)]
fn labelled_pick(paths: &[&str]) -> crate::App {
    use crate::{App, Policy};
    let mut app = App::new(vec!["/w".into()], 6, Policy::default());
    app.set_program(crate::program::Program::Pick);
    app.kinds = crate::Kinds::Files;
    for path in paths {
        app.insert(0, file(path));
    }
    app.scanning = false;
    app
}
#[cfg(test)]
fn file(path: &str) -> Candidate {
    Candidate {
        path: path.into(),
        directory: false,
        depth: 0,
        root: 0,
        order: 0,
    }
}
#[cfg(test)]
fn row_paths(app: &crate::App) -> Vec<String> {
    app.rows
        .iter()
        .map(|c| c.path.display().to_string())
        .collect()
}
#[cfg(test)]
#[test]
fn labelled_rows_match_what_they_show_whatever_their_depth() {
    let book = "/w/minicozzi_colding_a_course_in_minimal_surfaces.djvu";
    let paper = "/w/problems/limit/bibliography/krupa_szmolyan_oscillation_canard_explosion.pdf";
    let brezis = "/w/notes_books/functional_analysis/Brezis.pdf";
    let mut app = labelled_pick(&[book, paper, brezis]);
    app.query = "canard".into();
    app.refresh();
    assert!(app.labels_shown());
    // Depth no longer outweighs the match, and Brezis.pdf, which matches
    // only through folders the row does not show, is not listed.
    assert_eq!(row_paths(&app), [paper, book]);
    let paper = &app.rows.iter().next().expect("a row").clone();
    assert_eq!(
        app.matched_text(paper),
        "krupa_szmolyan_oscillation_canard_explosion.pdf"
    );
    // Typing on narrows by the labels too.
    app.query = "canarde".into();
    app.refresh();
    assert_eq!(row_paths(&app).len(), 1);
    // With folders listed, rows show paths, which the query matches, and
    // depth counts.
    app.kinds = crate::Kinds::All;
    app.query = "canard".into();
    app.refresh();
    assert!(!app.labels_shown());
    assert_eq!(row_paths(&app)[0], book);
    assert!(row_paths(&app).iter().any(|p| p == brezis));
}
#[cfg(test)]
#[test]
fn a_name_shared_later_relabels_its_entries_as_they_come_and_go() {
    let thesis = "/w/thesis/main.pdf";
    let mut app = labelled_pick(&[thesis]);
    app.query = "thesis".into();
    app.refresh();
    // Alone, it shows as main.pdf, which "thesis" does not match.
    assert!(app.rows.is_empty());
    // Another main.pdf lengthens its label to thesis/main.pdf.
    app.insert(0, file("/w/other/main.pdf"));
    app.refresh();
    assert_eq!(row_paths(&app), [thesis]);
    // Once the other one is gone, it is main.pdf again.
    app.generation += 1;
    app.insert(app.generation, file(thesis));
    app.reconcile(app.generation);
    app.refresh();
    assert!(app.rows.is_empty(), "{:?}", row_paths(&app));
}
#[cfg(test)]
#[test]
fn rows_show_paths_past_the_labelled_limit_and_say_so() {
    let paths: Vec<String> = (0..=LABELLED_AT)
        .map(|i| format!("/w/d{i}/f{i}.txt"))
        .collect();
    let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
    let mut app = labelled_pick(&refs);
    app.refresh();
    app.refresh();
    assert!(!app.labels_shown());
    assert!(
        app.status.contains("10,001 files are listed"),
        "{}",
        app.status
    );
    // Asking for names again says why they do not show.
    app.toggle_labels();
    app.toggle_labels();
    assert!(!app.labels_shown());
    assert!(
        app.status.contains("more than the 10,000"),
        "{}",
        app.status
    );
    // One fewer, and names come back on their own.
    app.generation += 1;
    for path in &refs[1..] {
        app.insert(app.generation, file(path));
    }
    app.reconcile(app.generation);
    app.refresh();
    app.refresh();
    assert!(app.labels_shown());
    assert!(app.status.contains("names again"), "{}", app.status);
    // With folders listed, gl says what shows names.
    app.kinds = crate::Kinds::All;
    app.toggle_labels();
    app.toggle_labels();
    assert!(!app.labels_shown());
    assert!(
        app.status.contains("while folders are listed"),
        "{}",
        app.status
    );
}
