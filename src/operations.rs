//! File operations. All mutations go through Engine.
//! Directory descriptors pin parents; publishing never replaces an entry.
use crate::platform::{self, Meta};
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, RenameFlags};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Read},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};
#[path = "undo.rs"]
pub mod undo;
use crate::trash::{Bin, Trash};

pub type Result<T> = std::result::Result<T, String>;
pub(crate) fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
static NEXT: AtomicU64 = AtomicU64::new(0);
pub(crate) fn unique() -> String {
    format!(
        "{}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}
pub(crate) fn open_dir(path: &Path) -> Result<File> {
    let path = path.canonicalize().map_err(err)?;
    platform::open_dir_nofollow(&path).map_err(err)
}
/// The names in the opened directory `dir`.
pub(crate) fn names(dir: &File) -> Result<Vec<OsString>> {
    Ok(platform::read_dir(dir)
        .map_err(err)?
        .into_iter()
        .map(|e| e.name)
        .collect())
}
pub(crate) fn child_dir(parent: &File, name: &OsStr) -> Result<File> {
    sys::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map(File::from)
    .map_err(err)
}
fn new_file(parent: &File, name: &OsStr) -> Result<File> {
    sys::openat(
        parent,
        name,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map(File::from)
    .map_err(err)
}
fn exists(parent: &File, name: &OsStr) -> Result<bool> {
    match sys::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => Ok(true),
        Err(rustix::io::Errno::NOENT) => Ok(false),
        Err(e) => Err(err(e)),
    }
}
fn rename(a: &File, name: &OsStr, b: &File, to: &OsStr) -> Result<()> {
    sys::renameat_with(a, name, b, to, RenameFlags::NOREPLACE).map_err(err)
}
/// `rename` into another directory. Linux moves a directory to a new parent
/// only if the directory itself is writable (its `..` changes), so one without
/// owner write permission gains it for the rename alone, as `cp` and `rm` allow.
pub(crate) fn reparent(a: &File, name: &OsStr, b: &File, to: &OsStr) -> Result<()> {
    let locked = child_dir(a, name).ok().and_then(|dir| {
        let mode = dir.metadata().ok()?.mode() & 0o7777;
        (mode & 0o200 == 0
            && dir
                .set_permissions(fs::Permissions::from_mode(mode | 0o200))
                .is_ok())
        .then_some((dir, mode))
    });
    let result = rename(a, name, b, to);
    if let Some((dir, mode)) = locked {
        let _ = dir.set_permissions(fs::Permissions::from_mode(mode));
    }
    result
}
/// Where an opened directory is now, without symlinks.
pub(crate) fn canonical_dir(dir: &File) -> Result<PathBuf> {
    platform::dir_path(dir).map_err(err)
}
struct Entry {
    parent: File,
    name: OsString,
    path: PathBuf,
}
impl Entry {
    fn at(path: &Path) -> Result<Self> {
        if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
            return Err("Use an absolute path without '..'".into());
        }
        let name = path
            .file_name()
            .ok_or("Cannot operate on a filesystem root")?
            .to_owned();
        let parent_path = path
            .parent()
            .ok_or("Missing parent directory")?
            .canonicalize()
            .map_err(err)?;
        Ok(Self {
            parent: open_dir(&parent_path)?,
            path: parent_path.join(&name),
            name,
        })
    }
}
/// One entry of an inventoried tree, as it was: its identity, size, times
/// (seconds and nanoseconds) and mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    dev: u64,
    ino: u64,
    len: u64,
    mtime: (i64, i64),
    ctime: (i64, i64),
    mode: u32,
}
impl Stamp {
    /// The entry's identity, as `identity` gives it.
    fn id(&self) -> (u64, u64) {
        (self.dev, self.ino)
    }
    fn is_dir(&self) -> bool {
        self.mode & 0o170000 == 0o040000
    }
    fn is_file(&self) -> bool {
        self.mode & 0o170000 == 0o100000
    }
}
fn stamp(m: &Meta) -> Stamp {
    Stamp {
        dev: m.dev,
        ino: m.ino,
        len: m.len,
        mtime: (m.mtime, m.mtime_nsec),
        ctime: (m.ctime, m.ctime_nsec),
        mode: m.mode,
    }
}
fn inventory(
    parent: &File,
    name: &OsStr,
    relative: &Path,
    out: &mut BTreeMap<PathBuf, Stamp>,
    depth: usize,
) -> Result<u64> {
    if depth > 128 || out.len() >= 100_000 {
        return Err("Directory tree is too large or deep".into());
    }
    let m = platform::lstat_at(parent, name).map_err(err)?;
    // Never copy or delete through anything mounted inside a tree, including
    // bind mounts from the same filesystem, which keep the device number.
    if depth > 0
        && (out.get(Path::new("")).is_some_and(|root| root.dev != m.dev)
            || mount_root(parent, name)?)
    {
        return Err(format!(
            "Tree contains a mount point or subvolume: {relative:?}"
        ));
    }
    out.insert(relative.to_owned(), stamp(&m));
    #[cfg(test)]
    slow();
    if m.is_dir() {
        let dir = child_dir(parent, name)?;
        let mut total = 0u64;
        for name in names(&dir)? {
            total = total
                .checked_add(inventory(
                    &dir,
                    &name,
                    &relative.join(&name),
                    out,
                    depth + 1,
                )?)
                .ok_or("Size overflow")?;
        }
        Ok(total)
    } else if m.is_file() || m.is_symlink() {
        Ok(m.len)
    } else {
        Err("Sockets, devices and special files are not supported".into())
    }
}
// Renaming changes the root inode's ctime, but must not change its identity,
// contents, permissions, mtime, or any descendant's metadata.
fn detached_matches(
    parent: &File,
    name: &OsStr,
    expected: &BTreeMap<PathBuf, Stamp>,
) -> Result<bool> {
    let mut current = BTreeMap::new();
    inventory(parent, name, Path::new(""), &mut current, 0)?;
    if let (Some(now), Some(was)) = (current.get_mut(Path::new("")), expected.get(Path::new(""))) {
        now.ctime = was.ctime;
    }
    Ok(&current == expected)
}
/// Whether `name` is the root of a mount.
fn mount_root(parent: &File, name: &OsStr) -> Result<bool> {
    platform::is_mount_root(parent, name).map_err(err)
}
/// Device and inode of `name`, without following a final symlink.
fn identity(parent: &File, name: &OsStr) -> Option<(u64, u64)> {
    let m = platform::lstat_at(parent, name).ok()?;
    Some((m.dev, m.ino))
}
/// Every directory from `dir` up to the filesystem root, by identity, so
/// aliases (bind mounts, case-insensitive names) cannot hide an ancestor.
/// Parents are only stat'ed, so one that cannot be listed (mode 0711) counts.
fn ancestors(dir: &File) -> Result<HashSet<(u64, u64)>> {
    let mut seen = HashSet::new();
    let mut current = dir.try_clone().map_err(err)?;
    for _ in 0..4096 {
        let m = current.metadata().map_err(err)?;
        if !seen.insert((m.dev(), m.ino())) {
            break;
        }
        current = platform::open_parent(&current).map_err(err)?;
    }
    Ok(seen)
}
/// A confirmed tree: every entry's identity and metadata when it was shown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot(BTreeMap<PathBuf, Stamp>);
/// The snapshot and cumulative size of `path`, for a confirmation listing.
fn snapshot(path: &Path) -> Result<(Snapshot, u64)> {
    let e = Entry::at(path)?;
    let mut out = BTreeMap::new();
    let size = inventory(&e.parent, &e.name, Path::new(""), &mut out, 0)?;
    Ok((Snapshot(out), size))
}
fn tree(entry: &Entry) -> Result<BTreeMap<PathBuf, Stamp>> {
    let mut out = BTreeMap::new();
    inventory(&entry.parent, &entry.name, Path::new(""), &mut out, 0)?;
    Ok(out)
}
/// Read-only preflight: reject unsupported/corrupt/unsafe archives before
/// prompting, backing up or staging. Do not trust a filename extension.
pub fn validate_archive(path: &Path) -> Result<()> {
    let source = Entry::at(path)?;
    let input = File::from(
        sys::openat(
            &source.parent,
            &source.name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(err)?,
    );
    let before = platform::fstat(&input).map_err(err)?;
    if !before.is_file() {
        return Err("Select a regular archive file".into());
    }
    let check = input.try_clone().map_err(err)?;
    crate::archive::validate(input)?;
    if stamp(&before) != stamp(&platform::fstat(&check).map_err(err)?) {
        return Err("Archive changed while being checked".into());
    }
    Ok(())
}
/// Cumulative logical bytes; links count their own length, never their targets.
pub fn weight(path: &Path) -> Result<u64> {
    let e = Entry::at(path)?;
    inventory(&e.parent, &e.name, Path::new(""), &mut BTreeMap::new(), 0)
}
// Copies keep access and modification times; the copy's ctime is necessarily new.
fn keep_times(dir: &File, name: &OsStr, m: &Meta) -> Result<()> {
    let times = sys::Timestamps {
        last_access: sys::Timespec {
            tv_sec: m.atime,
            tv_nsec: m.atime_nsec as _,
        },
        last_modification: sys::Timespec {
            tv_sec: m.mtime,
            tv_nsec: m.mtime_nsec as _,
        },
    };
    sys::utimensat(dir, name, &times, AtFlags::SYMLINK_NOFOLLOW).map_err(err)
}
/// `tick` hears of every chunk copied; an error from it (Ctrl+c) stops.
fn copy_tree(src: &File, name: &OsStr, dst: &File, to: &OsStr, tick: &Tick) -> Result<()> {
    copy_entry(src, name, dst, to, false, tick)
}
type Tick<'a> = dyn Fn(u64) -> Result<()> + 'a;
/// Files are copied in pieces this large, between progress reports.
const CHUNK: u64 = 8 << 20;
fn copy_entry(
    src: &File,
    name: &OsStr,
    dst: &File,
    to: &OsStr,
    nested: bool,
    tick: &Tick,
) -> Result<()> {
    let m = platform::lstat_at(src, name).map_err(err)?;
    if nested && mount_root(src, name)? {
        return Err("Tree contains a mount point".into());
    }
    if m.is_symlink() {
        let target = sys::readlinkat(src, name, Vec::new()).map_err(err)?;
        sys::symlinkat(target.as_c_str(), dst, to).map_err(err)?;
        keep_times(dst, to, &m)?;
    } else if m.is_dir() {
        let input = child_dir(src, name)?;
        if stamp(&platform::fstat(&input).map_err(err)?) != stamp(&m) {
            return Err("Source directory changed".into());
        }
        sys::mkdirat(dst, to, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(err)?;
        let output = child_dir(dst, to)?;
        for name in names(&input)? {
            copy_entry(&input, &name, &output, &name, true, tick)?;
        }
        output
            .set_permissions(fs::Permissions::from_mode(m.mode & 0o777))
            .map_err(err)?;
        keep_times(dst, to, &m)?;
        platform::sync(&output).map_err(err)?;
    } else if m.is_file() {
        let input = File::from(
            sys::openat(
                src,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(err)?,
        );
        if stamp(&platform::fstat(&input).map_err(err)?) != stamp(&m) {
            return Err("Source changed while copying".into());
        }
        let mut output = new_file(dst, to)?;
        // Pieces keep the kernel's fast copy and still report progress.
        loop {
            let n = io::copy(&mut (&input).take(CHUNK), &mut output).map_err(err)?;
            if n == 0 {
                break;
            }
            tick(n)?;
        }
        output
            .set_permissions(fs::Permissions::from_mode(m.mode & 0o777))
            .map_err(err)?;
        keep_times(dst, to, &m)?;
        platform::sync(&output).map_err(err)?;
        if stamp(&platform::fstat(&input).map_err(err)?) != stamp(&m) {
            return Err("Source changed while copying".into());
        }
    } else {
        return Err("Special files cannot be copied".into());
    }
    Ok(())
}
#[derive(Clone, Debug)]
pub enum Request {
    /// `replace` lists confirmed existing destinations to overwrite; any other
    /// collision aborts.
    ///
    /// `confirmed` holds the snapshots shown for confirmation (sources, then
    /// `replace`); when present, execution refuses trees that differ from them,
    /// so a confirmation applies to those files, not merely to their paths.
    Copy {
        sources: Vec<PathBuf>,
        directory: PathBuf,
        replace: Vec<PathBuf>,
        confirmed: Vec<Snapshot>,
        /// Device and inode of `directory` when confirmed.
        into: Option<(u64, u64)>,
    },
    Move {
        sources: Vec<PathBuf>,
        directory: PathBuf,
        replace: Vec<PathBuf>,
        confirmed: Vec<Snapshot>,
        /// Device and inode of `directory` when confirmed.
        into: Option<(u64, u64)>,
    },
    Symlink {
        sources: Vec<PathBuf>,
        directory: PathBuf,
        confirmed: Vec<Snapshot>,
        into: Option<(u64, u64)>,
    },
    Rename {
        sources: Vec<PathBuf>,
        targets: Vec<PathBuf>,
    },
    /// Each source with `trash[i]` set goes to the trash; the others (all of
    /// them when `trash` is empty) are deleted permanently.
    Delete {
        sources: Vec<PathBuf>,
        confirmed: Vec<Snapshot>,
        trash: Vec<bool>,
    },
    Create {
        path: PathBuf,
        directory: bool,
    },
    /// Entries at the top of a trash back to `targets` (where their records
    /// say they came from, or a free name beside it), overwriting the
    /// confirmed `replace`; their records go once they are back.
    Restore {
        sources: Vec<PathBuf>,
        targets: Vec<PathBuf>,
        replace: Vec<PathBuf>,
        confirmed: Vec<Snapshot>,
    },
    /// `destination: None` extracts each archive beside itself.
    Extract {
        archives: Vec<PathBuf>,
        destination: Option<PathBuf>,
    },
}
impl Request {
    /// A deletion moving every source to the trash.
    pub fn trashes_all(&self) -> bool {
        matches!(self, Request::Delete { trash, .. } if !trash.is_empty() && trash.iter().all(|&t| t))
    }
}
#[derive(Debug)]
pub struct Outcome {
    pub changed: bool,
    /// Set when the operation took effect but did not complete cleanly, such
    /// as a deletion stopped partway; `moved`/`removed` still list what happened.
    pub warning: Option<String>,
    pub moved: Vec<(PathBuf, PathBuf)>,
    pub removed: Vec<PathBuf>,
    /// How to undo what happened, in the order it happened.
    pub undo: Vec<undo::Step>,
    /// Entries moved to the trash, and entries deleted for good (including
    /// overwritten ones).
    pub trashed: usize,
    pub lost: usize,
}
impl Outcome {
    fn new(changed: bool) -> Self {
        Self {
            changed,
            warning: None,
            moved: vec![],
            removed: vec![],
            undo: vec![],
            trashed: 0,
            lost: 0,
        }
    }
}
/// An operation's progress, shared with the interface while it runs on
/// another thread.
#[derive(Debug, Default)]
pub struct Progress {
    /// Bytes copied or extracted, or entries deleted.
    pub done: AtomicU64,
    /// What `done` counts up to; 0 when unknown.
    pub total: AtomicU64,
    /// Ctrl+c asked the operation to stop.
    pub cancel: AtomicBool,
    /// Stopping now changes nothing: the trees are still being checked, or
    /// copies and extractions staged. A deletion stops between entries and
    /// puts the rest back.
    pub stoppable: AtomicBool,
    /// The chosen trees are being checked; nothing has changed yet.
    pub checking: AtomicBool,
}
/// Tests that need operations to overlap slow every step down by this many
/// microseconds.
#[cfg(test)]
pub(crate) static SLOW_MICROS: AtomicU64 = AtomicU64::new(0);
#[cfg(test)]
fn slow() {
    let micros = SLOW_MICROS.load(Ordering::Relaxed);
    if micros > 0 {
        std::thread::sleep(std::time::Duration::from_micros(micros));
    }
}
/// The error of an operation stopped with Ctrl+c.
pub const CANCELLED: &str = "Stopped";
#[derive(Default)]
pub struct Engine {
    #[cfg(test)]
    hook: Option<Box<dyn Fn(usize) + Send>>,
    /// Where `Request::Delete` puts entries marked for the trash, and
    /// overwritten entries; without it they are deleted permanently.
    pub trash: Option<Trash>,
    pub progress: Option<Arc<Progress>>,
}
impl Engine {
    pub fn new(trash: Option<Trash>, progress: Option<Arc<Progress>>) -> Self {
        Self {
            #[cfg(test)]
            hook: None,
            trash,
            progress,
        }
    }
    fn checkpoint(&self, _index: usize) {
        #[cfg(test)]
        slow();
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            hook(_index);
        }
    }
    /// Counts `n` more units done; an error once Ctrl+c asked to stop, while
    /// stopping is allowed.
    fn tick(&self, n: u64) -> Result<()> {
        #[cfg(test)]
        slow();
        let Some(p) = &self.progress else {
            return Ok(());
        };
        p.done.fetch_add(n, Ordering::Relaxed);
        if p.stoppable.load(Ordering::Relaxed) && p.cancel.load(Ordering::Relaxed) {
            return Err(CANCELLED.into());
        }
        Ok(())
    }
    /// Starts a phase counting up to `total` (0: unknown).
    fn phase(&self, total: u64, stoppable: bool) {
        if let Some(p) = &self.progress {
            p.done.store(0, Ordering::Relaxed);
            p.total.store(total, Ordering::Relaxed);
            p.stoppable.store(stoppable, Ordering::Relaxed);
            p.checking.store(false, Ordering::Relaxed);
        }
    }
    /// Checking ends and acting begins, unless Ctrl+c asked to stop first.
    fn act(&self) -> Result<()> {
        if let Some(p) = &self.progress {
            if p.cancel.load(Ordering::Relaxed) {
                return Err(CANCELLED.into());
            }
            p.checking.store(false, Ordering::Relaxed);
            p.stoppable.store(false, Ordering::Relaxed);
        }
        Ok(())
    }
    /// From here on, stopping would leave the operation half done.
    fn settle(&self) {
        if let Some(p) = &self.progress {
            p.stoppable.store(false, Ordering::Relaxed);
        }
    }
    /// The bin for `entry`, refusing entries in a bin or containing one.
    fn bin(&self, entry: &Entry) -> Result<Bin> {
        let trash = self.trash.as_ref().ok_or("The trash is off")?;
        let bin = trash.bin_for(&entry.parent, entry.path.parent().unwrap_or(&entry.path))?;
        if entry.path.starts_with(&bin.path) {
            return Err("already in the trash".into());
        }
        if bin.path.starts_with(&entry.path) {
            return Err("contains the trash folder".into());
        }
        Ok(bin)
    }
    pub fn execute(&self, request: Request) -> Result<Outcome> {
        // Closing the terminal or a termination signal waits until this
        // operation has finished or rolled back.
        let _deferred = interrupt::Deferred::new();
        let Some(prepared) = self.prepare(&request)? else {
            return Ok(Outcome::new(false));
        };
        self.act()?;
        match request {
            Request::Move { .. } => self.move_entries(prepared, false),
            Request::Rename { .. } => self.move_entries(prepared, true),
            Request::Copy { .. } => self.copy_entries(prepared, false),
            Request::Symlink { .. } => self.copy_entries(prepared, true),
            Request::Delete { trash, .. } => self.delete_entries(prepared, &trash),
            Request::Restore { .. } => self.restore_entries(prepared),
            Request::Create { directory, .. } => self.create_entry(prepared, directory),
            Request::Extract { .. } => self.extract(prepared),
        }
    }
    /// Opens and checks every entry the request names; nothing changes yet.
    /// `None` for a rename that changes no name.
    fn prepare(&self, request: &Request) -> Result<Option<Prepared>> {
        // Until every tree is checked, nothing changes, so Ctrl+c may stop.
        if let Some(p) = &self.progress {
            p.checking.store(true, Ordering::Relaxed);
            p.stoppable.store(true, Ordering::Relaxed);
        }
        let (mut sources, mut targets) = match request {
            Request::Copy {
                sources, directory, ..
            }
            | Request::Move {
                sources, directory, ..
            }
            | Request::Symlink {
                sources, directory, ..
            } => (
                sources.clone(),
                sources
                    .iter()
                    .map(|p| {
                        p.file_name()
                            .map(|n| directory.join(n))
                            .ok_or_else(|| "Invalid source".to_string())
                    })
                    .collect::<Result<Vec<_>>>()?,
            ),
            Request::Rename { sources, targets }
            | Request::Restore {
                sources, targets, ..
            } => (sources.clone(), targets.clone()),
            Request::Delete { sources, .. } => (sources.clone(), vec![]),
            Request::Create { path, .. } => (vec![], vec![path.clone()]),
            Request::Extract {
                archives,
                destination,
            } => {
                let mut targets: Vec<PathBuf> = destination.iter().cloned().collect();
                if destination.is_none() {
                    // Beside each archive: its folder itself, even when the
                    // path to it runs through a symlink.
                    for archive in archives {
                        let parent = archive.parent().ok_or("Archive has no parent")?;
                        let parent = parent.canonicalize().map_err(err)?;
                        if !targets.contains(&parent) {
                            targets.push(parent);
                        }
                    }
                }
                (archives.clone(), targets)
            }
        };
        if sources.is_empty() && !matches!(request, Request::Create { .. }) {
            return Err("No files selected".into());
        }
        if matches!(request, Request::Rename { .. }) {
            if sources.len() != targets.len() {
                return Err("Rename row count changed".into());
            }
            if sources
                .iter()
                .zip(&targets)
                .any(|(s, t)| s.parent() != t.parent())
            {
                return Err("Rename cannot change a parent directory".into());
            }
            // Unchanged rows take no part: an unchanged directory may contain
            // renamed rows, and needs no inventory.
            (sources, targets) = sources
                .into_iter()
                .zip(targets)
                .filter(|(s, t)| s != t)
                .unzip();
            // Rows may be nested. Renaming parents first is safe because every
            // entry keeps its parent pinned by descriptor, not by path.
            let mut pairs = sources.into_iter().zip(targets).collect::<Vec<_>>();
            pairs.sort_by_key(|(s, _)| s.components().count());
            (sources, targets) = pairs.into_iter().unzip();
            if sources.is_empty() {
                return Ok(None);
            }
        }
        let src = sources
            .iter()
            .map(|p| Entry::at(p))
            .collect::<Result<Vec<_>>>()?;
        let dst = targets
            .iter()
            .map(|p| Entry::at(p))
            .collect::<Result<Vec<_>>>()?;
        let old = match &request {
            Request::Move { replace, .. }
            | Request::Copy { replace, .. }
            | Request::Restore { replace, .. } => replace
                .iter()
                .map(|p| Entry::at(p))
                .collect::<Result<Vec<_>>>()?,
            _ => vec![],
        };
        // Paths are compared through their ancestors, not pair by pair, so
        // that thousands of selected entries are checked quickly.
        let destinations: HashSet<&Path> = dst.iter().map(|d| d.path.as_path()).collect();
        let overwritten: HashSet<&Path> = old.iter().map(|o| o.path.as_path()).collect();
        for o in &old {
            if !destinations.contains(o.path.as_path()) {
                return Err(format!("Not a move destination: {:?}", o.path));
            }
        }
        if let Some(o) = src
            .iter()
            .find_map(|s| s.path.ancestors().find(|a| overwritten.contains(a)))
        {
            return Err(format!(
                "Cannot overwrite {o:?}: it is or contains a selected path"
            ));
        }
        let renaming = matches!(request, Request::Rename { .. });
        let mut selected = HashSet::new();
        for s in &src {
            if !selected.insert(s.path.as_path()) {
                return Err("The same path is selected twice".into());
            }
        }
        // Renames keep their parents, so nested rows cannot collide.
        if !renaming
            && src
                .iter()
                .any(|s| s.path.ancestors().skip(1).any(|a| selected.contains(a)))
        {
            return Err("Overlapping selections; select each tree only once".into());
        }
        let mut seen = HashSet::new();
        for (i, d) in dst.iter().enumerate() {
            if !seen.insert(d.path.clone()) {
                return Err("Duplicate destination".into());
            }
            let existing = exists(&d.parent, &d.name)?;
            match request {
                Request::Extract { .. } => {
                    if !existing {
                        check_name(&d.name)?;
                    }
                }
                _ if existing && !overwritten.contains(d.path.as_path()) => {
                    return Err(format!("Destination already exists: {:?}", d.path));
                }
                // Copies, moves and links keep names that already exist.
                Request::Rename { .. } | Request::Create { .. } => check_name(&d.name)?,
                _ => {}
            }
            if matches!(request, Request::Rename { .. }) && src[i].path.parent() != d.path.parent()
            {
                return Err("Rename cannot change a parent directory".into());
            }
            // A rename target inside a renamed ancestor stays in its pinned parent.
            // Exchanges and cycles are refused above as existing destinations.
            if !renaming && d.path.ancestors().any(|a| selected.contains(a)) {
                return Err("Destination is inside a source".into());
            }
        }
        let before = src.iter().map(tree).collect::<Result<Vec<_>>>()?;
        let replaced = old.iter().map(tree).collect::<Result<Vec<_>>>()?;
        let confirmed = match &request {
            Request::Copy { confirmed, .. }
            | Request::Move { confirmed, .. }
            | Request::Symlink { confirmed, .. }
            | Request::Restore { confirmed, .. }
            | Request::Delete { confirmed, .. } => confirmed.as_slice(),
            _ => &[],
        };
        if !confirmed.is_empty()
            && !confirmed
                .iter()
                .map(|s| &s.0)
                .eq(before.iter().chain(&replaced))
        {
            return Err("Files changed since confirmation; review again".into());
        }
        let into = match &request {
            Request::Copy { into, .. }
            | Request::Move { into, .. }
            | Request::Symlink { into, .. } => *into,
            _ => None,
        };
        if let Some(into) = into {
            let m = dst[0].parent.metadata().map_err(err)?;
            if (m.dev(), m.ino()) != into {
                return Err("The destination directory changed since confirmation".into());
            }
        }
        // Compare identities as well as spellings: an alias of a source
        // directory (a bind mount, another letter case) is still that source.
        if matches!(request, Request::Copy { .. } | Request::Move { .. }) {
            let above = ancestors(&dst[0].parent)?;
            if before
                .iter()
                .filter_map(|t| t.get(Path::new("")))
                .any(|root| root.is_dir() && above.contains(&root.id()))
            {
                return Err("Destination is inside a source".into());
            }
        }
        if matches!(
            request,
            Request::Move { .. } | Request::Rename { .. } | Request::Restore { .. }
        ) && src.iter().zip(&dst).any(|(s, d)| {
            s.parent.metadata().map(|m| m.dev()).ok() != d.parent.metadata().map(|m| m.dev()).ok()
        }) {
            return Err("Moving across filesystems is not supported; copy first".into());
        }
        // Last look before acting: every source and overwritten tree is
        // still exactly as inventoried (and as confirmed, when confirmed).
        self.checkpoint(usize::MAX);
        for (s, was) in src.iter().chain(&old).zip(before.iter().chain(&replaced)) {
            if tree(s)? != *was {
                return Err("Source changed; operation cancelled".into());
            }
        }
        Ok(Some(Prepared {
            sources,
            targets,
            src,
            dst,
            old,
            before,
            replaced,
        }))
    }
    /// Moves or renames every source to its target, all or none.
    fn move_entries(&self, prepared: Prepared, renaming: bool) -> Result<Outcome> {
        let Prepared {
            sources,
            targets,
            src,
            dst,
            old,
            before,
            replaced,
        } = prepared;
        let mut outcome = Outcome::new(true);
        let mut over = Overwrite::detach(&old, &replaced)?;
        let mut done: Vec<usize> = vec![];
        for (i, (s, d)) in src.iter().zip(&dst).enumerate() {
            if s.path == d.path {
                continue;
            }
            self.checkpoint(i);
            let result = tree(s).and_then(|now| {
                if now == before[i] {
                    reparent(&s.parent, &s.name, &d.parent, &d.name)
                } else {
                    Err("Source changed before move".into())
                }
            });
            if let Err(e) = result {
                let mut left = vec![];
                for &j in done.iter().rev() {
                    let a: &Entry = &src[j];
                    let b: &Entry = &dst[j];
                    // Only move back the entry this operation published.
                    let root = &before[j][Path::new("")];
                    if !(identity(&b.parent, &b.name) == Some(root.id())
                        && reparent(&b.parent, &b.name, &a.parent, &a.name).is_ok())
                    {
                        left.push(b.path.as_path());
                    }
                }
                let restored = over.restore();
                return Err(format!(
                    "{e}; {}",
                    if left.is_empty() && restored {
                        "earlier moves rolled back".to_string()
                    } else {
                        format!("rollback incomplete; {}", over.left_behind(&left))
                    }
                ));
            }
            done.push(i);
        }
        over.finish(self, &mut outcome);
        let names = rename_map(&sources, &targets);
        for (i, (s, d)) in src.iter().zip(&dst).enumerate() {
            if s.path == d.path {
                continue;
            }
            let to = if renaming {
                renamed(&sources[i], &names)
            } else {
                d.path.clone()
            };
            // Undone deepest first, a renamed entry goes back to
            // its old name in its (still renamed) parent.
            let back = if renaming {
                to.with_file_name(&s.name)
            } else {
                s.path.clone()
            };
            let root = &before[i][Path::new("")];
            outcome.undo.push(undo::Step::Return {
                now: to.clone(),
                back,
                id: root.id(),
            });
            outcome.moved.push((sources[i].clone(), to));
        }
        Ok(outcome)
    }
    /// Moves entries out of the trash to their targets as a move does, then
    /// drops their records; undoing puts each back with its record.
    fn restore_entries(&self, prepared: Prepared) -> Result<Outcome> {
        let trash = self.trash.as_ref().ok_or("The trash is off")?;
        let mut records = vec![];
        for s in &prepared.src {
            if !trash.holds(&s.path) {
                return Err(format!("{:?} is not at the top of a trash", s.name));
            }
            let bin = Bin::open(crate::trash::bin_of(&s.path).unwrap_or(&s.path), false)?;
            let (record, text) = bin
                .record_of(&s.name)
                .ok_or_else(|| format!("{:?} has no record of where it came from", s.name))?;
            records.push((bin, record, text));
        }
        let sources = prepared
            .src
            .iter()
            .map(|s| s.path.clone())
            .collect::<Vec<_>>();
        let mut outcome = self.move_entries(prepared, false)?;
        for step in &mut outcome.undo {
            let undo::Step::Return { now, back, id } = step else {
                continue;
            };
            let Some(i) = sources.iter().position(|s| s == back) else {
                continue;
            };
            let (bin, record, text) = &records[i];
            bin.forget(record);
            *step = undo::Step::Retrash {
                now: now.clone(),
                bin: bin.path.clone(),
                slot: back.file_name().unwrap_or_default().to_owned(),
                record: text.clone(),
                id: *id,
            };
        }
        Ok(outcome)
    }
    /// Copies (or links) every source into a stage, then publishes them all,
    /// replacing confirmed destinations.
    fn copy_entries(&self, prepared: Prepared, linking: bool) -> Result<Outcome> {
        let Prepared {
            src,
            dst,
            old,
            before,
            replaced,
            ..
        } = prepared;
        let mut outcome = Outcome::new(true);
        // Stage all entries before publishing any. Never follow an
        // existing destination or replace one created concurrently.
        let stage = Stage::new(&dst[0].parent, &dst[0].path)?;
        let bytes = before
            .iter()
            .flat_map(|t| t.values())
            .filter(|s| s.is_file())
            .fold(0u64, |n, s| n.saturating_add(s.len));
        // Until everything is staged, stopping changes nothing.
        self.phase(bytes, true);
        let mut staged = vec![];
        for (i, s) in src.iter().enumerate() {
            let name = format!("{i}");
            if linking {
                sys::symlinkat(&s.path, &stage.dir, &name).map_err(err)?;
            } else {
                copy_tree(&s.parent, &s.name, &stage.dir, OsStr::new(&name), &|n| {
                    self.tick(n)
                })?;
            }
            staged.push(identity(&stage.dir, OsStr::new(&name)));
        }
        self.settle();
        for (source, was) in src.iter().zip(&before) {
            if tree(source)? != *was {
                return Err("Source changed while staging; nothing copied".into());
            }
        }
        // Staging is complete, so overwritten entries are absent only
        // while the copies are published.
        let mut over = Overwrite::detach(&old, &replaced)?;
        let mut done: Vec<usize> = vec![];
        for (i, d) in dst.iter().enumerate() {
            self.checkpoint(i);
            if let Err(e) = reparent(&stage.dir, OsStr::new(&i.to_string()), &d.parent, &d.name) {
                let mut left = vec![];
                for &j in done.iter().rev() {
                    let d: &Entry = &dst[j];
                    // Never pull back (and then delete) an entry that
                    // replaced the published copy concurrently.
                    if !(identity(&d.parent, &d.name) == staged[j]
                        && reparent(&d.parent, &d.name, &stage.dir, OsStr::new(&j.to_string()))
                            .is_ok())
                    {
                        left.push(d.path.as_path());
                    }
                }
                let restored = over.restore();
                return Err(format!(
                    "{e}; {}",
                    if left.is_empty() && restored {
                        "copies rolled back".to_string()
                    } else {
                        format!("rollback incomplete; {}", over.left_behind(&left))
                    }
                ));
            }
            done.push(i);
        }
        over.finish(self, &mut outcome);
        for (d, id) in dst.iter().zip(&staged) {
            if let Some(id) = *id {
                outcome.undo.push(undo::Step::Discard {
                    path: d.path.clone(),
                    id,
                });
            }
        }
        Ok(outcome)
    }
    /// Detaches every source, then trashes those marked in `trash` and
    /// deletes the others for good.
    fn delete_entries(&self, prepared: Prepared, trash: &[bool]) -> Result<Outcome> {
        let Prepared {
            sources,
            src,
            before,
            ..
        } = prepared;
        let mut outcome = Outcome::new(true);
        let trashing = |i: usize| trash.get(i).copied().unwrap_or(false);
        // Counted from the start: what deleting will remove.
        let entries = (0..src.len())
            .filter(|&i| !trashing(i))
            .map(|i| before[i].len() as u64)
            .sum();
        self.phase(entries, false);
        // Every bin is found before anything moves.
        let mut bins = vec![];
        for (i, s) in src.iter().enumerate() {
            bins.push(if trashing(i) {
                Some(
                    self.bin(s)
                        .map_err(|e| format!("{:?} cannot go to the trash: {e}", s.name))?,
                )
            } else {
                None
            });
        }
        // Detach all selected roots before deleting any contents;
        // a failure to detach rolls earlier roots back intact.
        let mut held: Vec<Hold> = vec![];
        for (i, s) in src.iter().enumerate() {
            self.checkpoint(i);
            let bin = bins[i].take();
            let result = tree(s).and_then(|now| {
                if now != before[i] {
                    return Err("Source changed before deletion".into());
                }
                Ok(match bin {
                    Some(bin) => {
                        let slot = bin.put(&s.parent, &s.name, &s.path)?;
                        Hold::Trash {
                            bin,
                            slot,
                            stranded: false,
                        }
                    }
                    None => {
                        let mut stage = Stage::new(&s.parent, &s.path)?;
                        reparent(&s.parent, &s.name, &stage.dir, OsStr::new("entry"))?;
                        stage.keep = true;
                        Hold::Stage(stage)
                    }
                })
            });
            match result {
                Ok(hold) => held.push(hold),
                Err(e) => {
                    for (j, hold) in held.iter_mut().enumerate().rev() {
                        hold.restore(&src[j]);
                    }
                    return Err(format!("{e}; {}", unrestored(&held)));
                }
            }
        }
        self.checkpoint(usize::MAX - 1);
        let validation: Result<()> = held.iter().enumerate().try_for_each(|(i, h)| {
            let (dir, name) = h.at();
            if detached_matches(dir, name, &before[i])? {
                Ok(())
            } else {
                Err("Source changed while preparing deletion".into())
            }
        });
        if let Err(error) = validation {
            for (i, hold) in held.iter_mut().enumerate().rev() {
                hold.restore(&src[i]);
            }
            return Err(format!("{error}; {}", unrestored(&held)));
        }
        // Trashed entries are done; undoing takes them back out.
        for (i, hold) in held.iter().enumerate() {
            if let Hold::Trash { bin, slot, .. } = hold {
                let root = &before[i][Path::new("")];
                outcome.undo.push(undo::Step::Restore {
                    bin: bin.path.clone(),
                    slot: slot.clone(),
                    back: src[i].path.clone(),
                    id: root.id(),
                });
                outcome.removed.push(sources[i].clone());
                outcome.trashed += 1;
            }
        }
        self.checkpoint(usize::MAX - 2);
        let permanent = (0..held.len())
            .filter(|&i| matches!(held[i], Hold::Stage(_)))
            .collect::<Vec<_>>();
        self.phase(entries, true);
        for (k, &i) in permanent.iter().enumerate() {
            let (dir, name) = held[i].at();
            let deleted = std::cell::Cell::new(0);
            let tick = |n| {
                deleted.set(deleted.get() + n);
                self.tick(n)
            };
            if let Err(e) = remove_counting(dir, name, &tick) {
                // Put back what remains of this root and every root not
                // yet deleted, so nothing is left hidden in staging.
                for &j in &permanent[k..] {
                    held[j].restore(&src[j]);
                }
                let rest = match permanent.len() - k - 1 {
                    0 => String::new(),
                    1 => ", and the entry after it,".into(),
                    n => format!(", and the {n} entries after it,"),
                };
                let left = permanent[k..].iter().map(|&j| &held[j]);
                let (what, it) = if deleted.get() > 0 {
                    ("was only partly deleted", "What remains of it")
                } else {
                    ("was not deleted", "It")
                };
                let back = if left.clone().all(|h| h.stranded().is_none()) {
                    format!("{it}{rest} is back in place.")
                } else {
                    format!("Could {}.", &unrestored(left)[6..])
                };
                outcome.warning = Some(if e == CANCELLED {
                    format!(
                        "Stopped after deleting {k} of {}; {:?} {what}. {back}",
                        permanent.len(),
                        sources[i],
                    )
                } else {
                    format!(
                        "Deleted {k} of {}; {:?} {what} ({e}). {back}",
                        permanent.len(),
                        sources[i],
                    )
                });
                return Ok(outcome);
            }
            if let Hold::Stage(stage) = &mut held[i] {
                stage.keep = false;
            }
            outcome.removed.push(sources[i].clone());
            outcome.lost += 1;
        }
        Ok(outcome)
    }
    /// A new empty file or directory.
    fn create_entry(&self, prepared: Prepared, directory: bool) -> Result<Outcome> {
        let Prepared { dst, .. } = prepared;
        let mut outcome = Outcome::new(true);
        let d = &dst[0];
        if directory {
            sys::mkdirat(&d.parent, &d.name, Mode::from_raw_mode(0o755)).map_err(err)?;
        } else {
            // Ordinary permissions, as the user's umask allows.
            platform::sync(&File::from(
                sys::openat(
                    &d.parent,
                    &d.name,
                    OFlags::WRONLY
                        | OFlags::CREATE
                        | OFlags::EXCL
                        | OFlags::NOFOLLOW
                        | OFlags::CLOEXEC,
                    Mode::from_raw_mode(0o666),
                )
                .map_err(err)?,
            ))
            .map_err(err)?;
        }
        if let Some(id) = identity(&d.parent, &d.name) {
            outcome.undo.push(undo::Step::Discard {
                path: d.path.clone(),
                id,
            });
        }
        Ok(outcome)
    }
    /// Extracts every archive into a stage per destination, then publishes
    /// what each holds.
    fn extract(&self, prepared: Prepared) -> Result<Outcome> {
        let Prepared {
            src, dst, before, ..
        } = prepared;
        let mut outcome = Outcome::new(true);
        // Until everything is staged, stopping changes nothing.
        self.phase(0, true);
        // One stage per destination holds every archive bound for it,
        // so entries colliding across archives abort before anything
        // is published.
        let mut groups = vec![];
        for (k, d) in dst.iter().enumerate() {
            // An existing destination hosts its stage, so its parent
            // need not be writable; otherwise the parent does.
            let existing = exists(&d.parent, &d.name)?;
            let dest = if existing {
                Some(child_dir(&d.parent, &d.name)?)
            } else {
                None
            };
            let stage = match &dest {
                Some(dest) => Stage::new(dest, &d.path.join("x"))?,
                None => Stage::new(&d.parent, &d.path)?,
            };
            for (i, s) in src.iter().enumerate() {
                // Without a common destination, each archive goes
                // to the target that is its own (resolved) folder.
                if dst.len() > 1 && s.path.parent() != Some(d.path.as_path()) {
                    continue;
                }
                // Non-blocking: a FIFO swapped in cannot hang the job.
                let input = File::from(
                    sys::openat(
                        &s.parent,
                        &s.name,
                        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                        Mode::empty(),
                    )
                    .map_err(err)?,
                );
                if !input.metadata().map_err(err)?.is_file() {
                    return Err(format!("{:?} is not a regular archive file", s.name));
                }
                crate::archive::extract(input, &stage.dir, &|n| self.tick(n)).map_err(|e| {
                    if e == CANCELLED {
                        e
                    } else {
                        format!("{:?}: {e}", s.name)
                    }
                })?;
                if tree(s)? != before[i] {
                    return Err(format!("{:?} changed during extraction", s.name));
                }
            }
            groups.push((k, existing, dest, stage));
        }
        self.settle();
        // Plan every publication and check every collision first.
        let mut plan = vec![];
        for (k, existing, dest, stage) in &groups {
            let d = &dst[*k];
            if exists(&d.parent, &d.name)? != *existing {
                return Err("Extraction destination changed".into());
            }
            if let Some(dest) = dest {
                for name in names(&stage.dir)? {
                    if exists(dest, &name)? {
                        return Err(format!("{:?} already contains {:?}", d.path, name));
                    }
                    let id = identity(&stage.dir, &name);
                    let path = d.path.join(&name);
                    plan.push((&stage.dir, name.clone(), dest, name, id, path));
                }
            } else {
                // The private stage becomes an ordinary new directory, as
                // the user's umask allows.
                stage
                    .dir
                    .set_permissions(fs::Permissions::from_mode(0o777 & !platform::umask()))
                    .map_err(err)?;
                let id = identity(&stage.parent, &stage.name);
                plan.push((
                    &stage.parent,
                    stage.name.clone(),
                    &d.parent,
                    d.name.clone(),
                    id,
                    d.path.clone(),
                ));
            }
        }
        let mut done: Vec<usize> = vec![];
        for (n, (from, name, to, as_name, _, _)) in plan.iter().enumerate() {
            if let Err(e) = rename(from, name, to, as_name) {
                let mut left = vec![];
                for &n in done.iter().rev() {
                    let (from, name, to, as_name, id, path) = &plan[n];
                    // Only take back what this extraction published.
                    if !(identity(to, as_name) == *id && rename(to, as_name, from, name).is_ok()) {
                        left.push(format!("{path:?}"));
                    }
                }
                if !left.is_empty() {
                    return Err(format!(
                        "{e}; extraction rollback incomplete, leaving {}",
                        left.join(", ")
                    ));
                }
                return Err(e);
            }
            done.push(n);
        }
        for (_, _, _, _, id, path) in plan {
            if let Some(id) = id {
                outcome.undo.push(undo::Step::Discard { path, id });
            }
        }
        Ok(outcome)
    }
}
/// A request's entries, opened and checked against what was confirmed.
struct Prepared {
    /// The paths as requested, for messages and undo.
    sources: Vec<PathBuf>,
    targets: Vec<PathBuf>,
    src: Vec<Entry>,
    dst: Vec<Entry>,
    /// Confirmed destinations to overwrite.
    old: Vec<Entry>,
    /// The trees of `src` and `old` when checked.
    before: Vec<BTreeMap<PathBuf, Stamp>>,
    replaced: Vec<BTreeMap<PathBuf, Stamp>>,
}
/// Where a delete root waits once detached: a private stage beside it, to
/// be deleted for good, or a slot in the trash.
enum Hold {
    Stage(Stage),
    Trash {
        bin: Bin,
        slot: OsString,
        /// It could not be moved back out of the trash.
        stranded: bool,
    },
}
impl Hold {
    fn at(&self) -> (&File, &OsStr) {
        match self {
            Hold::Stage(stage) => (&stage.dir, OsStr::new("entry")),
            Hold::Trash { bin, slot, .. } => (&bin.files, slot),
        }
    }
    /// Moves the root back to `s`; false when it stays held.
    fn restore(&mut self, s: &Entry) -> bool {
        match self {
            Hold::Stage(stage) => {
                stage.keep = reparent(&stage.dir, OsStr::new("entry"), &s.parent, &s.name).is_err();
                !stage.keep
            }
            Hold::Trash {
                bin,
                slot,
                stranded,
            } => {
                *stranded = bin.take(slot, &s.parent, &s.name).is_err();
                if !*stranded {
                    bin.forget(slot);
                }
                !*stranded
            }
        }
    }
    /// Where the root remains when it could not be moved back.
    fn stranded(&self) -> Option<PathBuf> {
        match self {
            Hold::Stage(stage) => stage.keep.then(|| stage.path.join("entry")),
            Hold::Trash {
                bin,
                slot,
                stranded,
            } => stranded.then(|| bin.path.join("files").join(slot)),
        }
    }
}
/// After putting detached delete roots back: what, if anything, remains held.
fn unrestored<'a>(held: impl IntoIterator<Item = &'a Hold>) -> String {
    let stranded = held
        .into_iter()
        .filter_map(Hold::stranded)
        .map(|path| format!("{path:?}"))
        .collect::<Vec<_>>();
    if stranded.is_empty() {
        "nothing changed".into()
    } else {
        format!("could not restore {}", stranded.join(", "))
    }
}
/// Termination signals arriving during an operation are held until it ends,
/// then delivered with their default action, so no operation stops halfway.
mod interrupt {
    use std::sync::{
        Once,
        atomic::{AtomicI32, AtomicUsize, Ordering},
    };
    static BUSY: AtomicUsize = AtomicUsize::new(0);
    static PENDING: AtomicI32 = AtomicI32::new(0);
    extern "C" fn held(signal: libc::c_int) {
        // Record first, then check: an operation ending concurrently either
        // sees the record or leaves BUSY at zero for this handler to see.
        PENDING.store(signal, Ordering::SeqCst);
        if BUSY.load(Ordering::SeqCst) == 0 {
            deliver(signal);
        }
    }
    fn deliver(signal: libc::c_int) {
        // Async-signal-safe: restore the default action and re-raise.
        unsafe {
            libc::signal(signal, libc::SIG_DFL);
            libc::raise(signal);
        }
    }
    pub struct Deferred;
    impl Deferred {
        pub fn new() -> Self {
            static INSTALL: Once = Once::new();
            INSTALL.call_once(|| {
                for signal in [libc::SIGHUP, libc::SIGTERM, libc::SIGINT] {
                    unsafe {
                        let mut action: libc::sigaction = std::mem::zeroed();
                        action.sa_sigaction = held as *const () as usize;
                        action.sa_flags = libc::SA_RESTART;
                        libc::sigemptyset(&mut action.sa_mask);
                        let mut previous: libc::sigaction = std::mem::zeroed();
                        libc::sigaction(signal, &action, &mut previous);
                        // A signal the user chose to ignore (nohup) stays ignored.
                        if previous.sa_sigaction == libc::SIG_IGN {
                            libc::sigaction(signal, &previous, std::ptr::null_mut());
                        }
                    }
                }
            });
            BUSY.fetch_add(1, Ordering::SeqCst);
            Self
        }
    }
    impl Drop for Deferred {
        fn drop(&mut self) {
            if BUSY.fetch_sub(1, Ordering::SeqCst) == 1 {
                let signal = PENDING.swap(0, Ordering::SeqCst);
                if signal != 0 {
                    deliver(signal);
                }
            }
        }
    }
}
/// Confirmed overwrite targets, detached into private stages (one beside
/// each folder holding them) before publication and deleted only after every
/// entry has been published.
struct Overwrite<'a> {
    old: &'a [Entry],
    expected: &'a [BTreeMap<PathBuf, Stamp>],
    stages: Vec<Stage>,
    /// The stage each detached entry waits in, by index into `stages`.
    at: Vec<usize>,
}
impl<'a> Overwrite<'a> {
    fn detach(old: &'a [Entry], expected: &'a [BTreeMap<PathBuf, Stamp>]) -> Result<Self> {
        let mut over = Self {
            old,
            expected,
            stages: vec![],
            at: vec![],
        };
        let mut folders: Vec<(u64, u64)> = vec![];
        for (j, o) in old.iter().enumerate() {
            let result = o.parent.metadata().map_err(err).and_then(|m| {
                let folder = (m.dev(), m.ino());
                let k = match folders.iter().position(|&f| f == folder) {
                    Some(k) => k,
                    None => {
                        over.stages.push(Stage::new(&o.parent, &o.path)?);
                        folders.push(folder);
                        folders.len() - 1
                    }
                };
                if tree(o)? != expected[j] {
                    return Err("An entry to overwrite changed".into());
                }
                let stage = &mut over.stages[k];
                reparent(&o.parent, &o.name, &stage.dir, OsStr::new(&j.to_string()))?;
                stage.keep = true;
                Ok(k)
            });
            match result {
                Ok(k) => over.at.push(k),
                Err(e) => {
                    let restored = over.restore();
                    return Err(format!(
                        "{e}; {}",
                        if restored {
                            "nothing changed".to_string()
                        } else {
                            format!("restore incomplete; {}", over.left_behind(&[]))
                        }
                    ));
                }
            }
        }
        Ok(over)
    }
    /// Puts every detached entry back after a failed publication.
    fn restore(&mut self) -> bool {
        let mut restored = vec![true; self.stages.len()];
        for (j, o) in self.old.iter().enumerate().take(self.at.len()).rev() {
            let k = self.at[j];
            restored[k] &= reparent(
                &self.stages[k].dir,
                OsStr::new(&j.to_string()),
                &o.parent,
                &o.name,
            )
            .is_ok();
        }
        for (stage, restored) in self.stages.iter_mut().zip(&restored) {
            stage.keep = !restored;
        }
        restored.iter().all(|&r| r)
    }
    /// What an incomplete rollback left: the published entries it did not
    /// take back (`left`, each left in place as it is), and the stages
    /// still holding overwritten entries that could not be restored.
    fn left_behind(&self, left: &[&Path]) -> String {
        let mut parts = vec![];
        if !left.is_empty() {
            let left = left.iter().map(|p| format!("{p:?}")).collect::<Vec<_>>();
            parts.push(format!("left in place: {}", left.join(", ")));
        }
        let kept = self
            .stages
            .iter()
            .filter(|s| s.keep)
            .map(|s| format!("{:?}", s.path))
            .collect::<Vec<_>>();
        if !kept.is_empty() {
            parts.push(format!("overwritten entries remain in {}", kept.join(", ")));
        }
        parts.join("; ")
    }
    /// Once every new entry is published, moves the overwritten entries to
    /// the trash (deleting them when it is off or unavailable), recording how
    /// to undo that. An entry changed while detached is kept, never removed
    /// unconfirmed.
    fn finish(mut self, engine: &Engine, outcome: &mut Outcome) {
        let mut kept = vec![];
        let mut keep = vec![false; self.stages.len()];
        for (j, o) in self.old.iter().enumerate() {
            let stage = &self.stages[self.at[j]];
            let name = j.to_string();
            let name = OsStr::new(&name);
            let removed = detached_matches(&stage.dir, name, &self.expected[j]).and_then(|same| {
                if !same {
                    return Err("it changed while being overwritten".into());
                }
                match engine.trash.as_ref().and_then(|_| engine.bin(o).ok()) {
                    Some(bin) => {
                        let slot = bin.put(&stage.dir, name, &o.path)?;
                        let root = &self.expected[j][Path::new("")];
                        outcome.undo.push(undo::Step::Restore {
                            bin: bin.path.clone(),
                            slot,
                            back: o.path.clone(),
                            id: root.id(),
                        });
                        outcome.trashed += 1;
                    }
                    None => {
                        remove_tree(&stage.dir, name)?;
                        outcome.lost += 1;
                    }
                }
                Ok(())
            });
            if let Err(e) = removed {
                keep[self.at[j]] = true;
                kept.push(format!(
                    "{:?} ({e}) is kept as {:?}",
                    o.name,
                    stage.path.join(name)
                ));
            }
        }
        for (stage, keep) in self.stages.iter_mut().zip(keep) {
            stage.keep = keep;
        }
        if !kept.is_empty() {
            outcome.warning = Some(format!("Done, but {}", kept.join("; ")));
        }
    }
}
pub const STAGE_PREFIX: &str = ".fff-stage-";
/// Whether `path` is in (or is) a staging folder: a copy or extraction
/// still being written, or data an interrupted operation left behind.
pub fn staged(path: &Path) -> bool {
    path.components().any(|c| {
        c.as_os_str()
            .as_encoded_bytes()
            .starts_with(STAGE_PREFIX.as_bytes())
    })
}
struct Stage {
    parent: File,
    dir: File,
    name: OsString,
    /// For messages: the stage beside `sibling`, a path in the same directory.
    path: PathBuf,
    keep: bool,
}
impl Stage {
    fn new(parent: &File, sibling: &Path) -> Result<Self> {
        let name = OsString::from(format!("{STAGE_PREFIX}{}", unique()));
        sys::mkdirat(parent, &name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(err)?;
        Ok(Self {
            parent: parent.try_clone().map_err(err)?,
            dir: child_dir(parent, &name)?,
            path: sibling.with_file_name(&name),
            name,
            keep: false,
        })
    }
}
/// Staging directories in `dir` left by an fff process that no longer runs,
/// such as one killed mid-operation. They may hold user data.
pub fn stranded(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return vec![];
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let Some(rest) = name.to_str().and_then(|n| n.strip_prefix(STAGE_PREFIX)) else {
                return false;
            };
            // Names start with the creating process id; skip operations still
            // running in a live fff (the same executable, despite pid reuse).
            let pid = rest.split('-').next().unwrap_or_default();
            let exe = platform::executable(pid);
            pid.is_empty() || exe.is_none() || exe != std::env::current_exe().ok()
        })
        .map(|e| e.path())
        .collect()
}
impl Drop for Stage {
    fn drop(&mut self) {
        if !self.keep {
            let _ = remove_tree(&self.parent, &self.name);
        }
    }
}
pub(crate) fn remove_tree(parent: &File, name: &OsStr) -> Result<()> {
    remove_counting(parent, name, &|_| Ok(()))
}
/// `remove_tree`, telling `tick` of every entry removed; an error from it
/// (Ctrl+c) stops with the rest in place.
fn remove_counting(parent: &File, name: &OsStr, tick: &Tick) -> Result<()> {
    let m = platform::lstat_at(parent, name).map_err(err)?;
    // Recheck while deleting: nothing mounted since validation is entered.
    if mount_root(parent, name)? {
        return Err(format!("{name:?} is a mount point"));
    }
    if m.is_dir() {
        let dir = child_dir(parent, name)?;
        // Read-only directories must be writable to empty them; if deletion
        // then fails, the directory gets its exact mode back.
        let original = m.mode & 0o7777;
        let unlocked = original & 0o700 != 0o700;
        if unlocked {
            dir.set_permissions(fs::Permissions::from_mode(original | 0o700))
                .map_err(err)?;
        }
        let emptied = (|| {
            for name in names(&dir)? {
                remove_counting(&dir, &name, tick)?;
            }
            Ok(())
        })();
        let removed = emptied.and_then(|()| {
            tick(0)?;
            sys::unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(err)
        });
        if let Err(e) = removed {
            if unlocked {
                let _ = dir.set_permissions(fs::Permissions::from_mode(original));
            }
            return Err(e);
        }
    } else {
        tick(0)?;
        sys::unlinkat(parent, name, AtFlags::empty()).map_err(err)?;
    }
    // A stop is heard before an unlink, never after one: an entry that is
    // gone must not be reported, and put back, as only partly deleted.
    let _ = tick(1);
    Ok(())
}
/// New names must be safe to see and type: one line of visible text that a
/// shell will not read as an option or a home directory.
pub fn check_name(name: &OsStr) -> Result<()> {
    let text = name.to_str().ok_or("Name is not valid UTF-8")?;
    if text.is_empty() {
        return Err("Name is empty".into());
    }
    if text.contains('\u{fffd}') {
        return Err(format!("Name contains invalid UTF-8: {text:?}"));
    }
    if text.chars().any(char::is_control) {
        return Err(format!(
            "Name contains a control character (tab, carriage return, escape...): {text:?}"
        ));
    }
    if text.starts_with(char::is_whitespace) || text.ends_with(char::is_whitespace) {
        return Err(format!("Name starts or ends with whitespace: {text:?}"));
    }
    if text.starts_with('-') {
        return Err(format!(
            "Name starts with '-', which commands read as an option: {text:?}"
        ));
    }
    if text.starts_with('~') {
        return Err(format!(
            "Name starts with '~', which shells expand to a home directory: {text:?}"
        ));
    }
    Ok(())
}
/// The buffer contains scope-relative original paths, never abbreviated labels.
pub fn rename_text(sources: &[PathBuf], cwd: &Path) -> Result<String> {
    sources
        .iter()
        .map(|p| {
            let text = p
                .strip_prefix(cwd)
                .unwrap_or(p)
                .to_str()
                .ok_or("Rename needs UTF-8 filenames")?;
            if text.contains(['\n', '\r']) {
                return Err("Rename cannot edit names containing newlines".into());
            }
            Ok(text.to_owned())
        })
        .collect::<Result<Vec<_>>>()
        .map(|v| v.join("\n"))
}
/// Each renamed source path with its new final name.
fn rename_map(sources: &[PathBuf], targets: &[PathBuf]) -> HashMap<PathBuf, OsString> {
    sources
        .iter()
        .zip(targets)
        .filter_map(|(s, t)| Some((s.clone(), t.file_name()?.to_owned())))
        .collect()
}
/// Where `path` ends up once every renamed ancestor (and itself) has its new name.
fn renamed(path: &Path, names: &HashMap<PathBuf, OsString>) -> PathBuf {
    let mut original = PathBuf::new();
    let mut result = PathBuf::new();
    for component in path.components() {
        original.push(component);
        result.push(names.get(&original).map_or(component.as_os_str(), |n| n));
    }
    result
}
/// Rows map to sources by position. Each row may change only its final name;
/// its parent may be written with either the original or the renamed directory
/// names, so a directory is renamed on its own row, never through a descendant.
pub fn rename_request(sources: Vec<PathBuf>, cwd: &Path, text: &str) -> Result<Request> {
    let lines = text.split('\n').collect::<Vec<_>>();
    if lines.len() != sources.len() {
        return Err("Rename row count changed; nothing renamed".into());
    }
    let written = lines
        .iter()
        .enumerate()
        .map(|(row, s)| {
            let p = Path::new(s);
            if s.is_empty() || p.file_name().is_none() {
                return Err(format!(
                    "Rename line {} has no filename; nothing renamed",
                    row + 1
                ));
            }
            if p.components().any(|c| matches!(c, Component::ParentDir)) {
                return Err(format!(
                    "Rename line {} contains '..'; nothing renamed",
                    row + 1
                ));
            }
            Ok(if p.is_absolute() {
                p.to_owned()
            } else {
                cwd.join(p)
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let names = rename_map(&sources, &written);
    let mut targets = vec![];
    for (row, (source, target)) in sources.iter().zip(&written).enumerate() {
        let (Some(parent), Some(name)) = (source.parent(), target.file_name()) else {
            return Err("Cannot rename a filesystem root".into());
        };
        let wrote = target.parent();
        if wrote != Some(parent) && wrote != Some(renamed(parent, &names).as_path()) {
            return Err(format!(
                "Rename line {} changes its directory; rename a directory on its own line. \
                 Nothing renamed",
                row + 1
            ));
        }
        targets.push(parent.join(name));
    }
    Ok(Request::Rename { sources, targets })
}

#[derive(Clone, Debug)]
pub enum EditKind {
    Rename(Vec<PathBuf>),
    Extract(Vec<PathBuf>),
    New(bool),
}
#[derive(Clone, Debug)]
pub struct Draft {
    pub kind: EditKind,
    pub cwd: PathBuf,
    pub text: String,
}
impl Draft {
    /// Rename lists span rows; names and destinations are single-line.
    pub fn multiline(&self) -> bool {
        matches!(self.kind, EditKind::Rename(_))
    }
    pub fn field(&self) -> crate::editor::Field {
        match self.kind {
            EditKind::Rename(_) => crate::editor::Field::Operation,
            EditKind::Extract(_) => crate::editor::Field::Extraction,
            EditKind::New(_) => crate::editor::Field::FileName,
        }
    }
    pub fn request(&self) -> Result<Request> {
        match &self.kind {
            EditKind::Rename(paths) => rename_request(paths.clone(), &self.cwd, &self.text),
            EditKind::Extract(archives) => {
                if self.text.contains(['\n', '\r']) {
                    return Err("Enter a single destination directory".into());
                }
                if self.text.starts_with('~') {
                    return Err("'~' is not expanded; enter an absolute or relative path".into());
                }
                let p = PathBuf::from(&self.text);
                let destination = if self.text.is_empty() {
                    None
                } else if p.is_absolute() {
                    Some(p)
                } else {
                    Some(self.cwd.join(p))
                };
                Ok(Request::Extract {
                    archives: archives.clone(),
                    destination,
                })
            }
            EditKind::New(directory) => {
                if self.text.is_empty() || self.text.contains(['\n', '\r']) {
                    return Err("Enter one nonempty name".into());
                }
                let path = Path::new(&self.text);
                if path.components().count() != 1
                    || !matches!(path.components().next(), Some(Component::Normal(_)))
                {
                    return Err("Enter a filename in the current directory".into());
                }
                Ok(Request::Create {
                    path: self.cwd.join(path),
                    directory: *directory,
                })
            }
        }
    }
}
pub fn human_size(bytes: u64) -> String {
    let mut n = bytes as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB", "TiB"] {
        if n < 1024.0 {
            break;
        }
        n /= 1024.0;
        unit = next;
    }
    if unit == "B" {
        format!("{bytes} B")
    } else {
        format!("{n:.1} {unit}")
    }
}
/// One row of a size listing: a path, or how many smaller entries were grouped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Listed {
    Path(PathBuf),
    More(usize),
}
/// Heaviest entries first, reserving a final row for all remaining entries.
pub fn deletion_rows(rows: &[(PathBuf, u64)], capacity: usize) -> Vec<(Listed, u64)> {
    if capacity == 0 {
        return vec![];
    }
    let mut sorted = rows.to_vec();
    sorted.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let take = if sorted.len() > capacity {
        capacity - 1
    } else {
        sorted.len()
    };
    let mut out = sorted[..take]
        .iter()
        .map(|(p, n)| (Listed::Path(p.clone()), *n))
        .collect::<Vec<_>>();
    if take < sorted.len() {
        out.push((
            Listed::More(sorted.len() - take),
            sorted[take..]
                .iter()
                .fold(0u64, |n, (_, size)| n.saturating_add(*size)),
        ));
    }
    out
}

/// Where `x` sends one entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Fate {
    Trash,
    /// It could go to the trash, but is larger than `[trash] ask_above`.
    Large,
    /// It cannot go to the trash, for this reason.
    Unavailable(String),
}
#[derive(Clone)]
pub struct DeletePlan {
    pub rows: Vec<(PathBuf, u64)>,
    snapshots: Vec<Snapshot>,
    /// Each row's fate when trashing; empty for a permanent deletion.
    pub fates: Vec<Fate>,
}
impl DeletePlan {
    pub fn new(sources: Vec<PathBuf>) -> Result<Self> {
        if sources.is_empty() {
            return Err("No files selected".into());
        }
        let resolved = sources
            .iter()
            .map(|p| Entry::at(p).map(|e| e.path))
            .collect::<Result<Vec<_>>>()?;
        // Through each path's ancestors, not pair by pair, for many paths.
        let mut seen = HashSet::new();
        let overlap = resolved.iter().any(|p| !seen.insert(p.as_path()))
            || resolved
                .iter()
                .any(|p| p.ancestors().skip(1).any(|a| seen.contains(a)));
        if overlap {
            return Err("Overlapping selections; select each tree only once".into());
        }
        let (snapshots, sizes): (Vec<_>, Vec<_>) = sources
            .iter()
            .map(|p| snapshot(p))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .unzip();
        Ok(Self {
            rows: sources.into_iter().zip(sizes).collect(),
            snapshots,
            fates: vec![],
        })
    }
    /// A plan to move `sources` to the trash, finding out which cannot go
    /// there or are large enough to ask first.
    pub fn trash(sources: Vec<PathBuf>, trash: &Trash) -> Result<Self> {
        let mut plan = Self::new(sources)?;
        let engine = Engine::new(Some(trash.clone()), None);
        plan.fates = plan
            .rows
            .iter()
            .map(
                |(path, size)| match Entry::at(path).and_then(|e| engine.bin(&e)) {
                    Err(reason) => Fate::Unavailable(reason),
                    Ok(_) if *size > trash.ask_above => Fate::Large,
                    Ok(_) => Fate::Trash,
                },
            )
            .collect();
        Ok(plan)
    }
    /// A plan for showing `rows` with their `fates`, without looking at disk.
    #[cfg(test)]
    pub(crate) fn shown(rows: Vec<(PathBuf, u64)>, fates: Vec<Fate>) -> Self {
        Self {
            rows,
            snapshots: vec![],
            fates,
        }
    }
    /// The rows to ask about before trashing: too large or not trashable.
    pub fn asking(&self) -> Vec<(PathBuf, u64)> {
        self.rows
            .iter()
            .zip(&self.fates)
            .filter(|(_, fate)| **fate != Fate::Trash)
            .map(|(row, _)| row.clone())
            .collect()
    }
    /// Deletes every row permanently.
    pub fn request(&self) -> Result<Request> {
        Ok(Request::Delete {
            sources: self.rows.iter().map(|(p, _)| p.clone()).collect(),
            confirmed: self.snapshots.clone(),
            trash: vec![],
        })
    }
    /// Trashes what can go to the trash (large rows too, when `large`) and
    /// deletes the rest permanently.
    pub fn trash_request(&self, large: bool) -> Request {
        Request::Delete {
            sources: self.rows.iter().map(|(p, _)| p.clone()).collect(),
            confirmed: self.snapshots.clone(),
            trash: self
                .fates
                .iter()
                .map(|fate| *fate == Fate::Trash || (large && *fate == Fate::Large))
                .collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    Move,
    Copy,
    Link,
}
/// A move, copy or link of the marked paths into `directory`, as confirmed:
/// the sources with their sizes, and existing destinations a move or copy
/// would overwrite.
#[derive(Clone)]
pub struct TransferPlan {
    pub kind: Transfer,
    pub directory: PathBuf,
    into: (u64, u64),
    pub sources: Vec<(PathBuf, u64)>,
    pub overwrites: Vec<(PathBuf, u64)>,
    snapshots: Vec<Snapshot>,
    /// Files dropped onto the terminal: the marks play no part and are kept.
    pub dropped: bool,
}
impl TransferPlan {
    pub fn new(kind: Transfer, sources: Vec<PathBuf>, directory: PathBuf) -> Result<Self> {
        if sources.is_empty() {
            return Err("No files selected".into());
        }
        let mut names = HashSet::new();
        let mut listed = vec![];
        let mut snapshots = vec![];
        for source in &sources {
            let name = source.file_name().ok_or("Invalid source")?;
            if !names.insert(name) {
                return Err(format!("Two selected entries are named {name:?}"));
            }
            let (snap, size) = snapshot(source)?;
            snapshots.push(snap);
            listed.push((source.clone(), size));
        }
        let m = fs::metadata(&directory).map_err(err)?;
        let into = (m.dev(), m.ino());
        let mut overwrites = vec![];
        for source in &sources {
            let target = directory.join(source.file_name().ok_or("Invalid source")?);
            if fs::symlink_metadata(&target).is_err() {
                continue;
            }
            let entry = Entry::at(&target)?;
            if Entry::at(source)?.path == entry.path {
                return Err(format!("{:?} is already in this directory", entry.name));
            }
            if kind == Transfer::Link {
                return Err(format!("Destination already exists: {:?}", entry.path));
            }
            let (snap, size) = snapshot(&target)?;
            snapshots.push(snap);
            overwrites.push((target, size));
        }
        Ok(Self {
            kind,
            directory,
            into,
            sources: listed,
            overwrites,
            snapshots,
            dropped: false,
        })
    }
    pub fn request(&self) -> Result<Request> {
        let sources = self.sources.iter().map(|(p, _)| p.clone()).collect();
        let directory = self.directory.clone();
        let replace = self.overwrites.iter().map(|(p, _)| p.clone()).collect();
        let confirmed = self.snapshots.clone();
        let into = Some(self.into);
        Ok(match self.kind {
            Transfer::Move => Request::Move {
                sources,
                directory,
                replace,
                confirmed,
                into,
            },
            Transfer::Copy => Request::Copy {
                sources,
                directory,
                replace,
                confirmed,
                into,
            },
            Transfer::Link => Request::Symlink {
                sources,
                directory,
                confirmed,
                into,
            },
        })
    }
}

/// What to do with entries whose place is taken when restoring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Taken {
    /// Overwrite what is there; it goes to the trash, as overwrites do.
    Replace,
    /// Restore under the first free name beside it, `name (1)`.
    KeepBoth,
}
/// R in a trash: each entry with where it goes back to, as checked.
#[derive(Clone, Debug)]
pub struct RestorePlan {
    /// Each entry with the place it came from.
    pub entries: Vec<(PathBuf, PathBuf)>,
    /// The entries whose place is taken, by index, each with the free name
    /// beside it that keeping both would use.
    pub taken: Vec<(usize, PathBuf)>,
    sources: Vec<Snapshot>,
    replaced: Vec<Snapshot>,
}
impl RestorePlan {
    pub fn new(paths: Vec<PathBuf>, trash: &Trash) -> Result<Self> {
        if paths.is_empty() {
            return Err("Nothing to restore".into());
        }
        let name = |p: &Path| {
            p.file_name()
                .unwrap_or(p.as_os_str())
                .to_string_lossy()
                .into_owned()
        };
        let mut entries: Vec<(PathBuf, PathBuf)> = vec![];
        let mut sources = vec![];
        for path in paths {
            if !trash.holds(&path) {
                return Err(format!(
                    "{} is not at the top of a trash; only those can be restored",
                    name(&path)
                ));
            }
            let origin =
                crate::trash::origin(&path).map_err(|e| format!("{}: {e}", name(&path)))?;
            let folder = origin.path.parent().ok_or("A trash record names a root")?;
            if !folder.is_dir() {
                return Err(format!(
                    "{} came from {}, which no longer exists",
                    name(&path),
                    folder.display()
                ));
            }
            if let Some((other, _)) = entries.iter().find(|(_, o)| *o == origin.path) {
                return Err(format!(
                    "{} and {} both came from {}; restore them one at a time",
                    name(other),
                    name(&path),
                    origin.path.display()
                ));
            }
            sources.push(snapshot(&path)?.0);
            entries.push((path, origin.path));
        }
        let mut taken = vec![];
        let mut replaced = vec![];
        for (i, (_, place)) in entries.iter().enumerate() {
            if fs::symlink_metadata(place).is_err() {
                continue;
            }
            replaced.push(snapshot(place)?.0);
            let folder = place.parent().expect("checked above");
            let both = crate::explorer::free_name(folder, place.file_name().unwrap_or_default());
            taken.push((i, both));
        }
        Ok(Self {
            entries,
            taken,
            sources,
            replaced,
        })
    }
    /// A plan as checked, for frame tests.
    #[cfg(test)]
    pub(crate) fn shown(entries: Vec<(PathBuf, PathBuf)>, taken: Vec<(usize, PathBuf)>) -> Self {
        Self {
            entries,
            taken,
            sources: vec![],
            replaced: vec![],
        }
    }
    /// The restore, with taken places overwritten or kept beside.
    pub fn request(&self, taken: Taken) -> Request {
        let sources = self.entries.iter().map(|(s, _)| s.clone()).collect();
        let mut targets: Vec<PathBuf> = self.entries.iter().map(|(_, t)| t.clone()).collect();
        let mut confirmed = self.sources.clone();
        let mut replace = vec![];
        for (&(i, ref both), snapshot) in self.taken.iter().zip(&self.replaced) {
            match taken {
                Taken::Replace => {
                    replace.push(targets[i].clone());
                    confirmed.push(snapshot.clone());
                }
                Taken::KeepBoth => targets[i] = both.clone(),
            }
        }
        Request::Restore {
            sources,
            targets,
            replace,
            confirmed,
        }
    }
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
