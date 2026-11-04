//! The order of the rows and everything that reads them in that order, pinned
//! while a scan adds paths and after: whatever the rows are kept as, they
//! must read as a fresh ranking of the paths merged so far would.
use fuzzy_file_finder::{App, Candidate, Policy, program::Program, write_paths};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn candidate(name: &str, directory: bool, depth: usize, root: usize) -> Candidate {
    let base = ["/fixture", "/pinned"][root];
    Candidate {
        path: Path::new(base).join(name),
        directory,
        depth,
        root,
    }
}
fn app(program: Program) -> App {
    let mut app = App::new(
        vec![PathBuf::from("/fixture"), PathBuf::from("/pinned")],
        4,
        Policy::default(),
    );
    app.set_program(program);
    app
}
/// What ranking `paths` afresh, all at once, lists.
fn fresh(program: Program, query: &str, paths: &[Candidate]) -> App {
    let mut app = app(program);
    app.query = query.into();
    for c in paths {
        app.insert(0, c.clone());
    }
    app.refresh();
    app
}
fn names(app: &App) -> Vec<String> {
    app.rows
        .iter()
        .map(|c| c.path.to_string_lossy().into_owned())
        .collect()
}
/// A tree delivered breadth-first, as scans do: shallow paths first, each
/// depth's in no particular order, some names shared across folders and
/// depths, folders among the files.
fn tree() -> Vec<Candidate> {
    let mut paths = Vec::new();
    for depth in 1..=4 {
        for i in 0..400u64 {
            let n = i.wrapping_mul(2_654_435_761) % 400;
            let folders: String = (1..depth)
                .map(|d| format!("d{}/", (n + d as u64) % 3))
                .collect();
            let name = match n % 5 {
                0 => "notes".to_string(),
                1 => format!("notes{}", n % 7),
                _ => format!("f{n:03}"),
            };
            let root = usize::from(n % 9 == 0);
            paths.push(candidate(
                &format!("{folders}{name}"),
                n % 4 == 0,
                depth,
                root,
            ));
        }
    }
    // One path per name and place: repeats would only replace each other.
    let mut seen = std::collections::HashSet::new();
    paths.retain(|c| seen.insert(c.path.clone()));
    paths
}

#[test]
fn the_same_name_at_several_depths_ranks_by_depth_or_by_match_first() {
    let paths = [
        candidate("d/notes", false, 2, 0),
        candidate("notes", true, 1, 0),
        candidate("d/e/notes", false, 3, 0),
        candidate("notes", false, 1, 1),
        candidate("d/notes.md", false, 2, 0),
        candidate("nxoxtxexs", false, 1, 0),
        candidate("d/e/f/notes", true, 4, 0),
        candidate("a", true, 1, 0),
    ];
    // "notes.md" matches "notes" as well as "notes" does; a scattered match
    // ranks below.
    let listed = |program, query: &str| {
        names(&fresh(program, query, &paths))
            .into_iter()
            .map(|n| n.replace("/fixture/", "").replace("/pinned/", "pinned:"))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        listed(Program::Roam, ""),
        [
            "a",
            "notes",
            "nxoxtxexs",
            "pinned:notes",
            "d/notes",
            "d/notes.md",
            "d/e/notes",
            "d/e/f/notes"
        ]
    );
    assert_eq!(
        listed(Program::Roam, "notes"),
        [
            "notes",
            "pinned:notes",
            "nxoxtxexs",
            "d/notes",
            "d/notes.md",
            "d/e/notes",
            "d/e/f/notes"
        ]
    );
    assert_eq!(
        listed(Program::Pick, "notes"),
        [
            "notes",
            "pinned:notes",
            "d/notes",
            "d/notes.md",
            "d/e/notes",
            "d/e/f/notes",
            "nxoxtxexs"
        ]
    );
    assert_eq!(
        listed(Program::Seek, "notes"),
        [
            "pinned:notes",
            "d/notes",
            "d/notes.md",
            "d/e/notes",
            "nxoxtxexs"
        ]
    );
    assert_eq!(
        listed(Program::Pick, ""),
        [
            "a",
            "notes",
            "nxoxtxexs",
            "pinned:notes",
            "d/notes",
            "d/notes.md",
            "d/e/notes",
            "d/e/f/notes"
        ]
    );
}

