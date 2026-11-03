//! Bounded background traversal. Replacing a scan cancels its predecessor.
use crate::{App, Candidate, Policy};
use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, SyncSender},
    },
    thread,
    time::{Duration, Instant},
};

/// Worker messages. The walker sends `Batch`es; `Found` remains for injected
/// single events in tests.
pub enum Event {
    Found(u64, Candidate),
    Batch(u64, Vec<Candidate>),
    Error(u64, String),
    Done(u64, bool),
}

pub struct Scan {
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    failed: bool,
    completed: bool,
    /// The walker finished (with whether the directory is empty); applied
    /// once the rows reflect everything it delivered.
    done: Option<bool>,
    /// `[performance] max_paths`: the scan stops once it found more paths.
    pub max_paths: usize,
    /// Paths this scan delivered. Known paths left from an earlier scan, say
    /// a truncated deeper one, do not count against `max_paths`.
    found: usize,
    /// Input events waiting: taking discoveries and merging stop for them.
    input: Option<Arc<AtomicUsize>>,
    /// Minimum time between re-sorts while results stream in (zero: every drain).
    pub refresh_every: Duration,
    last_refresh: Option<Instant>,
    /// Known paths changed since the last re-sort.
    pending: bool,
}
/// Most events handled per drain before the frame is drawn again.
const DRAIN_BUDGET: Duration = Duration::from_millis(8);
/// How long the first drain after a scan starts waits for it to finish, so
/// that a small listing is drawn once, complete, rather than first empty or
/// from the cache and then again: within a frame, it looks immediate.
const FIRST_FRAME: Duration = Duration::from_millis(12);
/// While waiting, keys waiting to be read are checked this often.
const SETTLE_STEP: Duration = Duration::from_millis(1);
/// Time per drain for merging new paths into the rows while a scan streams in.
const MERGE_BUDGET: Duration = Duration::from_millis(4);
/// Unmerged paths at which a drain stops taking events, leaving the walker
/// waiting until the rows catch up.
const BACKLOG: usize = 8192;
/// The walker sends a batch at this size, or sooner once it is this old.
const BATCH_SIZE: usize = 256;
const BATCH_AGE: Duration = Duration::from_millis(10);
impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
impl Scan {
    pub fn start(app: &mut App, clear: bool) -> Self {
        Self::start_waking(app, clear, None)
    }
    fn start_waking(app: &mut App, clear: bool, wake: Option<SyncSender<()>>) -> Self {
        Self::reset(app, clear);
        // Bounded in batches: at most 16 × BATCH_SIZE candidates wait in flight.
        let (events_tx, events) = mpsc::sync_channel(16);
        let tx = EventSender {
            events: events_tx,
            wake,
        };
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let roots = app.scope.clone();
        let current = app.cwd.clone();
        let depth = app.depth;
        let generation = app.generation;
        let policy = app.policy.clone();
        thread::spawn(move || walk(roots, current, depth, policy, generation, tx, stopped));
        Self {
            events,
            cancel,
            failed: false,
            completed: false,
            done: None,
            max_paths: usize::MAX,
            found: 0,
            input: None,
            refresh_every: Duration::ZERO,
            last_refresh: None,
            pending: false,
        }
    }
    fn reset(app: &mut App, clear: bool) {
        app.generation += 1;
        app.scanning = true;
        app.quiet_scan = false;
        app.physical_empty = false;
        app.truncated = false;
        app.cache_pending = false;
        app.status.clear();
        if clear {
            app.known.clear();
            app.known_version += 1;
        }
        app.refresh();
    }
    /// Apply waiting events for up to `DRAIN_BUDGET`, then re-sort if due: the
    /// first results, completion, and otherwise at most every `refresh_every`.
    /// Returns whether anything visible may have changed.
    pub fn drain(&mut self, app: &mut App) -> bool {
        self.drain_for(app, Duration::ZERO)
    }
    fn drain_for(&mut self, app: &mut App, settle: Duration) -> bool {
        let start = Instant::now();
        let mut changed = false;
        let input = |scan: &Self| {
            scan.input
                .as_ref()
                .is_some_and(|waiting| waiting.load(Ordering::Relaxed) > 0)
        };
        // Take events only while the rows keep up: a bounded backlog keeps
        // each merge, and any refresh a key press needs, short.
        while start.elapsed() < DRAIN_BUDGET.max(settle)
            && self.done.is_none()
            && !self.completed
            && app.unmerged() < BACKLOG
            && !input(self)
        {
            let event = match self.events.try_recv() {
                Ok(event) => event,
                Err(mpsc::TryRecvError::Empty) if start.elapsed() < settle => {
                    let wait = settle.saturating_sub(start.elapsed()).min(SETTLE_STEP);
                    match self.events.recv_timeout(wait) {
                        Ok(event) => event,
                        Err(mpsc::RecvTimeoutError::Timeout) => continue,
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                }
                Err(_) => break,
            };
            changed = true;
            match event {
                Event::Found(g, c) if g == app.generation => {
                    self.found += 1;
                    app.insert(g, c);
                    self.pending = true;
                }
                Event::Batch(g, batch) if g == app.generation => {
                    self.found += batch.len();
                    for c in batch {
                        app.insert(g, c);
                    }
                    self.pending = true;
                }
                Event::Error(g, s) if g == app.generation => {
                    self.failed = true;
                    app.status = s;
                }
                Event::Done(g, empty) if g == app.generation => {
                    self.done = Some(empty);
                    self.pending = true;
                }
                _ => {}
            }
            if self.found > self.max_paths {
                self.truncate(app);
            }
        }
        let due = self
            .last_refresh
            .is_none_or(|t| t.elapsed() >= self.refresh_every);
        let finishing = self.done.is_some() || self.completed;
        // A ranking on another thread wakes the loop when it is done; until
        // then a refresh would only find it pending.
        if (self.pending || !app.settled())
            && !app.ranking_pending()
            && (finishing || due || app.unmerged() >= BACKLOG)
            && !input(self)
        {
            app.refresh_within(Some(MERGE_BUDGET));
            self.pending = !app.settled();
            self.last_refresh = Some(Instant::now());
            changed = true;
        }
        if let Some(empty) = self.done
            && app.settled()
        {
            self.done = None;
            app.scanning = false;
            app.cache_pending = false;
            app.physical_empty = empty;
            app.reconcile(app.generation);
            app.refresh();
            self.completed = true;
            changed = true;
        }
        changed
    }
    /// More paths are known than `max_paths` allows: stop here. The listing
    /// is partial, so it is neither reconciled nor kept as complete.
    fn truncate(&mut self, app: &mut App) {
        self.cancel.store(true, Ordering::Relaxed);
        self.failed = true;
        self.completed = true;
        self.pending = true;
        app.scanning = false;
        app.cache_pending = false;
        app.truncated = true;
        app.status = format!(
            "Stopped listing at {} paths ([performance] max_paths); lower the depth to see the rest",
            self.max_paths
        );
    }
    /// A re-sort is waiting for `refresh_every` to pass, or the rows are
    /// catching up with what was found.
    pub fn pending(&self) -> bool {
        self.pending || self.done.is_some()
    }
}
/// Session discovery controller: recent exhaustive snapshots avoid redundant scans.
pub struct Discovery {
    scan: Option<Scan>,
    cache: crate::cache::Cache,
    options: crate::config::CacheConfig,
    key: Option<crate::cache::Key>,
    stored: bool,
    /// A scan started and has not been drained yet: the next drain waits
    /// up to `FIRST_FRAME` for it to finish.
    started: bool,
    verified_at: Option<Instant>,
    refresh_at: Option<Instant>,
    wake: Option<SyncSender<()>>,
    prefetch: crate::prefetch::Prefetch,
    /// `[performance] refresh_ms` for every scan this controller starts.
    pub refresh_every: Duration,
    /// `[performance] max_paths` for every scan this controller starts.
    pub max_paths: usize,
    input: Option<Arc<AtomicUsize>>,
}
impl Discovery {
    pub fn new(options: crate::config::CacheConfig) -> Self {
        Self {
            scan: None,
            cache: crate::cache::Cache::new(options.clone()),
            options,
            key: None,
            stored: false,
            started: false,
            verified_at: None,
            refresh_at: None,
            wake: None,
            prefetch: crate::prefetch::Prefetch::default(),
            refresh_every: Duration::ZERO,
            max_paths: usize::MAX,
            input: None,
        }
    }
    /// New `[cache]` settings: the snapshots kept so far are dropped.
    pub fn configure(&mut self, options: crate::config::CacheConfig) {
        if options != self.options {
            self.cache = crate::cache::Cache::new(options.clone());
            self.options = options;
        }
    }
    /// Input waiting to be read, which scans yield to.
    pub fn yield_to(&mut self, waiting: Arc<AtomicUsize>) {
        self.input = Some(waiting);
    }
    /// Coalesced notifications shared with terminal input. Never blocks a worker.
    pub fn set_waker(&mut self, wake: SyncSender<()>) {
        self.wake = Some(wake);
    }
    pub fn start(&mut self, app: &mut App, clear: bool) {
        self.restart(app, clear, false);
    }
    pub fn force_refresh(&mut self, app: &mut App) {
        self.restart(app, true, true);
    }
    fn interval(&self, app: &App) -> Duration {
        // Recursive walks can be expensive; only depth-1 views refresh frequently.
        Duration::from_secs(if app.depth == 1 && self.options.fresh_seconds > 0 {
            self.options.fresh_seconds
        } else {
            self.options.ttl_seconds
        })
    }
    fn restart(&mut self, app: &mut App, clear: bool, force: bool) {
        self.scan.take(); // Disconnect a cancelled sender before its successor.
        let now = Instant::now();
        let key = crate::cache::Key::of(app);
        if force {
            self.cache.invalidate(&key);
            self.prefetch.invalidate(&key);
        }
        let snapshot = if force {
            None
        } else {
            self.cache
                .get(&key, now)
                .or_else(|| self.prefetch.take(&key, now))
        };
        self.prefetch.cancel();
        // Reuse known paths, even on a first child visit. Attribution is recomputed
        // by App::insert; these are only a provisional prefix. Bound work on the UI.
        let reused: Vec<_> = if !force && snapshot.is_none() && self.options.enabled {
            app.rows
                .iter()
                .chain(app.known.values())
                .take(16_384)
                .filter(|c| {
                    app.scope.iter().any(|root| {
                        c.path.strip_prefix(root).is_ok_and(|p| {
                            let depth = p.components().count();
                            depth > 0 && depth <= app.depth && app.policy.allows(p)
                        })
                    })
                })
                .take(self.options.max_candidates)
                .map(|c| Candidate::clone(c))
                .collect()
        } else {
            Vec::new()
        };
        let fresh = snapshot.as_ref().is_some_and(|s| {
            s.complete
                && s.exhaustive
                && !s.physical_empty
                && !s.stale
                && self.options.fresh_seconds > 0
                && now.saturating_duration_since(s.verified_at)
                    < Duration::from_secs(self.options.fresh_seconds.min(self.options.ttl_seconds))
        });
        if fresh {
            Scan::reset(app, clear);
            app.scanning = false;
        } else {
            let mut scan = Scan::start_waking(app, clear, self.wake.clone());
            scan.refresh_every = self.refresh_every;
            scan.max_paths = self.max_paths;
            scan.input = self.input.clone();
            self.scan = Some(scan);
        }
        self.verified_at = None;
        self.refresh_at = None;
        self.started = !fresh;
        if let Some(snapshot) = snapshot {
            if fresh {
                self.verified_at = Some(snapshot.verified_at);
                self.refresh_at = Some(snapshot.verified_at + self.interval(app));
            }
            for candidate in snapshot.candidates.iter() {
                app.replay(candidate.clone());
            }
            app.cache_pending = true; // Returned paths/kinds still need validation.
        } else if !reused.is_empty() {
            for candidate in reused {
                app.replay(candidate);
            }
            app.cache_pending = !app.known.is_empty();
        }
        app.refresh();
        self.key = Some(key);
        self.stored = fresh;
    }
    /// The current directory changed on disk: reconcile with a quiet rescan
    /// that keeps the rows, and stop replaying the outdated snapshot.
    pub fn changed(&mut self, app: &App) {
        let key = crate::cache::Key::of(app);
        self.cache.invalidate(&key);
        self.prefetch.invalidate(&key);
        // A checkpoint before the rescan must not store the old rows as verified.
        self.verified_at = None;
        self.refresh_at = Some(Instant::now());
    }
    /// Preserve the verification age: navigation/query changes do not renew it.
    pub fn checkpoint(&mut self, app: &App) {
        // Rows being ranked again may be those of other settings.
        if let Some(key) = &self.key
            && !app.ranking_pending()
            && key == &crate::cache::Key::of(app)
            && self.scan.as_ref().is_none_or(|s| !s.failed)
        {
            let complete = self.verified_at.is_some();
            self.cache.capture_visit(
                key.clone(),
                app,
                self.verified_at.unwrap_or_else(Instant::now),
                complete,
            );
        }
    }
    pub fn drain(&mut self, app: &mut App) -> bool {
        self.drain_at(app, Instant::now())
    }
    pub fn drain_at(&mut self, app: &mut App, now: Instant) -> bool {
        if self.refresh_at.is_some_and(|at| now >= at) {
            self.refresh_at = None;
            self.prefetch.cancel();
            // Keep all current rows while a quiet background refresh reconciles.
            let status = std::mem::take(&mut app.status);
            let mut scan = Scan::start_waking(app, false, self.wake.clone());
            app.status = status;
            scan.refresh_every = self.refresh_every;
            scan.max_paths = self.max_paths;
            scan.input = self.input.clone();
            app.cache_pending = true;
            app.quiet_scan = true;
            self.scan = Some(scan);
            self.stored = false;
            self.verified_at = None;
            self.started = true;
        }
        let Some(scan) = self.scan.as_mut() else {
            return false;
        };
        let settle = if std::mem::take(&mut self.started) {
            FIRST_FRAME
        } else {
            Duration::ZERO
        };
        let changed = scan.drain_for(app, settle);
        if scan.completed && !self.stored {
            let key = self.key.as_ref().unwrap();
            if key == &crate::cache::Key::of(app) {
                if scan.failed {
                    self.cache.invalidate(key);
                } else {
                    self.cache.capture(key.clone(), app, now);
                    self.verified_at = Some(now);
                }
                self.refresh_at = Some(now + self.interval(app));
            }
            self.stored = true;
        }
        changed
    }
    /// Schedule only stable results highlights, never chooser rows.
    pub fn idle(&mut self, app: &App, results_focus: bool, now: Instant) {
        let eligible = results_focus
            && self.options.enabled
            && self.options.prefetch
            && app.depth == 1
            && app.program().navigates()
            && !app.scanning;
        // Do not clone roots/rules on every idle tick in recursive views or
        // choosers, where no speculative work can be admitted.
        let already_cached = eligible
            && app.highlight.as_ref().is_some_and(|path| {
                self.cache
                    .recently_verified(&crate::cache::Key::for_directory(app, path.clone()), now)
            });
        self.prefetch
            .tick(app, eligible && !already_cached, now, self.wake.clone());
    }
    /// A scan started and was not drained yet: its first drain, which waits
    /// up to `FIRST_FRAME` for it, belongs before the next frame.
    pub fn just_started(&self) -> bool {
        self.started
    }
    pub fn busy(&self, app: &App) -> bool {
        app.scanning || self.scan.as_ref().is_some_and(Scan::pending)
    }
    /// The rows are behind what was found and the next drain merges more,
    /// whatever `refresh_ms` says. Not while a ranking runs elsewhere: the
    /// drain could not merge, and its thread wakes the loop when done.
    pub fn catching_up(&self, app: &App) -> bool {
        !app.settled()
            && !app.ranking_pending()
            && self
                .scan
                .as_ref()
                .is_some_and(|s| s.done.is_some() || s.completed || app.unmerged() >= BACKLOG)
    }
    pub fn stats(&self) -> crate::cache::Stats {
        self.cache.stats
    }
}
struct EventSender {
    events: SyncSender<Event>,
    wake: Option<SyncSender<()>>,
}
impl EventSender {
    fn send(&self, event: Event) -> Result<(), mpsc::SendError<Event>> {
        self.events.send(event)?;
        if let Some(wake) = &self.wake {
            let _ = wake.try_send(());
        }
        Ok(())
    }
}

fn walk(
    roots: Vec<PathBuf>,
    current: PathBuf,
    max: usize,
    policy: Policy,
    generation: u64,
    tx: EventSender,
    cancel: Arc<AtomicBool>,
) {
    let empty = fs::read_dir(&current)
        .ok()
        .is_some_and(|mut r| r.next().is_none());
    let mut batch = Vec::with_capacity(BATCH_SIZE);
    let mut since = Instant::now();
    // Hand the batch over; false when the receiver is gone (cancelled).
    let flush = |batch: &mut Vec<Candidate>, since: &mut Instant| {
        *since = Instant::now();
        batch.is_empty()
            || tx
                .send(Event::Batch(generation, std::mem::take(batch)))
                .is_ok()
    };
    let mut queue: VecDeque<_> = roots
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i, 1))
        .collect();
    while let Some((directory, root, depth)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        // Do not hold results back while a slow directory is opened.
        if since.elapsed() >= BATCH_AGE && !flush(&mut batch, &mut since) {
            return;
        }
        let entries = match fs::read_dir(&directory) {
            Ok(e) => e,
            Err(e) => {
                if tx
                    .send(Event::Error(
                        generation,
                        format!("{}: {e}", directory.display()),
                    ))
                    .is_err()
                {
                    return;
                }
                continue;
            }
        };
        for entry in entries {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    let _ = tx.send(Event::Error(generation, e.to_string()));
                    continue;
                }
            };
            let path = entry.path();
            if !policy.allows(path.strip_prefix(&roots[root]).unwrap()) {
                continue;
            }
            let kind = match entry.file_type() {
                Ok(t) => t,
                Err(e) => {
                    let _ = tx.send(Event::Error(generation, e.to_string()));
                    continue;
                }
            };
            // Symlink directories are selectable, but never followed during traversal.
            let directory = kind.is_dir() || (kind.is_symlink() && path.is_dir());
            if kind.is_dir() && depth < max {
                queue.push_back((path.clone(), root, depth + 1));
            }
            batch.push(Candidate {
                path,
                directory,
                depth,
                root,
            });
            if (batch.len() >= BATCH_SIZE || since.elapsed() >= BATCH_AGE)
                && !flush(&mut batch, &mut since)
            {
                return;
            }
        }
    }
    if flush(&mut batch, &mut since) {
        let _ = tx.send(Event::Done(generation, empty));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn late_events_neither_restore_deeper_rows_nor_complete_a_new_generation() {
        let mut app = App::browse("/fixture".into(), 1, Policy::default());
        app.generation = 2;
        let (tx, events) = mpsc::sync_channel(8);
        let scan = Scan {
            events,
            cancel: Arc::new(AtomicBool::new(false)),
            failed: false,
            completed: false,
            done: None,
            max_paths: usize::MAX,
            found: 0,
            input: None,
            refresh_every: Duration::ZERO,
            last_refresh: None,
            pending: false,
        };
        let mut discovery = Discovery::new(crate::config::CacheConfig::default());
        discovery.scan = Some(scan);
        discovery.key = Some(crate::cache::Key::of(&app));
        tx.send(Event::Found(
            1,
            Candidate {
                path: "/fixture/obsolete".into(),
                directory: false,
                depth: 1,
                root: 0,
            },
        ))
        .unwrap();
        tx.send(Event::Done(1, true)).unwrap();
        discovery.drain(&mut app);
        assert!(app.known.is_empty());
        assert!(app.scanning);
        assert_eq!(discovery.stats().stores, 0);
        tx.send(Event::Found(
            2,
            Candidate {
                path: "/fixture/deep/file".into(),
                directory: false,
                depth: 2,
                root: 0,
            },
        ))
        .unwrap();
        tx.send(Event::Done(2, false)).unwrap();
        discovery.drain(&mut app);
        assert!(app.rows.is_empty());
        assert!(!app.scanning);
        assert_eq!(discovery.stats().stores, 1);
    }
    #[test]
    fn checkpointing_preserves_the_original_verification_deadline() {
        let mut app = App::browse("/fixture".into(), 1, Policy::default());
        app.insert(
            0,
            Candidate {
                path: "/fixture/file".into(),
                directory: false,
                depth: 1,
                root: 0,
            },
        );
        app.refresh();
        app.scanning = false;
        let mut d = Discovery::new(crate::config::CacheConfig::default());
        let key = crate::cache::Key::of(&app);
        let verified = Instant::now() - Duration::from_secs(2);
        d.key = Some(key.clone());
        d.verified_at = Some(verified);
        for _ in 0..3 {
            app.query.push('f');
            app.refresh();
            d.checkpoint(&app);
            assert_eq!(
                d.cache.get(&key, Instant::now()).unwrap().verified_at,
                verified
            );
            d.start(&mut app, true);
            assert_eq!(d.refresh_at, Some(verified + Duration::from_secs(3)));
            assert!(!app.scanning);
        }
    }
    #[test]
    fn full_wakeup_slot_coalesces_without_blocking_discovery_events() {
        let (wake, waking) = mpsc::sync_channel(1);
        let (events, incoming) = mpsc::sync_channel(2);
        let sender = EventSender {
            events,
            wake: Some(wake),
        };
        sender.send(Event::Done(1, false)).unwrap();
        sender.send(Event::Done(2, false)).unwrap();
        assert!(waking.try_recv().is_ok());
        assert!(waking.try_recv().is_err());
        assert!(matches!(
            incoming.try_recv().unwrap(),
            Event::Done(1, false)
        ));
        assert!(matches!(
            incoming.try_recv().unwrap(),
            Event::Done(2, false)
        ));
    }
}
