use fuzzy_file_finder::{App, Policy, config::CacheConfig, discovery::Discovery};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
// Existing reconciliation scenarios explicitly request immediate revalidation.
fn always_refresh() -> CacheConfig {
    CacheConfig {
        fresh_seconds: 0,
        ..CacheConfig::default()
    }
}
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        // Canonical, as fff's paths are (macOS's temporary directory is below
        // the `/var` symlink).
        let p = std::env::temp_dir().canonicalize().unwrap().join(format!(
            "fff-cache-{}-{}",
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
    let mut d = Discovery::new(always_refresh());
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
    let mut d = Discovery::new(always_refresh());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    app.policy.hidden = true;
    d.start(&mut app, true);
    assert!(app.scanning);
    complete(&mut d, &mut app);
    assert_eq!(app.rows.len(), 2);
    app.policy.hidden = false;
    d.start(&mut app, true);
    assert!(app.cache_pending);
    assert_eq!(app.rows.len(), 1);
    complete(&mut d, &mut app);
    app.depth = 1;
    d.start(&mut app, true);
    assert!(app.scanning);
    complete(&mut d, &mut app);
    app.policy.set_rules(vec!["a".into()]).unwrap();
    d.start(&mut app, true);
    assert!(app.scanning);
    complete(&mut d, &mut app);
    // File filters only choose among discovered files, so the snapshot is reused.
    let hits = d.stats().hits;
    app.policy.set_filters(vec!["*.tex".into()]).unwrap();
    d.start(&mut app, true);
    assert_eq!(d.stats().hits, hits + 1);
    assert!(app.rows.is_empty());
    complete(&mut d, &mut app);
}
#[test]
fn cancelled_or_failed_scans_are_never_stored() {
    let f = Fixture::new();
    f.file("a");
    let mut app = f.app();
    let mut d = Discovery::new(always_refresh());
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
    let mut d = Discovery::new(always_refresh());
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
        ..always_refresh()
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
    let mut d = Discovery::new(always_refresh());
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
        ..always_refresh()
    });
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(app.physical_empty);
    d.start(&mut app, true);
    assert!(!app.cache_pending);
    assert!(!app.physical_empty);
    complete(&mut d, &mut app);
    assert_eq!(d.stats().stores, 0);
    let mut d = Discovery::new(always_refresh());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    d.start(&mut app, true);
    assert!(app.cache_pending);
    // A replayed empty folder is no answer until a scan verifies it.
    assert_eq!(app.empty_folder(), None);
    complete(&mut d, &mut app);
    assert_eq!(app.empty_folder(), Some(vec![f.0.clone()]));
}
#[test]
fn overlapping_roots_keep_effective_depth_and_scope_attribution_on_hits() {
    let f = Fixture::new();
    let file = f.file("nested/file.tex");
    let root = f.0.clone();
    let nested = root.join("nested");
    let mut app = App::new(vec![root, nested], 3, Policy::default());
    let mut d = Discovery::new(always_refresh());
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
    let mut d = Discovery::new(always_refresh());
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
fn workspaces_filter_and_navigate_without_mutating_roots_depth_or_marks() {
    let f = Fixture::new();
    let file = f.file("notes folder/file");
    let directory = file.parent().unwrap().to_path_buf();
    let mut app = App::new(vec![f.0.clone()], 2, Policy::default());
    app.query = "old query".into();
    app.marks.push(file.clone());
    app.workspaces = vec![
        fuzzy_file_finder::config::Workspace {
            name: "My notes".into(),
            path: Some(directory.clone()),
            icon: None,
            ..Default::default()
        },
        fuzzy_file_finder::config::Workspace {
            name: "File".into(),
            path: Some(file.clone()),
            icon: None,
            ..Default::default()
        },
        fuzzy_file_finder::config::Workspace {
            name: "Missing".into(),
            path: Some(f.0.join("missing")),
            icon: None,
            ..Default::default()
        },
    ];
    assert_eq!(app.matching_workspaces("mnt"), vec![0]);
    assert!(app.matching_workspaces("zzzz").is_empty());
    let roots = app.roots.clone();
    assert!(!app.open_workspace(1));
    assert!(!app.open_workspace(2));
    assert_eq!(app.cwd, f.0);
    assert_eq!(app.query, "old query");
    assert!(app.open_workspace(0));
    assert_eq!(app.cwd, directory);
    // A folder-only workspace changes nothing else, the query included.
    assert_eq!(app.query, "old query");
    assert_eq!(app.roots, roots);
    assert_eq!(app.depth, 2);
    assert_eq!(app.marks, vec![file]);
}

#[test]
fn workspaces_apply_the_settings_they_keep_and_leave_out_missing_roots() {
    use fuzzy_file_finder::{config::Workspace, program::Program};
    let f = Fixture::new();
    let papers = f.file("papers/a.pdf").parent().unwrap().to_path_buf();
    let books = f.file("books/b.pdf").parent().unwrap().to_path_buf();
    let mut app = App::browse(f.0.clone(), 1, Policy::default());
    app.query = "kept".into();
    let workspace = Workspace {
        name: "findnotes".into(),
        program: Some(Program::Pick),
        roots: Some(vec![papers.clone(), f.0.join("gone"), books.clone()]),
        filters: Some(vec!["*.pdf".into()]),
        hidden: Some(true),
        contents: Some("lemma".into()),
        ..Default::default()
    };
    assert!(app.apply_workspace(&workspace));
    assert_eq!(app.program(), Program::Pick);
    // No depth kept: the new program's own.
    assert_eq!(app.depth, Program::Pick.default_depth());
    // Without a folder, the first root is where it starts.
    assert_eq!(
        (app.cwd.clone(), app.roots.clone()),
        (papers.clone(), vec![papers.clone(), books.clone()])
    );
    assert!(app.scope.contains(&books));
    assert_eq!(app.policy.filters, ["*.pdf"]);
    assert!(app.policy.hidden);
    assert_eq!(
        (app.query.as_str(), app.grep.text.as_str()),
        ("kept", "lemma")
    );
    assert!(
        app.status.contains("left out missing roots"),
        "{}",
        app.status
    );
    // Nothing changes when none of the roots is there.
    let gone = Workspace {
        name: "gone".into(),
        roots: Some(vec![f.0.join("gone")]),
        depth: Some(5),
        ..Default::default()
    };
    assert!(!app.apply_workspace(&gone));
    assert_eq!(
        (app.depth, app.roots.len()),
        (Program::Pick.default_depth(), 2)
    );
    // A window of its own keeps its program and applies the rest; one
    // that would only change the program is not offered there.
    let mut window = App::browse(f.0.clone(), 1, Policy::default());
    window.set_program(Program::Window);
    window.workspaces = vec![
        Workspace {
            name: "only".into(),
            program: Some(Program::Seek),
            ..Default::default()
        },
        Workspace {
            name: "deep".into(),
            program: Some(Program::Seek),
            depth: Some(4),
            ..Default::default()
        },
    ];
    assert_eq!(window.matching_workspaces(""), vec![1]);
    assert!(window.open_workspace(1));
    assert_eq!((window.program(), window.depth), (Program::Window, 4));
    assert!(
        window.status.contains("fff window stays"),
        "{}",
        window.status
    );
}

#[test]
fn cached_file_changed_to_directory_waits_for_the_refresh_instead_of_confirming() {
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
    let row = fuzzy_file_finder::Candidate::clone(&app.rows[0]);
    assert!(!app.ready_entry(&row));
    assert!(app.status.contains("waiting for refresh"), "{}", app.status);
    app.navigate(path.clone(), false);
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(app.physical_empty);
}

#[test]
fn partial_visits_and_current_query_are_checkpointed_and_replay_before_drain() {
    let f = Fixture::new();
    for i in 0..100 {
        f.file(&format!("file{i:03}"));
    }
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig {
        max_candidates: 3,
        ..always_refresh()
    });
    d.start(&mut app, true);
    // Deterministic partially discovered prefix, before the worker completes.
    app.insert(
        app.generation,
        fuzzy_file_finder::Candidate {
            path: f.0.join("file099"),
            directory: false,
            depth: 1,
            root: 0,
            order: 0,
        },
    );
    app.refresh();
    d.checkpoint(&app);
    assert_eq!(d.stats().stores, 1);
    d.start(&mut app, true);
    assert!(app.cache_pending);
    assert_eq!(app.rows.len(), 1);
    // The first drain waits about a frame for the scan: a quick one is drawn
    // complete, never first from the cache; a slow one keeps the cached rows.
    d.drain(&mut app);
    if app.scanning {
        assert!(app.cache_pending);
        assert_eq!(app.rows.len(), 1);
    } else {
        assert!(!app.cache_pending);
        assert!(app.rows.len() > 1);
    }
    complete(&mut d, &mut app);
    app.query = "099".into();
    app.refresh();
    d.checkpoint(&app);
    app.query.clear();
    d.start(&mut app, true);
    assert!(app.known.contains_key(&f.0.join("file099")));
    complete(&mut d, &mut app);
}
#[test]
fn report_warm_replay_cost_for_large_directory_without_waiting_for_worker() {
    let f = Fixture::new();
    let mut app = f.app();
    let mut d = Discovery::new(always_refresh());
    d.start(&mut app, true);
    for i in 0..20_000 {
        app.insert(
            app.generation,
            fuzzy_file_finder::Candidate {
                path: f.0.join(format!("file{i:06}")),
                directory: false,
                depth: 1,
                root: 0,
                order: 0,
            },
        );
    }
    app.refresh();
    d.checkpoint(&app);
    let start = Instant::now();
    d.start(&mut app, true);
    let replay = start.elapsed();
    assert_eq!(app.rows.len(), 4096);
    assert!(app.cache_pending);
    println!("Warm replay of 4096 cached paths from 20000 candidates: {replay:?}");
}

