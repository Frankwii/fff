use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use fuzzy_file_finder::{
    App, Candidate, Kinds, Policy, discovery::Scan, glob, keys::KeyMap, relative_to, score,
    ui::numbered_key, write_paths,
};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
fn app() -> App {
    App::new(vec![PathBuf::from("/fixture")], 3, Policy::default())
}
fn candidate(name: &str, directory: bool, depth: usize) -> Candidate {
    Candidate {
        path: Path::new("/fixture").join(name),
        directory,
        depth,
        root: 0,
    }
}
#[test]
fn live_order_highlight_late_depth_and_rendered_choice() {
    let mut app = app();
    for (name, dir, depth) in [
        ("alpha/beta/00.txt", false, 3),
        ("alpha/aa.txt", false, 2),
        ("zebra.txt", false, 1),
        ("alpha/beta", true, 2),
        ("alpha", true, 1),
    ] {
        let old = app.highlight.clone();
        app.insert(0, candidate(name, dir, depth));
        app.refresh();
        if old.is_some() {
            assert_eq!(app.highlight, old);
        }
        assert!(app.rows.windows(2).all(|w| w[0].depth <= w[1].depth));
    }
    assert_eq!(
        app.rows
            .iter()
            .map(|c| c.path.strip_prefix("/fixture").unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "alpha",
            "zebra.txt",
            "alpha/beta",
            "alpha/aa.txt",
            "alpha/beta/00.txt"
        ]
    );
    app.rendered = app.rows.iter().map(|c| c.path.clone()).collect();
    assert_eq!(app.numbered(2), Path::new("/fixture/zebra.txt"));
    app.insert(0, candidate("aardvark", true, 1));
    app.refresh();
    assert_eq!(app.numbered(2), Path::new("/fixture/zebra.txt"));
    app.depth = 2;
    app.refresh();
    app.insert(0, candidate("alpha/beta/late.txt", false, 3));
    app.refresh();
    assert!(app.rows.iter().all(|c| c.depth <= 2));
    app.generation = 1;
    app.insert(0, candidate("obsolete", false, 1));
    assert!(!app.known.contains_key(Path::new("/fixture/obsolete")));
}
#[test]
fn bookmark_jump_keeps_the_highlight_on_the_first_row_until_moved() {
    let mut app = app();
    app.bookmarks = vec![fuzzy_file_finder::config::Bookmark {
        name: "here".into(),
        path: std::env::temp_dir(),
        icon: None,
    }];
    assert!(app.open_bookmark(0));
    let cwd = app.cwd.clone();
    let at = |name: &str, dir, depth| Candidate {
        path: cwd.join(name),
        directory: dir,
        depth,
        root: 0,
    };
    // Better-ranked rows streaming in later take over the highlight.
    for (name, dir, depth) in [
        ("deep/z.txt", false, 2),
        ("b.txt", false, 1),
        ("a", true, 1),
    ] {
        app.insert(0, at(name, dir, depth));
        app.refresh();
        assert_eq!(app.highlight.as_ref(), Some(&app.rows[0].path));
    }
    assert_eq!(app.highlight, Some(cwd.join("a")));
    // So does a new search there.
    app.query = "b".into();
    app.refresh();
    assert_eq!(app.highlight, Some(cwd.join("b.txt")));
    // Once the user moves, the highlight stays put again.
    app.query.clear();
    app.refresh();
    app.move_by(1);
    let moved = app.highlight.clone();
    app.insert(0, at("0", true, 1));
    app.refresh();
    assert_eq!(app.highlight, moved);
}
#[test]
fn overlap_and_root_order() {
    let mut app = App::new(
        vec![
            "/fixture".into(),
            "/other".into(),
            "/fixture/appendix".into(),
        ],
        3,
        Policy::default(),
    );
    let path = PathBuf::from("/fixture/appendix/turing.pdf");
    for (depth, root) in [(2, 0), (1, 2), (2, 0)] {
        app.insert(
            0,
            Candidate {
                path: path.clone(),
                directory: false,
                depth,
                root,
            },
        );
    }
    app.insert(
        0,
        Candidate {
            path: "/other/turing.pdf".into(),
            directory: false,
            depth: 1,
            root: 1,
        },
    );
    app.refresh();
    assert_eq!(app.rows.len(), 2);
    assert_eq!(app.rows[0].root, 1);
    assert_eq!(app.rows[1].depth, 1);
    // Attribution is computed before the overlapping root reports a duplicate.
    assert_eq!(app.known[&path].root, 2);
}
#[test]
fn subsequence_and_depth_before_score() {
    assert!(score("Downloads", "dwn").is_some());
    assert!(score("report.tex", "rpx").is_some());
    assert!(score("scratch", "sh").is_some());
    assert!(score("short_notes", "sh") > score("scratch", "sh"));
    let mut app = app();
    app.query = "ab".into();
    app.insert(0, candidate("a_x_b", false, 1));
    app.insert(0, candidate("dir/ab", true, 2));
    app.refresh();
    assert_eq!(app.rows[0].depth, 1);
}
#[test]
fn a_query_with_a_slash_matches_paths_below_the_root() {
    use fuzzy_file_finder::Matcher;
    let matcher = Matcher::new("pde/stab");
    assert!(
        matcher
            .score_path(&["pde", "notes", "stability.tex"])
            .is_some()
    );
    assert!(matcher.score_path(&["notes", "stability.tex"]).is_none());
    // The last part matches the basename, the others ancestors in order.
    assert!(matcher.score_path(&["stab", "pde.tex"]).is_none());
    assert!(
        Matcher::new("notes/pde/s")
            .score_path(&["pde", "notes", "s"])
            .is_none()
    );
    // Whole components and consecutive ones score higher.
    let no = Matcher::new("no/stab");
    assert!(no.score_path(&["no", "stability.tex"]) > no.score_path(&["notes", "stability.tex"]));
    assert!(
        no.score_path(&["notes", "x", "stability.tex"])
            < no.score_path(&["x", "notes", "stability.tex"])
    );
    // Empty parts are dropped; a trailing slash lists what is below.
    assert!(
        Matcher::new("/stab")
            .score_path(&["stability.tex"])
            .is_some()
    );
    assert!(Matcher::new("pde/").score_path(&["pde", "x"]).is_some());
    assert!(Matcher::new("pde/").score_path(&["pde"]).is_none());
    assert_eq!(
        Matcher::new("pde/stab").path_positions(&["pde", "notes", "stability.tex"]),
        Some(vec![vec![0, 1, 2], vec![], vec![0, 1, 2, 3]])
    );
    // Names alone never contain a slash.
    assert!(score("pde", "pde/").is_none());

    let mut app = app();
    for (name, dir, depth) in [
        ("pde", true, 1),
        ("pde/notes", true, 2),
        ("pde/notes/stability.tex", false, 3),
        ("notes", true, 1),
        ("notes/stability.tex", false, 2),
        ("pdes.txt", false, 1),
    ] {
        app.insert(0, candidate(name, dir, depth));
    }
    let listed = |app: &mut App| {
        app.refresh();
        let rows: Vec<_> = app
            .rows
            .iter()
            .map(|c| {
                c.path
                    .strip_prefix("/fixture")
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        // Narrowing from earlier rows gives what ranking afresh would.
        let mut fresh = App::new(vec![PathBuf::from("/fixture")], 3, Policy::default());
        for c in app.known.values() {
            fresh.insert(0, (**c).clone());
        }
        fresh.query = app.query.clone();
        fresh.refresh();
        assert_eq!(fresh.rows, app.rows, "{:?}", app.query);
        rows
    };
    // Typing a slash turns basename matches into path matches, so it does
    // not narrow; the letters after it do.
    for (query, rows) in [
        ("pde", vec!["pde", "pdes.txt"]),
        ("pde/", vec!["pde/notes", "pde/notes/stability.tex"]),
        ("pde/st", vec!["pde/notes/stability.tex"]),
        ("pde/", vec!["pde/notes", "pde/notes/stability.tex"]),
        ("pde", vec!["pde", "pdes.txt"]),
        (
            "stab",
            vec!["notes/stability.tex", "pde/notes/stability.tex"],
        ),
        (
            "no/stab",
            vec!["notes/stability.tex", "pde/notes/stability.tex"],
        ),
    ] {
        app.query = query.into();
        assert_eq!(listed(&mut app), rows, "{query:?}");
    }
}
#[test]
fn numbered_choice_zero_fallback_and_key_decoding() {
    let mut app = app();
    app.marks.push("/unrelated".into());
    assert_eq!(app.numbered(2), app.cwd);
    app.rendered = (1..=15)
        .map(|i| PathBuf::from(format!("/row{i}")))
        .collect();
    assert_eq!(app.numbered(0), Path::new("/row15"));
    assert_eq!(app.numbered(9), Path::new("/row9"));
    app.rendered.truncate(3);
    assert_eq!(app.numbered(8), Path::new("/row3"));
    assert_eq!(
        numbered_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::CONTROL)),
        Some(2)
    );
    assert_eq!(
        numbered_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::NONE)),
        None
    );
    assert_eq!(
        numbered_key(KeyEvent::new(KeyCode::Null, KeyModifiers::CONTROL)),
        None
    );
}
#[test]
fn output_bytes_are_exact_and_base_is_stable() {
    let paths = vec![
        PathBuf::from("/home/user/a b"),
        PathBuf::from("/home/other/file"),
    ];
    let mut bytes = Vec::new();
    write_paths(&mut bytes, &paths, Path::new("/home/user"), false, true).unwrap();
    assert_eq!(bytes, b"/home/user/a b\0/home/other/file\0");
    bytes.clear();
    write_paths(&mut bytes, &paths, Path::new("/home/user"), true, false).unwrap();
    assert_eq!(bytes, b"a b\n../other/file\n");
    assert_eq!(
        relative_to(Path::new("/home/user"), Path::new("/home/user")),
        Path::new(".")
    );
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fff-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn file(&self, name: &str) -> PathBuf {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn complete(scan: &mut Scan, app: &mut App) {
    let start = Instant::now();
    while app.scanning {
        scan.drain(app);
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
}
#[test]
fn real_discovery_glob_depth_and_retained_state() {
    let f = Fixture::new();
    for name in ["zebra.tex", "topic/alpha.tex", "topic/details/beta.tex"] {
        f.file(name);
    }
    let mut policy = Policy::default();
    policy.set_filters(vec!["*.tex".into()]).unwrap();
    let mut app = App::new(vec![f.0.clone()], 1, policy);
    for depth in [1, 2, 3, 2, 4] {
        app.depth = depth;
        let mut scan = Scan::start(&mut app, false);
        complete(&mut scan, &mut app);
        assert_eq!(app.rows.len(), depth.min(3));
        assert!(app.rows.iter().all(|c| !c.directory && c.depth <= depth));
    }
}
#[test]
fn file_filters_list_files_matching_any_and_replace_atomically() {
    let mut app = app();
    for (name, dir, depth) in [
        ("notes", true, 1),
        ("notes/a.pdf", false, 2),
        ("b.DJVU", false, 1),
        ("c.epub", false, 1),
        ("d.txt", false, 1),
    ] {
        app.insert(0, candidate(name, dir, depth));
    }
    let listed = |app: &mut App| {
        app.refresh();
        let mut names: Vec<_> = app
            .rows
            .iter()
            .map(|c| c.path.strip_prefix("/fixture").unwrap().to_owned())
            .collect();
        names.sort();
        names
    };
    app.policy
        .set_filters(vec!["*.pdf".into(), "*.{djvu,DJVU}".into()])
        .unwrap();
    // Any filter admits a file; directories are hidden while filters apply.
    assert_eq!(
        listed(&mut app),
        [Path::new("b.DJVU"), Path::new("notes/a.pdf")]
    );
    // An invalid glob leaves the previous filters in force.
    assert!(glob("[").is_err());
    assert!(
        app.policy
            .set_filters(vec!["*.epub".into(), "[".into()])
            .is_err()
    );
    assert_eq!(app.policy.filters, ["*.pdf", "*.{djvu,DJVU}"]);
    app.policy.set_filters(Vec::new()).unwrap();
    assert_eq!(listed(&mut app).len(), 5);
}
#[test]
fn marks_confirmation_and_missing_paths() {
    let f = Fixture::new();
    let a = f.file("a");
    let b = f.file("b");
    let mut app = App::new(vec![f.0.clone()], 3, Policy::default());
    assert_eq!(app.confirm(None), None);
    app.toggle(a.clone());
    app.toggle(b.clone());
    app.query = "zzzz".into();
    app.depth = 1;
    app.refresh();
    assert_eq!(
        app.confirm(Some(a.clone())),
        Some(vec![a.clone(), b.clone()])
    );
    fs::remove_file(&a).unwrap();
    assert_eq!(app.confirm(None), None);
    assert!(app.status.contains("Missing"));
    app.toggle(a);
    assert_eq!(app.confirm(None), Some(vec![b]));
}
#[test]
fn policy_toggles_prune_and_preserve_marks() {
    let f = Fixture::new();
    let main = f.file("main.tex");
    f.file(".draft.tex");
    f.file("build/output.log");
    let mut policy = Policy {
        ignore: true,
        ..Policy::default()
    };
    policy.set_rules(vec!["build".into()]).unwrap();
    let mut app = App::new(vec![f.0.clone()], 2, policy);
    app.toggle(main.clone());
    let mut scan = Scan::start(&mut app, true);
    complete(&mut scan, &mut app);
    assert_eq!(app.rows.len(), 1);
    app.policy.hidden = true;
    let mut scan = Scan::start(&mut app, true);
    complete(&mut scan, &mut app);
    assert_eq!(app.rows.len(), 2);
    app.policy.ignore = false;
    let mut scan = Scan::start(&mut app, true);
    complete(&mut scan, &mut app);
    assert_eq!(app.rows.len(), 4);
    app.policy
        .set_rules(vec!["build".into(), "*.tex".into()])
        .unwrap();
    app.refresh();
    assert_eq!(app.rows.len(), 4);
    app.policy.ignore = true;
    app.refresh();
    assert!(app.rows.is_empty());
    assert_eq!(app.marks, vec![main]);
}
#[test]
fn key_remapping_and_printable_text_safety() {
    let map = KeyMap::parse(&["ctrl+n=alt+j".into(), "alt+j=none".into()]).unwrap();
    assert_eq!(
        map.translate(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL))
            .unwrap()
            .code,
        KeyCode::Char('j')
    );
    assert!(
        map.translate(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::ALT))
            .is_none()
    );
    assert!(KeyMap::parse(&["2=ctrl+2".into()]).is_err());
}
#[test]
fn palette_keys_can_be_remapped_in_either_shift_encoding() {
    let map = KeyMap::parse(&["ctrl+t=alt+p".into(), "alt+P=ctrl+r".into()]).unwrap();
    let chooser = map
        .translate(KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(
        (chooser.code, chooser.modifiers),
        (KeyCode::Char('p'), KeyModifiers::ALT)
    );
    // Legacy terminals send `P`; kitty-protocol terminals send `p` with SHIFT.
    for code in ['P', 'p'] {
        let key = KeyEvent::new(KeyCode::Char(code), KeyModifiers::ALT | KeyModifiers::SHIFT);
        assert_eq!(map.translate(key).unwrap().code, KeyCode::Char('r'));
    }
    let reload = KeyMap::parse(&["ctrl+r=alt+P".into()])
        .unwrap()
        .translate(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::CONTROL))
        .unwrap();
    assert_eq!(reload.code, KeyCode::Char('P'));
    assert!(
        reload
            .modifiers
            .contains(KeyModifiers::ALT | KeyModifiers::SHIFT)
    );
}

#[test]
fn navigation_combines_one_current_directory_and_fixed_roots() {
    let f = Fixture::new();
    let root = f.0.join("root");
    let other = f.0.join("other");
    let browsed = f.0.join("browsed");
    for dir in [&root, &other, &browsed] {
        fs::create_dir(dir).unwrap();
        fs::write(dir.join("file"), "").unwrap();
    }
    let mut app = App::new(vec![root.clone(), other.clone()], 1, Policy::default());
    app.query = "fi".into();
    app.marks.push(root.join("file"));
    app.navigate(browsed.clone(), true);
    assert_eq!(app.cwd, browsed);
    assert_eq!(app.roots, vec![root.clone(), other]);
    assert_eq!(app.scope.len(), 3);
    assert_eq!(app.query, "fi");
    assert_eq!(app.marks.len(), 1);
    let mut scan = Scan::start(&mut app, true);
    complete(&mut scan, &mut app);
    assert_eq!(app.rows.len(), 3);
    app.navigate(root, false);
    assert_eq!(app.scope.len(), 2);
    assert!(app.query.is_empty());
}

#[test]
fn alt_numbered_directories_navigate_and_keep_session_state() {
    use fuzzy_file_finder::EnterOutcome;
    let mut app = app();
    app.insert(0, candidate("alpha", true, 1));
    app.insert(0, candidate("beta", true, 1));
    app.refresh();
    app.rendered = app.rows.iter().map(|c| c.path.clone()).collect();
    app.query = "a".into();
    app.marks.push("/marked".into());
    let roots = app.roots.clone();
    let depth = app.depth;
    // Last rendered row is beta even if a new shallow result arrives.
    app.insert(0, candidate("aardvark", true, 1));
    app.refresh();
    assert_eq!(app.enter_numbered(9), EnterOutcome::Navigated);
    assert_eq!(app.cwd, Path::new("/fixture/beta"));
    assert!(app.query.is_empty());
    assert_eq!(app.marks, vec![PathBuf::from("/marked")]);
    assert_eq!(app.roots, roots);
    assert_eq!(app.depth, depth);
}
#[test]
fn alt_numbered_file_matches_enter_including_marks_missing_and_zero() {
    use fuzzy_file_finder::EnterOutcome;
    let f = Fixture::new();
    let a = f.file("a");
    let b = f.file("b");
    let mut app = App::browse(f.0.clone(), 1, Policy::default());
    let mut scan = Scan::start(&mut app, true);
    complete(&mut scan, &mut app);
    app.rendered = app.rows.iter().map(|c| c.path.clone()).collect();
    app.toggle(a.clone());
    assert_eq!(
        app.enter_numbered(0),
        EnterOutcome::Selected(vec![a.clone(), b.clone()])
    );
    assert_eq!(
        app.enter_numbered(1),
        EnterOutcome::Selected(vec![a.clone()])
    );
    fs::remove_file(&a).unwrap();
    assert_eq!(app.enter_numbered(2), EnterOutcome::Stay);
    assert!(app.status.contains("Missing"));
    // Ctrl+number bypasses marks and remains independent of Enter.
    assert_eq!(app.numbered(2), b);
    app.rendered.clear();
    app.physical_empty = false;
    assert_eq!(app.enter_numbered(3), EnterOutcome::Stay);
    app.toggle(a);
    app.physical_empty = true;
    app.scanning = true;
    assert_eq!(app.enter_numbered(3), EnterOutcome::Stay);
    app.scanning = false;
    assert_eq!(
        app.enter_numbered(3),
        EnterOutcome::Selected(vec![f.0.clone()])
    );
}
#[test]
fn alt_numbers_are_distinct_from_plain_and_control_digits() {
    use fuzzy_file_finder::ui::alt_numbered_key;
    assert_eq!(
        alt_numbered_key(KeyEvent::new(KeyCode::Char('2'), KeyModifiers::ALT)),
        Some(2)
    );
    for modifier in [
        KeyModifiers::NONE,
        KeyModifiers::CONTROL,
        KeyModifiers::ALT | KeyModifiers::CONTROL,
    ] {
        assert_eq!(
            alt_numbered_key(KeyEvent::new(KeyCode::Char('2'), modifier)),
            None
        );
    }
}
#[test]
fn no_dir_hides_directories_and_never_navigates_or_returns_them() {
    let mut app = app();
    app.no_dir = true;
    for (name, dir, depth) in [
        ("alpha", true, 1),
        ("zebra.txt", false, 1),
        ("alpha/aa.txt", false, 2),
    ] {
        app.insert(0, candidate(name, dir, depth));
    }
    app.refresh();
    let names: Vec<_> = app.rows.iter().map(|c| c.path.clone()).collect();
    assert_eq!(
        names,
        [
            PathBuf::from("/fixture/zebra.txt"),
            PathBuf::from("/fixture/alpha/aa.txt")
        ]
    );
    // A directory candidate (e.g. a cached kind change) is not entered.
    assert_eq!(
        app.enter(Some(candidate("alpha", true, 1))),
        fuzzy_file_finder::EnterOutcome::Stay
    );
    assert!(app.status.contains("fff seek"));
    assert!(!app.navigate_highlighted());
    assert_eq!(app.cwd, Path::new("/fixture"));
    // An empty directory never returns itself.
    app.rows = Default::default();
    app.scanning = false;
    app.physical_empty = true;
    assert_eq!(app.enter(None), fuzzy_file_finder::EnterOutcome::Stay);
}
#[test]
fn an_empty_folder_answers_enter_only_where_a_folder_is_an_answer() {
    use fuzzy_file_finder::{EnterOutcome, explorer::Dialog};
    let f = Fixture::new();
    let empty = |set: &dyn Fn(&mut App)| {
        let mut app = App::browse(f.0.clone(), 1, Policy::default());
        app.scanning = false;
        app.physical_empty = true;
        set(&mut app);
        app.enter(None)
    };
    let folder = EnterOutcome::Selected(vec![f.0.clone()]);
    assert_eq!(empty(&|_| {}), folder);
    let dialog = |directory| Dialog {
        multiple: false,
        directory,
        save: None,
    };
    assert_eq!(empty(&|app| app.dialog = Some(dialog(true))), folder);
    // A file dialog cannot take a folder, and a window has no shell to
    // leave into: a stray Enter must not end either.
    assert_eq!(
        empty(&|app| app.dialog = Some(dialog(false))),
        EnterOutcome::Stay
    );
    assert_eq!(empty(&|app| app.window = true), EnterOutcome::Stay);
}
#[test]
fn status_messages_expire_and_restart_when_reported_again() {
    use fuzzy_file_finder::ui::StatusTimer;
    let mut app = app();
    let mut timer = StatusTimer::default();
    let start = Instant::now();
    let at = |ms| start + Duration::from_millis(ms);
    app.status = "Navigation is disabled".into();
    timer.tick_at(&mut app, at(0));
    timer.tick_at(&mut app, at(3_900));
    assert_eq!(app.status, "Navigation is disabled");
    // An unrelated key leaves the message and its deadline alone.
    timer.before_key(&mut app);
    timer.tick_at(&mut app, at(3_950));
    assert_eq!(app.status, "Navigation is disabled");
    timer.tick_at(&mut app, at(4_000));
    assert!(app.status.is_empty());
    // The same text reported again by a key restarts the timer.
    app.status = "Again".into();
    timer.tick_at(&mut app, at(5_000));
    timer.before_key(&mut app);
    app.status = "Again".into();
    timer.tick_at(&mut app, at(8_000));
    timer.tick_at(&mut app, at(11_900));
    assert_eq!(app.status, "Again");
    timer.tick_at(&mut app, at(12_000));
    assert!(app.status.is_empty());
    // Without a timeout, messages stay until replaced.
    app.status_timeout = None;
    app.status = "Sticky".into();
    timer.tick_at(&mut app, at(13_000));
    timer.tick_at(&mut app, at(99_000));
    assert_eq!(app.status, "Sticky");
}
#[test]
fn incremental_refresh_matches_a_full_refresh_for_every_query_edit() {
    // Pseudo-random typing/backspacing over names with case, digits, separators,
    // and a Greek final sigma (context-sensitive lowercasing).
    let names = [
        "Report.tex",
        "report_2024.md",
        "ReadMe",
        "a-b-c",
        "ΟΔΟΣ.txt",
        "οδοσ",
        "abcabc",
        "Downloads",
        "dwn",
        "zebra",
        "Zero_Day",
        "data",
        "DATA.csv",
    ];
    let build = || {
        let mut app = App::browse("/fixture".into(), 3, Policy::default());
        for (i, name) in names.iter().enumerate() {
            for dir in ["", "sub/", "sub/deep/"] {
                let depth = 1 + dir.matches('/').count();
                app.insert(0, candidate(&format!("{dir}{name}"), i % 4 == 0, depth));
            }
        }
        app
    };
    let alphabet: Vec<char> = "adeorstxz_.ΣΟσ2".chars().collect();
    let mut seed = 0x9e37_79b9_u32;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize % n
    };
    let paths = |a: &App| a.rows.iter().map(|c| c.path.clone()).collect::<Vec<_>>();
    // Kept shorter queries (none, one, the default) restore or narrow exactly.
    for cached in [0, 1, 5] {
        let mut app = build();
        app.cached_queries = cached;
        app.refresh();
        for _ in 0..2_000 {
            match next(6) {
                0 | 1 => {
                    app.query.pop();
                }
                // An edit inside the query, not at its end.
                2 if app.query.chars().count() > 1 => {
                    let mut chars: Vec<char> = app.query.chars().collect();
                    chars[0] = alphabet[next(alphabet.len())];
                    app.query = chars.into_iter().collect();
                }
                _ => app.query.push(alphabet[next(alphabet.len())]),
            }
            if app.query.chars().count() > 8 {
                app.query.clear();
            }
            app.refresh();
            let mut fresh = build();
            fresh.query = app.query.clone();
            fresh.refresh();
            assert_eq!(paths(&app), paths(&fresh), "query {:?}", app.query);
        }
    }
    let mut app = build();
    // A basis change (depth) after narrowing rebuilds from every known path.
    app.query = "re".into();
    app.refresh();
    app.depth = 1;
    app.refresh();
    assert!(app.rows.iter().all(|c| c.depth == 1));
    app.depth = 3;
    app.refresh();
    assert!(app.rows.iter().any(|c| c.depth == 3));
}
#[test]
fn fast_scoring_matches_the_reference_definition() {
    // The original whole-string implementation, kept as the specification.
    fn reference(name: &str, query: &str) -> Option<i64> {
        if query.is_empty() {
            return Some(0);
        }
        // Either sigma matches: ς folds to σ.
        let name: Vec<char> = name.to_lowercase().replace('ς', "σ").chars().collect();
        let (mut position, mut previous, mut score) = (0, None, 0);
        for letter in query.to_lowercase().replace('ς', "σ").chars() {
            let found = (position..name.len()).find(|&i| name[i] == letter)?;
            score += 20 - found as i64;
            if previous == Some(found.wrapping_sub(1)) {
                score += 30;
            }
            if found == 0 || !name[found - 1].is_alphanumeric() {
                score += 20;
            }
            previous = Some(found);
            position = found + 1;
        }
        Some(score)
    }
    let alphabet: Vec<char> = "aAbBkK_-.1 zZéÉΣσ\u{212a}".chars().collect();
    let mut seed = 0x1234_5678_u32;
    let mut next = |n: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed as usize % n
    };
    let mut word = |max: usize| -> String {
        let len = next(max + 1);
        (0..len).map(|_| alphabet[next(alphabet.len())]).collect()
    };
    for _ in 0..20_000 {
        let (name, query) = (word(12), word(4));
        assert_eq!(
            score(&name, &query),
            reference(&name, &query),
            "{name:?} {query:?}"
        );
    }
}
#[test]
fn byte_level_policy_matches_path_components() {
    let roots = ["/", "/r", "/r/sub", "/r.d"];
    let tails = [
        "a", "a/b", ".h", "a/.h", ".h/a", "a/b.c", "r/a", "..", "a/..b", "./a", "a//b", "a/",
    ];
    for hidden in [false, true] {
        let mut policy = Policy {
            hidden,
            ..Policy::default()
        };
        for root in roots {
            for tail in tails {
                for joined in [format!("{root}/{tail}"), format!("{root}{tail}")] {
                    let (root, path) = (Path::new(root), Path::new(&joined));
                    let slow = path.strip_prefix(root).is_ok_and(|p| policy.allows(p));
                    if let Some(fast) = policy.allows_below(root, path) {
                        assert_eq!(fast, slow, "{root:?} {path:?} hidden={hidden}");
                    }
                }
            }
        }
        // Active exclusion rules always take the component path.
        policy.ignore = true;
        policy.set_rules(vec!["b".into()]).unwrap();
        assert_eq!(
            policy.allows_below(Path::new("/r"), Path::new("/r/a/b")),
            None
        );
    }
}
#[test]
fn tabs_keep_their_own_directory_and_history_and_share_marks() {
    let f = Fixture::new();
    let (a, b) = (f.0.join("a"), f.0.join("b"));
    fs::create_dir(&a).unwrap();
    fs::create_dir(&b).unwrap();
    let mut app = App::browse(f.0.clone(), 1, Policy::default());
    // Every tab starts in the session's first directory.
    assert_eq!(app.tab_dirs(), vec![f.0.as_path(); 3]);
    app.navigate(a.clone(), false);
    app.marks.push(a.join("marked"));
    // Tab 2 still shows the starting directory and has no history.
    assert!(app.switch_tab(true));
    assert_eq!((app.tab, &app.cwd), (1, &f.0));
    assert!(!app.jump(true));
    app.navigate(b.clone(), false);
    // Up from tab 2 returns to tab 1 where it was left, with its own history.
    assert!(app.switch_tab(false));
    assert_eq!((app.tab, &app.cwd), (0, &a));
    assert_eq!(app.marks, vec![a.join("marked")]);
    assert!(app.jump(true));
    assert_eq!(app.cwd, f.0);
    // Wrapping around: Up from tab 1 is tab 3, still at the start.
    assert!(!app.switch_tab(false));
    assert_eq!(app.tab, 2);
    assert_eq!(
        app.tab_dirs(),
        vec![f.0.as_path(), b.as_path(), f.0.as_path()]
    );
    // A vanished tab directory falls back to its nearest existing ancestor.
    fs::remove_dir(&b).unwrap();
    assert!(!app.switch_tab(false));
    assert_eq!((app.tab, &app.cwd), (1, &f.0));
}
#[test]
fn highlighted_row_stays_correct_when_rows_or_highlight_change_elsewhere() {
    let mut app = app();
    for name in ["a1", "a2", "b1", "b2", "c1"] {
        app.insert(0, candidate(name, false, 1));
    }
    app.refresh();
    let position = |a: &App| {
        a.rows
            .iter()
            .position(|c| Some(&c.path) == a.highlight.as_ref())
    };
    app.move_by(3);
    assert_eq!(app.highlighted(), position(&app));
    // Set directly, as clicks and jumps do.
    app.highlight = Some(Path::new("/fixture/a2").into());
    assert_eq!(app.highlighted(), Some(1));
    app.move_by(2);
    assert_eq!(app.highlighted(), position(&app));
    app.query = "1".into();
    app.refresh();
    assert_eq!(app.highlighted(), position(&app));
    app.highlight = Some(Path::new("/fixture/missing").into());
    assert_eq!(app.highlighted(), None);
}
#[test]
fn a_saved_session_resumes_as_it_ended() {
    let root = std::env::temp_dir().join(format!("fff-resume-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    for dir in ["a/deep", "b", "pinned"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    let mut app = App::new(vec![root.join("pinned")], 2, Policy::default());
    app.navigate(root.join("a"), false);
    app.navigate(root.join("a/deep"), false);
    assert!(app.switch_tab(true));
    app.navigate(root.join("b"), false);
    app.query = "notes".into();
    app.depth = 4;
    app.policy.hidden = true;
    app.policy.ignore = false;
    app.policy.set_rules(vec!["target".into()]).unwrap();
    app.policy.set_filters(vec!["*.pdf".into()]).unwrap();
    app.kinds = Kinds::Files;
    app.preview.shown = true;
    app.highlight = Some(root.join("b/notes.pdf"));
    app.toggle(root.join("a/x"));
    let saved = app.session();
    // The first tab's folder is gone by the next session.
    fs::remove_dir_all(root.join("a/deep")).unwrap();
    let mut resumed = App::browse(root.clone(), 2, Policy::default());
    resumed.restore(saved.clone());
    assert_eq!(resumed.cwd, root.join("b"));
    assert_eq!(
        (resumed.tab, resumed.roots.clone()),
        (1, vec![root.join("pinned")])
    );
    assert!(resumed.scope.contains(&root.join("pinned")));
    assert_eq!(resumed.back, [root.join("pinned")]);
    assert_eq!((resumed.query.as_str(), resumed.depth), ("notes", 4));
    assert!(resumed.policy.hidden && !resumed.policy.ignore);
    assert_eq!(resumed.policy.rules, ["target"]);
    assert_eq!(resumed.policy.filters, ["*.pdf"]);
    assert!(resumed.kinds == Kinds::Files && resumed.preview.shown);
    assert_eq!(resumed.wanted, Some(root.join("b/notes.pdf")));
    assert_eq!(resumed.marks.to_vec(), [root.join("a/x")]);
    // Switching back finds the vanished folder's parent, with its history.
    assert!(resumed.switch_tab(false));
    assert_eq!(resumed.cwd, root.join("a"));
    assert_eq!(resumed.session().tabs[1].cwd, root.join("b"));
    fs::remove_dir_all(&root).unwrap();
}
#[test]
fn budgeted_merges_match_ranking_everything_including_replaced_kinds() {
    // A deterministic shuffle of names that tie on depth, kind and score,
    // with deeper basenames shared across folders.
    let names: Vec<_> = (0..5000u64)
        .map(|i| {
            let n = i.wrapping_mul(2_654_435_761) % 5000;
            let depth = 1 + (n % 3) as usize;
            let path = match depth {
                1 => format!("n{n:04}"),
                2 => format!("a{}/x{:03}", n / 1000, n % 1000),
                _ => format!("a{}/b/x{:03}", n / 1000, n % 1000),
            };
            (path, n % 7 == 0, depth)
        })
        .collect();
    for (query, no_dir, short_labels) in [
        ("", false, false),
        ("n1", false, false),
        ("4", false, false),
        ("", true, false),
        ("x1", true, false),
        ("", false, true),
    ] {
        let mut merged = app();
        merged.no_dir = no_dir;
        merged.short_labels = short_labels;
        merged.query = query.into();
        merged.refresh();
        // Batches outgrow a round, so later ones arrive while some wait.
        for chunk in names.chunks(2500) {
            for (name, dir, depth) in chunk {
                merged.insert(0, candidate(name, *dir, *depth));
            }
            merged.refresh_within(Some(Duration::ZERO));
            assert!(merged.unmerged() > 0);
        }
        // Directories that become files (and back) replace their rows.
        for (name, dir, depth) in names.iter().step_by(11) {
            merged.insert(0, candidate(name, !dir, *depth));
        }
        while !merged.settled() {
            merged.refresh_within(Some(Duration::ZERO));
        }
        let mut ranked = app();
        ranked.no_dir = no_dir;
        ranked.short_labels = short_labels;
        ranked.query = query.into();
        for (i, (name, dir, depth)) in names.iter().enumerate() {
            ranked.insert(0, candidate(name, *dir != (i % 11 == 0), *depth));
        }
        ranked.refresh();
        let paths = |app: &App| {
            app.rows
                .iter()
                .map(|c| (c.path.clone(), c.directory))
                .collect::<Vec<_>>()
        };
        assert_eq!(paths(&merged), paths(&ranked), "query {query:?}");
        assert!(!merged.rows.is_empty());
        // `fff pick` and `fff seek` label entries that share a basename.
        assert_eq!(merged.labels, ranked.labels, "query {query:?}");
        assert_eq!(!merged.labels.is_empty(), no_dir || short_labels);
    }
}
#[test]
fn a_new_location_keeps_the_highlight_on_the_first_row_until_moved() {
    let mut app = app();
    app.navigate(PathBuf::from("/fixture"), false);
    for name in ["m", "z", "c", "a"] {
        app.insert(0, candidate(name, false, 1));
        app.refresh();
        assert_eq!(app.highlighted(), Some(0), "after {name}");
    }
    app.move_by(1);
    assert_eq!(app.highlight, Some(PathBuf::from("/fixture/c")));
    // Once moved, the highlight follows its row.
    app.insert(0, candidate("b", false, 1));
    app.refresh();
    assert_eq!(app.highlight, Some(PathBuf::from("/fixture/c")));
    assert_eq!(app.highlighted(), Some(2));
}
#[test]
fn partial_merges_list_the_shallowest_paths_first() {
    let mut app = app();
    app.refresh();
    // A breadth-first scan delivers every shallower path before deeper ones.
    let mut names = Vec::new();
    for depth in 1..=3 {
        for i in 0..1500 {
            let path = (1..depth).map(|_| "d/").collect::<String>() + &format!("f{i:04}");
            names.push((path, depth));
        }
    }
    for (name, depth) in &names {
        app.insert(0, candidate(name, false, *depth));
    }
    let mut deepest = 0;
    while app.unmerged() > 0 {
        app.refresh_within(Some(Duration::ZERO));
        let shown = app.rows.last().unwrap().depth;
        assert!(shown >= deepest);
        // Rows of a depth only show once every shallower one does.
        let shallower = app.rows.iter().filter(|c| c.depth < shown).count();
        assert_eq!(shallower, 1500 * (shown - 1));
        deepest = shown;
    }
    assert_eq!(app.rows.len(), 4500);
}
#[test]
fn large_rankings_run_in_the_background_and_match_ranking_in_place() {
    let names: Vec<_> = (0..40_000u64)
        .map(|i| {
            let n = i.wrapping_mul(2_654_435_761) % 40_000;
            let depth = 1 + (n % 3) as usize;
            let path = match depth {
                1 => format!("n{n:05}"),
                2 => format!("a{}/x{:03}", n / 1000, n % 1000),
                _ => format!("a{}/b/x{:03}", n / 1000, n % 1000),
            };
            (path, n % 7 == 0, depth)
        })
        .collect();
    let filled = |short_labels| {
        let mut app = app();
        app.short_labels = short_labels;
        for (name, dir, depth) in &names {
            app.insert(0, candidate(name, *dir, *depth));
        }
        app
    };
    let rows = |app: &App| app.rows.iter().map(|c| c.path.clone()).collect::<Vec<_>>();
    for short_labels in [false, true] {
        let (wake, woken) = std::sync::mpsc::sync_channel(1);
        let mut background = filled(short_labels);
        background.set_waker(wake);
        let mut here = filled(short_labels);
        for query in ["", "x1", "x12", "n", "x1"] {
            background.query = query.into();
            background.refresh();
            here.query = query.into();
            here.refresh();
            if background.ranking_pending() {
                woken.recv_timeout(Duration::from_secs(10)).unwrap();
                assert!(background.poll_ranking());
            }
            assert_eq!(rows(&background), rows(&here), "query {query:?}");
            assert_eq!(background.labels, here.labels, "query {query:?}");
        }
        // A newer query supersedes a running ranking; waiting gives its rows.
        background.query.clear();
        background.refresh();
        background.query = "n1".into();
        background.refresh();
        background.finish_ranking();
        here.query = "n1".into();
        here.refresh();
        assert_eq!(rows(&background), rows(&here));
        // Paths found meanwhile join the rows once the ranking is installed.
        background.query.clear();
        background.kinds = Kinds::Files;
        background.refresh();
        assert!(background.ranking_pending());
        background.insert(0, candidate("late", false, 1));
        background.refresh();
        assert!(!background.settled());
        background.finish_ranking();
        background.refresh();
        here.query.clear();
        here.kinds = Kinds::Files;
        here.insert(0, candidate("late", false, 1));
        here.refresh();
        assert_eq!(rows(&background), rows(&here));
        assert!(background.settled());
    }
}
#[test]
fn either_sigma_matches_a_word_final_capital_sigma() {
    for query in ["Σ", "σ", "ς", "οσ"] {
        for name in ["ΟΔΟΣ", "ΟΔΟΣ_1", "ΟΔΟΣ.txt"] {
            assert!(score(name, query).is_some(), "{query:?} in {name:?}");
        }
    }
}
