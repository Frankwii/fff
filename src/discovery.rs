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
    time::Instant,
};

pub enum Event {
    Found(u64, Candidate),
    Error(u64, String),
    Done(u64, bool),
}

pub struct Scan {
    pub events: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    seen: HashSet<PathBuf>,
    failed: bool,
    completed: bool,
}
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
        }
        app.refresh();
        let (tx, events) = mpsc::sync_channel(256);
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
        }
    }
    pub fn drain(&mut self, app: &mut App) {
        let mut changed = false;
        for event in self.events.try_iter().take(512) {
            match event {
                Event::Found(g, c) if g == app.generation => {
                    self.seen.insert(c.path.clone());
                    app.insert(g, c);
                    changed = true;
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
                    self.completed = true;
                    changed = true;
                }
                _ => {}
            }
        }
        if changed {
            app.refresh();
        }
    }
}
/// Session discovery controller: restore a snapshot, then always revalidate it.
pub struct Discovery {
    scan: Option<Scan>,
    cache: crate::cache::Cache,
    key: Option<crate::cache::Key>,
    stored: bool,
    defer_drain: bool,
}
impl Discovery {
    pub fn new(options: crate::config::CacheConfig) -> Self {
        Self {
            scan: None,
            cache: crate::cache::Cache::new(options),
            key: None,
            stored: false,
            defer_drain: false,
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
        let scan = Scan::start(app, clear);
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
    pub fn drain(&mut self, app: &mut App) {
        // Give a warm visit its own completed frame before any incoming batch
        // can trigger a potentially expensive full-index sort.
        if self.defer_drain {
            self.defer_drain = false;
            return;
        }
        let Some(scan) = self.scan.as_mut() else {
            return;
        };
        scan.drain(app);
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
    let mut queue: VecDeque<_> = roots
        .iter()
        .enumerate()
        .map(|(i, p)| (p.clone(), i, 1))
        .collect();
    while let Some((directory, root, depth)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
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
            if tx
                .send(Event::Found(
                    generation,
                    Candidate {
                        path,
                        directory,
                        depth,
                        root,
                    },
                ))
                .is_err()
            {
                return;
            }
        }
    }
    let _ = tx.send(Event::Done(generation, empty));
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