#[test]
fn recent_complete_views_skip_scanning_but_expire_and_f5_forces_freshness() {
    let f = Fixture::new();
    let a = f.file("a");
    let mut app = f.app();
    app.depth = 1;
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    let verified = Instant::now();
    let b = f.file("b");
    app.marks.push(a.clone());
    app.rendered = vec![a.clone()];
    d.checkpoint(&app);
    d.start(&mut app, true);
    assert!(!app.scanning);
    assert!(app.cache_pending);
    assert_eq!(app.rows.len(), 1); // a new file can wait a few seconds
    assert_eq!(d.stats().stores, 2); // scan plus checkpoint, no revisit scan
    // Checkpointing/revisiting must not postpone the original verification age.
    for _ in 0..3 {
        d.checkpoint(&app);
        d.start(&mut app, true);
    }
    // Expired: a quiet rescan starts (and a quick one ends within the drain).
    let generation = app.generation;
    d.drain_at(&mut app, verified + Duration::from_secs(4));
    assert!(app.generation > generation);
    complete(&mut d, &mut app);
    assert!(app.known.contains_key(&b));
    assert_eq!(app.rendered, vec![a.clone()]); // discovery never publishes shortcuts
    assert_eq!(app.marks, vec![a]);
    let c = f.file("c");
    d.force_refresh(&mut app);
    assert!(app.scanning && app.rows.is_empty());
    complete(&mut d, &mut app);
    assert!(app.known.contains_key(&c));
}

