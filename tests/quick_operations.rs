//! Operations a key or a frame waits for, on listings far larger than usual,
//! within budgets generous enough for an unoptimized build on a busy
//! machine: a regression to quadratic work fails them, ordinary noise does
//! not. Nothing here touches the filesystem.
use fuzzy_file_finder::{App, Candidate, Policy, rows::natural, score, write_paths};
use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

const ROWS: usize = 30_000;

/// `ROWS` files in 300 folders of 100, two levels below the root.
fn listing() -> App {
    let mut app = App::new(vec![PathBuf::from("/fixture")], 3, Policy::default());
    for folder in 0..ROWS / 100 {
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

/// Merges what was inserted and waits for a ranking on another thread.
fn settle(app: &mut App) {
    let start = Instant::now();
    app.refresh();
    while !app.settled() {
        app.poll_ranking();
        app.refresh();
        assert!(start.elapsed() < Duration::from_secs(30), "never settled");
        std::thread::yield_now();
    }
}

/// Runs `work`, failing when it takes longer than `budget`.
fn within<T>(what: &str, budget: Duration, work: impl FnOnce() -> T) -> T {
    let start = Instant::now();
    let result = work();
    let took = start.elapsed();
    eprintln!("{what}: {took:?}");
    assert!(took < budget, "{what} took {took:?}, over {budget:?}");
    result
}

#[test]
fn typing_a_query_over_a_large_listing_ranks_it_quickly() {
    let mut app = listing();
    assert_eq!(app.shown_len(), ROWS + ROWS / 100);
    // Each key narrows the rows ranked for the one before.
    for query in ["r", "re", "rep", "repo", "report-4", "report-42"] {
        within(query, Duration::from_secs(2), || {
            app.query = query.into();
            settle(&mut app);
        });
    }
    assert_eq!(app.shown_len(), ROWS / 100);
    assert!(
        app.shown()
            .iter()
            .all(|c| c.path.to_string_lossy().ends_with("report-42.tex"))
    );
    // Deleting back to an earlier query restores its rows.
    within("back to empty", Duration::from_secs(2), || {
        app.query.clear();
        settle(&mut app);
    });
    assert_eq!(app.shown_len(), ROWS + ROWS / 100);
}

#[test]
fn moving_to_either_end_and_back_is_quick() {
    let mut app = listing();
    within("G and gg", Duration::from_secs(1), || {
        for _ in 0..50 {
            app.move_by(isize::MAX);
            app.move_by(isize::MIN);
        }
    });
    app.move_by(isize::MAX);
    assert_eq!(app.highlight, app.shown().last().map(|c| c.path.clone()));
}

#[test]
fn marking_and_inverting_every_row_is_quick() {
    let mut app = listing();
    let rows = within("listed paths", Duration::from_secs(1), || app.shown_paths());
    within("m", Duration::from_secs(1), || app.mark_rows(&rows, false));
    assert_eq!(app.marks.len(), rows.len());
    // Again changes nothing; inverting twice leaves them all marked.
    within("m again", Duration::from_secs(1), || {
        app.mark_rows(&rows, false)
    });
    assert_eq!(app.marks.len(), rows.len());
    within("M", Duration::from_secs(1), || app.mark_rows(&rows, true));
    assert!(app.marks.is_empty());
    within("M again", Duration::from_secs(1), || {
        app.mark_rows(&rows, true)
    });
    assert_eq!(app.marks.to_vec(), rows);
    within("membership", Duration::from_secs(1), || {
        assert!(rows.iter().all(|p| app.marks.contains(p)))
    });
}

#[test]
fn printing_many_paths_is_quick_and_exact() {
    let paths: Vec<PathBuf> = (0..ROWS)
        .map(|i| format!("/home/user/notes/{i:05}.txt").into())
        .collect();
    let mut bytes = Vec::new();
    within("printing relative paths", Duration::from_secs(1), || {
        write_paths(&mut bytes, &paths, Path::new("/home/user"), true, true).unwrap()
    });
    let printed: Vec<_> = bytes.split(|&b| b == 0).collect();
    // One per path, each ended by a NUL, relative to the base.
    assert_eq!(printed.len(), ROWS + 1);
    assert_eq!(printed[0], b"notes/00000.txt");
    assert_eq!(printed[ROWS], b"");
}

#[test]
fn scoring_and_natural_comparison_are_quick() {
    let names: Vec<String> = (0..ROWS * 3)
        .map(|i| format!("chapter {i} - draft v{}.tex", i % 7))
        .collect();
    let matched = within("scoring names", Duration::from_secs(2), || {
        names
            .iter()
            .filter(|n| score(n, "chdrv6").is_some())
            .count()
    });
    assert!(matched > 0 && matched < names.len(), "{matched}");
    let mut sorted = names.clone();
    within("natural sort", Duration::from_secs(2), || {
        sorted.sort_by(|a, b| natural(a, b))
    });
    // Numbers by value: chapter 2 before chapter 10.
    let position = |n: &str| sorted.iter().position(|s| s.starts_with(n)).unwrap();
    assert!(position("chapter 2 ") < position("chapter 10 "));
}
