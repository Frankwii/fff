//! The trash: `x` moves entries into a folder of fff's own instead of
//! deleting them, and a purge at startup deletes what has waited there longer
//! than `[trash] days`.
//!
//! A move into the trash must be a rename, never a copy, so each filesystem
//! has its own bin: entries on the filesystem holding `$XDG_DATA_HOME` go to
//! `$XDG_DATA_HOME/fff/trash`, others to `.fff-trash-UID` at the top of their
//! filesystem. Every bin holds `files/NAME` and `info/NAME.trashinfo` (the
//! original path and the deletion time), laid out like the freedesktop.org
//! trash, so the entries are easy to find and put back by hand. The purge
//! moves an entry to `expunged/` and drops its record before deleting it, so
//! one cut short (fff quit meanwhile) never leaves a half-deleted entry that
//! looks whole; the next purge finishes what is there.
use crate::operations::{Result, child_dir, err, fdpath, open_dir, remove_tree, reparent, unique};
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, StatxFlags};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::{Read, Write},
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        fs::{DirBuilderExt, MetadataExt},
    },
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// `[trash]` as the session uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trash {
    /// The bin for the filesystem holding the data directory.
    pub home: PathBuf,
    /// The bins fff made on other filesystems, one path per line, so the
    /// purge finds them again.
    pub registry: PathBuf,
    /// Entries older than this many days are deleted at startup.
    pub days: u64,
    /// Entries larger than this many bytes are not trashed without asking.
    pub ask_above: u64,
}
impl Trash {
    /// The bins under `data` (`$XDG_DATA_HOME`, or `~/.local/share`).
    pub fn new(data: &Path, days: u64, ask_above: u64) -> Self {
        Self {
            home: data.join("fff/trash"),
            registry: data.join("fff/trash-volumes"),
            days,
            ask_above,
        }
    }
    /// `$XDG_DATA_HOME/fff/trash`, or `~/.local/share/fff/trash`.
    pub fn from_env(days: u64, ask_above: u64) -> Option<Self> {
        let absolute = |v: OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
        let data = std::env::var_os("XDG_DATA_HOME")
            .and_then(absolute)
            .or_else(|| {
                std::env::var_os("HOME")
                    .and_then(absolute)
                    .map(|h| h.join(".local/share"))
            })?;
        Some(Self::new(&data, days, ask_above))
    }
    /// The bin for an entry in `parent` (`parent_path`, canonical): on the
    /// same filesystem, so that trashing is a rename.
    pub(crate) fn bin_for(&self, parent: &File, parent_path: &Path) -> Result<Bin> {
        let here = place(parent)?;
        if let Ok(bin) = self.home_bin()
            && place(&bin.files)? == here
        {
            return Ok(bin);
        }
        let top = top(parent_path, here)?;
        let path = top.join(format!(
            ".fff-trash-{}",
            rustix::process::geteuid().as_raw()
        ));
        let bin = Bin::open(&path, true).map_err(|e| {
            format!(
                "no trash folder can be made on its filesystem ({}: {e})",
                path.display()
            )
        })?;
        if place(&bin.files)? != here {
            return Err("its filesystem has no trash folder".into());
        }
        self.register(&bin.path);
        Ok(bin)
    }
    fn home_bin(&self) -> Result<Bin> {
        if let Some(parent) = self.home.parent()
            && !parent.exists()
        {
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(err)?;
        }
        Bin::open(&self.home, true)
    }
    /// Remembers a bin on another filesystem for the purge.
    fn register(&self, bin: &Path) {
        let known = fs::read(&self.registry).unwrap_or_default();
        let line = bin.as_os_str().as_bytes();
        if line.contains(&b'\n') || known.split(|&b| b == b'\n').any(|l| l == line) {
            return;
        }
        let _ = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.registry)
            .and_then(|mut f| f.write_all(&[line, b"\n"].concat()));
    }
    /// Every bin the purge looks at: the home bin and the registered ones.
    pub fn bins(&self) -> Vec<PathBuf> {
        let mut bins = vec![self.home.clone()];
        for line in fs::read(&self.registry)
            .unwrap_or_default()
            .split(|&b| b == b'\n')
        {
            let path = PathBuf::from(OsString::from_vec(line.to_vec()));
            if path.is_absolute() && !bins.contains(&path) {
                bins.push(path);
            }
        }
        bins
    }
    /// Deletes entries trashed more than `days` days before `now` from every
    /// bin that is present; unmounted ones wait for a later purge.
    pub fn purge(&self, now: SystemTime) -> Purged {
        let mut purged = Purged::default();
        let age = Duration::from_secs(self.days.saturating_mul(86_400));
        for path in self.bins() {
            let Ok(bin) = Bin::open(&path, false) else {
                continue;
            };
            // What an interrupted purge left half deleted.
            if let Ok(expunged) = child_dir(&bin.root, OsStr::new(EXPUNGED))
                && let Ok(leftovers) = fs::read_dir(fdpath(&expunged))
            {
                for item in leftovers.flatten() {
                    let _ = remove_tree(&expunged, &item.file_name());
                }
            }
            let Ok(listing) = fs::read_dir(fdpath(&bin.info)) else {
                continue;
            };
            for item in listing.flatten() {
                let info = item.file_name();
                let Some(slot) = info.as_bytes().strip_suffix(b".trashinfo") else {
                    continue;
                };
                // A record names one entry of files/, never the bin itself.
                if matches!(slot, b"" | b"." | b"..") {
                    continue;
                }
                let slot = OsStr::from_bytes(slot);
                let Ok(text) = fs::read(item.path()) else {
                    continue;
                };
                // Undated entries are never deleted.
                let Some(deleted) = deletion_date(&text) else {
                    continue;
                };
                // The record's entry, by its inode when the record has one:
                // another entry may have taken the slot after a rename.
                let recorded = record_inode(&text);
                let ours = |slot: &OsStr| {
                    bin.inode(slot)
                        .map(|held| held.is_some_and(|i| recorded.is_none_or(|r| r == i)))
                };
                // An entry renamed in the trash takes its record along.
                let mut slot = slot.to_owned();
                let mut info = info.clone();
                if let Ok(false) = ours(&slot)
                    && let Some(inode) = recorded
                    && let Some(renamed) = bin.renamed(inode)
                    && bin.follow(&slot, &renamed).is_ok()
                {
                    slot = renamed;
                    info = info_name(&slot);
                }
                let slot = slot.as_os_str();
                if now.duration_since(deleted).is_ok_and(|waited| waited > age) {
                    // A record whose entry is gone goes; an entry in its
                    // slot that is another stays.
                    let gone = match ours(slot) {
                        Ok(true) => bin.expunge(slot),
                        Ok(false) => Ok(()),
                        Err(e) => Err(e),
                    };
                    match gone {
                        Ok(()) => {
                            let _ = sys::unlinkat(&bin.info, &info, AtFlags::empty());
                            purged.removed += 1;
                        }
                        Err(e) => purged
                            .errors
                            .push(format!("{}: {e}", path.join("files").join(slot).display())),
                    }
                }
            }
        }
        purged
    }
    /// Purges on a thread of its own, so a large trash never delays startup.
    pub fn purge_in_background(&self) -> std::thread::JoinHandle<Purged> {
        let trash = self.clone();
        std::thread::spawn(move || trash.purge(SystemTime::now()))
    }
}
/// Where an entry at the top of a trash came from, by its record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Origin {
    /// Its original path.
    pub path: PathBuf,
    pub deleted: Option<SystemTime>,
}
/// The bin holding `entry` at its top (`BIN/files/NAME`), by its place in
/// that layout alone.
pub fn bin_of(entry: &Path) -> Option<&Path> {
    let files = entry.parent()?;
    (files.file_name()? == "files" && entry.file_name().is_some())
        .then(|| files.parent())
        .flatten()
}
/// Where `entry`, at the top of a bin, came from: read from its record,
/// which it keeps when renamed in the trash.
pub fn origin(entry: &Path) -> Result<Origin> {
    let name = entry.file_name().ok_or("Not an entry")?;
    let bin = Bin::open(bin_of(entry).ok_or("Not at the top of a trash")?, false)?;
    let (_, text) = bin
        .record_of(name)
        .ok_or("The trash has no record of where it came from")?;
    let top = bin.path.parent().unwrap_or(&bin.path);
    Ok(Origin {
        path: record_path(&text, top).ok_or("Its trash record names no place")?,
        deleted: deletion_date(&text),
    })
}
impl Trash {
    /// Whether `entry` is at the top of one of the bins: a path below
    /// `files/` of the home bin or of a registered one.
    pub fn holds(&self, entry: &Path) -> bool {
        let Some(bin) = bin_of(entry).and_then(|b| b.canonicalize().ok()) else {
            return false;
        };
        self.bins()
            .iter()
            .any(|b| b == &bin || b.canonicalize().is_ok_and(|b| b == bin))
    }
    /// The bins' `files/` folders that exist, the home bin's first: what
    /// the trash shows.
    pub fn folders(&self) -> Vec<PathBuf> {
        self.bins()
            .into_iter()
            .map(|b| b.join("files"))
            .filter(|f| f.is_dir())
            .collect()
    }
}
#[derive(Debug, Default)]
pub struct Purged {
    pub removed: usize,
    pub errors: Vec<String>,
}
/// One trash folder, opened without following a final symlink and owned by
/// this user.
pub(crate) struct Bin {
    pub(crate) path: PathBuf,
    root: File,
    pub(crate) files: File,
    info: File,
}
/// Where the purge deletes entries, inside a bin.
const EXPUNGED: &str = "expunged";
impl Bin {
    /// Opens the bin at `path`, creating it (mode 0700) when `create`.
    pub(crate) fn open(path: &Path, create: bool) -> Result<Self> {
        let name = path.file_name().ok_or("A trash folder needs a name")?;
        let parent = open_dir(path.parent().ok_or("A trash folder needs a parent")?)?;
        let owned = |parent: &File, name: &OsStr| -> Result<File> {
            if create {
                match sys::mkdirat(parent, name, Mode::RWXU) {
                    Ok(()) | Err(rustix::io::Errno::EXIST) => {}
                    Err(e) => return Err(err(e)),
                }
            }
            let dir = child_dir(parent, name)?;
            if dir.metadata().map_err(err)?.uid() != rustix::process::geteuid().as_raw() {
                return Err("the trash folder belongs to another user".into());
            }
            Ok(dir)
        };
        let root = owned(&parent, name)?;
        Ok(Self {
            path: crate::operations::canonical_dir(&root)?,
            files: owned(&root, OsStr::new("files"))?,
            info: owned(&root, OsStr::new("info"))?,
            root,
        })
    }
    /// Deletes `slot` for good: moved to `expunged/` and its record removed
    /// first, then deleted there.
    fn expunge(&self, slot: &OsStr) -> Result<()> {
        match sys::mkdirat(&self.root, EXPUNGED, Mode::RWXU) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(e) => return Err(err(e)),
        }
        let expunged = child_dir(&self.root, OsStr::new(EXPUNGED))?;
        let name = OsString::from(unique());
        reparent(&self.files, slot, &expunged, &name)?;
        let _ = sys::unlinkat(&self.info, info_name(slot), AtFlags::empty());
        remove_tree(&expunged, &name)
    }
    fn contains(&self, slot: &OsStr) -> Result<bool> {
        Ok(self.inode(slot)?.is_some())
    }
    /// The inode of the entry in `slot`, if there is one.
    fn inode(&self, slot: &OsStr) -> Result<Option<u64>> {
        match sys::statat(&self.files, slot, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(st) => Ok(Some(st.st_ino)),
            Err(rustix::io::Errno::NOENT) => Ok(None),
            Err(e) => Err(err(e)),
        }
    }
    /// Moves `name` in `parent` into the bin, after recording that it came
    /// from `original` now; the slot it went to, named after `original`, is
    /// returned.
    pub(crate) fn put(&self, parent: &File, name: &OsStr, original: &Path) -> Result<OsString> {
        let inode = sys::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(err)?
            .st_ino;
        let text = info_text(original, SystemTime::now(), Some(inode));
        let base = original.file_name().unwrap_or(name);
        self.put_record(parent, name, base, text.as_bytes())
    }
    /// Moves `name` in `parent` into the bin with the record `text`, into
    /// the first free slot named after `base`.
    pub(crate) fn put_record(
        &self,
        parent: &File,
        name: &OsStr,
        base: &OsStr,
        text: &[u8],
    ) -> Result<OsString> {
        let mut base = base.as_bytes().to_vec();
        // Room for a numeric suffix and ".trashinfo" within NAME_MAX.
        base.truncate(200);
        for n in 1..=10_000u32 {
            let mut slot = base.clone();
            if n > 1 {
                slot.extend_from_slice(format!(".{n}").as_bytes());
            }
            let slot = OsString::from_vec(slot);
            if self.contains(&slot)? {
                continue;
            }
            let info = info_name(&slot);
            let mut file = match sys::openat(
                &self.info,
                &info,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::RUSR | Mode::WUSR,
            ) {
                Ok(fd) => File::from(fd),
                Err(rustix::io::Errno::EXIST) => continue,
                Err(e) => return Err(err(e)),
            };
            let moved = file
                .write_all(text)
                .and_then(|()| file.sync_all())
                .map_err(err)
                .and_then(|()| reparent(parent, name, &self.files, &slot));
            if let Err(e) = moved {
                let _ = sys::unlinkat(&self.info, &info, AtFlags::empty());
                return Err(e);
            }
            return Ok(slot);
        }
        Err("The trash holds too many entries with this name".into())
    }
    /// Moves `slot` back to `name` in `parent`, never replacing an entry
    /// there; its record stays until `forget`.
    pub(crate) fn take(&self, slot: &OsStr, parent: &File, name: &OsStr) -> Result<()> {
        reparent(&self.files, slot, parent, name)
    }
    /// Drops the record of an entry taken back out of the bin.
    pub(crate) fn forget(&self, slot: &OsStr) {
        let _ = sys::unlinkat(&self.info, info_name(slot), AtFlags::empty());
    }
    /// The record `info/NAME.trashinfo`, read without following a symlink.
    fn read_record(&self, name: &OsStr) -> Option<Vec<u8>> {
        let fd = sys::openat(
            &self.info,
            name,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok()?;
        let mut text = vec![];
        File::from(fd)
            .take(RECORD_LIMIT)
            .read_to_end(&mut text)
            .ok()?;
        Some(text)
    }
    /// The record of the entry `slot`, as (the slot the record is named
    /// after, its text). Its own record comes first, unless that one names
    /// another entry; otherwise the entry was renamed in the trash, and its
    /// record is the one naming its inode (a rename keeps it) whose own slot
    /// is empty, the latest deleted if several do (an inode is reused only
    /// after its entry is gone).
    pub(crate) fn record_of(&self, slot: &OsStr) -> Option<(OsString, Vec<u8>)> {
        let inode = sys::statat(&self.files, slot, AtFlags::SYMLINK_NOFOLLOW)
            .ok()?
            .st_ino;
        if let Some(text) = self.read_record(&info_name(slot))
            && record_inode(&text).is_none_or(|i| i == inode)
        {
            return Some((slot.to_owned(), text));
        }
        let mut found: Option<(OsString, Vec<u8>, Option<SystemTime>)> = None;
        for item in fs::read_dir(fdpath(&self.info)).ok()?.flatten() {
            let name = item.file_name();
            let Some(other) = name.as_bytes().strip_suffix(b".trashinfo") else {
                continue;
            };
            let other = OsStr::from_bytes(other);
            if other == slot || matches!(other.as_bytes(), b"" | b"." | b"..") {
                continue;
            }
            let Some(text) = self.read_record(&name) else {
                continue;
            };
            if record_inode(&text) != Some(inode) || self.contains(other).unwrap_or(true) {
                continue;
            }
            let deleted = deletion_date(&text);
            if found.as_ref().is_none_or(|(_, _, d)| deleted > *d) {
                found = Some((other.to_owned(), text, deleted));
            }
        }
        found.map(|(other, text, _)| (other, text))
    }
    /// Renames the record of `from` to be the record of `to`, which has
    /// none: after the entry was renamed in the trash.
    fn follow(&self, from: &OsStr, to: &OsStr) -> Result<()> {
        sys::renameat_with(
            &self.info,
            info_name(from),
            &self.info,
            info_name(to),
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(err)
    }
    /// The entry of files/ with this inode that has no record of its own,
    /// for a record whose entry is gone from its slot.
    fn renamed(&self, inode: u64) -> Option<OsString> {
        use std::os::unix::fs::DirEntryExt;
        fs::read_dir(fdpath(&self.files))
            .ok()?
            .flatten()
            .find(|item| {
                item.ino() == inode
                    && sys::statat(
                        &self.info,
                        info_name(&item.file_name()),
                        AtFlags::SYMLINK_NOFOLLOW,
                    )
                    .is_err()
            })
            .map(|item| item.file_name())
    }
}
fn info_name(slot: &OsStr) -> OsString {
    let mut name = slot.to_owned();
    name.push(".trashinfo");
    name
}
/// The mount and device of a directory: a rename works only within both.
fn place(dir: &File) -> Result<(u64, u32, u32)> {
    let x = sys::statx(dir, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID).map_err(err)?;
    if x.stx_mask & StatxFlags::MNT_ID.bits() == 0 {
        return Err("Cannot identify mounts; Linux 5.8 or newer is required".into());
    }
    Ok((x.stx_mnt_id, x.stx_dev_major, x.stx_dev_minor))
}
/// The highest directory above `dir` (canonical) on the same mount and
/// device: where a filesystem's bin goes.
fn top(dir: &Path, here: (u64, u32, u32)) -> Result<PathBuf> {
    let mut top = dir.to_path_buf();
    for ancestor in dir.ancestors().skip(1) {
        let Ok(opened) = open_dir(ancestor) else {
            break;
        };
        if place(&opened)? != here {
            break;
        }
        top = ancestor.to_path_buf();
    }
    Ok(top)
}
/// The freedesktop.org record: percent-encoded path, local deletion time,
/// and the entry's inode (`INODE_KEY`) when known.
fn info_text(original: &Path, when: SystemTime, inode: Option<u64>) -> String {
    let mut path = String::new();
    for &b in original.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            path.push(b as char);
        } else {
            path.push_str(&format!("%{b:02X}"));
        }
    }
    let mut text = format!(
        "[Trash Info]\nPath={path}\nDeletionDate={}\n",
        local_time(when)
    );
    if let Some(inode) = inode {
        text += &format!("{INODE_KEY}{inode}\n");
    }
    text
}
/// fff's own key in its records: the entry's inode number, which renaming
/// the entry in the trash keeps, so that its record can still be found.
const INODE_KEY: &str = "X-fff-Inode=";
/// Records are small; a larger file in info/ is not read whole.
const RECORD_LIMIT: u64 = 64 * 1024;
/// The value of `key` (with its `=`) in a record.
fn field<'a>(text: &'a [u8], key: &str) -> Option<&'a str> {
    let text = std::str::from_utf8(text).ok()?;
    text.lines()
        .find_map(|l| l.trim().strip_prefix(key))
        .map(str::trim)
}
fn record_inode(text: &[u8]) -> Option<u64> {
    field(text, INODE_KEY)?.parse().ok()
}
/// The original path of a record, decoded; a relative one is relative to
/// `top`, the directory holding the bin.
fn record_path(text: &[u8], top: &Path) -> Option<PathBuf> {
    let value = field(text, "Path=")?.as_bytes();
    let mut bytes = Vec::with_capacity(value.len());
    let mut i = 0;
    while i < value.len() {
        if value[i] == b'%' {
            let hex = std::str::from_utf8(value.get(i + 1..i + 3)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            bytes.push(value[i]);
            i += 1;
        }
    }
    if bytes.is_empty() || bytes.contains(&0) {
        return None;
    }
    let path = PathBuf::from(OsString::from_vec(bytes));
    Some(if path.is_absolute() {
        path
    } else {
        top.join(path)
    })
}
/// `YYYY-MM-DDThh:mm:ss` in local time.
fn local_time(when: SystemTime) -> String {
    let secs = when.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&secs, &mut tm) };
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min,
        tm.tm_sec
    )
}
/// The `DeletionDate` of a record, read as local time.
fn deletion_date(text: &[u8]) -> Option<SystemTime> {
    let value = field(text, "DeletionDate=")?;
    let (date, time) = value.split_once('T')?;
    let mut date = date.split('-').map(str::parse::<i32>);
    let mut time = time.split(':').map(str::parse::<i32>);
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    tm.tm_year = date.next()?.ok()? - 1900;
    tm.tm_mon = date.next()?.ok()? - 1;
    tm.tm_mday = date.next()?.ok()?;
    tm.tm_hour = time.next()?.ok()?;
    tm.tm_min = time.next()?.ok()?;
    tm.tm_sec = time.next()?.ok()?;
    tm.tm_isdst = -1;
    if date.next().is_some() || time.next().is_some() {
        return None;
    }
    // mktime would normalize "2026-13-45" into a real date; an impossible
    // one is unreadable, and its entry is kept.
    let year = tm.tm_year + 1900;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match tm.tm_mon + 1 {
        2 if leap => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        1..=12 => 31,
        _ => return None,
    };
    if !(1..=days).contains(&tm.tm_mday)
        || !(0..24).contains(&tm.tm_hour)
        || !(0..60).contains(&tm.tm_min)
        || !(0..=60).contains(&tm.tm_sec)
    {
        return None;
    }
    let secs = unsafe { libc::mktime(&mut tm) };
    (secs >= 0).then(|| UNIX_EPOCH + Duration::from_secs(secs as u64))
}
/// `[trash] ask_above`: bytes, or a number with a unit such as "10 GiB".
pub fn parse_size(text: &str) -> std::result::Result<u64, String> {
    let text = text.trim();
    let split = text
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number
        .parse()
        .map_err(|_| format!("{text:?} is not a size such as \"10 GiB\""))?;
    let scale: u64 = match unit.trim().to_ascii_lowercase().as_str() {
        "" | "b" => 1,
        "k" | "kb" | "kib" => 1 << 10,
        "m" | "mb" | "mib" => 1 << 20,
        "g" | "gb" | "gib" => 1 << 30,
        "t" | "tb" | "tib" => 1 << 40,
        other => {
            return Err(format!(
                "unknown size unit {other:?}; use B, KiB, MiB, GiB or TiB"
            ));
        }
    };
    let bytes = number * scale as f64;
    if !bytes.is_finite() || bytes > u64::MAX as f64 {
        return Err(format!("{text:?} is too large"));
    }
    Ok(bytes as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_take_binary_units() {
        assert_eq!(parse_size("10 GiB"), Ok(10 << 30));
        assert_eq!(parse_size("10GB"), Ok(10 << 30));
        assert_eq!(parse_size("1.5k"), Ok(1536));
        assert_eq!(parse_size("512"), Ok(512));
        assert_eq!(parse_size(" 2 TiB "), Ok(2 << 40));
        assert!(parse_size("ten").is_err());
        assert!(parse_size("10 parsecs").is_err());
        assert!(parse_size("").is_err());
        assert!(parse_size("-1 GiB").is_err());
    }
    #[test]
    fn records_round_trip_the_local_deletion_time() {
        let when = UNIX_EPOCH + Duration::from_secs(1_762_000_000);
        let text = info_text(Path::new("/home/me/a b%c/ü"), when, Some(7));
        assert!(
            text.starts_with("[Trash Info]\nPath=/home/me/a%20b%25c/%C3%BC\n"),
            "{text}"
        );
        assert_eq!(deletion_date(text.as_bytes()), Some(when));
        assert_eq!(deletion_date(b"[Trash Info]\nPath=/x\n"), None);
        assert_eq!(deletion_date(b"DeletionDate=2026-13-45"), None);
        assert_eq!(deletion_date(b"DeletionDate=yesterday"), None);
        // Impossible dates are not normalized into real ones.
        for text in [
            "2026-13-01T00:00:00",
            "2026-02-29T00:00:00",
            "2026-04-31T00:00:00",
            "2026-01-01T24:00:00",
            "2026-01-01T00:60:00",
        ] {
            let line = format!("DeletionDate={text}");
            assert_eq!(deletion_date(line.as_bytes()), None, "{text}");
        }
        assert!(deletion_date(b"DeletionDate=2024-02-29T12:00:00").is_some());
    }
}