#[test]
fn truncated_or_empty_snapshots_never_skip_verification() {
    let f = Fixture::new();
    f.file("a");
    f.file("b");
    let mut app = f.app();
    let mut d = Discovery::new(CacheConfig {
        max_candidates: 1,
        ..CacheConfig::default()
    });
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    d.start(&mut app, true);
    assert!(app.scanning);
    assert_eq!(app.known.len(), 1);
    complete(&mut d, &mut app);
    assert_eq!(app.known.len(), 2);
    let empty = f.0.join("empty");
    fs::create_dir(&empty).unwrap();
    app.navigate(empty, false);
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    d.start(&mut app, true);
    assert!(app.scanning && !app.physical_empty);
    assert_eq!(app.empty_folder(), None);
}

#[test]
fn first_child_visit_reuses_known_descendants_without_claiming_completeness() {
    let f = Fixture::new();
    let immediate = f.file("child/immediate");
    let deeper = f.file("child/one/two/deeper");
    let mut app = f.app();
    app.depth = 3;
    let mut d = Discovery::new(CacheConfig::default());
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(!app.known.contains_key(&deeper));
    app.navigate(f.0.join("child"), false);
    d.start(&mut app, true);
    assert!(app.cache_pending && app.scanning);
    assert_eq!(app.known[&immediate].depth, 1);
    assert!(!app.physical_empty);
    complete(&mut d, &mut app);
    assert_eq!(app.known[&deeper].depth, 3);
}