#[test]
fn rows_read_as_a_fresh_ranking_while_a_scan_adds_paths_and_after() {
    let paths = tree();
    for program in [Program::Roam, Program::Pick, Program::Seek] {
        for query in ["", "notes", "f1"] {
            let mut app = app(program);
            app.query = query.into();
            // As in a newly opened folder.
            app.highlight_top = true;
            app.refresh();
            let mut inserted = 0;
            for batch in paths.chunks(97) {
                for c in batch {
                    app.insert(0, c.clone());
                }
                inserted += batch.len();
                // A budget too small for every path: some wait.
                app.refresh_within(Some(Duration::ZERO));
                let merged = inserted - app.unmerged();
                let expected = fresh(program, query, &paths[..merged]);
                // Read a few from the front and the middle before every row.
                let want = names(&expected);
                let front: Vec<_> = app
                    .shown_from(0)
                    .take(30)
                    .map(|c| c.path.to_string_lossy().into_owned())
                    .collect();
                assert_eq!(front, want[..want.len().min(30)]);
                let mid = want.len() / 2;
                let row = app
                    .shown_row(mid)
                    .map(|c| c.path.to_string_lossy().into_owned());
                assert_eq!(row.as_ref(), want.get(mid));
                if let Some(name) = want.last() {
                    let at = app.shown_position(|c| c.path.to_string_lossy() == *name);
                    assert_eq!(at, Some(want.len() - 1));
                }
                assert_eq!(names(&app), want, "{program:?} {query:?}");
                assert_eq!(app.rows.len(), expected.rows.len());
                // The highlight stays on the first row while the scan
                // streams in.
                assert_eq!(app.highlighted(), (!app.rows.is_empty()).then_some(0));
            }
            while !app.settled() {
                app.refresh_within(Some(Duration::ZERO));
            }
            let expected = fresh(program, query, &paths);
            assert_eq!(names(&app), names(&expected), "{program:?} {query:?}");
            assert_eq!(app.labels, expected.labels, "{program:?} {query:?}");
        }
    }
}

#[test]
fn labels_change_as_paths_with_the_same_name_arrive() {
    let mut app = app(Program::Pick);
    assert!(app.short_labels);
    let label = |app: &App, name: &str| {
        app.labels
            .get(Path::new(name))
            .map(|l| l.to_string_lossy().into_owned())
    };
    let add = |app: &mut App, name: &str, depth| {
        app.insert(0, candidate(name, false, depth, 0));
        while !app.settled() {
            app.refresh_within(Some(Duration::ZERO));
        }
    };
    add(&mut app, "a/x.txt", 2);
    assert_eq!(label(&app, "/fixture/a/x.txt"), None);
    add(&mut app, "b/x.txt", 2);
    assert_eq!(label(&app, "/fixture/a/x.txt").as_deref(), Some("a/x.txt"));
    assert_eq!(label(&app, "/fixture/b/x.txt").as_deref(), Some("b/x.txt"));
    add(&mut app, "c/a/x.txt", 3);
    assert_eq!(
        label(&app, "/fixture/a/x.txt").as_deref(),
        Some("fixture/a/x.txt")
    );
    assert_eq!(
        label(&app, "/fixture/c/a/x.txt").as_deref(),
        Some("c/a/x.txt")
    );
    add(&mut app, "x.txt", 1);
    assert_eq!(
        label(&app, "/fixture/x.txt").as_deref(),
        Some("fixture/x.txt")
    );
    assert_eq!(
        names(&app),
        [
            "/fixture/x.txt",
            "/fixture/a/x.txt",
            "/fixture/b/x.txt",
            "/fixture/c/a/x.txt"
        ]
    );
    // A query does not change the labels, which depend on what is listed.
    app.query = "b/x".into();
    app.refresh();
    assert_eq!(names(&app), ["/fixture/b/x.txt"]);
    assert_eq!(label(&app, "/fixture/b/x.txt").as_deref(), Some("b/x.txt"));
}

