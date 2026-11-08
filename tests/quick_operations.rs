//! Operations a key or a frame waits for, on listings far larger than usual:
//! a regression to quadratic work fails them, a slow or busy machine does
//! not. Each is measured in processor time on ten times the rows against a
//! tenth of them, which linear work takes about ten times as long for and
//! quadratic work about a hundred; the ratio is the same on any machine, and
//! a busy one does not change the work, unlike the time it takes. Nothing
//! here touches the filesystem.
use fuzzy_file_finder::{App, Candidate, Policy, rows::natural, score, write_paths};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};

const ROWS: usize = 30_000;

/// `rows` files in folders of 100, two levels below the root.
fn listing(rows: usize) -> App {
    let mut app = App::new(vec![PathBuf::from("/fixture")], 3, Policy::default());
    for folder in 0..rows / 100 {
        app.insert(
            0,
            Candidate {
                path: format!("/fixture/folder{folder:03}").into(),
                directory: true,
                depth: 1,
                root: 0,
                order: 0,
            },
        );
        for file in 0..100 {
            app.insert(
                0,
                Candidate {
                    path: format!("/fixture/folder{folder:03}/report-{file:02}.tex").into(),
                    directory: false,
                    depth: 2,
                    root: 0,
                    order: 0,
                },
            );
        }
    }
    settle(&mut app);
    app
}

/// Merges what was inserted and waits for a ranking on another thread,
/// asleep: spinning would count the wait as work.
fn settle(app: &mut App) {
    let start = std::time::Instant::now();
    app.refresh();
    while !app.settled() {
        assert!(start.elapsed() < Duration::from_secs(300), "never settled");
        app.poll_ranking();
        app.refresh();
        std::thread::sleep(Duration::from_millis(1));
    }
}

/// The processor time this process has used, its ranking threads' too.
fn processor_time() -> Duration {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `now` is a valid timespec for the call to fill.
    let status = unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut now) };
    assert_eq!(status, 0, "{}", std::io::Error::last_os_error());
    Duration::new(now.tv_sec as u64, now.tv_nsec as u32)
}

/// The tests take turns, so that the process's processor time is one's.
static TURN: Mutex<()> = Mutex::new(());

/// The processor time `work` takes.
fn timed(work: impl FnOnce()) -> Duration {
    let start = processor_time();
    work();
    processor_time() - start
}

/// Fails when `check`, given how many rows to work on and answering how
/// long its measured part took, takes much more than ten times as long on
/// ten times the rows. Under 50 ms is quick whatever the ratio: there a
/// moment's noise can count, and quadratic work on these rows takes
/// seconds. A processor quickening or slowing between the two can skew one
/// pair, so a pair that fails is measured again, up to three, keeping the
/// least time of each size: quadratic work fails every pair.
fn linear(what: &str, mut check: impl FnMut(usize) -> Duration) {
    let _turn = TURN.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let (mut few, mut many) = (Duration::MAX, Duration::MAX);
    for _ in 0..3 {
        few = few.min(check(ROWS / 10));
        many = many.min(check(ROWS));
        let ratio = many.as_secs_f64() / few.as_secs_f64().max(1e-9);
        eprintln!("{what}: {few:?} on {} rows, {many:?} on {ROWS}", ROWS / 10);
        if many < Duration::from_millis(50) || ratio < 30.0 {
            return;
        }
    }
    panic!(
        "{what}: {few:?} on {} rows but {many:?} on {ROWS}",
        ROWS / 10
    );
}

#[test]
fn typing_a_query_over_a_large_listing_ranks_it_quickly() {
    linear("typing a query", |rows| {
        let mut app = listing(rows);
        assert_eq!(app.shown_len(), rows + rows / 100);
        // Each key narrows the rows ranked for the one before.
        let took = timed(|| {
            for query in ["r", "re", "rep", "repo", "report-4", "report-42"] {
                app.query = query.into();
                settle(&mut app);
            }
        });
        assert_eq!(app.shown_len(), rows / 100);
        assert!(
            app.shown()
                .iter()
                .all(|c| c.path.to_string_lossy().ends_with("report-42.tex"))
        );
        // Deleting back to an earlier query restores its rows.
        let back = timed(|| {
            app.query.clear();
            settle(&mut app);
        });
        assert_eq!(app.shown_len(), rows + rows / 100);
        took + back
    });
}

#[test]
fn moving_to_either_end_and_back_is_quick() {
    linear("G and gg", |rows| {
        let mut app = listing(rows);
        let took = timed(|| {
            for _ in 0..50 {
                app.move_by(isize::MAX);
                app.move_by(isize::MIN);
            }
        });
        app.move_by(isize::MAX);
        assert_eq!(app.highlight, app.shown().last().map(|c| c.path.clone()));
        took
    });
}

#[test]
fn marking_and_inverting_every_row_is_quick() {
    linear("marking every row", |rows| {
        let mut app = listing(rows);
        let mut listed = vec![];
        let mut took = timed(|| listed = app.shown_paths());
        took += timed(|| app.mark_rows(&listed, false));
        assert_eq!(app.marks.len(), listed.len());
        // Again changes nothing; inverting twice leaves them all marked.
        took += timed(|| app.mark_rows(&listed, false));
        assert_eq!(app.marks.len(), listed.len());
        took += timed(|| app.mark_rows(&listed, true));
        assert!(app.marks.is_empty());
        took += timed(|| app.mark_rows(&listed, true));
        assert_eq!(app.marks.to_vec(), listed);
        took + timed(|| assert!(listed.iter().all(|p| app.marks.contains(p))))
    });
}

#[test]
fn printing_many_paths_is_quick_and_exact() {
    linear("printing relative paths", |rows| {
        let paths: Vec<PathBuf> = (0..rows)
            .map(|i| format!("/home/user/notes/{i:05}.txt").into())
            .collect();
        let mut bytes = Vec::new();
        let took =
            timed(|| write_paths(&mut bytes, &paths, Path::new("/home/user"), true, true).unwrap());
        let printed: Vec<_> = bytes.split(|&b| b == 0).collect();
        // One per path, each ended by a NUL, relative to the base.
        assert_eq!(printed.len(), rows + 1);
        assert_eq!(printed[0], b"notes/00000.txt");
        assert_eq!(printed[rows], b"");
        took
    });
}

#[test]
fn scoring_and_natural_comparison_are_quick() {
    let names = |rows: usize| -> Vec<String> {
        (0..rows * 3)
            .map(|i| format!("chapter {i} - draft v{}.tex", i % 7))
            .collect()
    };
    linear("scoring names", |rows| {
        let names = names(rows);
        let mut matched = 0;
        let took = timed(|| {
            matched = names
                .iter()
                .filter(|n| score(n, "chdrv6").is_some())
                .count()
        });
        assert!(matched > 0 && matched < names.len(), "{matched}");
        took
    });
    linear("natural sort", |rows| {
        let mut sorted = names(rows);
        let took = timed(|| sorted.sort_by(|a, b| natural(a, b)));
        // Numbers by value: chapter 2 before chapter 10.
        let position = |n: &str| sorted.iter().position(|s| s.starts_with(n)).unwrap();
        assert!(position("chapter 2 ") < position("chapter 10 "));
        took
    });
}
