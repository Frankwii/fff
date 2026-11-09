//! Folders' sizes: the bytes of every file below a folder, on its own
//! filesystem, added up by background workers, one per mount, so that a
//! stuck mount holds up only its own folders. Drawing never reads the
//! filesystem: it names the folders it wants ([`Sizes::want`], committed
//! once a frame is drawn) and shows what the workers found. Workers take
//! the folders on screen first, then those a page away, and give up on a
//! folder once it is neither.
//!
//! What the workers find is kept as a tree of folders, each with the bytes
//! of its own files and the identity of the folder when it was read (its
//! inode, modification and change times). Adding a folder up again reads
//! only the folders that changed: one whose times are the same keeps its
//! files' bytes, and only its subfolders are looked at, a `stat` each. A
//! file that grows in place does not change its folder's times, so it is
//! caught by a change in a watched folder ([`Sizes::touched`]) or F5
//! ([`Sizes::refresh`]), which reads every folder again. The tree is saved
//! when the session ends ([`Sizes::keep`]) and read when the next one draws
//! its first frame, so sizes show at once and are checked in the background.
//! Each walk reads its folders on several threads.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::SyncSender,
    },
    time::{Duration, Instant},
};

/// A total stays current this long, unless something below it changes in
/// a watched folder or F5 rescans; then its folders are checked again.
const FRESH: Duration = Duration::from_secs(600);
/// How often a growing total is shown.
const PROGRESS: Duration = Duration::from_millis(150);
/// Folders kept; past it, the tree starts again from the folders wanted.
const KEPT: usize = 1_000_000;
/// Threads reading one walk's folders.
const READERS: usize = 8;
/// The saved tree's first bytes.
const HEADER: &[u8] = b"fff-sizes 1\0";
/// No parent: the tree's root, `/`.
const NONE: u32 = u32::MAX;

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

/// A folder as it was when read: if any of these changed, so may have its
/// entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    mtime: i64,
    ctime: i64,
    mtime_nsec: u32,
    ctime_nsec: u32,
}
impl Stamp {
    fn of(meta: &fs::Metadata) -> Self {
        Self {
            dev: meta.dev(),
            ino: meta.ino(),
            mtime: meta.mtime(),
            ctime: meta.ctime(),
            mtime_nsec: meta.mtime_nsec() as u32,
            ctime_nsec: meta.ctime_nsec() as u32,
        }
    }
}

/// A file with several links: counted once below any folder.
type Link = (u64, u64, u64);

/// A subtree's bytes as far as they are known.
#[derive(Clone, Copy, Debug)]
struct Sum {
    /// Files with one link.
    plain: u64,
    /// Files with several links are below, to be counted once each.
    links: bool,
    /// Some folder below could not be read, or some entry.
    partial: bool,
}

/// Bytes and two flags, or nothing, in one word: a saved tree holds
/// hundreds of thousands of folders, each with three of these. Sizes stay
/// far below the 2^61 bytes there is room for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Kept(u64);
impl Kept {
    const SET: u64 = 1 << 63;
    const FLAGS: [u64; 2] = [1 << 62, 1 << 61];
    const MOST: u64 = (1 << 61) - 1;
    fn of(bytes: u64, flags: [bool; 2]) -> Self {
        let flags = (u64::from(flags[0]) * Self::FLAGS[0]) | (u64::from(flags[1]) * Self::FLAGS[1]);
        Self(Self::SET | flags | bytes.min(Self::MOST))
    }
    fn get(self) -> Option<(u64, [bool; 2])> {
        (self.0 & Self::SET != 0).then(|| {
            let flags = Self::FLAGS.map(|flag| self.0 & flag != 0);
            (self.0 & Self::MOST, flags)
        })
    }
    fn is_some(self) -> bool {
        self.0 != 0
    }
    /// As a total: the bytes and whether some could not be read.
    fn total(self) -> Option<(u64, bool)> {
        self.get().map(|(bytes, [partial, _])| (bytes, partial))
    }
    fn of_total((bytes, partial): (u64, bool)) -> Self {
        Self::of(bytes, [partial, false])
    }
}

/// When a folder was last checked, to the second, in four bytes: seconds
/// since the first one was, from 1 (0 is never).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Checked(u32);
impl Checked {
    fn start() -> Instant {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *START.get_or_init(Instant::now)
    }
    fn at(now: Instant) -> Self {
        let secs = now.saturating_duration_since(Self::start()).as_secs();
        Self(u32::try_from(secs + 1).unwrap_or(u32::MAX))
    }
    fn is_some(self) -> bool {
        self.0 != 0
    }
    fn fresh(self) -> bool {
        self.0 != 0 && Self::at(Instant::now()).0 - self.0 < FRESH.as_secs() as u32
    }
}

/// A folder of the tree, kept small (128 bytes): its name is in the tree's
/// `names`, and what is added up below it is in one word each.
#[derive(Debug, Default)]
struct Node {
    /// Where its name is in `Tree::names`, and how long.
    name: u32,
    name_len: u16,
    parent: u32,
    children: Box<[u32]>,
    stamp: Stamp,
    /// The bytes of its files with one link.
    files: u64,
    links: Box<[Link]>,
    /// Its entries were read (or found unreadable); else it is only on the
    /// way to a folder that was.
    listed: bool,
    /// Something in it changed: read its entries again, whatever its stamp.
    dirty: bool,
    unreadable: bool,
    /// Some of its entries could not be read.
    partial: bool,
    /// On another filesystem than its parent: not part of its total.
    foreign: bool,
    /// When every folder below was last found unchanged or read again.
    checked: Checked,
    /// Its subtree's sum (`Sum`), while nothing below changes.
    sum: Kept,
    /// Its total (`Tree::total`), while nothing below changes: files with
    /// several links are counted once by walking the subtree.
    total: Kept,
    /// The last total found, shown while it is added up again.
    shown: Kept,
}
impl Node {
    fn sum(&self) -> Option<Sum> {
        self.sum.get().map(|(plain, [links, partial])| Sum {
            plain,
            links,
            partial,
        })
    }
}

