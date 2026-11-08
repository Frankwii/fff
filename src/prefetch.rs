//! One cancellable speculative directory read; a small separate RAM cache.
use crate::{
    App, Candidate, Policy,
    cache::{Key, Snapshot},
};
use std::{
    collections::VecDeque,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

const DWELL: Duration = Duration::from_millis(500);
const COOLDOWN: Duration = Duration::from_secs(60);
const MAX_PATHS: usize = 2048;
// At most 3 MiB of retained candidate allocations plus a 1 MiB worker result.
const JOB_BYTES: usize = 1024 * 1024;
const CACHE_BYTES: usize = 3 * JOB_BYTES;
const MAX_SNAPSHOTS: usize = 4;
const WORK_BUDGET: Duration = Duration::from_millis(100);

struct Seed {
    key: Key,
    snapshot: Snapshot,
    bytes: usize,
}
struct Job {
    identity: (u64, u64, PathBuf),
    cancel: Arc<AtomicBool>,
    result: Receiver<Option<Seed>>,
    handle: JoinHandle<()>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Default)]
pub(crate) struct Prefetch {
    job: Option<Job>,
    stable: Option<((u64, u64, PathBuf), Instant)>,
    attempts: VecDeque<(Key, Instant)>,
    seeds: VecDeque<Seed>,
    pub attempts_started: u64,
}
impl Prefetch {
    pub fn cancel(&mut self) {
        if let Some(job) = &self.job {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.stable = None;
    }
    pub fn invalidate(&mut self, key: &Key) {
        self.cancel();
        self.seeds.retain(|s| &s.key != key);
    }
    pub fn take(&mut self, key: &Key, now: Instant) -> Option<Snapshot> {
        self.collect();
        let index = self.seeds.iter().position(|s| &s.key == key)?;
        let seed = self.seeds.remove(index)?;
        // Old prefetch is a useful provisional prefix, never proof of freshness.
        let mut snapshot = seed.snapshot;
        snapshot.stale = now.saturating_duration_since(snapshot.verified_at) >= COOLDOWN;
        Some(snapshot)
    }
    pub fn tick(&mut self, app: &App, allow: bool, now: Instant, wake: Option<SyncSender<()>>) {
        let target = if allow && app.depth == 1 && app.program().navigates() && !app.scanning {
            app.highlight
                .as_ref()
                .filter(|p| p.parent() == Some(app.cwd.as_path()))
                .filter(|p| app.known.get(p).is_some_and(|c| c.directory))
                .cloned()
        } else {
            None
        };
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        app.query.hash(&mut hasher);
        let identity = target.map(|p| (app.generation, hasher.finish(), p));
        if let Some(job) = &self.job
            && identity.as_ref() != Some(&job.identity)
        {
            job.cancel.store(true, Ordering::Relaxed);
        }
        self.collect();
        let Some(identity) = identity else {
            self.stable = None;
            return;
        };
        if self.stable.as_ref().is_none_or(|(old, _)| old != &identity) {
            self.stable = Some((identity, now));
            return;
        }
        if self.job.is_some()
            || now.saturating_duration_since(self.stable.as_ref().unwrap().1) < DWELL
        {
            return;
        }
        let key = Key::for_directory(app, identity.2.clone());
        if self
            .attempts
            .iter()
            .any(|(k, t)| *k == key && now.saturating_duration_since(*t) < COOLDOWN)
        {
            return;
        }
        if self.attempts.len() == 32 {
            self.attempts.pop_front();
        }
        self.attempts.push_back((key.clone(), now));
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let (tx, result) = mpsc::sync_channel(1);
        let cwd = app.cwd.clone();
        let target = identity.2.clone();
        let policy = app.policy.clone();
        let handle = thread::spawn(move || {
            let seed = read_seed(key, &cwd, &target, policy, &stopped);
            let _ = tx.send(seed);
            if let Some(wake) = wake {
                let _ = wake.try_send(());
            }
        });
        self.attempts_started += 1;
        self.job = Some(Job {
            identity,
            cancel,
            result,
            handle,
        });
    }
    fn collect(&mut self) {
        // A result is ready before the worker's final wake/return. Consume it
        // immediately, but keep the job slot until the thread really exits.
        let seed = self.job.as_ref().and_then(|job| {
            if job.cancel.load(Ordering::Relaxed) {
                None
            } else {
                job.result.try_recv().ok().flatten()
            }
        });
        if let Some(seed) = seed {
            self.store(seed);
        }
        if self
            .job
            .as_ref()
            .is_some_and(|job| job.handle.is_finished())
        {
            self.job.take();
        }
    }
    fn store(&mut self, seed: Seed) {
        self.seeds.retain(|s| s.key != seed.key);
        while self.seeds.len() >= MAX_SNAPSHOTS
            || self.seeds.iter().map(|s| s.bytes).sum::<usize>() + seed.bytes > CACHE_BYTES
        {
            self.seeds.pop_front();
        }
        self.seeds.push_back(seed);
    }
}

fn read_seed(
    key: Key,
    parent: &Path,
    target: &Path,
    policy: Policy,
    cancel: &AtomicBool,
) -> Option<Seed> {
    let start = Instant::now();
    // Never follow symlinks, cross a mount, or touch a remote/unknown filesystem.
    if cancel.load(Ordering::Relaxed)
        || !crate::mounts::current().local_same_mount(parent, target)
        || start.elapsed() >= WORK_BUDGET
    {
        return None;
    }
    let meta = fs::symlink_metadata(target).ok()?;
    if !meta.is_dir() || cancel.load(Ordering::Relaxed) || start.elapsed() >= WORK_BUDGET {
        return None;
    }
    let mut candidates = Vec::with_capacity(MAX_PATHS);
    let mut bytes = candidates.capacity() * std::mem::size_of::<Candidate>();
    let mut exhaustive = true;
    let mut physical_empty = true;
    for (visited, entry) in fs::read_dir(target).ok()?.enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        // Bound entries examined, including hidden/excluded entries.
        if visited >= MAX_PATHS || start.elapsed() >= WORK_BUDGET {
            exhaustive = false;
            break;
        }
        let entry = entry.ok()?;
        physical_empty = false;
        let path = entry.path();
        if !policy.allows(target, path.strip_prefix(target).ok()?) {
            continue;
        }
        let kind = entry.file_type().ok()?;
        // Avoid a potentially remote stat through an entry symlink. Foreground
        // discovery resolves its kind; the speculative prefix omits it.
        if kind.is_symlink() {
            exhaustive = false;
            continue;
        }
        let cost = path.capacity();
        if bytes + cost > JOB_BYTES {
            exhaustive = false;
            break;
        }
        bytes += cost;
        candidates.push(Candidate {
            path,
            directory: kind.is_dir(),
            depth: 1,
            root: 0,
            order: 0,
        });
    }
    if cancel.load(Ordering::Relaxed) {
        return None;
    }
    Some(Seed {
        snapshot: Snapshot {
            candidates: candidates.into(),
            complete: exhaustive,
            exhaustive: exhaustive && key.single_level(),
            physical_empty,
            stale: false,
            verified_at: Instant::now(),
        },
        key,
        bytes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
            let root = std::env::temp_dir().join(format!(
                "fff-prefetch-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::create_dir(root.join("child")).unwrap();
            Self(root)
        }
        fn app(&self) -> App {
            let mut app = App::browse(self.0.clone(), 1, Policy::default());
            let child = self.0.join("child");
            app.insert(
                0,
                Candidate {
                    path: child.clone(),
                    directory: true,
                    depth: 1,
                    root: 0,
                    order: 0,
                },
            );
            app.refresh();
            app.highlight = Some(child);
            app.scanning = false;
            app
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn finish(prefetch: &mut Prefetch, app: &App, now: Instant) {
        let start = Instant::now();
        while prefetch.job.is_some() {
            prefetch.tick(app, true, now, None);
            assert!(start.elapsed() < Duration::from_secs(300));
            thread::yield_now();
        }
    }
    #[test]
    fn stable_highlight_prefetches_first_visit_and_respects_cooldown_and_focus() {
        let f = Fixture::new();
        let app = f.app();
        if !crate::mounts::current().local_same_mount(&f.0, &f.0.join("child")) {
            return;
        }
        fs::write(f.0.join("child/file"), "").unwrap();
        let mut p = Prefetch::default();
        let now = Instant::now();
        p.tick(&app, false, now, None);
        p.tick(&app, false, now + DWELL, None);
        assert_eq!(p.attempts_started, 0);
        p.tick(&app, true, now, None);
        p.tick(&app, true, now + DWELL - Duration::from_millis(1), None);
        assert_eq!(p.attempts_started, 0);
        p.tick(&app, true, now + DWELL, None);
        finish(&mut p, &app, now + DWELL);
        assert_eq!(p.attempts_started, 1);
        let key = Key::for_directory(&app, f.0.join("child"));
        let seed = p.take(&key, now + DWELL).unwrap();
        assert!(seed.exhaustive && seed.complete);
        assert_eq!(seed.candidates[0].path, f.0.join("child/file"));
        for t in 1..20 {
            p.tick(&app, true, now + Duration::from_secs(t), None);
        }
        assert_eq!(p.attempts_started, 1);
    }
    #[test]
    fn policy_symlinks_depth_and_entry_budget_keep_prefixes_provisional() {
        let f = Fixture::new();
        let mut app = f.app();
        if !crate::mounts::current().local_same_mount(&f.0, &f.0.join("child")) {
            return;
        }
        for name in ["file", "skip.log", ".hidden"] {
            fs::write(f.0.join("child").join(name), "").unwrap();
        }
        app.policy.ignore = true;
        app.policy.set_rules(vec!["*.log".into()]).unwrap();
        let stopped = AtomicBool::new(false);
        let seed = read_seed(
            Key::for_directory(&app, f.0.join("child")),
            &f.0,
            &f.0.join("child"),
            app.policy.clone(),
            &stopped,
        )
        .unwrap();
        assert_eq!(seed.snapshot.candidates.len(), 1);
        assert!(seed.snapshot.exhaustive);
        app.depth = 2;
        let seed = read_seed(
            Key::for_directory(&app, f.0.join("child")),
            &f.0,
            &f.0.join("child"),
            app.policy.clone(),
            &stopped,
        )
        .unwrap();
        assert!(!seed.snapshot.exhaustive);
        app.depth = 1;
        std::os::unix::fs::symlink("file", f.0.join("child/link")).unwrap();
        let seed = read_seed(
            Key::for_directory(&app, f.0.join("child")),
            &f.0,
            &f.0.join("child"),
            app.policy.clone(),
            &stopped,
        )
        .unwrap();
        assert!(!seed.snapshot.complete);
        for n in 0..MAX_PATHS + 10 {
            fs::write(f.0.join(format!("child/f{n}")), "").unwrap();
        }
        let seed = read_seed(
            Key::for_directory(&app, f.0.join("child")),
            &f.0,
            &f.0.join("child"),
            app.policy.clone(),
            &stopped,
        )
        .unwrap();
        assert!(seed.snapshot.candidates.len() <= MAX_PATHS);
        assert!(seed.bytes <= JOB_BYTES);
        assert!(!seed.snapshot.exhaustive);
        stopped.store(true, Ordering::Relaxed);
        assert!(
            read_seed(
                Key::of(&app),
                &f.0,
                &f.0.join("child"),
                app.policy,
                &stopped
            )
            .is_none()
        );
    }
    fn complete(discovery: &mut crate::discovery::Discovery, app: &mut App) {
        let start = Instant::now();
        while app.scanning {
            discovery.drain(app);
            assert!(start.elapsed() < Duration::from_secs(300));
            thread::yield_now();
        }
    }

    #[test]
    fn prefetched_child_first_visit_can_skip_foreground_scan_and_f5_bypasses_it() {
        let f = Fixture::new();
        let file = f.0.join("child/file");
        fs::write(&file, "").unwrap();
        if !crate::mounts::current().local_same_mount(&f.0, &f.0.join("child")) {
            return;
        }
        let mut app = f.app();
        app.depth = 1;
        let (wake, waking) = std::sync::mpsc::sync_channel(1);
        let mut d = crate::discovery::Discovery::new(crate::config::CacheConfig::default());
        d.set_waker(wake);
        d.start(&mut app, true);
        complete(&mut d, &mut app);
        while waking.try_recv().is_ok() {}
        let now = Instant::now();
        d.idle(&app, true, now);
        d.idle(&app, true, now + Duration::from_millis(501));
        // Woken by the bounded speculative job, without a keyboard event or sleeps.
        waking.recv_timeout(Duration::from_secs(300)).unwrap();
        app.navigate(f.0.join("child"), false);
        d.start(&mut app, true);
        assert!(
            app.cache_pending && !app.scanning,
            "fresh prefetched child should be ready on its first visit"
        );
        assert_eq!(app.rows[0].path, file);
        assert_eq!(
            d.stats().stores,
            1,
            "prefetch does not displace visited snapshots"
        );
        d.force_refresh(&mut app);
        assert!(app.scanning && app.rows.is_empty());
        complete(&mut d, &mut app);
    }
    #[test]
    fn long_paths_hit_the_byte_budget_before_the_entry_limit() {
        let f = Fixture::new();
        let app = f.app();
        let mut target = f.0.clone();
        for _ in 0..8 {
            target.push("a".repeat(120));
        }
        // Paths this long exceed macOS's PATH_MAX (1024 bytes).
        if fs::create_dir_all(&target).is_err_and(|e| e.raw_os_error() == Some(libc::ENAMETOOLONG))
        {
            return;
        }
        let parent = target.parent().unwrap();
        if !crate::mounts::current().local_same_mount(parent, &target) {
            return;
        }
        for n in 0..1000 {
            fs::write(target.join(format!("{n:04}{}", "x".repeat(200))), "").unwrap();
        }
        let seed = read_seed(
            Key::for_directory(&app, target.clone()),
            parent,
            &target,
            app.policy.clone(),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(seed.bytes <= JOB_BYTES);
        assert!(seed.snapshot.candidates.len() < 1000);
        assert!(!seed.snapshot.complete && !seed.snapshot.exhaustive);
    }
    #[test]
    fn speculative_storage_has_separate_byte_and_entry_limits() {
        let f = Fixture::new();
        let app = f.app();
        let mut p = Prefetch::default();
        for n in 0..10 {
            let key = Key::for_directory(&app, f.0.join(format!("child{n}")));
            p.store(Seed {
                key,
                bytes: JOB_BYTES,
                snapshot: Snapshot {
                    candidates: Arc::from([]),
                    complete: false,
                    exhaustive: false,
                    physical_empty: false,
                    stale: false,
                    verified_at: Instant::now(),
                },
            });
            assert!(p.seeds.len() <= MAX_SNAPSHOTS);
            assert!(p.seeds.iter().map(|s| s.bytes).sum::<usize>() <= CACHE_BYTES);
        }
        assert_eq!(p.seeds.len(), 3);
        assert!(p.seeds.front().unwrap().key == Key::for_directory(&app, f.0.join("child7")));
    }
    #[test]
    fn cancelled_stuck_worker_cannot_spawn_successors_or_publish_results() {
        let f = Fixture::new();
        let mut app = f.app();
        let (release, wait) = mpsc::channel();
        let (tx, result) = mpsc::sync_channel(1);
        let cancel = Arc::new(AtomicBool::new(false));
        let handle = thread::spawn(move || {
            wait.recv().unwrap();
            let _ = tx.send(None);
        });
        let mut p = Prefetch {
            job: Some(Job {
                identity: (app.generation, 0, f.0.join("child")),
                cancel: cancel.clone(),
                result,
                handle,
            }),
            ..Prefetch::default()
        };
        let now = Instant::now();
        app.query = "changed".into();
        p.tick(&app, true, now, None);
        p.tick(&app, true, now + DWELL, None);
        assert!(cancel.load(Ordering::Relaxed));
        assert!(p.job.is_some());
        assert_eq!(p.attempts_started, 0);
        release.send(()).unwrap();
        // No new scheduling when leaving results focus.
        let start = Instant::now();
        while p.job.is_some() {
            p.tick(&app, false, now, None);
            assert!(start.elapsed() < Duration::from_secs(300));
            thread::yield_now();
        }
        assert!(p.seeds.is_empty());
    }
}