#[test]
fn a_query_typed_while_a_scan_adds_paths_ranks_what_is_merged() {
    let paths = tree();
    for program in [Program::Roam, Program::Pick] {
        let mut app = app(program);
        app.refresh();
        let mut inserted = 0;
        for (i, batch) in paths.chunks(131).enumerate() {
            for c in batch {
                app.insert(0, c.clone());
            }
            inserted += batch.len();
            app.refresh_within(Some(Duration::ZERO));
            // Typing, then deleting, between batches.
            let query = ["", "n", "no", "not", "no", "n", "f", ""][i % 8];
            app.query = query.into();
            app.refresh();
            let merged = inserted - app.unmerged();
            let expected = fresh(program, query, &paths[..merged]);
            assert_eq!(names(&app), names(&expected), "{program:?} {query:?}");
            // Typing highlights the best match.
            assert_eq!(
                app.highlight,
                expected.rows.first().map(|c| c.path.clone()),
                "{program:?} {query:?}"
            );
        }
        while !app.settled() {
            app.refresh_within(Some(Duration::ZERO));
        }
        let query = app.query.clone();
        assert_eq!(names(&app), names(&fresh(program, &query, &paths)));
    }
}

#[test]
fn the_last_row_count_marks_and_printed_paths_follow_the_order_during_a_scan_and_after() {
    let paths = tree();
    let mut app = app(Program::Pick);
    app.refresh();
    let mut inserted = 0;
    let check = |app: &mut App, merged: &[Candidate]| {
        let expected = fresh(Program::Pick, "", merged);
        let order: Vec<PathBuf> = expected.rows.iter().map(|c| c.path.clone()).collect();
        // The count.
        assert_eq!(app.shown().len(), order.len());
        // Every row, each once, in order: what m, M and r read.
        assert_eq!(app.shown_paths(), order);
        // G: the last row.
        app.move_by(isize::MAX);
        assert_eq!(app.highlight.as_ref(), order.last());
        assert_eq!(app.highlighted(), order.len().checked_sub(1));
        // PageDown near the end stops at the last row.
        app.move_by(-3);
        app.move_by(10);
        assert_eq!(app.highlight.as_ref(), order.last());
        // gg: the first.
        app.move_by(isize::MIN);
        assert_eq!(app.highlight.as_ref(), order.first());
        // m marks every row in order, after marks on other paths; printing
        // the marks writes them in that order.
        app.marks = vec![PathBuf::from("/elsewhere")].into();
        let rows = app.shown_paths();
        app.mark_rows(&rows, false);
        let mut printed = Vec::new();
        write_paths(
            &mut printed,
            &app.marks.to_vec(),
            Path::new("/"),
            false,
            false,
        )
        .unwrap();
        let mut want = b"/elsewhere\n".to_vec();
        for path in &order {
            want.extend(path.as_os_str().as_encoded_bytes());
            want.push(b'\n');
        }
        assert_eq!(printed, want);
        // M inverts them.
        app.mark_rows(&rows, true);
        assert_eq!(app.marks, [PathBuf::from("/elsewhere")]);
        app.marks.clear();
        // The last drawn row, as Alt+0 and Ctrl+0 read it.
        app.rendered = order.iter().take(5).cloned().collect();
        assert_eq!(Some(&app.numbered(0)), order.get(order.len().min(5) - 1));
        app.highlight_numbered(0);
        assert_eq!(app.highlighted(), Some(order.len().min(5) - 1));
        app.move_by(isize::MIN);
    };
    for batch in paths.chunks(211) {
        for c in batch {
            app.insert(0, c.clone());
        }
        inserted += batch.len();
        app.refresh_within(Some(Duration::ZERO));
        let merged = inserted - app.unmerged();
        if merged > 0 {
            check(&mut app, &paths[..merged]);
        }
    }
    while !app.settled() {
        app.refresh_within(Some(Duration::ZERO));
    }
    check(&mut app, &paths);
}

