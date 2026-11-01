use picker::{App, Policy, config::CacheConfig, discovery::Discovery};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "picker-cache-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&p).unwrap();
        Self(p)
    }
    fn file(&self, name: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, "").unwrap();
        p
    }
    fn app(&self) -> App {
        App::browse(self.0.clone(), 2, Policy::default())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn complete(discovery: &mut Discovery, app: &mut App) {
    let start = Instant::now();
    while app.scanning {
        discovery.drain(app);
        assert!(start.elapsed() < Duration::from_secs(5), "scan timed out");
        std::thread::yield_now();
    }
}
#[test]
fn completed_visit_restores_synchronously_then_revalidates() {
    let f = Fixture::new();
    let file = f.file("report.tex");
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert_eq!(d.stats().stores, 1);
    assert!(!app.cache_pending);
    let other = f.0.join("visited");
    fs::create_dir(&other).unwrap();
    app.navigate(other, false);
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    app.navigate(f.0.clone(), false);
    app.query = "rpx".into();
    app.marks.push(file.clone());
    app.offset = 9;
    d.start(&mut app, true);
    // No sleeps or channel draining: these rows must be installed by start itself.
    assert!(app.cache_pending && app.scanning);
    assert_eq!(app.rows[0].path, file);
    assert_eq!(app.query, "rpx");
    assert_eq!(app.marks, vec![file]);
    assert!(!app.physical_empty);
    assert_eq!(d.stats().hits, 1);
    let generation = app.generation;
    app.query.clear();
    app.refresh();
    assert_eq!(app.generation, generation);
    complete(&mut d, &mut app);
    assert!(!app.cache_pending);
    assert_eq!(d.stats().stores, 3);
}
#[test]
fn changed_options_miss_and_restoring_options_hits() {
    let f = Fixture::new();
    f.file("a");
    f.file(".hidden");
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    app.policy.hidden = true;
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    complete(&mut d, &mut app);
    assert_eq!(app.rows.len(), 2);
    app.policy.hidden = false;
    d.start(&mut app, true);
    assert!(app.cache_pending);
    assert_eq!(app.rows.len(), 1);
    complete(&mut d, &mut app);
    app.depth = 1;
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    complete(&mut d, &mut app);
    app.policy.set_rules(vec!["a".into()]).unwrap();
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    complete(&mut d, &mut app);
    app.policy.glob = Some(picker::glob("*.tex").unwrap());
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    complete(&mut d, &mut app);
}
#[test]
fn cancelled_or_failed_scans_are_never_stored() {
    let f = Fixture::new();
    f.file("a");
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    assert_eq!(d.stats().stores, 0);
    assert_eq!(d.stats().hits, 0);
    complete(&mut d, &mut app);
    assert_eq!(d.stats().stores, 1);
    app.navigate(f.0.join("absent"), false);
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(!app.status.is_empty());
    assert!(!app.physical_empty);
    assert_eq!(d.stats().stores, 1);
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    complete(&mut d, &mut app);
    assert_eq!(d.stats().stores, 1);
}
#[test]
fn refresh_reconciles_deletions_additions_kind_changes_and_preserves_marks() {
    let f = Fixture::new();
    let a = f.file("a");
    let b = f.file("b");
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    app.marks.push(a.clone());
    fs::remove_file(&a).unwrap();
    fs::remove_file(&b).unwrap();
    fs::create_dir(&b).unwrap();
    let c = f.file("c");
    d.start(&mut app, true);
    assert!(app.known.contains_key(&a));
    assert!(!app.ready_path(&a));
    app.highlight = Some(b.clone());
    complete(&mut d, &mut app);
    assert!(!app.known.contains_key(&a));
    assert!(app.known[&b].directory);
    assert!(app.known.contains_key(&c));
    assert_eq!(app.highlight, Some(b));
    assert_eq!(app.marks, vec![a]);
    assert!(app.confirm(None).is_none());
}
#[test]
fn snapshots_bound_rows_and_prioritize_first_completed_matches() {
    let f = Fixture::new();
    for i in 0..100 {
        f.file(&format!("file{i:03}"));
    }
    let mut app = f.app();
    app.query = "099".into();
    let mut d = Discovery::new(CacheConfig {
        max_candidates: 3,
        ..CacheConfig::default()
    });
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert_eq!(app.known.len(), 100);
    d.start(&mut app, true);
    assert_eq!(app.known.len(), 3);
    assert_eq!(app.rows[0].path, f.0.join("file099"));
    app.query.clear();
    app.refresh();
    assert_eq!(app.rows.len(), 3);
    complete(&mut d, &mut app);
    assert_eq!(app.rows.len(), 100);
}
#[test]
fn explicit_refresh_bypasses_cache_without_losing_session_settings() {
    let f = Fixture::new();
    let file = f.file("a");
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    app.marks.push(file);
    app.query = "a".into();
    let depth = app.depth;
    d.force_refresh(&mut app);
    assert!(!app.cache_pending);
    assert!(app.rows.is_empty());
    assert_eq!(app.query, "a");
    assert_eq!(app.depth, depth);
    assert_eq!(app.marks.len(), 1);
    complete(&mut d, &mut app);
    d.start(&mut app, true);
    assert!(app.cache_pending);
    complete(&mut d, &mut app);
}
#[test]
fn disabled_cache_and_empty_directory_confirmation_are_safe() {
    let f = Fixture::new();
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig {
        enabled: false,
        ..CacheConfig::default()
    });
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(app.physical_empty);
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    assert!(!app.physical_empty);
    complete(&mut d, &mut app);
    assert_eq!(d.stats().stores, 0);
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    d.start(&mut app, true);
    assert!(app.cache_pending);
    assert_eq!(app.enter_numbered(1), picker::EnterOutcome::Stay);
    complete(&mut d, &mut app);
    assert_eq!(
        app.enter_numbered(1),
        picker::EnterOutcome::Selected(vec![f.0.clone()])
    );
}
#[test]
fn overlapping_roots_keep_effective_depth_and_scope_attribution_on_hits() {
    let f = Fixture::new();
    let file = f.file("nested/file.tex");
    let root = f.0.clone();
    let nested = root.join("nested");
    let mut app = App::new(vec![root, nested], 3, Policy::default());
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert_eq!(app.known[&file].depth, 1);
    assert_eq!(app.known[&file].root, 1);
    d.start(&mut app, true);
    assert!(app.cache_pending);
    assert_eq!(app.known[&file].depth, 1);
    assert_eq!(app.known[&file].root, 1);
    complete(&mut d, &mut app);
}
#[test]
fn navigation_only_action_never_confirms_files_or_empty_directories() {
    let f = Fixture::new();
    let file = f.file("file");
    let directory = f.0.join("directory");
    fs::create_dir(&directory).unwrap();
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    app.highlight = Some(file.clone());
    app.marks.push(file);
    assert!(!app.navigate_highlighted());
    assert_eq!(app.cwd, f.0);
    app.highlight = Some(directory.clone());
    assert!(app.navigate_highlighted());
    assert_eq!(app.cwd, directory);
    assert_eq!(app.marks.len(), 1);
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(app.physical_empty);
    assert!(!app.navigate_highlighted());
    assert_eq!(app.cwd, Path::new(&directory));
}