/// The folders known, by their parents: node 0 is `/`.
struct Tree {
    nodes: Vec<Node>,
    free: Vec<u32>,
    /// The folders' names, one after another; those of folders removed
    /// stay until `tidy`.
    names: Vec<u8>,
    /// The bytes of `names` that folders still have.
    named: usize,
}
impl Default for Tree {
    fn default() -> Self {
        Self {
            nodes: vec![Node {
                parent: NONE,
                ..Node::default()
            }],
            free: Vec::new(),
            names: Vec::new(),
            named: 0,
        }
    }
}
impl Tree {
    fn name(&self, at: u32) -> &OsStr {
        let node = &self.nodes[at as usize];
        let start = node.name as usize;
        OsStr::from_bytes(&self.names[start..start + node.name_len as usize])
    }
    /// Drops the names of folders removed, once they are most of `names`.
    fn tidy(&mut self) {
        if self.names.len() < 2 * self.named + (1 << 20) {
            return;
        }
        let mut names = Vec::with_capacity(self.named);
        for node in &mut self.nodes {
            let start = node.name as usize;
            node.name = names.len() as u32;
            names.extend_from_slice(&self.names[start..start + node.name_len as usize]);
        }
        self.names = names;
    }
    fn live(&self) -> usize {
        self.nodes.len() - self.free.len()
    }
    fn child(&self, at: u32, name: &OsStr) -> Option<u32> {
        self.nodes[at as usize]
            .children
            .iter()
            .copied()
            .find(|&c| self.name(c) == name)
    }
    fn names(path: &Path) -> Option<impl Iterator<Item = &OsStr>> {
        path.is_absolute().then(|| {
            path.components().filter_map(|c| match c {
                Component::Normal(name) => Some(name),
                _ => None,
            })
        })
    }
    fn lookup(&self, path: &Path) -> Option<u32> {
        Self::names(path)?.try_fold(0, |at, name| self.child(at, name))
    }
    /// `path`'s node, made with those on the way to it if missing.
    fn ensure(&mut self, path: &Path) -> Option<u32> {
        let mut at = 0;
        for name in Self::names(path)? {
            at = match self.child(at, name) {
                Some(child) => child,
                None => {
                    let child = self.add(at, name);
                    let mut children =
                        std::mem::take(&mut self.nodes[at as usize].children).into_vec();
                    children.push(child);
                    self.nodes[at as usize].children = children.into();
                    child
                }
            };
        }
        Some(at)
    }
    /// A new folder of `parent`, which does not have it among its
    /// children yet.
    fn add(&mut self, parent: u32, name: &OsStr) -> u32 {
        self.tidy();
        // Names are at most 255 bytes long.
        let name = &name.as_bytes()[..name.len().min(u16::MAX as usize)];
        let node = Node {
            name: self.names.len() as u32,
            name_len: name.len() as u16,
            parent,
            ..Node::default()
        };
        self.names.extend_from_slice(name);
        self.named += name.len();
        match self.free.pop() {
            Some(at) => {
                self.nodes[at as usize] = node;
                at
            }
            None => {
                self.nodes.push(node);
                (self.nodes.len() - 1) as u32
            }
        }
    }
    fn remove(&mut self, at: u32) {
        let mut stack = vec![at];
        while let Some(at) = stack.pop() {
            let node = std::mem::take(&mut self.nodes[at as usize]);
            self.named -= node.name_len as usize;
            stack.extend(node.children);
            self.free.push(at);
        }
    }
    #[cfg(test)]
    fn path(&self, mut at: u32) -> PathBuf {
        let mut names = Vec::new();
        while at != 0 && at != NONE {
            let node = &self.nodes[at as usize];
            names.push(self.name(at));
            at = node.parent;
        }
        let mut path = PathBuf::from("/");
        path.extend(names.into_iter().rev());
        path
    }
    /// Something at `at` changed: its sum and those above it are gone, and
    /// it and they are no longer checked.
    fn invalidate(&mut self, mut at: u32) {
        while at != NONE {
            let node = &mut self.nodes[at as usize];
            node.sum = Kept::default();
            node.total = Kept::default();
            node.checked = Checked::default();
            at = node.parent;
        }
    }
    fn fresh(&self, at: u32) -> bool {
        let node = &self.nodes[at as usize];
        !node.dirty && node.checked.fresh()
    }
    /// The subtree's sum, if every folder in it was read.
    fn sum(&mut self, at: u32) -> Option<Sum> {
        let node = &self.nodes[at as usize];
        if let Some(sum) = node.sum() {
            return Some(sum);
        }
        if !node.listed {
            return None;
        }
        let mut sum = Sum {
            plain: node.files,
            links: !node.links.is_empty(),
            partial: node.partial,
        };
        for child in node.children.clone().into_vec() {
            let below = &self.nodes[child as usize];
            if below.foreign {
                continue;
            }
            let unreadable = below.unreadable;
            let more = self.sum(child)?;
            sum.plain += more.plain;
            sum.links |= more.links;
            sum.partial |= more.partial || unreadable;
        }
        self.nodes[at as usize].sum = Kept::of(sum.plain, [sum.links, sum.partial]);
        Some(sum)
    }
    /// The subtree's bytes and whether some of it could not be read, files
    /// with several links counted once; remembered to show while it is
    /// added up again.
    fn total(&mut self, at: u32) -> Option<(u64, bool)> {
        if let Some(total) = self.nodes[at as usize].total.total() {
            return Some(total);
        }
        let sum = self.sum(at)?;
        let mut bytes = sum.plain;
        if sum.links {
            let mut seen = HashSet::new();
            let mut stack = vec![at];
            while let Some(at) = stack.pop() {
                let node = &self.nodes[at as usize];
                for &(dev, ino, len) in node.links.iter() {
                    if seen.insert((dev, ino)) {
                        bytes += len;
                    }
                }
                stack.extend(node.children.iter().copied().filter(|&c| {
                    let child = &self.nodes[c as usize];
                    !child.foreign && child.sum().is_some_and(|s| s.links)
                }));
            }
        }
        let total = (bytes, sum.partial);
        let node = &mut self.nodes[at as usize];
        node.shown = Kept::of_total(total);
        node.total = node.shown;
        Some(total)
    }
    /// A walk of `root` is done: each folder whose subtree has nothing left
    /// to read again is checked now.
    fn checked(&mut self, root: u32) {
        let now = Checked::at(Instant::now());
        // Fresh subtrees were left alone by the walk, and keep their time.
        let mut order = vec![root];
        let mut i = 0;
        while i < order.len() {
            let node = &self.nodes[order[i] as usize];
            order.extend(
                node.children
                    .iter()
                    .copied()
                    .filter(|&c| !self.nodes[c as usize].foreign && !self.fresh(c)),
            );
            i += 1;
        }
        for &at in order.iter().rev() {
            let node = &self.nodes[at as usize];
            let done = node.listed
                && !node.dirty
                && node.children.iter().all(|&c| {
                    let child = &self.nodes[c as usize];
                    child.foreign || child.checked.is_some()
                });
            self.nodes[at as usize].checked = if done { now } else { Checked::default() };
        }
    }
    fn dirty_all(&mut self) {
        for node in &mut self.nodes {
            node.dirty = true;
            node.checked = Checked::default();
        }
    }

