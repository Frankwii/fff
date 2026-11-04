//! Folders' sizes: the bytes of every file below a folder, on its own
//! filesystem, added up by background workers, one per mount, so that a
//! stuck mount holds up only its own folders. Drawing never reads the
//! filesystem: it names the folders it wants ([`Sizes::want`], committed
//! once a frame is drawn) and shows what the workers found. Workers take
//! the folders on screen first, then those a page away, and give up on a
//! folder once it is neither; the totals of the folders below it they
//! finished are kept, so adding it up again, or opening one of them, goes
//! faster.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, Instant},
};

/// A total stays current this long, unless something below it changes in
/// a watched folder or F5 rescans.
const FRESH: Duration = Duration::from_secs(600);
/// How often a growing total is shown.
const PROGRESS: Duration = Duration::from_millis(150);
/// Totals kept before those of folders not wanted are dropped.
const KEPT: usize = 200_000;

/// What the size column shows for a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub bytes: u64,
    /// Still being added up: `bytes` so far.
    pub counting: bool,
    /// Some folders below could not be read: at least `bytes`.
    pub partial: bool,
    /// The folder itself could not be read.
    pub unreadable: bool,
}

struct Total {
    bytes: u64,
    partial: bool,
    unreadable: bool,
    at: Instant,
    /// False once something below it changed: added up again when wanted,
    /// shown meanwhile.
    current: bool,
}
impl Total {
    fn fresh(&self) -> bool {
        self.current && self.at.elapsed() < FRESH
    }
}

#[derive(Default)]
struct State {
    /// Folders the latest frame wanted, in order, with their mounts.
    wanted: Vec<(PathBuf, PathBuf)>,
    /// Bumped as `wanted` changes, so that workers look again.
    version: u64,
    totals: HashMap<PathBuf, Total>,
    /// Growing totals of the folders being added up.
    running: HashMap<PathBuf, u64>,
    ended: bool,
}
impl State {
    /// The first folder of `mount` wanted whose total is not fresh.
    fn next(&self, mount: &Path) -> Option<PathBuf> {
        self.wanted
            .iter()
            .filter(|(_, m)| m == mount)
            .map(|(p, _)| p)
            .find(|p| !self.totals.get(*p).is_some_and(Total::fresh))
            .cloned()
    }
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    changed: AtomicBool,
}

#[derive(Default)]
pub struct Sizes {
    shared: Arc<Shared>,
    /// Mounts with a worker.
    workers: RefCell<HashSet<PathBuf>>,
    /// Folders the frame being drawn wants, with their mounts.
    frame: RefCell<Vec<(PathBuf, PathBuf)>>,
    wake: Option<SyncSender<()>>,
}
impl Sizes {
    pub fn set_waker(&mut self, wake: SyncSender<()>) {
        self.wake = Some(wake);
    }
    /// The frame being drawn wants `path`'s size, on `mount`.
    pub fn want(&self, path: &Path, mount: PathBuf) {
        self.frame.borrow_mut().push((path.to_path_buf(), mount));
    }
    /// What the workers found for `path`: its total, kept while it is added
    /// up again, else its growing total; `None` before either.
    pub fn get(&self, path: &Path) -> Option<Size> {
        let state = self.shared.state.lock().unwrap();
        if let Some(total) = state.totals.get(path) {
            return Some(Size {
                bytes: total.bytes,
                counting: false,
                partial: total.partial,
                unreadable: total.unreadable,
            });
        }
        state.running.get(path).map(|&bytes| Size {
            bytes,
            counting: true,
            partial: false,
            unreadable: false,
        })
    }
    /// The frame is drawn: what it wanted, in order, is what workers do.
    pub fn commit(&self) {
        let mut frame = std::mem::take(&mut *self.frame.borrow_mut());
        let mut seen = HashSet::new();
        frame.retain(|(path, _)| seen.insert(path.clone()));
        let mut state = self.shared.state.lock().unwrap();
        if state.wanted == frame {
            return;
        }
        let mut workers = self.workers.borrow_mut();
        for (_, mount) in &frame {
            if workers.insert(mount.clone()) {
                self.spawn(mount.clone());
            }
        }
        state.wanted = frame;
        state.version += 1;
        self.shared.ready.notify_all();
    }
    /// Whether totals arrived or grew since the last call.
    pub fn drain(&self) -> bool {
        self.shared.changed.swap(false, Ordering::Relaxed)
    }
    /// `path` changed on disk: the totals of the folders above it are added
    /// up again when wanted.
    pub fn touched(&self, path: &Path) {
        let mut state = self.shared.state.lock().unwrap();
        for folder in path.ancestors() {
            if let Some(total) = state.totals.get_mut(folder) {
                total.current = false;
            }
        }
        self.shared.ready.notify_all();
    }
    /// F5: every total is added up again when wanted.
    pub fn refresh(&self) {
        let mut state = self.shared.state.lock().unwrap();
        for total in state.totals.values_mut() {
            total.current = false;
        }
        self.shared.ready.notify_all();
    }
    fn spawn(&self, mount: PathBuf) {
        let shared = self.shared.clone();
        let wake = self.wake.clone();
        // Deep trees recurse deeply.
        let _ = std::thread::Builder::new()
            .stack_size(16 << 20)
            .spawn(move || work(&shared, &mount, wake.as_ref()));
    }
}
impl Drop for Sizes {
    fn drop(&mut self) {
        self.shared.state.lock().unwrap().ended = true;
        self.shared.ready.notify_all();
    }
}