#[test]
fn bookmarks_filter_and_navigate_without_mutating_roots_depth_or_marks() {
    let f = Fixture::new();
    let file = f.file("notes folder/file");
    let directory = file.parent().unwrap().to_path_buf();
    let mut app = App::new(vec![f.0.clone()], 2, Policy::default());
    app.query = "old query".into();
    app.marks.push(file.clone());
    app.bookmarks = vec![
        picker::config::Bookmark {
            name: "My notes".into(),
            path: directory.clone(),
        },
        picker::config::Bookmark {
            name: "File".into(),
            path: file.clone(),
        },
        picker::config::Bookmark {
            name: "Missing".into(),
            path: f.0.join("missing"),
        },
    ];
    assert_eq!(app.matching_bookmarks("mnt"), vec![0]);
    assert!(app.matching_bookmarks("zzzz").is_empty());
    let roots = app.roots.clone();
    assert!(!app.open_bookmark(1));
    assert!(!app.open_bookmark(2));
    assert_eq!(app.cwd, f.0);
    assert_eq!(app.query, "old query");
    assert!(app.open_bookmark(0));
    assert_eq!(app.cwd, directory);
    assert!(app.query.is_empty());
    assert_eq!(app.roots, roots);
    assert_eq!(app.depth, 2);
    assert_eq!(app.marks, vec![file]);
}

#[test]
fn cached_file_changed_to_directory_navigates_instead_of_confirming() {
    let f = Fixture::new();
    let path = f.file("target");
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    fs::remove_file(&path).unwrap();
    fs::create_dir(&path).unwrap();
    d.start(&mut app, true);
    assert!(app.cache_pending);
    assert!(!app.rows[0].directory);
    app.rendered = vec![path.clone()];
    assert_eq!(app.enter_numbered(1), picker::EnterOutcome::Navigated);
    assert_eq!(app.cwd, path);
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(app.physical_empty);
}