    /// The tree as bytes: its folders breadth first, each after its parent.
    fn save(&self) -> Vec<u8> {
        let mut out = HEADER.to_vec();
        let mut order = vec![0u32];
        let mut index = HashMap::new();
        let mut i = 0;
        while i < order.len() {
            let at = order[i];
            index.insert(at, i as u32);
            let node = &self.nodes[at as usize];
            order.extend(node.children.iter());
            let parent = if node.parent == NONE {
                NONE
            } else {
                index[&node.parent]
            };
            let word = |out: &mut Vec<u8>, v: u64| out.extend(v.to_le_bytes());
            out.extend(parent.to_le_bytes());
            out.extend(u32::from(node.name_len).to_le_bytes());
            out.extend(self.name(at).as_bytes());
            let s = node.stamp;
            for v in [s.dev, s.ino, s.mtime as u64, s.mtime_nsec.into()] {
                word(&mut out, v);
            }
            for v in [s.ctime as u64, s.ctime_nsec.into(), node.files] {
                word(&mut out, v);
            }
            let flags = u8::from(node.listed && !node.dirty)
                | u8::from(node.unreadable) << 1
                | u8::from(node.partial) << 2
                | u8::from(node.foreign) << 3;
            out.push(flags);
            out.extend((node.children.len() as u32).to_le_bytes());
            out.extend((node.links.len() as u32).to_le_bytes());
            for &(dev, ino, len) in node.links.iter() {
                for v in [dev, ino, len] {
                    word(&mut out, v);
                }
            }
            i += 1;
        }
        out
    }
    /// A saved tree; `None` when it is not one. Its folders are to be
    /// checked: none is fresh.
    fn load(bytes: &[u8]) -> Option<Self> {
        let mut rest = bytes.strip_prefix(HEADER)?;
        let mut take = |n: usize| -> Option<&[u8]> {
            let (head, tail) = rest.split_at_checked(n)?;
            rest = tail;
            Some(head)
        };
        let mut nodes: Vec<Node> = Vec::new();
        let mut names = Vec::new();
        // Each folder's parent and how many subfolders it was saved with.
        let mut parents = Vec::new();
        let mut counts = Vec::new();
        while let Some(parent) = take(4) {
            let parent = u32::from_le_bytes(parent.try_into().ok()?);
            let u32_of = |b: &[u8]| -> Option<u32> { Some(u32::from_le_bytes(b.try_into().ok()?)) };
            let length = u32_of(take(4)?)? as usize;
            let name = take(u16::try_from(length).ok()?.into())?;
            let mut words = [0u64; 7];
            for word in &mut words {
                *word = u64::from_le_bytes(take(8)?.try_into().ok()?);
            }
            let flags = take(1)?[0];
            let children = u32_of(take(4)?)?;
            let links = u32_of(take(4)?)? as usize;
            let mut found = Vec::with_capacity(links.min(1 << 16));
            for _ in 0..links {
                let link = take(24)?;
                let w = |i: usize| u64::from_le_bytes(link[i * 8..i * 8 + 8].try_into().unwrap());
                found.push((w(0), w(1), w(2)));
            }
            let at = nodes.len() as u32;
            match (at, parent) {
                (0, NONE) => {}
                (0, _) | (_, NONE) => return None,
                (_, parent) if parent >= at => return None,
                _ => {}
            }
            parents.push(parent);
            nodes.push(Node {
                name: u32::try_from(names.len()).ok()?,
                name_len: name.len() as u16,
                parent,
                stamp: Stamp {
                    dev: words[0],
                    ino: words[1],
                    mtime: words[2] as i64,
                    ctime: words[4] as i64,
                    mtime_nsec: words[3] as u32,
                    ctime_nsec: words[5] as u32,
                },
                files: words[6],
                links: found.into(),
                listed: flags & 1 != 0,
                unreadable: flags & 2 != 0,
                partial: flags & 4 != 0,
                foreign: flags & 8 != 0,
                ..Node::default()
            });
            names.extend_from_slice(name);
            counts.push(children);
        }
        if nodes.is_empty() {
            return None;
        }
        // Each folder's subfolders, in the order saved: counted, then put
        // in a list of just their number.
        let mut found = vec![0u32; nodes.len()];
        for &parent in &parents[1..] {
            found[parent as usize] += 1;
        }
        let mut children: Vec<Vec<u32>> = found
            .iter()
            .map(|&n| Vec::with_capacity(n as usize))
            .collect();
        for (at, &parent) in parents.iter().enumerate().skip(1) {
            children[parent as usize].push(at as u32);
        }
        for ((node, children), count) in nodes.iter_mut().zip(children).zip(counts) {
            // A folder missing some of its subfolders is read again.
            if children.len() != count as usize {
                node.listed = false;
            }
            node.children = children.into();
        }
        Some(Self {
            nodes,
            free: Vec::new(),
            named: names.len(),
            names,
        })
    }
}

