//! Bounded background traversal. Replacing a scan cancels its predecessor.
use crate::{App, Candidate, Policy};
use std::{
    collections::{HashSet, VecDeque},
    fs,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
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
    seen: HashSet<PathBuf>,
    failed: bool,
    completed: bool,
    /// Minimum time between re-sorts while results stream in (zero: every drain).
    pub refresh_every: Duration,
    last_refresh: Option<Instant>,
    /// Known paths changed since the last re-sort.
    pending: bool,
}
/// Most events handled per drain before the frame is drawn again.
const DRAIN_BUDGET: Duration = Duration::from_millis(8);
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
        app.generation += 1;
        app.scanning = true;
        app.physical_empty = false;
        app.cache_pending = false;
        app.status.clear();
        if clear {
            app.known.clear();
            app.known_version += 1;
        }
        app.refresh();
        // Bounded in batches: at most 16 × BATCH_SIZE candidates wait in flight.
        let (tx, events) = mpsc::sync_channel(16);
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
            seen: HashSet::new(),
            failed: false,
            completed: false,
            refresh_every: Duration::ZERO,
            last_refresh: None,
            pending: false,
        }
    }
    /// Apply waiting events for up to `DRAIN_BUDGET`, then re-sort if due: the
    /// first results, completion, and otherwise at most every `refresh_every`.
    /// Returns whether anything visible may have changed.
    pub fn drain(&mut self, app: &mut App) -> bool {
        let start = Instant::now();
        let mut changed = false;
        let mut finished = false;
        while start.elapsed() < DRAIN_BUDGET {
            let Ok(event) = self.events.try_recv() else {
                break;
            };
            changed = true;
            match event {
                Event::Found(g, c) if g == app.generation => {
                    self.seen.insert(c.path.clone());
                    app.insert(g, c);
                    self.pending = true;
                }
                Event::Batch(g, batch) if g == app.generation => {
                    for c in batch {
                        self.seen.insert(c.path.clone());
                        app.insert(g, c);
                    }
                    self.pending = true;
                }
                Event::Error(g, s) if g == app.generation => {
                    self.failed = true;
                    app.status = s;
                }
                Event::Done(g, empty) if g == app.generation => {
                    app.scanning = false;
                    app.cache_pending = false;
                    app.physical_empty = empty;
                    app.known.retain(|path, _| self.seen.contains(path));
                    app.known_version += 1;
                    // Only needed to reconcile; release it for the idle session.
                    self.seen = HashSet::new();
                    self.completed = true;
                    self.pending = true;
                    finished = true;
                }
                _ => {}
            }
        }
        let due = self
            .last_refresh
            .is_none_or(|t| t.elapsed() >= self.refresh_every);
        if self.pending && (finished || due) {
            app.refresh();
            self.pending = false;
            self.last_refresh = Some(Instant::now());
            changed = true;
        }
        changed
    }
    /// A re-sort is waiting for `refresh_every` to pass.
    pub fn pending(&self) -> bool {
        self.pending
    }
}
/// Session discovery controller: restore a snapshot, then always revalidate it.
pub struct Discovery {
    scan: Option<Scan>,
    cache: crate::cache::Cache,
    key: Option<crate::cache::Key>,
    stored: bool,
    defer_drain: bool,
    /// `[performance] refresh_ms` for every scan this controller starts.
    pub refresh_every: Duration,
}
impl Discovery {
    pub fn new(options: crate::config::CacheConfig) -> Self {
        Self {
            scan: None,
            cache: crate::cache::Cache::new(options),
            key: None,
            stored: false,
            defer_drain: false,
            refresh_every: Duration::ZERO,
        }
    }
    pub fn start(&mut self, app: &mut App, clear: bool) {
        self.restart(app, clear, false);
    }
    pub fn force_refresh(&mut self, app: &mut App) {
        self.restart(app, true, true);
    }
    fn restart(&mut self, app: &mut App, clear: bool, force: bool) {
        // Drop the receiver before starting a successor, unblocking a cancelled sender.
        self.scan.take();
        let key = crate::cache::Key::of(app);
        if force {
            self.cache.invalidate(&key);
        }
        let snapshot = if force {
            None
        } else {
            self.cache.get(&key, Instant::now())
        };
        let mut scan = Scan::start(app, clear);
        scan.refresh_every = self.refresh_every;
        self.defer_drain = snapshot.is_some();
        if let Some(snapshot) = snapshot {
            for candidate in snapshot.candidates.iter() {
                app.insert(app.generation, candidate.clone());
            }
            app.cache_pending = true;
            app.refresh();
        }
        self.scan = Some(scan);
        self.key = Some(key);
        self.stored = false;
    }
    /// Save current visible matches before a navigation changes the scope.
    pub fn checkpoint(&mut self, app: &App) {
        if let (Some(scan), Some(key)) = (&self.scan, &self.key)
            && !scan.failed
            && key == &crate::cache::Key::of(app)
        {
            self.cache
                .capture_visit(key.clone(), app, Instant::now(), scan.completed);
        }
    }
    /// Returns whether anything visible may have changed.
    pub fn drain(&mut self, app: &mut App) -> bool {
        // Give a warm visit its own completed frame before any incoming batch
        // can trigger a potentially expensive full-index sort.
        if self.defer_drain {
            self.defer_drain = false;
            return true;
        }
        let Some(scan) = self.scan.as_mut() else {
            return false;
        };
        let changed = scan.drain(app);
        if scan.completed && !self.stored {
            let key = self.key.as_ref().unwrap();
            if key == &crate::cache::Key::of(app) {
                if scan.failed {
                    self.cache.invalidate(key);
                } else {
                    self.cache.capture(key.clone(), app, Instant::now());
                }
            }
            self.stored = true;
        }
        changed
    }
    /// Discovery may still change the rows: a scan runs or a re-sort is due.
    pub fn busy(&self, app: &App) -> bool {
        app.scanning || self.scan.as_ref().is_some_and(Scan::pending)
    }
    pub fn stats(&self) -> crate::cache::Stats {
        self.cache.stats
    }
}

fn walk(
    roots: Vec<PathBuf>,
    current: PathBuf,
    max: usize,
    policy: Policy,
    generation: u64,
    tx: SyncSender<Event>,
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
            seen: HashSet::new(),
            failed: false,
            completed: false,
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
}