#[test]
fn the_highlight_follows_its_row_as_paths_arrive_above_it() {
    let paths = tree();
    let mut app = app(Program::Roam);
    app.refresh();
    let mut held = None;
    let mut inserted = 0;
    for batch in paths.chunks(150) {
        for c in batch {
            app.insert(0, c.clone());
        }
        inserted += batch.len();
        app.refresh_within(Some(Duration::ZERO));
        match &held {
            None if app.rows.len() > 40 => {
                app.move_by(37);
                held = app.highlight.clone();
            }
            Some(path) => {
                assert_eq!(app.highlight.as_ref(), Some(path));
                let at = app.rows.iter().position(|c| &c.path == path);
                assert_eq!(app.highlighted(), at);
            }
            None => {}
        }
        let merged = inserted - app.unmerged();
        assert_eq!(
            names(&app),
            names(&fresh(Program::Roam, "", &paths[..merged]))
        );
    }
    assert!(held.is_some());
}

#[test]
fn kept_queries_come_back_as_ranked_and_take_new_paths_after() {
    let paths = tree();
    let (first, rest) = paths.split_at(paths.len() / 2);
    for program in [Program::Roam, Program::Pick] {
        let mut app = app(program);
        app.cached_queries = 4;
        for c in first {
            app.insert(0, c.clone());
        }
        app.refresh();
        // Typing keeps each shorter query's rows; deleting brings them back.
        for query in [
            "n", "no", "not", "note", "not", "no", "n", "", "f", "f1", "f", "",
        ] {
            app.query = query.into();
            app.refresh();
            assert_eq!(
                names(&app),
                names(&fresh(program, query, first)),
                "{program:?} {query:?}"
            );
            assert_eq!(
                app.highlight,
                app.rows.first().map(|c| c.path.clone()),
                "{program:?} {query:?}"
            );
        }
        // Rows restored from a kept query merge new paths as ranked ones do.
        for query in ["n", "no", "n"] {
            app.query = query.into();
            app.refresh();
        }
        for c in rest {
            app.insert(0, c.clone());
        }
        while !app.settled() {
            app.refresh_within(Some(Duration::ZERO));
        }
        assert_eq!(
            names(&app),
            names(&fresh(program, "n", &paths)),
            "{program:?}"
        );
        // And the kept rows went with the change: deleting the letter ranks
        // every path again.
        app.query = String::new();
        app.refresh();
        assert_eq!(
            names(&app),
            names(&fresh(program, "", &paths)),
            "{program:?}"
        );
    }
}

#[test]
fn forgotten_and_replaced_paths_leave_the_rows_in_order() {
    use fuzzy_file_finder::{config::CacheConfig, discovery::Discovery};
    let root = std::env::temp_dir().join(format!("fff-row-order-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let made: Vec<PathBuf> = (0..300)
        .map(|i| root.join(format!("d{}/e{}/f{i:03}", i % 4, i % 3)))
        .collect();
    for path in &made {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }
    let mut app = App::browse(root.clone(), 3, Policy::default());
    let mut discovery = Discovery::new(CacheConfig {
        fresh_seconds: 0,
        ..CacheConfig::default()
    });
    let complete = |discovery: &mut Discovery, app: &mut App| {
        let start = std::time::Instant::now();
        while app.scanning || !app.settled() {
            discovery.drain(app);
            assert!(start.elapsed() < Duration::from_secs(10), "scan timed out");
            std::thread::yield_now();
        }
    };
    discovery.start(&mut app, true);
    complete(&mut discovery, &mut app);
    app.move_by(100);
    let held = app.highlight.clone().unwrap();
    // Every third file goes, every fifth turns into a folder; a quiet
    // rescan forgets and replaces their rows.
    for (i, path) in made.iter().enumerate() {
        if i % 3 == 1 {
            std::fs::remove_file(path).unwrap();
        } else if i % 5 == 0 && *path != held {
            std::fs::remove_file(path).unwrap();
            std::fs::create_dir(path).unwrap();
        }
    }
    discovery.force_refresh(&mut app);
    complete(&mut discovery, &mut app);
    let listed: Vec<_> = app.known.values().map(|c| (**c).clone()).collect();
    let mut expected = App::browse(root.clone(), 3, Policy::default());
    for c in listed {
        expected.insert(expected.generation, c);
    }
    expected.refresh();
    assert_eq!(names(&app), names(&expected));
    let directories = |app: &App| app.rows.iter().filter(|c| c.directory).count();
    assert_eq!(directories(&app), directories(&expected));
    assert_eq!(app.rows.len(), 4 + 12 + 200);
    std::fs::remove_dir_all(&root).unwrap();
}