#[derive(Default)]
struct State {
    /// Folders the latest frame wanted, in order, with their mounts.
    wanted: Vec<(PathBuf, PathBuf)>,
    /// Bumped as `wanted` changes, so that workers look again.
    version: u64,
    tree: Tree,
    /// Bumped as the tree is replaced: walks on the old one give up.
    epoch: u64,
    /// The saved tree is being read: workers wait for it.
    loading: bool,
    /// The tree changed since it was read or saved.
    unsaved: bool,
    /// Growing totals of the folders being added up.
    running: HashMap<PathBuf, u64>,
    /// While the rows are sorted by size, the folders listed, by mount,
    /// sorted by path (a folder before those below it), added up after
    /// those wanted: each mount's from its place on.
    listed: HashMap<PathBuf, (Vec<PathBuf>, usize)>,
    /// Bumped as `listed` is replaced.
    listing: u64,
    /// Mounts with a worker.
    workers: HashSet<PathBuf>,
    ended: bool,
}
impl State {
    /// The first folder of `mount` wanted whose total is not fresh.
    fn next(&self, mount: &Path) -> Option<PathBuf> {
        self.wanted
            .iter()
            .filter(|(_, m)| m == mount)
            .map(|(p, _)| p)
            .find(|p| !self.tree.lookup(p).is_some_and(|at| self.tree.fresh(at)))
            .cloned()
    }
    /// The next folder of `mount` listed whose total is not fresh, and its
    /// place: those below a fresh one are passed over with it, as its walk
    /// read them. `Err` after looking at `SKIPPED` of them, to look again
    /// without holding the lock throughout.
    fn next_listed(&mut self, mount: &Path) -> Result<Option<(PathBuf, usize)>, ()> {
        let Some((folders, at)) = self.listed.get_mut(mount) else {
            return Ok(None);
        };
        let mut looked = 0;
        while let Some(folder) = folders.get(*at) {
            if !self.tree.lookup(folder).is_some_and(|n| self.tree.fresh(n)) {
                return Ok(Some((folder.clone(), *at)));
            }
            *at += 1;
            while folders.get(*at).is_some_and(|f| below(f, folder)) {
                *at += 1;
            }
            looked += 1;
            if looked >= SKIPPED {
                return Err(());
            }
        }
        Ok(None)
    }
}
/// Listed folders looked at, at most, while holding the lock once.
const SKIPPED: usize = 1024;
/// Whether `path` is below `folder`, by their bytes.
fn below(path: &Path, folder: &Path) -> bool {
    let (path, folder) = (path.as_os_str().as_bytes(), folder.as_os_str().as_bytes());
    path.len() > folder.len()
        && path.starts_with(folder)
        && (folder.ends_with(b"/") || path[folder.len()] == b'/')
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    ready: Condvar,
    changed: AtomicBool,
    /// A total was added up (or the saved ones read) since `Sizes::added`.
    added: AtomicBool,
}
impl Shared {
    fn wake(&self, wake: Option<&SyncSender<()>>) {
        self.changed.store(true, Ordering::Relaxed);
        if let Some(wake) = wake {
            let _ = wake.try_send(());
        }
    }
}

