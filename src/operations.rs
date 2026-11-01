//! File operations. All mutations go through Engine and its temporary BackupGuard.
//! Linux directory descriptors pin parents; publishing never replaces an entry.
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, RenameFlags, ResolveFlags};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{self, Write},
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
        },
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
    // Never back up, copy or delete another filesystem mounted inside a tree.
    if out.get(Path::new("")).is_some_and(|root| root.0 != m.dev()) {
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
    let m = fs::symlink_metadata(fdpath(src).join(name)).map_err(err)?;
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
            copy_tree(&input, &name, &output, &name)?;
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
/// Temporary development guard. Replace this type's prepare method to remove
/// backups later; callers must continue to use the same transaction boundary.
#[derive(Clone)]
pub struct BackupGuard {
    root: PathBuf,
}
impl Default for BackupGuard {
    fn default() -> Self {
        Self {
            root: "/tmp/fff/backup".into(),
        }
    }
}
impl BackupGuard {
    /// Alternate root for isolated tests; production always uses Default.
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }
    fn directory(&self) -> Result<File> {
        // Create each component without following symlinks. /tmp is shared;
        // fff and backup must be owned by this uid and private.
        let mut dir = File::from(
            sys::openat(
                sys::CWD,
                "/",
                OFlags::DIRECTORY | OFlags::RDONLY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(err)?,
        );
        for component in self.root.components() {
            let Component::Normal(name) = component else {
                if matches!(component, Component::RootDir) {
                    continue;
                }
                return Err("Invalid backup directory".into());
            };
            let private = name == OsStr::new("fff") || name == OsStr::new("backup");
            match sys::mkdirat(&dir, name, Mode::RUSR | Mode::WUSR | Mode::XUSR) {
                Ok(()) => (),
                Err(rustix::io::Errno::EXIST) => (),
                Err(e) => return Err(err(e)),
            }
            dir = child_dir(&dir, name)?;
            let m = dir.metadata().map_err(err)?;
            if private && (m.uid() != rustix::process::geteuid().as_raw() || m.mode() & 0o077 != 0)
            {
                return Err("Backup directory must be private and owned by you".into());
            }
        }
        Ok(dir)
    }
    fn prepare(&self, sources: &[&Entry], targets: &[PathBuf]) -> Result<PathBuf> {
        let root = self.directory()?;
        let name = unique();
        sys::mkdirat(&root, &name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(err)?;
        let batch = child_dir(&root, OsStr::new(&name))?;
        let mut manifest = new_file(&batch, OsStr::new("manifest.txt"))?;
        writeln!(
            manifest,
            "fff backup v1; original paths are hex-encoded Unix bytes"
        )
        .map_err(err)?;
        for (i, entry) in sources.iter().enumerate() {
            let hex = entry
                .path
                .as_os_str()
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>();
            writeln!(manifest, "{i:04}\t{hex}\t{:?}", entry.path).map_err(err)?;
            copy_tree(
                &entry.parent,
                &entry.name,
                &batch,
                OsStr::new(&format!("{i:04}")),
            )?;
        }
        for path in targets {
            let state = if fs::symlink_metadata(path).is_ok() {
                "existing destination"
            } else {
                "absent"
            };
            writeln!(manifest, "{state}\t{:?}", path).map_err(err)?;
        }
        manifest.sync_all().map_err(err)?;
        new_file(&batch, OsStr::new("COMPLETE"))?
            .sync_all()
            .map_err(err)?;
        batch.sync_all().map_err(err)?;
        root.sync_all().map_err(err)?;
        Ok(self.root.join(name))
    }
}
#[derive(Clone, Debug)]
pub enum Request {
    Copy {
        sources: Vec<PathBuf>,
        directory: PathBuf,
    },
    /// `replace` lists confirmed existing destinations; any other collision aborts.
    Move {
        sources: Vec<PathBuf>,
        directory: PathBuf,
        replace: Vec<PathBuf>,
    },
    Symlink {
        sources: Vec<PathBuf>,
        directory: PathBuf,
    },
    Rename {
        sources: Vec<PathBuf>,
        targets: Vec<PathBuf>,
    },
    Delete {
        sources: Vec<PathBuf>,
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
    pub backup: PathBuf,
    pub moved: Vec<(PathBuf, PathBuf)>,
    pub removed: Vec<PathBuf>,
}
#[derive(Default)]
pub struct Engine {
    guard: BackupGuard,
    #[cfg(test)]
    hook: Option<Box<dyn Fn(usize)>>,
}
impl Engine {
    pub fn with_guard(guard: BackupGuard) -> Self {
        Self {
            guard,
            #[cfg(test)]
            hook: None,
        }
    }
    fn checkpoint(&self, _index: usize) {
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            hook(_index);
        }
    }
    pub fn execute(&self, request: Request) -> Result<Outcome> {
        let (mut sources, mut targets) = match &request {
            Request::Copy { sources, directory }
            | Request::Move {
                sources, directory, ..
            }
            | Request::Symlink { sources, directory } => (
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
            Request::Delete { sources } => (sources.clone(), vec![]),
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
            // renamed rows, and needs neither inventory nor backup.
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
                    backup: PathBuf::new(),
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
            Request::Move { replace, .. } => replace
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
                    "Cannot overwrite {:?}: it is or contains a moved path",
                    o.path
                ));
            }
        }
        let backup_path = self.guard.root.clone();
        for e in src.iter().chain(dst.iter()).chain(old.iter()) {
            if e.path.starts_with(&backup_path) || backup_path.starts_with(&e.path) {
                return Err("Cannot operate inside or above the backup directory".into());
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
        if matches!(request, Request::Move { .. } | Request::Rename { .. })
            && src.iter().zip(&dst).any(|(s, d)| {
                s.parent.metadata().map(|m| m.dev()).ok()
                    != d.parent.metadata().map(|m| m.dev()).ok()
            })
        {
            return Err("Moving across filesystems is not supported; copy first".into());
        }
        let backup = self
            .guard
            .prepare(&src.iter().chain(&old).collect::<Vec<_>>(), &targets)
            .map_err(|e| format!("Backup failed; nothing changed: {e}"))?;
        self.checkpoint(usize::MAX);
        for (s, was) in src.iter().chain(&old).zip(before.iter().chain(&replaced)) {
            if tree(s)? != *was {
                return Err(format!(
                    "Source changed; operation cancelled. Backup: {}",
                    backup.display()
                ));
            }
        }
        let mut outcome = Outcome {
            changed: true,
            backup: backup.clone(),
            moved: vec![],
            removed: vec![],
        };
        let result = (|| -> Result<()> {
            match request {
                Request::Move { .. } | Request::Rename { .. } => {
                    // Confirmed overwrites are detached into a private stage
                    // first, and deleted only once every move has published.
                    let mut held = match old.is_empty() {
                        true => None,
                        false => Some(Stage::new(&dst[0].parent)?),
                    };
                    let restore = |held: &mut Option<Stage>, count: usize| {
                        let Some(stage) = held else {
                            return true;
                        };
                        let mut restored = true;
                        for (j, o) in old.iter().enumerate().take(count).rev() {
                            restored &=
                                rename(&stage.dir, OsStr::new(&j.to_string()), &o.parent, &o.name)
                                    .is_ok();
                        }
                        stage.keep = !restored;
                        restored
                    };
                    if let Some(stage) = &mut held {
                        for (j, o) in old.iter().enumerate() {
                            let result = tree(o).and_then(|now| {
                                if now == replaced[j] {
                                    rename(
                                        &o.parent,
                                        &o.name,
                                        &stage.dir,
                                        OsStr::new(&j.to_string()),
                                    )
                                } else {
                                    Err("An entry to overwrite changed".into())
                                }
                            });
                            if let Err(e) = result {
                                return Err(format!(
                                    "{e}; {}",
                                    if restore(&mut held, j) {
                                        "nothing moved"
                                    } else {
                                        "restore incomplete; originals retained in staging"
                                    }
                                ));
                            }
                            stage.keep = true;
                        }
                    }
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
                                rollback &= rename(&b.parent, &b.name, &a.parent, &a.name).is_ok();
                            }
                            rollback &= restore(&mut held, old.len());
                            return Err(format!(
                                "{e}; {}",
                                if rollback {
                                    "earlier moves rolled back"
                                } else {
                                    "rollback incomplete; restore from backup"
                                }
                            ));
                        }
                        done.push(i);
                    }
                    if let Some(mut stage) = held {
                        for j in 0..old.len() {
                            remove_tree(&stage.dir, OsStr::new(&j.to_string()))?;
                        }
                        stage.keep = false;
                    }
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
                    let stage = Stage::new(&dst[0].parent)?;
                    let linking = matches!(request, Request::Symlink { .. });
                    for (i, s) in src.iter().enumerate() {
                        let name = format!("{i}");
                        if linking {
                            sys::symlinkat(&s.path, &stage.dir, &name).map_err(err)?;
                        } else {
                            copy_tree(&s.parent, &s.name, &stage.dir, OsStr::new(&name))?;
                        }
                    }
                    for (source, was) in src.iter().zip(&before) {
                        if tree(source)? != *was {
                            return Err("Source changed while staging; nothing copied".into());
                        }
                    }
                    let mut done: Vec<usize> = vec![];
                    for (i, d) in dst.iter().enumerate() {
                        self.checkpoint(i);
                        if let Err(e) =
                            rename(&stage.dir, OsStr::new(&i.to_string()), &d.parent, &d.name)
                        {
                            let mut rollback = true;
                            for &j in done.iter().rev() {
                                let d: &Entry = &dst[j];
                                rollback &= rename(
                                    &d.parent,
                                    &d.name,
                                    &stage.dir,
                                    OsStr::new(&j.to_string()),
                                )
                                .is_ok();
                            }
                            return Err(format!(
                                "{e}; {}",
                                if rollback {
                                    "copies rolled back"
                                } else {
                                    "rollback incomplete"
                                }
                            ));
                        }
                        done.push(i);
                    }
                }
                Request::Delete { .. } => {
                    // Detach all selected roots before deleting any contents;
                    // a failure to detach rolls earlier roots back intact.
                    let mut held = src
                        .iter()
                        .map(|s| Stage::new(&s.parent))
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
                            let mut rollback = true;
                            for j in (0..i).rev() {
                                let s = &src[j];
                                let restored =
                                    rename(&held[j].dir, OsStr::new("entry"), &s.parent, &s.name)
                                        .is_ok();
                                held[j].keep = !restored;
                                rollback &= restored;
                            }
                            return Err(format!(
                                "{e}; {}",
                                if rollback {
                                    "deletion cancelled"
                                } else {
                                    "rollback incomplete; original data retained in staging"
                                }
                            ));
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
                        let mut rollback = true;
                        for (i, st) in held.iter_mut().enumerate().rev() {
                            let restored =
                                rename(&st.dir, OsStr::new("entry"), &src[i].parent, &src[i].name)
                                    .is_ok();
                            st.keep = !restored;
                            rollback &= restored;
                        }
                        return Err(format!(
                            "{error}; {}",
                            if rollback {
                                "deletion cancelled"
                            } else {
                                "rollback incomplete; originals retained in staging"
                            }
                        ));
                    }
                    for (i, mut st) in held.into_iter().enumerate() {
                        remove_tree(&st.dir, OsStr::new("entry"))?;
                        st.keep = false;
                        outcome.removed.push(sources[i].clone());
                    }
                }

                Request::Create { directory, .. } => {
                    let d = &dst[0];
                    if directory {
                        sys::mkdirat(&d.parent, &d.name, Mode::from_raw_mode(0o755))
                            .map_err(err)?;
                    } else {
                        new_file(&d.parent, &d.name)?.sync_all().map_err(err)?;
                    }
                }
                Request::Extract { .. } => {
                    let d = dst.remove(0);
                    let stage = Stage::new(&d.parent)?;
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
                    if !exists(&d.parent, &d.name)? {
                        // The private stage becomes an ordinary new directory.
                        stage
                            .dir
                            .set_permissions(fs::Permissions::from_mode(0o755))
                            .map_err(err)?;
                        rename(&stage.parent, &stage.name, &d.parent, &d.name)?;
                    } else {
                        let dest = child_dir(&d.parent, &d.name)?;
                        let names = fs::read_dir(fdpath(&stage.dir))
                            .map_err(err)?
                            .map(|e| e.map(|e| e.file_name()).map_err(err))
                            .collect::<Result<Vec<_>>>()?;
                        for name in &names {
                            if exists(&dest, name)? {
                                return Err(format!(
                                    "Extraction destination already contains {:?}",
                                    name
                                ));
                            }
                        }
                        let mut done: Vec<OsString> = vec![];
                        for name in &names {
                            if let Err(e) = rename(&stage.dir, name, &dest, name) {
                                let mut rollback = true;
                                for name in done.iter().rev() {
                                    rollback &= rename(&dest, name, &stage.dir, name).is_ok();
                                }
                                if !rollback {
                                    return Err(format!("{e}; extraction rollback incomplete"));
                                }
                                return Err(e);
                            }
                            done.push(name.clone());
                        }
                    }
                }
            }
            Ok(())
        })();
        result.map_err(|e| format!("{e}. Backup: {}", backup.display()))?;
        Ok(outcome)
    }
}
struct Stage {
    parent: File,
    dir: File,
    name: OsString,
    keep: bool,
}
impl Stage {
    fn new(parent: &File) -> Result<Self> {
        let name = OsString::from(format!(".fff-stage-{}", unique()));
        sys::mkdirat(parent, &name, Mode::RUSR | Mode::WUSR | Mode::XUSR).map_err(err)?;
        Ok(Self {
            parent: parent.try_clone().map_err(err)?,
            dir: child_dir(parent, &name)?,
            name,
            keep: false,
        })
    }
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
    if m.is_dir() {
        let dir = child_dir(parent, name)?;
        // Copied read-only directories must still be removable on rollback.
        dir.set_permissions(fs::Permissions::from_mode(m.mode() & 0o777 | 0o700))
            .map_err(err)?;
        for item in fs::read_dir(fdpath(&dir)).map_err(err)? {
            remove_tree(&dir, &item.map_err(err)?.file_name())?;
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
    snapshots: Vec<BTreeMap<PathBuf, Stamp>>,
}
impl DeletePlan {
    pub fn new(sources: Vec<PathBuf>) -> Result<Self> {
        if sources.is_empty() {
            return Err("No files selected".into());
        }
        let snapshots = sources
            .iter()
            .map(|p| Entry::at(p).and_then(|e| tree(&e)))
            .collect::<Result<Vec<_>>>()?;
        let rows = sources
            .into_iter()
            .map(|p| weight(&p).map(|n| (p, n)))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { rows, snapshots })
    }
    pub fn request(&self) -> Result<Request> {
        for ((path, _), was) in self.rows.iter().zip(&self.snapshots) {
            if tree(&Entry::at(path)?)? != *was {
                return Err("Files changed since confirmation; review deletion again".into());
            }
        }
        Ok(Request::Delete {
            sources: self.rows.iter().map(|(p, _)| p.clone()).collect(),
        })
    }
}

/// A move into `directory`; `rows` lists existing destinations it would
/// overwrite, with their sizes, for confirmation.
#[derive(Clone)]
pub struct MovePlan {
    pub sources: Vec<PathBuf>,
    pub directory: PathBuf,
    pub rows: Vec<(PathBuf, u64)>,
    snapshots: Vec<BTreeMap<PathBuf, Stamp>>,
}
impl MovePlan {
    pub fn new(sources: Vec<PathBuf>, directory: PathBuf) -> Result<Self> {
        if sources.is_empty() {
            return Err("No files selected".into());
        }
        let mut rows = vec![];
        let mut snapshots = vec![];
        for source in &sources {
            let target = directory.join(source.file_name().ok_or("Invalid source")?);
            if fs::symlink_metadata(&target).is_err() {
                continue;
            }
            let entry = Entry::at(&target)?;
            if Entry::at(source)?.path == entry.path {
                return Err(format!("{:?} is already in this directory", entry.name));
            }
            snapshots.push(tree(&entry)?);
            rows.push((target.clone(), weight(&target)?));
        }
        Ok(Self {
            sources,
            directory,
            rows,
            snapshots,
        })
    }
    pub fn request(&self) -> Result<Request> {
        for ((path, _), was) in self.rows.iter().zip(&self.snapshots) {
            if tree(&Entry::at(path)?)? != *was {
                return Err("Files changed since confirmation; review the move again".into());
            }
        }
        Ok(Request::Move {
            sources: self.sources.clone(),
            directory: self.directory.clone(),
            replace: self.rows.iter().map(|(p, _)| p.clone()).collect(),
        })
    }
}

#[cfg(test)]
#[path = "operations_tests.rs"]
mod tests;