#[test]
fn worker_notifies_ui_without_keyboard_input() {
    let f = Fixture::new();
    f.file("file");
    let mut app = f.app();
    let (wake, waking) = std::sync::mpsc::sync_channel(1);
    let mut d = Discovery::new(CacheConfig::default());
    d.set_waker(wake);
    d.start(&mut app, true);
    waking
        .recv_timeout(Duration::from_secs(2))
        .expect("discovery must wake a sleeping UI");
    complete(&mut d, &mut app);
    assert_eq!(app.rows.len(), 1);
}
#[test]
fn scans_stop_past_max_paths_without_reconciling_or_caching() {
    let f = Fixture::new();
    for i in 0..300 {
        f.file(&format!("file{i:03}"));
    }
    let mut app = f.app();
    let mut d = Discovery::new(always_refresh());
    d.max_paths = 10;
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    // At most one batch beyond the limit is ever held.
    assert!(app.known.len() > 10 && app.known.len() < 300);
    assert!(app.status.contains("max_paths"), "{}", app.status);
    assert!(app.truncated);
    assert_eq!(app.rows.len(), app.known.len());
    assert_eq!(d.stats().stores, 0);
    assert!(!d.busy(&app));
}
#[test]
fn a_shallower_depth_after_truncation_lists_completely() {
    let f = Fixture::new();
    for i in 0..300 {
        f.file(&format!("dir{}/file{i:03}", i % 3));
    }
    let mut app = f.app();
    app.depth = 2;
    let mut d = Discovery::new(always_refresh());
    d.max_paths = 100;
    d.start(&mut app, true);
    complete(&mut d, &mut app);
    assert!(app.truncated);
    // Depth 1 holds three folders: the paths left from the deeper scan do not
    // count against the limit.
    app.depth = 1;
    d.start(&mut app, false);
    complete(&mut d, &mut app);
    assert!(!app.truncated, "{}", app.status);
    assert_eq!(app.rows.len(), 3);
}
#[test]
fn a_small_tree_is_complete_after_the_first_drain() {
    let f = Fixture::new();
    for i in 0..40 {
        f.file(&format!("dir{}/file{i}.txt", i % 5));
    }
    let mut app = f.app();
    let mut d = Discovery::new(always_refresh());
    // A first visit, then a revisit replaying the cache: either way the frame
    // after starting shows the whole listing, never empty or "cached".
    for visit in ["first", "cached"] {
        // A loaded machine may take longer; the drain waits about a frame.
        let mut finished = false;
        for _ in 0..20 {
            d.start(&mut app, true);
            d.drain(&mut app);
            if !app.scanning {
                finished = true;
                break;
            }
            complete(&mut d, &mut app);
        }
        assert!(finished, "{visit} visit never finished within a drain");
        assert!(!app.cache_pending, "{visit}");
        assert_eq!(app.rows.len(), 45, "{visit}");
        d.checkpoint(&app);
    }
}
#[test]
fn a_ranking_on_another_thread_leaves_the_loop_idle_until_it_is_done() {
    let f = Fixture::new();
    f.file("a");
    let mut app = f.app();
    let mut d = Discovery::new(always_refresh());
    d.start(&mut app, false);
    complete(&mut d, &mut app);
    // Enough rows for a ranking on its own thread.
    for i in 0..20_000 {
        app.insert(
            app.generation,
            fuzzy_file_finder::Candidate {
                path: f.0.join(format!("f{i}")),
                directory: false,
                depth: 1,
                root: 0,
                order: 0,
            },
        );
    }
    app.refresh();
    let (wake, woken) = std::sync::mpsc::sync_channel(1);
    app.set_waker(wake);
    app.kinds = fuzzy_file_finder::Kinds::Files;
    app.refresh();
    assert!(app.ranking_pending());
    // Nothing to merge or draw until the ranking thread wakes the loop.
    assert!(!d.catching_up(&app));
    assert!(!d.drain(&mut app));
    woken.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(app.poll_ranking());
    assert!(!d.catching_up(&app) && app.settled());
    assert_eq!(app.rows.len(), 20_001);
}