#[derive(Default)]
pub struct Sizes {
    shared: Arc<Shared>,
    /// Folders the frame being drawn wants, with their mounts.
    frame: RefCell<Vec<(PathBuf, PathBuf)>>,
    wake: Option<SyncSender<()>>,
    /// Where the tree is saved, and whether it was read yet.
    kept: Option<(PathBuf, std::cell::Cell<bool>)>,
}
impl Sizes {
    pub fn set_waker(&mut self, wake: SyncSender<()>) {
        self.wake = Some(wake);
    }
    /// Keeps what was found in `path` between sessions: read when the first
    /// frame is drawn, written when the session ends.
    pub fn keep(&mut self, path: PathBuf) {
        self.kept = Some((path, std::cell::Cell::new(false)));
    }
    /// The frame being drawn wants `path`'s size, on `mount`.
    pub fn want(&self, path: &Path, mount: PathBuf) {
        self.frame.borrow_mut().push((path.to_path_buf(), mount));
    }
    /// The folders the frame being drawn wants so far.
    #[cfg(test)]
    pub fn wanted(&self) -> Vec<PathBuf> {
        self.frame.borrow().iter().map(|(p, _)| p.clone()).collect()
    }
    /// What the workers found for `path`: its total, the last one while it
    /// is added up again, else its growing total; `None` before either.
    pub fn get(&self, path: &Path) -> Option<Size> {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(at) = state.tree.lookup(path) {
            let unreadable = state.tree.nodes[at as usize].unreadable;
            let found = state
                .tree
                .total(at)
                .or(state.tree.nodes[at as usize].shown.total());
            if let Some((bytes, partial)) = found {
                return Some(Size {
                    bytes,
                    counting: false,
                    partial,
                    unreadable,
                });
            }
        }
        state.running.get(path).map(|&bytes| Size {
            bytes,
            counting: true,
            partial: false,
            unreadable: false,
        })
    }
    /// The totals found so far, for ranking on other threads.
    pub fn totals(&self) -> Totals {
        Totals(self.shared.clone())
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
        self.load(&mut state);
        for (_, mount) in &frame {
            spawn(&self.shared, &mut state, mount, self.wake.as_ref());
        }
        state.wanted = frame;
        state.version += 1;
        self.shared.ready.notify_all();
    }
    /// The saved tree is read once, before anything is added up.
    fn load(&self, state: &mut State) {
        if let Some((path, read)) = &self.kept
            && !read.replace(true)
        {
            state.loading = true;
            let (shared, path, wake) = (self.shared.clone(), path.clone(), self.wake.clone());
            std::thread::spawn(move || load(&shared, &path, wake.as_ref()));
        }
    }
    /// The rows are sorted by size: the folders in `runs` are added up
    /// after those the frames want, but for those on a kernel filesystem
    /// (by `mounts`); with no runs, none are. They are gathered on a thread
    /// of their own, which hands them to the workers.
    pub fn list(&self, runs: Vec<Arc<crate::rows::Run>>, mounts: Arc<crate::mounts::Mounts>) {
        let mut state = self.shared.state.lock().unwrap();
        state.listing += 1;
        let listing = state.listing;
        if runs.is_empty() {
            if !state.listed.is_empty() {
                state.listed.clear();
                state.version += 1;
                self.shared.ready.notify_all();
            }
            return;
        }
        self.load(&mut state);
        drop(state);
        let (shared, wake) = (self.shared.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().spawn(move || {
            let mut listed: HashMap<PathBuf, Vec<PathBuf>> = HashMap::new();
            for c in runs.iter().flat_map(|r| r.rows.iter()) {
                if c.directory && !mounts.kernel(&c.path) {
                    let folders = listed.entry(mounts.point(&c.path)).or_default();
                    folders.push(c.path.clone());
                }
            }
            for folders in listed.values_mut() {
                folders.sort_unstable();
                folders.dedup();
            }
            let mut state = shared.state.lock().unwrap();
            if state.listing != listing {
                return;
            }
            for mount in listed.keys() {
                spawn(&shared, &mut state, mount, wake.as_ref());
            }
            state.listed = listed.into_iter().map(|(m, f)| (m, (f, 0))).collect();
            state.version += 1;
            shared.ready.notify_all();
        });
    }
    /// Whether totals were added up, or the saved ones read, since the last
    /// call.
    pub fn added(&self) -> bool {
        self.shared.added.swap(false, Ordering::Relaxed)
    }
    /// Whether totals arrived or grew since the last call.
    pub fn drain(&self) -> bool {
        self.shared.changed.swap(false, Ordering::Relaxed)
    }
    /// `path` changed on disk: its folder is read again, and the folders
    /// above it added up again, when wanted.
    pub fn touched(&self, path: &Path) {
        let mut state = self.shared.state.lock().unwrap();
        let tree = &mut state.tree;
        for folder in [Some(path), path.parent()].into_iter().flatten() {
            if let Some(at) = tree.lookup(folder) {
                tree.nodes[at as usize].dirty = true;
                tree.invalidate(at);
            }
        }
        self.shared.ready.notify_all();
    }
    /// F5: every folder is read again when wanted, the listed ones again
    /// from the first.
    pub fn refresh(&self) {
        let mut state = self.shared.state.lock().unwrap();
        state.tree.dirty_all();
        for (_, at) in state.listed.values_mut() {
            *at = 0;
        }
        drop(state);
        self.shared.ready.notify_all();
    }
}
/// Starts `mount`'s worker, unless it has one.
fn spawn(shared: &Arc<Shared>, state: &mut State, mount: &Path, wake: Option<&SyncSender<()>>) {
    if state.workers.insert(mount.to_path_buf()) {
        let (shared, mount, wake) = (shared.clone(), mount.to_path_buf(), wake.cloned());
        let _ = std::thread::Builder::new().spawn(move || work(&shared, &mount, wake.as_ref()));
    }
}
/// Folders' totals as the workers find them, read where sorting by size
/// needs them; a ranking on another thread holds one.
#[derive(Clone, Default)]
pub struct Totals(Arc<Shared>);
impl Totals {
    /// `path`'s total once added up (the last one while it is added up
    /// again); `None` before.
    pub fn bytes(&self, path: &Path) -> Option<u64> {
        let mut state = self.0.state.lock().unwrap();
        let at = state.tree.lookup(path)?;
        let found = state
            .tree
            .total(at)
            .or(state.tree.nodes[at as usize].shown.total());
        found.map(|(bytes, _)| bytes)
    }
    /// `bytes` of the first of `paths`, under one lock held for at most
    /// about `HELD` (and for one path at least), so that drawing, which
    /// reads totals too, waits little: fewer than asked once it is up.
    pub fn bytes_each(&self, paths: &[&Path]) -> Vec<Option<u64>> {
        // A frame reads each folder's total under a lock of its own.
        const HELD: Duration = Duration::from_micros(100);
        let start = Instant::now();
        let mut state = self.0.state.lock().unwrap();
        let tree = &mut state.tree;
        let mut found = Vec::with_capacity(paths.len());
        for path in paths {
            if !found.is_empty() && start.elapsed() >= HELD {
                break;
            }
            let total = tree.lookup(path).and_then(|at| {
                let total = tree.total(at).or(tree.nodes[at as usize].shown.total());
                total.map(|(bytes, _)| bytes)
            });
            found.push(total);
        }
        found
    }
}
impl Drop for Sizes {
    fn drop(&mut self) {
        let mut state = self.shared.state.lock().unwrap();
        state.ended = true;
        self.shared.ready.notify_all();
        // Saved only once read, so a session too short to read it keeps it.
        if let Some((path, read)) = &self.kept
            && read.get()
            && !state.loading
            && state.unsaved
        {
            let bytes = state.tree.save();
            drop(state);
            let _ = crate::state::write(path, &bytes);
        }
    }
}

/// `$XDG_CACHE_HOME/fff/sizes`, else `~/.cache/fff/sizes`.
pub fn path() -> Option<PathBuf> {
    crate::grep::cache_dir().and_then(|text| Some(text.parent()?.join("sizes")))
}

/// Reads the saved tree into a session that has found nothing yet.
fn load(shared: &Shared, path: &Path, wake: Option<&SyncSender<()>>) {
    let tree = fs::read(path).ok().and_then(|bytes| Tree::load(&bytes));
    let mut state = shared.state.lock().unwrap();
    if let Some(tree) = tree {
        state.tree = tree;
        state.epoch += 1;
    }
    state.loading = false;
    shared.ready.notify_all();
    drop(state);
    shared.added.store(true, Ordering::Relaxed);
    shared.wake(wake);
}

/// A mount's worker: adds up the folders wanted on it, first first, then
/// those listed.
fn work(shared: &Shared, mount: &Path, wake: Option<&SyncSender<()>>) {
    loop {
        let (root, listed, version) = {
            let mut state = shared.state.lock().unwrap();
            loop {
                if state.ended {
                    return;
                }
                if !state.loading {
                    if let Some(root) = state.next(mount) {
                        break (root, None, state.version);
                    }
                    match state.next_listed(mount) {
                        Ok(Some((root, at))) => {
                            break (root, Some((state.listing, at)), state.version);
                        }
                        Ok(None) => {}
                        // Others may take the lock between looks.
                        Err(()) => {
                            drop(state);
                            state = shared.state.lock().unwrap();
                            continue;
                        }
                    }
                }
                state = shared.ready.wait(state).unwrap();
            }
        };
        let walk = Walk {
            shared,
            wake,
            mount,
            root: &root,
            listed,
            queue: Mutex::new((Vec::new(), 0)),
            more: Condvar::new(),
            version: Mutex::new(version),
            epoch: Default::default(),
            running: Mutex::new((0, Instant::now())),
        };
        let done = walk.run();
        if let Some((listing, at)) = listed {
            // Done or not read (no such folder), it is passed; given up,
            // it is taken again once nothing wanted comes first.
            let mut state = shared.state.lock().unwrap();
            if done
                && state.listing == listing
                && let Some((_, next)) = state.listed.get_mut(mount)
                && *next == at
            {
                *next = at + 1;
            }
        }
        if done {
            shared.added.store(true, Ordering::Relaxed);
            shared.wake(wake);
        }
    }
}

/// A folder to read: its node and path, and its metadata when its parent's
/// listing just read it.
type Task = (u32, PathBuf, Option<fs::Metadata>);

/// Adding up one wanted folder: its folders are read by several threads,
/// taking them from a shared queue.
struct Walk<'a> {
    shared: &'a Shared,
    wake: Option<&'a SyncSender<()>>,
    mount: &'a Path,
    root: &'a Path,
    /// A folder listed (`State::listed`) rather than wanted: the listing
    /// and its place there.
    listed: Option<(u64, usize)>,
    /// Folders to read, and how many threads are reading one.
    queue: Mutex<(Vec<Task>, usize)>,
    more: Condvar,
    /// The `wanted` version last checked.
    version: Mutex<u64>,
    /// The tree's epoch the walk's nodes belong to.
    epoch: std::sync::atomic::AtomicU64,
    /// The root's bytes so far, and when they were last shown.
    running: Mutex<(u64, Instant)>,
}
impl Walk<'_> {
    /// Reads what changed below the root; true when it is done, false when
    /// it was given up.
    fn run(&self) -> bool {
        {
            let mut state = self.shared.state.lock().unwrap();
            if state.tree.live() > KEPT {
                state.tree = Tree::default();
                state.epoch += 1;
            }
            self.epoch.store(state.epoch, Ordering::Relaxed);
            let Some(at) = state.tree.ensure(self.root) else {
                return true;
            };
            self.queue
                .lock()
                .unwrap()
                .0
                .push((at, self.root.to_path_buf(), None));
        }
        let readers = std::thread::available_parallelism().map_or(1, |n| n.get().min(READERS));
        std::thread::scope(|scope| {
            for _ in 1..readers {
                let _ = std::thread::Builder::new().spawn_scoped(scope, || self.read());
            }
            self.read();
        });
        let mut state = self.shared.state.lock().unwrap();
        state.running.remove(self.root);
        if self.abandoned(&state) {
            return false;
        }
        if let Some(at) = state.tree.lookup(self.root) {
            state.tree.checked(at);
        }
        true
    }
    /// One reader: takes folders until none are left or the walk is given up.
    fn read(&self) {
        loop {
            let task = {
                let mut queue = self.queue.lock().unwrap();
                loop {
                    if let Some(task) = queue.0.pop() {
                        queue.1 += 1;
                        break Some(task);
                    }
                    if queue.1 == 0 {
                        break None;
                    }
                    queue = self.more.wait(queue).unwrap();
                }
            };
            let Some(task) = task else {
                self.more.notify_all();
                return;
            };
            let found = self.folder(task);
            let mut queue = self.queue.lock().unwrap();
            queue.1 -= 1;
            match found {
                Some(found) => queue.0.extend(found),
                // Given up: nothing more is read.
                None => queue.0.clear(),
            }
            drop(queue);
            self.more.notify_all();
        }
    }
    /// Reads the folder if it changed, else keeps what was read; returns
    /// its subfolders to read, `None` when the walk is given up.
    fn folder(&self, (at, path, meta): Task) -> Option<Vec<Task>> {
        let meta = meta.map_or_else(|| fs::symlink_metadata(&path), Ok);
        let stamp = meta.as_ref().ok().map(Stamp::of);
        {
            let mut state = self.shared.state.lock().unwrap();
            if self.abandoned(&state) {
                return None;
            }
            let tree = &mut state.tree;
            let node = tree.nodes.get(at as usize)?;
            if Some(node.stamp) == stamp && node.listed && !node.dirty && !node.unreadable {
                let files = node.files;
                let below = node
                    .children
                    .iter()
                    .filter(|&&c| !tree.nodes[c as usize].foreign && !tree.fresh(c))
                    .map(|&c| (c, path.join(tree.name(c)), None))
                    .collect();
                drop(state);
                self.add(files);
                return Some(below);
            }
        }
        // A link to a folder on a kernel filesystem (whose sizes are made
        // up) is not added up: it shows as unreadable.
        let kernel = path == self.root && crate::mounts::kernel_file(&path);
        let read = stamp
            .filter(|_| !kernel)
            .and_then(|stamp| Some((stamp, fs::read_dir(&path).ok()?)));
        let mut listing = Listing::default();
        if let Some((stamp, entries)) = read {
            listing.stamp = stamp;
            for entry in entries {
                let Ok(entry) = entry else {
                    listing.partial = true;
                    continue;
                };
                // Read beside the folder (`statx` on its descriptor).
                let Ok(meta) = entry.metadata() else {
                    listing.partial = true;
                    continue;
                };
                if meta.is_dir() {
                    listing.dirs.push((entry.file_name(), meta));
                } else if meta.nlink() > 1 {
                    listing.links.push((meta.dev(), meta.ino(), meta.len()));
                } else {
                    listing.files += meta.len();
                }
            }
        } else {
            listing.unreadable = true;
        }
        let files = listing.files;
        let below = self.store(at, &path, listing)?;
        self.add(files);
        Some(below)
    }
    /// Puts a folder's entries in the tree: subfolders still there keep
    /// what was found below them, new ones are to be read.
    fn store(&self, at: u32, path: &Path, listing: Listing) -> Option<Vec<Task>> {
        let mut state = self.shared.state.lock().unwrap();
        if self.abandoned(&state) {
            return None;
        }
        state.unsaved = true;
        let tree = &mut state.tree;
        let mut old: HashMap<OsString, u32> = tree.nodes[at as usize]
            .children
            .iter()
            .map(|&c| (tree.name(c).to_owned(), c))
            .collect();
        let mut children = Vec::with_capacity(listing.dirs.len());
        let mut below = Vec::new();
        for (name, meta) in listing.dirs {
            let foreign = meta.dev() != listing.stamp.dev;
            let child = match old.remove(&name) {
                Some(child) => child,
                None => tree.add(at, &name),
            };
            tree.nodes[child as usize].foreign = foreign;
            children.push(child);
            // A subfolder checked lately, and the same folder, is left alone.
            let same = tree.fresh(child) && tree.nodes[child as usize].stamp == Stamp::of(&meta);
            if !foreign && !same {
                below.push((child, path.join(&name), Some(meta)));
            }
        }
        for (_, gone) in old {
            tree.remove(gone);
        }
        let node = &mut tree.nodes[at as usize];
        node.children = children.into();
        node.stamp = listing.stamp;
        node.files = listing.files;
        node.links = listing.links.into();
        node.partial = listing.partial;
        node.unreadable = listing.unreadable;
        node.listed = true;
        node.dirty = false;
        tree.invalidate(at);
        Some(below)
    }
    /// Shows the root's growing total now and then.
    fn add(&self, bytes: u64) {
        let mut running = self.running.lock().unwrap();
        running.0 += bytes;
        if running.1.elapsed() < PROGRESS {
            return;
        }
        running.1 = Instant::now();
        let so_far = running.0;
        drop(running);
        let mut state = self.shared.state.lock().unwrap();
        // A total found before shows until the new one is done.
        if state
            .tree
            .lookup(self.root)
            .is_some_and(|at| state.tree.nodes[at as usize].shown.is_some())
        {
            return;
        }
        state.running.insert(self.root.to_path_buf(), so_far);
        drop(state);
        self.shared.wake(self.wake);
    }
    /// Whether to give up on the root: the session ends, or since the
    /// frames changed, it is no longer the first folder of its mount to do.
    fn abandoned(&self, state: &State) -> bool {
        if state.ended || state.epoch != self.epoch.load(Ordering::Relaxed) {
            return true;
        }
        let mut version = self.version.lock().unwrap();
        if state.version == *version {
            return false;
        }
        *version = state.version;
        // Checked against the tree as the walk left it, the root is not
        // fresh until the walk ends, so it is still first if wanted first.
        let first = state
            .wanted
            .iter()
            .filter(|(_, m)| m == self.mount)
            .map(|(p, _)| p)
            .find(|p| {
                p.as_path() == self.root
                    || !state.tree.lookup(p).is_some_and(|at| state.tree.fresh(at))
            })
            .map(PathBuf::as_path);
        match self.listed {
            None => first != Some(self.root),
            // A listed folder gives way to any wanted, and goes once no
            // longer listed.
            Some(_) if first.is_some() => first != Some(self.root),
            Some((listing, _)) => {
                listing != state.listing
                    && !state.listed.get(self.mount).is_some_and(|(folders, _)| {
                        folders
                            .binary_search_by(|f| f.as_path().cmp(self.root))
                            .is_ok()
                    })
            }
        }
    }
}

