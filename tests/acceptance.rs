use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use picker::{
    App, Candidate, Policy, discovery::Scan, glob, keys::KeyMap, relative_to, score,
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
fn example13_live_order_highlight_late_depth_and_rendered_choice() {
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
fn example04_overlap_and_root_order() {
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
fn example01_subsequence_and_depth_before_score() {
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
fn example12_numbered_choice_zero_fallback_and_key_decoding() {
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
            "picker-test-{}-{}",
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
fn example03_real_discovery_glob_depth_and_retained_state() {
    let f = Fixture::new();
    for name in ["zebra.tex", "topic/alpha.tex", "topic/details/beta.tex"] {
        f.file(name);
    }
    let policy = Policy {
        glob: Some(glob("*.tex").unwrap()),
        ..Policy::default()
    };
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
fn example05_marks_confirmation_and_missing_paths() {
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
fn example07_policy_toggles_prune_and_preserve_marks() {
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
    use picker::EnterOutcome;
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
    use picker::EnterOutcome;
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
    use picker::ui::alt_numbered_key;
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