/// A mount's worker: adds up the folders wanted on it, first first.
fn work(shared: &Shared, mount: &Path, wake: Option<&SyncSender<()>>) {
    loop {
        let (root, version) = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if state.ended {
                    return;
                }
                if let Some(root) = state.next(mount) {
                    break (root, state.version);
                }
                state = shared.ready.wait(state).unwrap();
            }
        };
        let mut walk = Walk {
            shared,
            wake,
            mount,
            root: &root,
            version,
            dev: 0,
            seen: HashSet::new(),
            running: 0,
            shown_at: Instant::now(),
        };
        let done = match fs::metadata(&root) {
            Ok(m) => {
                walk.dev = m.dev();
                walk.total(&root).is_some()
            }
            Err(_) => {
                walk.store(&root, 0, false, true);
                true
            }
        };
        shared.state.lock().unwrap().running.remove(&root);
        if done {
            shared.changed.store(true, Ordering::Relaxed);
            if let Some(wake) = wake {
                let _ = wake.try_send(());
            }
        }
    }
}

struct Walk<'a> {
    shared: &'a Shared,
    wake: Option<&'a SyncSender<()>>,
    mount: &'a Path,
    root: &'a Path,
    /// The `wanted` version last checked.
    version: u64,
    /// The root's filesystem: folders on another are not added.
    dev: u64,
    /// Files with several links, added once.
    seen: HashSet<(u64, u64)>,
    /// The root's total so far.
    running: u64,
    shown_at: Instant,
}
impl Walk<'_> {
    /// `dir`'s bytes and whether some of it could not be read, stored for
    /// later walks; `None` when the root is no longer wanted, or another
    /// folder before it is.
    fn total(&mut self, dir: &Path) -> Option<(u64, bool)> {
        if self.abandoned() {
            return None;
        }
        let Ok(entries) = fs::read_dir(dir) else {
            self.store(dir, 0, false, true);
            return Some((0, true));
        };
        let (mut bytes, mut partial) = (0, false);
        for entry in entries {
            let Ok(meta) = entry.and_then(|e| Ok((e.path(), e.metadata()?))) else {
                partial = true;
                continue;
            };
            let (path, meta) = meta;
            if meta.is_dir() {
                if meta.dev() != self.dev {
                    continue;
                }
                let kept = {
                    let state = self.shared.state.lock().unwrap();
                    state
                        .totals
                        .get(&path)
                        .filter(|t| t.fresh())
                        .map(|t| (t.bytes, t.partial || t.unreadable))
                };
                let (more, unread) = match kept {
                    Some(kept) => {
                        self.running += kept.0;
                        kept
                    }
                    None => self.total(&path)?,
                };
                bytes += more;
                partial |= unread;
            } else {
                if meta.nlink() > 1 && !self.seen.insert((meta.dev(), meta.ino())) {
                    continue;
                }
                bytes += meta.len();
                self.running += meta.len();
            }
            self.progress();
        }
        self.store(dir, bytes, partial, false);
        Some((bytes, partial))
    }
    fn store(&self, dir: &Path, bytes: u64, partial: bool, unreadable: bool) {
        let mut state = self.shared.state.lock().unwrap();
        if state.totals.len() >= KEPT {
            let wanted: HashSet<_> = state.wanted.iter().map(|(p, _)| p.clone()).collect();
            state.totals.retain(|p, _| wanted.contains(p));
        }
        state.totals.insert(
            dir.to_path_buf(),
            Total {
                bytes,
                partial,
                unreadable,
                at: Instant::now(),
                current: true,
            },
        );
    }
    /// Shows the root's growing total now and then.
    fn progress(&mut self) {
        if self.shown_at.elapsed() < PROGRESS {
            return;
        }
        self.shown_at = Instant::now();
        let mut state = self.shared.state.lock().unwrap();
        // A kept total shows until the new one is done.
        if state.totals.contains_key(self.root) {
            return;
        }
        state.running.insert(self.root.to_path_buf(), self.running);
        drop(state);
        self.shared.changed.store(true, Ordering::Relaxed);
        if let Some(wake) = self.wake {
            let _ = wake.try_send(());
        }
    }
    /// Whether to give up on the root: the session ends, or since the
    /// frames changed, it is no longer the first folder of its mount to do.
    fn abandoned(&mut self) -> bool {
        let state = self.shared.state.lock().unwrap();
        if state.ended {
            return true;
        }
        if state.version == self.version {
            return false;
        }
        self.version = state.version;
        state.next(self.mount).as_deref() != Some(self.root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fff-sizes-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("a/deep/er")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::create_dir_all(root.join("empty")).unwrap();
        fs::write(root.join("a/one"), vec![0u8; 1000]).unwrap();
        fs::write(root.join("a/deep/er/two"), vec![0u8; 24]).unwrap();
        fs::hard_link(root.join("a/one"), root.join("a/deep/same")).unwrap();
        fs::write(root.join("b/three"), vec![0u8; 7]).unwrap();
        root
    }
    /// Commits `wanted` as a frame would, then waits for each total.
    fn sizes_of(sizes: &Sizes, wanted: &[PathBuf]) -> Vec<Option<Size>> {
        for path in wanted {
            sizes.want(path, PathBuf::from("/"));
        }
        sizes.commit();
        let deadline = Instant::now() + Duration::from_secs(5);
        let done = |p: &PathBuf| sizes.get(p).is_some_and(|s| !s.counting);
        while !wanted.iter().all(done) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        wanted.iter().map(|p| sizes.get(p)).collect()
    }
    fn bytes(size: Option<Size>) -> Option<u64> {
        size.map(|s| s.bytes)
    }
    #[test]
    fn folders_add_up_the_files_below_them_once_each() {
        let root = fixture("total");
        let sizes = Sizes::default();
        let found = sizes_of(&sizes, &[root.join("a"), root.join("empty"), root.clone()]);
        assert_eq!(
            found.into_iter().map(bytes).collect::<Vec<_>>(),
            [Some(1024), Some(0), Some(1031)]
        );
        // The folders below were added up on the way and are kept.
        assert_eq!(bytes(sizes.get(&root.join("a/deep"))), Some(24));
        // A folder that cannot be read says so.
        let missing = sizes_of(&sizes, &[root.join("gone")]);
        assert!(missing[0].unwrap().unreadable);
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn only_wanted_folders_are_added_up() {
        let root = fixture("wanted");
        let sizes = Sizes::default();
        sizes_of(&sizes, &[root.join("a")]);
        assert_eq!(sizes.get(&root.join("b")), None);
        // A frame without the folder: the worker does nothing more.
        sizes.commit();
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(sizes.get(&root.join("b")), None);
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn a_change_below_a_folder_adds_it_up_again_showing_the_old_total_meanwhile() {
        let root = fixture("touched");
        let sizes = Sizes::default();
        assert_eq!(bytes(sizes_of(&sizes, std::slice::from_ref(&root))[0]), Some(1031));
        let added = root.join("a/deep/er/four");
        fs::write(&added, vec![0u8; 9]).unwrap();
        // Unchanged until something says the folder changed.
        assert_eq!(bytes(sizes_of(&sizes, std::slice::from_ref(&root))[0]), Some(1031));
        sizes.touched(&added);
        assert_eq!(bytes(sizes.get(&root)), Some(1031));
        let deadline = Instant::now() + Duration::from_secs(5);
        while bytes(sizes.get(&root)) != Some(1040) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(bytes(sizes.get(&root)), Some(1040));
        // F5 adds every folder up again.
        fs::write(root.join("b/five"), vec![0u8; 60]).unwrap();
        sizes.refresh();
        let deadline = Instant::now() + Duration::from_secs(5);
        while bytes(sizes.get(&root)) != Some(1100) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        assert_eq!(bytes(sizes.get(&root)), Some(1100));
        fs::remove_dir_all(&root).unwrap();
    }
}