/// A folder's entries as read.
#[derive(Default)]
struct Listing {
    stamp: Stamp,
    files: u64,
    links: Vec<Link>,
    dirs: Vec<(OsString, fs::Metadata)>,
    partial: bool,
    unreadable: bool,
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
        let deadline = Instant::now() + Duration::from_secs(300);
        let done = |p: &PathBuf| {
            let state = sizes.shared.state.lock().unwrap();
            state.tree.lookup(p).is_some_and(|at| state.tree.fresh(at))
        };
        while !wanted.iter().all(done) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        wanted.iter().map(|p| sizes.get(p)).collect()
    }
    fn bytes(size: Option<Size>) -> Option<u64> {
        size.map(|s| s.bytes)
    }
    fn wait_for(sizes: &Sizes, path: &Path, want: u64) -> Option<u64> {
        let deadline = Instant::now() + Duration::from_secs(300);
        while bytes(sizes.get(path)) != Some(want) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(2));
        }
        bytes(sizes.get(path))
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
        // The folders below were added up on the way, with the linked file
        // counted in each folder it is in.
        assert_eq!(bytes(sizes.get(&root.join("a/deep"))), Some(1024));
        assert_eq!(bytes(sizes.get(&root.join("a/deep/er"))), Some(24));
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
    fn listed_folders_are_added_up_after_wanted_ones_while_listed() {
        let root = fixture("listed");
        let sizes = Sizes::default();
        let folder = |path: PathBuf| {
            Arc::new(crate::Candidate {
                path,
                directory: true,
                depth: 1,
                root: 0,
                order: 0,
            })
        };
        let run = crate::rows::Run {
            rows: vec![
                folder(root.join("b")),
                folder(root.join("a")),
                folder(root.join("a/deep")),
                folder(PathBuf::from("/proc/self")),
            ],
            keys: Vec::new(),
        };
        let table = "/dev/sda1 / ext4 rw 0 0\nproc /proc proc rw 0 0\n";
        let mounts = Arc::new(crate::mounts::Mounts::parse(table));
        sizes.list(vec![Arc::new(run)], mounts);
        // No frame wants them: listed, they are added up all the same.
        assert_eq!(wait_for(&sizes, &root.join("b"), 7), Some(7));
        assert_eq!(wait_for(&sizes, &root.join("a"), 1024), Some(1024));
        assert!(sizes.added());
        assert!(!sizes.added());
        // Those on a kernel filesystem are not listed.
        let state = sizes.shared.state.lock().unwrap();
        assert_eq!(state.listed.len(), 1);
        drop(state);
        // No longer listed, nothing more is added up.
        sizes.list(Vec::new(), Arc::default());
        assert!(sizes.shared.state.lock().unwrap().listed.is_empty());
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn listed_folders_below_one_added_up_are_passed_with_it() {
        let paths = ["/a", "/a/b", "/a/b/c", "/a-b", "/ab", "/b"].map(PathBuf::from);
        let mut sorted = paths.to_vec();
        sorted.sort();
        assert_eq!(sorted, paths);
        assert!(below(&paths[1], &paths[0]) && below(&paths[2], &paths[0]));
        assert!(!below(&paths[3], &paths[0]) && !below(&paths[4], &paths[0]));
        assert!(!below(&paths[0], &paths[0]) && below(&paths[1], Path::new("/")));
    }
    #[test]
    fn a_change_below_a_folder_adds_it_up_again_showing_the_old_total_meanwhile() {
        let root = fixture("touched");
        let sizes = Sizes::default();
        assert_eq!(
            bytes(sizes_of(&sizes, std::slice::from_ref(&root))[0]),
            Some(1031)
        );
        let added = root.join("a/deep/er/four");
        fs::write(&added, vec![0u8; 9]).unwrap();
        // Unchanged until something says the folder changed.
        assert_eq!(
            bytes(sizes_of(&sizes, std::slice::from_ref(&root))[0]),
            Some(1031)
        );
        sizes.touched(&added);
        assert_eq!(bytes(sizes.get(&root)), Some(1031));
        assert_eq!(wait_for(&sizes, &root, 1040), Some(1040));
        // A file growing in place leaves its folder's times alone: F5 reads
        // every folder again.
        fs::write(root.join("b/three"), vec![0u8; 67]).unwrap();
        sizes.refresh();
        assert_eq!(wait_for(&sizes, &root, 1100), Some(1100));
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn a_saved_tree_shows_at_once_and_only_changed_folders_are_read_again() {
        let root = fixture("kept");
        let kept = root.with_extension("sizes");
        let mut sizes = Sizes::default();
        sizes.keep(kept.clone());
        assert_eq!(
            bytes(sizes_of(&sizes, std::slice::from_ref(&root))[0]),
            Some(1031)
        );
        drop(sizes);
        // A new file in a folder changes its times.
        fs::write(root.join("b/six"), vec![0u8; 100]).unwrap();
        let mut sizes = Sizes::default();
        sizes.keep(kept.clone());
        sizes.want(&root, PathBuf::from("/"));
        sizes.commit();
        assert_eq!(wait_for(&sizes, &root, 1131), Some(1131));
        let state = sizes.shared.state.lock().unwrap();
        let unread = state.tree.lookup(&root.join("a/deep/er")).unwrap();
        assert!(state.tree.nodes[unread as usize].listed);
        drop(state);
        drop(sizes);
        // A damaged file is ignored.
        fs::write(&kept, b"fff-sizes 1\0\x01").unwrap();
        assert!(Tree::load(&fs::read(&kept).unwrap()).is_none());
        fs::remove_file(&kept).unwrap();
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn names_of_removed_folders_are_dropped_in_time() {
        let mut tree = Tree::default();
        let kept = tree.ensure(Path::new("/keep/this")).unwrap();
        // Folders that come and go, as a build's or a test's do.
        let name = "n".repeat(1000);
        for _ in 0..3000 {
            let at = tree.add(0, OsStr::new(&name));
            tree.remove(at);
        }
        assert!(tree.names.len() < 2 << 20, "{}", tree.names.len());
        assert_eq!(tree.named, "keepthis".len());
        assert_eq!(tree.path(kept), Path::new("/keep/this"));
        assert_eq!(tree.lookup(Path::new("/keep/this")), Some(kept));
    }
    #[test]
    fn trees_round_trip() {
        let mut tree = Tree::default();
        let a = tree.ensure(Path::new("/x/a")).unwrap();
        let node = &mut tree.nodes[a as usize];
        node.listed = true;
        node.files = 5;
        node.links = vec![(1, 2, 3)].into();
        (node.stamp.mtime, node.stamp.mtime_nsec) = (-1, 7);
        let x = tree.lookup(Path::new("/x")).unwrap();
        tree.nodes[x as usize].listed = true;
        let back = Tree::load(&tree.save()).unwrap();
        let a = back.lookup(Path::new("/x/a")).unwrap() as usize;
        assert_eq!(back.nodes[a].files, 5);
        assert_eq!(*back.nodes[a].links, [(1, 2, 3)]);
        let stamp = back.nodes[a].stamp;
        assert_eq!((stamp.mtime, stamp.mtime_nsec), (-1, 7));
        assert_eq!(back.path(a as u32), Path::new("/x/a"));
        assert_eq!(std::mem::size_of::<Node>(), 128);
        assert!(back.nodes[a].listed);
        assert_eq!(back.path(a as u32), Path::new("/x/a"));
    }
    /// `FFF_SIZES_BENCH=/usr cargo test --release --lib sizes::tests::time -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn time_a_folder() {
        let path = PathBuf::from(std::env::var("FFF_SIZES_BENCH").unwrap());
        let kept = std::env::temp_dir().join("fff-sizes-bench");
        for run in ["walk", "saved"] {
            let mut sizes = Sizes::default();
            sizes.keep(kept.clone());
            let start = Instant::now();
            let found = sizes_of(&sizes, std::slice::from_ref(&path));
            eprintln!(
                "{run}: {} {found:?} in {:?}",
                path.display(),
                start.elapsed()
            );
            let start = Instant::now();
            drop(sizes);
            let length = fs::metadata(&kept).map_or(0, |m| m.len());
            eprintln!("saved in {:?}, {length} bytes", start.elapsed());
        }
        let start = Instant::now();
        let tree = Tree::load(&fs::read(&kept).unwrap()).unwrap();
        eprintln!("{} folders read in {:?}", tree.live(), start.elapsed());
        let _ = fs::remove_file(kept);
    }
}
