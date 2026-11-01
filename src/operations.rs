//! File operations. All mutations go through Engine.
//! Linux directory descriptors pin parents; publishing never replaces an entry.
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, RenameFlags, ResolveFlags};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs::{self, File},
    io,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

pub type Result<T> = std::result::Result<T, String>;
fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}
static NEXT: AtomicU64 = AtomicU64::new(0);
fn unique() -> String {
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
fn fdpath(f: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", f.as_raw_fd()))
}
fn open_dir(path: &Path) -> Result<File> {
    let path = path.canonicalize().map_err(err)?;
    sys::openat2(
        sys::CWD,
        &path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )
    .map(File::from)
    .map_err(err)
}
fn child_dir(parent: &File, name: &OsStr) -> Result<File> {
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
type Stamp = (u64, u64, u64, i64, i64, i64, i64, u32);
fn stamp(m: &fs::Metadata) -> Stamp {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
        m.mode(),
    )
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
    let m = fs::symlink_metadata(fdpath(parent).join(name)).map_err(err)?;
    // Never copy or delete through anything mounted inside a tree, including
    // bind mounts from the same filesystem, which keep the device number.
    if depth > 0
        && (out.get(Path::new("")).is_some_and(|root| root.0 != m.dev())
            || mount_root(parent, name)?)
    {
        return Err(format!(
            "Tree contains a mount point or subvolume: {relative:?}"
        ));
    }
    out.insert(relative.to_owned(), stamp(&m));
    if m.is_dir() {
        let dir = child_dir(parent, name)?;
        let mut total = 0u64;
        for item in fs::read_dir(fdpath(&dir)).map_err(err)? {
            let name = item.map_err(err)?.file_name();
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
        Ok(m.len())
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
        now.5 = was.5;
        now.6 = was.6;
    }
    Ok(&current == expected)
}
/// Whether `name` is the root of a mount, as reported by statx (Linux 5.8+).
fn mount_root(parent: &File, name: &OsStr) -> Result<bool> {
    let x = sys::statx(
        parent,
        name,
        AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
        sys::StatxFlags::BASIC_STATS,
    )
    .map_err(err)?;
    if !x
        .stx_attributes_mask
        .contains(sys::StatxAttributes::MOUNT_ROOT)
    {
        return Err("Cannot detect mount points; Linux 5.8 or newer is required".into());
    }
    Ok(x.stx_attributes.contains(sys::StatxAttributes::MOUNT_ROOT))
}
/// Device and inode of `name`, without following a final symlink.
fn identity(parent: &File, name: &OsStr) -> Option<(u64, u64)> {
    let m = fs::symlink_metadata(fdpath(parent).join(name)).ok()?;
    Some((m.dev(), m.ino()))
}
/// Every directory from `dir` up to the filesystem root, by identity, so
/// aliases (bind mounts, case-insensitive names) cannot hide an ancestor.
fn ancestors(dir: &File) -> Result<HashSet<(u64, u64)>> {
    let mut seen = HashSet::new();
    let mut current = dir.try_clone().map_err(err)?;
    for _ in 0..4096 {
        let m = current.metadata().map_err(err)?;
        if !seen.insert((m.dev(), m.ino())) {
            break;
        }
        current = child_dir(&current, OsStr::new(".."))?;
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
    let before = input.metadata().map_err(err)?;
    if !before.is_file() {
        return Err("Select a regular archive file".into());
    }
    let check = input.try_clone().map_err(err)?;
    crate::archive::validate(input)?;
    if stamp(&before) != stamp(&check.metadata().map_err(err)?) {
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
fn keep_times(dir: &File, name: &OsStr, m: &fs::Metadata) -> Result<()> {
    let times = sys::Timestamps {
        last_access: sys::Timespec {
            tv_sec: m.atime(),
            tv_nsec: m.atime_nsec() as _,
        },
        last_modification: sys::Timespec {
            tv_sec: m.mtime(),
            tv_nsec: m.mtime_nsec() as _,
        },
    };
    sys::utimensat(dir, name, &times, AtFlags::SYMLINK_NOFOLLOW).map_err(err)
}
fn copy_tree(src: &File, name: &OsStr, dst: &File, to: &OsStr) -> Result<()> {
    copy_entry(src, name, dst, to, false)
}
fn copy_entry(src: &File, name: &OsStr, dst: &File, to: &OsStr, nested: bool) -> Result<()> {
    let m = fs::symlink_metadata(fdpath(src).join(name)).map_err(err)?;
    if nested && mount_root(src, name)? {
        return Err("Tree contains a mount point".into());
    }
    if m.is_symlink() {
        let target = sys::readlinkat(src, name, Vec::new()).map_err(err)?;
        sys::symlinkat(target.as_c_str(), dst, to).map_err(err)?;
        keep_times(dst, to, &m)?;
    } else if m.is_dir() {
        let input = child_dir(src, name)?;
        if stamp(&input.metadata().map_err(err)?) != stamp(&m) {
            return Err("Source directory changed".into());
        }
        sys::mkdirat(dst, to, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(err)?;
        let output = child_dir(dst, to)?;
        for item in fs::read_dir(fdpath(&input)).map_err(err)? {
            let name = item.map_err(err)?.file_name();
            copy_entry(&input, &name, &output, &name, true)?;
        }
        output
            .set_permissions(fs::Permissions::from_mode(m.mode() & 0o777))
            .map_err(err)?;
        keep_times(dst, to, &m)?;
        output.sync_all().map_err(err)?;
    } else if m.is_file() {
        let mut input = File::from(
            sys::openat(
                src,
                name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(err)?,
        );
        if stamp(&input.metadata().map_err(err)?) != stamp(&m) {
            return Err("Source changed while copying".into());
        }
        let mut output = new_file(dst, to)?;
        io::copy(&mut input, &mut output).map_err(err)?;
        output
            .set_permissions(fs::Permissions::from_mode(m.mode() & 0o777))
            .map_err(err)?;
        keep_times(dst, to, &m)?;
        output.sync_all().map_err(err)?;
        if stamp(&input.metadata().map_err(err)?) != stamp(&m) {
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
    Delete {
        sources: Vec<PathBuf>,
        confirmed: Vec<Snapshot>,
    },
    Create {
        path: PathBuf,
        directory: bool,
    },
    Extract {
        archive: PathBuf,
        destination: PathBuf,
    },
}
#[derive(Debug)]
pub struct Outcome {
    pub changed: bool,
    /// Set when the operation took effect but did not complete cleanly, such
    /// as a deletion stopped partway; `moved`/`removed` still list what happened.
    pub warning: Option<String>,
    pub moved: Vec<(PathBuf, PathBuf)>,
    pub removed: Vec<PathBuf>,
}
#[derive(Default)]
pub struct Engine {
    #[cfg(test)]
    hook: Option<Box<dyn Fn(usize)>>,
}
impl Engine {
    fn checkpoint(&self, _index: usize) {
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            hook(_index);
        }
    }
    pub fn execute(&self, request: Request) -> Result<Outcome> {
        // Closing the terminal or a termination signal waits until this
        // operation has finished or rolled back.
        let _deferred = interrupt::Deferred::new();
        let (mut sources, mut targets) = match &request {
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
            Request::Rename { sources, targets } => (sources.clone(), targets.clone()),
            Request::Delete { sources, .. } => (sources.clone(), vec![]),
            Request::Create { path, .. } => (vec![], vec![path.clone()]),
            Request::Extract {
                archive,
                destination,
            } => (vec![archive.clone()], vec![destination.clone()]),
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
                return Ok(Outcome {
                    changed: false,
                    warning: None,
                    moved: vec![],
                    removed: vec![],
                });
            }
        }
        let src = sources
            .iter()
            .map(|p| Entry::at(p))
            .collect::<Result<Vec<_>>>()?;
        let mut dst = targets
            .iter()
            .map(|p| Entry::at(p))
            .collect::<Result<Vec<_>>>()?;
        let old = match &request {
            Request::Move { replace, .. } | Request::Copy { replace, .. } => replace
                .iter()
                .map(|p| Entry::at(p))
                .collect::<Result<Vec<_>>>()?,
            _ => vec![],
        };
        for o in &old {
            if !dst.iter().any(|d| d.path == o.path) {
                return Err(format!("Not a move destination: {:?}", o.path));
            }
            if src.iter().any(|s| s.path.starts_with(&o.path)) {
                return Err(format!(
                    "Cannot overwrite {:?}: it is or contains a selected path",
                    o.path
                ));
            }
        }
        let renaming = matches!(request, Request::Rename { .. });
        for (i, a) in src.iter().enumerate() {
            for b in &src[..i] {
                if a.path == b.path {
                    return Err("The same path is selected twice".into());
                }
                // Renames keep their parents, so nested rows cannot collide.
                if !renaming && (a.path.starts_with(&b.path) || b.path.starts_with(&a.path)) {
                    return Err("Overlapping selections; select each tree only once".into());
                }
            }
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
                _ if existing && !old.iter().any(|o| o.path == d.path) => {
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
            if !renaming && src.iter().any(|s| d.path.starts_with(&s.path)) {
                return Err("Destination is inside a source".into());
            }
        }
        let before = src.iter().map(tree).collect::<Result<Vec<_>>>()?;
        let replaced = old.iter().map(tree).collect::<Result<Vec<_>>>()?;
        let confirmed = match &request {
            Request::Copy { confirmed, .. }
            | Request::Move { confirmed, .. }
            | Request::Symlink { confirmed, .. }
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
                .any(|root| (root.7 & 0o170000) == 0o040000 && above.contains(&(root.0, root.1)))
            {
                return Err("Destination is inside a source".into());
            }
        }
        if matches!(request, Request::Move { .. } | Request::Rename { .. })
            && src.iter().zip(&dst).any(|(s, d)| {
                s.parent.metadata().map(|m| m.dev()).ok()
                    != d.parent.metadata().map(|m| m.dev()).ok()
            })
        {
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
        let mut outcome = Outcome {
            changed: true,
            warning: None,
            moved: vec![],
            removed: vec![],
        };
        let result = (|| -> Result<()> {
            match request {
                Request::Move { .. } | Request::Rename { .. } => {
                    let mut over = Overwrite::detach(&old, &replaced, &dst)?;
                    let mut done: Vec<usize> = vec![];
                    for (i, (s, d)) in src.iter().zip(&dst).enumerate() {
                        if s.path == d.path {
                            continue;
                        }
                        self.checkpoint(i);
                        let result = tree(s).and_then(|now| {
                            if now == before[i] {
                                rename(&s.parent, &s.name, &d.parent, &d.name)
                            } else {
                                Err("Source changed before move".into())
                            }
                        });
                        if let Err(e) = result {
                            let mut rollback = true;
                            for &j in done.iter().rev() {
                                let a: &Entry = &src[j];
                                let b: &Entry = &dst[j];
                                // Only move back the entry this operation published.
                                let root = &before[j][Path::new("")];
                                rollback &= identity(&b.parent, &b.name) == Some((root.0, root.1))
                                    && rename(&b.parent, &b.name, &a.parent, &a.name).is_ok();
                            }
                            rollback &= over.restore();
                            return Err(format!(
                                "{e}; {}",
                                if rollback {
                                    "earlier moves rolled back".to_string()
                                } else {
                                    format!("rollback incomplete; {}", over.whereabouts())
                                }
                            ));
                        }
                        done.push(i);
                    }
                    outcome.warning = over.finish();
                    let names = rename_map(&sources, &targets);
                    outcome.moved = src
                        .iter()
                        .zip(&dst)
                        .enumerate()
                        .filter(|(_, (s, d))| s.path != d.path)
                        .map(|(i, (_, d))| {
                            let to = if renaming {
                                renamed(&sources[i], &names)
                            } else {
                                d.path.clone()
                            };
                            (sources[i].clone(), to)
                        })
                        .collect();
                }
                Request::Copy { .. } | Request::Symlink { .. } => {
                    // Stage all entries before publishing any. Never follow an
                    // existing destination or replace one created concurrently.
                    let stage = Stage::new(&dst[0].parent, &dst[0].path)?;
                    let linking = matches!(request, Request::Symlink { .. });
                    let mut staged = vec![];
                    for (i, s) in src.iter().enumerate() {
                        let name = format!("{i}");
                        if linking {
                            sys::symlinkat(&s.path, &stage.dir, &name).map_err(err)?;
                        } else {
                            copy_tree(&s.parent, &s.name, &stage.dir, OsStr::new(&name))?;
                        }
                        staged.push(identity(&stage.dir, OsStr::new(&name)));
                    }
                    for (source, was) in src.iter().zip(&before) {
                        if tree(source)? != *was {
                            return Err("Source changed while staging; nothing copied".into());
                        }
                    }
                    // Staging is complete, so overwritten entries are absent only
                    // while the copies are published.
                    let mut over = Overwrite::detach(&old, &replaced, &dst)?;
                    let mut done: Vec<usize> = vec![];
                    for (i, d) in dst.iter().enumerate() {
                        self.checkpoint(i);
                        if let Err(e) =
                            rename(&stage.dir, OsStr::new(&i.to_string()), &d.parent, &d.name)
                        {
                            let mut rollback = true;
                            for &j in done.iter().rev() {
                                let d: &Entry = &dst[j];
                                // Never pull back (and then delete) an entry that
                                // replaced the published copy concurrently.
                                rollback &= identity(&d.parent, &d.name) == staged[j]
                                    && rename(
                                        &d.parent,
                                        &d.name,
                                        &stage.dir,
                                        OsStr::new(&j.to_string()),
                                    )
                                    .is_ok();
                            }
                            rollback &= over.restore();
                            return Err(format!(
                                "{e}; {}",
                                if rollback {
                                    "copies rolled back".to_string()
                                } else {
                                    format!("rollback incomplete; {}", over.whereabouts())
                                }
                            ));
                        }
                        done.push(i);
                    }
                    outcome.warning = over.finish();
                }
                Request::Delete { .. } => {
                    // Detach all selected roots before deleting any contents;
                    // a failure to detach rolls earlier roots back intact.
                    let mut held = src
                        .iter()
                        .map(|s| Stage::new(&s.parent, &s.path))
                        .collect::<Result<Vec<_>>>()?;
                    for (i, s) in src.iter().enumerate() {
                        self.checkpoint(i);
                        let result = tree(s).and_then(|now| {
                            if now == before[i] {
                                rename(&s.parent, &s.name, &held[i].dir, OsStr::new("entry"))
                            } else {
                                Err("Source changed before deletion".into())
                            }
                        });
                        if let Err(e) = result {
                            for j in (0..i).rev() {
                                let s = &src[j];
                                held[j].keep =
                                    rename(&held[j].dir, OsStr::new("entry"), &s.parent, &s.name)
                                        .is_err();
                            }
                            return Err(format!("{e}; {}", unrestored(&held)));
                        }
                        held[i].keep = true;
                    }
                    self.checkpoint(usize::MAX - 1);
                    let validation: Result<()> = held.iter().enumerate().try_for_each(|(i, st)| {
                        if detached_matches(&st.dir, OsStr::new("entry"), &before[i])? {
                            Ok(())
                        } else {
                            Err("Source changed while preparing deletion".into())
                        }
                    });
                    if let Err(error) = validation {
                        for (i, st) in held.iter_mut().enumerate().rev() {
                            st.keep =
                                rename(&st.dir, OsStr::new("entry"), &src[i].parent, &src[i].name)
                                    .is_err();
                        }
                        return Err(format!("{error}; {}", unrestored(&held)));
                    }
                    self.checkpoint(usize::MAX - 2);
                    for i in 0..held.len() {
                        if let Err(e) = remove_tree(&held[i].dir, OsStr::new("entry")) {
                            // Put back what remains of this root and every root not
                            // yet deleted, so nothing is left hidden in staging.
                            for (j, st) in held.iter_mut().enumerate().skip(i) {
                                let s = &src[j];
                                st.keep = rename(&st.dir, OsStr::new("entry"), &s.parent, &s.name)
                                    .is_err();
                            }
                            let rest = match held.len() - i - 1 {
                                0 => String::new(),
                                1 => ", and the entry after it,".into(),
                                n => format!(", and the {n} entries after it,"),
                            };
                            let back = if held[i..].iter().all(|st| !st.keep) {
                                format!("What remains of it{rest} is back in place.")
                            } else {
                                format!("Could {}.", &unrestored(&held[i..])[6..])
                            };
                            outcome.warning = Some(format!(
                                "Deleted {i} of {}; {:?} was only partly deleted ({e}). {back}",
                                held.len(),
                                sources[i],
                            ));
                            return Ok(());
                        }
                        held[i].keep = false;
                        outcome.removed.push(sources[i].clone());
                    }
                }

                Request::Create { directory, .. } => {
                    let d = &dst[0];
                    if directory {
                        sys::mkdirat(&d.parent, &d.name, Mode::from_raw_mode(0o755))
                            .map_err(err)?;
                    } else {
                        // Ordinary permissions, as the user's umask allows.
                        File::from(
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
                        )
                        .sync_all()
                        .map_err(err)?;
                    }
                }
                Request::Extract { .. } => {
                    let d = dst.remove(0);
                    // An existing destination hosts the stage itself, so its
                    // parent need not be writable; otherwise the parent does.
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
                    let input = File::from(
                        sys::openat(
                            &src[0].parent,
                            &src[0].name,
                            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                            Mode::empty(),
                        )
                        .map_err(err)?,
                    );
                    if !input.metadata().map_err(err)?.is_file() {
                        return Err("Select a regular archive file".into());
                    }
                    crate::archive::extract(input, &stage.dir)?;
                    if tree(&src[0])? != before[0] {
                        return Err("Archive changed during extraction".into());
                    }
                    if exists(&d.parent, &d.name)? != existing {
                        return Err("Extraction destination changed".into());
                    }
                    if let Some(dest) = dest {
                        let names = fs::read_dir(fdpath(&stage.dir))
                            .map_err(err)?
                            .map(|e| e.map(|e| e.file_name()).map_err(err))
                            .collect::<Result<Vec<_>>>()?;
                        let staged = names
                            .iter()
                            .map(|n| identity(&stage.dir, n))
                            .collect::<Vec<_>>();
                        for name in &names {
                            if exists(&dest, name)? {
                                return Err(format!(
                                    "Extraction destination already contains {:?}",
                                    name
                                ));
                            }
                        }
                        let mut done: Vec<usize> = vec![];
                        for (k, name) in names.iter().enumerate() {
                            if let Err(e) = rename(&stage.dir, name, &dest, name) {
                                let mut rollback = true;
                                for &k in done.iter().rev() {
                                    let name = &names[k];
                                    rollback &= identity(&dest, name) == staged[k]
                                        && rename(&dest, name, &stage.dir, name).is_ok();
                                }
                                if !rollback {
                                    return Err(format!("{e}; extraction rollback incomplete"));
                                }
                                return Err(e);
                            }
                            done.push(k);
                        }
                    } else {
                        // The private stage becomes an ordinary new directory.
                        stage
                            .dir
                            .set_permissions(fs::Permissions::from_mode(0o755))
                            .map_err(err)?;
                        rename(&stage.parent, &stage.name, &d.parent, &d.name)?;
                    }
                }
            }
            Ok(())
        })();
        result?;
        Ok(outcome)
    }
}
/// After putting detached delete roots back: what, if anything, remains staged.
fn unrestored(held: &[Stage]) -> String {
    let stranded = held
        .iter()
        .filter(|st| st.keep)
        .map(|st| format!("{:?}", st.path.join("entry")))
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
/// Confirmed overwrite targets, detached into a private stage before
/// publication and deleted only after every entry has been published.
struct Overwrite<'a> {
    old: &'a [Entry],
    expected: &'a [BTreeMap<PathBuf, Stamp>],
    stage: Option<Stage>,
}
impl<'a> Overwrite<'a> {
    fn detach(
        old: &'a [Entry],
        expected: &'a [BTreeMap<PathBuf, Stamp>],
        dst: &[Entry],
    ) -> Result<Self> {
        let mut over = Self {
            old,
            expected,
            stage: None,
        };
        if old.is_empty() {
            return Ok(over);
        }
        let mut stage = Stage::new(&dst[0].parent, &dst[0].path)?;
        for (j, o) in old.iter().enumerate() {
            let result = tree(o).and_then(|now| {
                if now == expected[j] {
                    rename(&o.parent, &o.name, &stage.dir, OsStr::new(&j.to_string()))
                } else {
                    Err("An entry to overwrite changed".into())
                }
            });
            if let Err(e) = result {
                over.stage = Some(stage);
                let restored = over.restore_first(j);
                return Err(format!(
                    "{e}; {}",
                    if restored {
                        "nothing changed"
                    } else {
                        "restore incomplete; originals retained in staging"
                    }
                ));
            }
            stage.keep = true;
        }
        over.stage = Some(stage);
        Ok(over)
    }
    fn restore_first(&mut self, count: usize) -> bool {
        let Some(stage) = &mut self.stage else {
            return true;
        };
        let mut restored = true;
        for (j, o) in self.old.iter().enumerate().take(count).rev() {
            restored &= rename(&stage.dir, OsStr::new(&j.to_string()), &o.parent, &o.name).is_ok();
        }
        stage.keep = !restored;
        restored
    }
    /// Puts every detached entry back after a failed publication.
    fn restore(&mut self) -> bool {
        self.restore_first(self.old.len())
    }
    /// Where detached entries remain when they could not be restored.
    fn whereabouts(&self) -> String {
        match &self.stage {
            Some(stage) if stage.keep => format!("overwritten entries remain in {:?}", stage.path),
            _ => "entries remain at their destinations".into(),
        }
    }
    /// Deletes the overwritten entries once every new entry is published.
    /// An entry changed while detached is kept, never deleted unconfirmed.
    fn finish(self) -> Option<String> {
        let mut stage = self.stage?;
        let mut kept = vec![];
        for (j, o) in self.old.iter().enumerate() {
            let name = j.to_string();
            let removed = detached_matches(&stage.dir, OsStr::new(&name), &self.expected[j])
                .and_then(|same| {
                    if same {
                        remove_tree(&stage.dir, OsStr::new(&name))
                    } else {
                        Err("it changed while being overwritten".into())
                    }
                });
            if let Err(e) = removed {
                kept.push(format!(
                    "{:?} ({e}) is kept as {:?}",
                    o.name,
                    stage.path.join(&name)
                ));
            }
        }
        stage.keep = !kept.is_empty();
        (!kept.is_empty()).then(|| format!("Done, but {}", kept.join("; ")))
    }
}
pub const STAGE_PREFIX: &str = ".fff-stage-";
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
            let exe = fs::read_link(Path::new("/proc").join(pid).join("exe")).ok();
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
fn remove_tree(parent: &File, name: &OsStr) -> Result<()> {
    let m = fs::symlink_metadata(fdpath(parent).join(name)).map_err(err)?;
    // Recheck while deleting: nothing mounted since validation is entered.
    if mount_root(parent, name)? {
        return Err(format!("{name:?} is a mount point"));
    }
    if m.is_dir() {
        let dir = child_dir(parent, name)?;
        // Read-only directories must be writable to empty them; if deletion
        // then fails, the directory gets its exact mode back.
        let original = m.mode() & 0o7777;
        let unlocked = original & 0o700 != 0o700;
        if unlocked {
            dir.set_permissions(fs::Permissions::from_mode(original | 0o700))
                .map_err(err)?;
        }
        let emptied = (|| {
            for item in fs::read_dir(fdpath(&dir)).map_err(err)? {
                remove_tree(&dir, &item.map_err(err)?.file_name())?;
            }
            Ok(())
        })();
        if let Err(e) = emptied {
            if unlocked {
                let _ = dir.set_permissions(fs::Permissions::from_mode(original));
            }
            return Err(e);
        }
        sys::unlinkat(parent, name, AtFlags::REMOVEDIR).map_err(err)
    } else {
        sys::unlinkat(parent, name, AtFlags::empty()).map_err(err)
    }
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
    Extract(PathBuf),
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
            EditKind::Extract(archive) => {
                if self.text.contains(['\n', '\r']) {
                    return Err("Enter a single destination directory".into());
                }
                if self.text.starts_with('~') {
                    return Err("'~' is not expanded; enter an absolute or relative path".into());
                }
                let p = PathBuf::from(&self.text);
                let destination = if self.text.is_empty() {
                    archive.parent().ok_or("Archive has no parent")?.to_owned()
                } else if p.is_absolute() {
                    p
                } else {
                    self.cwd.join(p)
                };
                Ok(Request::Extract {
                    archive: archive.clone(),
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

#[derive(Clone)]
pub struct DeletePlan {
    pub rows: Vec<(PathBuf, u64)>,
    snapshots: Vec<Snapshot>,
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
        for (i, a) in resolved.iter().enumerate() {
            if resolved[..i]
                .iter()
                .any(|b| a.starts_with(b) || b.starts_with(a))
            {
                return Err("Overlapping selections; select each tree only once".into());
            }
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
        })
    }
    pub fn request(&self) -> Result<Request> {
        Ok(Request::Delete {
            sources: self.rows.iter().map(|(p, _)| p.clone()).collect(),
            confirmed: self.snapshots.clone(),
        })
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

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
