//! The trash: `x` moves entries into a folder of fff's own instead of
//! deleting them, and a purge at startup deletes what has waited there longer
//! than `[trash] days`.
//!
//! A move into the trash must be a rename, never a copy, so each filesystem
//! has its own bin: entries on the filesystem holding `$XDG_DATA_HOME` go to
//! `$XDG_DATA_HOME/fff/trash`, others to `.fff-trash-UID` at the top of their
//! filesystem. Every bin holds `files/NAME` and `info/NAME.trashinfo` (the
//! original path and the deletion time), laid out like the freedesktop.org
//! trash, so the entries are easy to find and put back by hand.
use crate::operations::{Result, child_dir, err, open_dir, remove_tree, reparent};
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, StatxFlags};
use std::{
    ffi::{OsStr, OsString},
    fs::{self, File},
    io::Write,
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
                // Undated entries are never deleted.
                let Some(deleted) = fs::read(item.path())
                    .ok()
                    .and_then(|text| deletion_date(&text))
                else {
                    continue;
                };
                if now.duration_since(deleted).is_ok_and(|waited| waited > age) {
                    let gone = match bin.contains(slot) {
                        Ok(true) => remove_tree(&bin.files, slot),
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
#[derive(Debug, Default)]
pub struct Purged {
    pub removed: usize,
    pub errors: Vec<String>,
}
/// One trash folder, opened without following a final symlink and owned by
/// this user.
pub(crate) struct Bin {
    pub(crate) path: PathBuf,
    pub(crate) files: File,
    info: File,
}
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
        })
    }
    fn contains(&self, slot: &OsStr) -> Result<bool> {
        match sys::statat(&self.files, slot, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => Ok(true),
            Err(rustix::io::Errno::NOENT) => Ok(false),
            Err(e) => Err(err(e)),
        }
    }
    /// Moves `name` in `parent` into the bin, after recording that it came
    /// from `original` now; the slot it went to, named after `original`, is
    /// returned.
    pub(crate) fn put(&self, parent: &File, name: &OsStr, original: &Path) -> Result<OsString> {
        let mut base = original.file_name().unwrap_or(name).as_bytes().to_vec();
        // Room for a numeric suffix and ".trashinfo" within NAME_MAX.
        base.truncate(200);
        let text = info_text(original, SystemTime::now());
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
                .write_all(text.as_bytes())
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
}
fn info_name(slot: &OsStr) -> OsString {
    let mut name = slot.to_owned();
    name.push(".trashinfo");
    name
}
fn fdpath(f: &File) -> PathBuf {
    use std::os::fd::AsRawFd;
    PathBuf::from(format!("/proc/self/fd/{}", f.as_raw_fd()))
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
/// The freedesktop.org record: percent-encoded path, local deletion time.
fn info_text(original: &Path, when: SystemTime) -> String {
    let mut path = String::new();
    for &b in original.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/-_.~".contains(&b) {
            path.push(b as char);
        } else {
            path.push_str(&format!("%{b:02X}"));
        }
    }
    format!(
        "[Trash Info]\nPath={path}\nDeletionDate={}\n",
        local_time(when)
    )
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
    let text = std::str::from_utf8(text).ok()?;
    let value = text
        .lines()
        .find_map(|l| l.trim().strip_prefix("DeletionDate="))?
        .trim();
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
        let text = info_text(Path::new("/home/me/a b%c/ü"), when);
        assert!(
            text.starts_with("[Trash Info]\nPath=/home/me/a%20b%25c/%C3%BC\n"),
            "{text}"
        );
        assert_eq!(deletion_date(text.as_bytes()), Some(when));
        assert_eq!(deletion_date(b"[Trash Info]\nPath=/x\n"), None);
        assert_eq!(deletion_date(b"DeletionDate=2026-13-45"), None);
        assert_eq!(deletion_date(b"DeletionDate=yesterday"), None);
    }
}
