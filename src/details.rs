//! Row details for the metadata column, the footer and link markers. Only rows
//! on screen are inspected: one lstat each, cached until the next scan. Counting
//! a directory's entries can be slow (huge or remote directories), so a worker
//! does it and the frame shows the count once it arrives.
use serde::Deserialize;
use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    time::SystemTime,
};

/// `[ui] columns`: what the right-hand side of each result row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Column {
    /// Permissions, like `ls -l`: `drwxr-xr-x`.
    Permissions,
    /// A file's size (`4.2K`), or a directory's entry count.
    Size,
    /// Time since the last modification (`3d`).
    Modified,
}
pub const DEFAULT_COLUMNS: [Column; 2] = [Column::Size, Column::Modified];

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// `st_mode`, file type bits included.
    pub mode: u32,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub uid: u32,
    pub gid: u32,
    /// A symbolic link's target as written, and whether it resolves.
    pub link: Option<(PathBuf, bool)>,
    /// The entry is, or resolves to, a directory.
    pub directory: bool,
}
impl Info {
    pub fn read(path: &Path) -> Option<Self> {
        let m = fs::symlink_metadata(path).ok()?;
        let link = m.is_symlink().then(|| {
            let target = fs::read_link(path).unwrap_or_default();
            (target, fs::metadata(path).is_ok())
        });
        Some(Self {
            mode: m.mode(),
            size: m.len(),
            modified: m.modified().ok(),
            uid: m.uid(),
            gid: m.gid(),
            directory: m.is_dir() || (link.is_some() && path.is_dir()),
            link,
        })
    }
    pub fn broken(&self) -> bool {
        self.link.as_ref().is_some_and(|(_, resolves)| !resolves)
    }
}

/// A directory's entry count; counting stops at `COUNT_LIMIT`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    Entries(usize),
    Unreadable,
}
const COUNT_LIMIT: usize = 10_000;
/// Cached counts kept across directories before the cache starts over.
const COUNT_CACHE: usize = 4096;

struct Worker {
    requests: Sender<(u64, PathBuf)>,
    results: Receiver<(u64, PathBuf, Count)>,
}

#[derive(Default)]
pub struct Details {
    infos: RefCell<HashMap<PathBuf, Option<Info>>>,
    /// Counts, and whether each is current; stale ones show until replaced.
    counts: RefCell<HashMap<PathBuf, (Option<Count>, bool)>>,
    worker: RefCell<Option<Worker>>,
    /// Counts requested before the last `forget` are skipped by the worker.
    generation: Arc<AtomicU64>,
    names: RefCell<HashMap<(bool, u32), String>>,
    wake: Option<SyncSender<()>>,
}
impl Details {
    /// Coalesced notification for arriving counts, shared with terminal input.
    pub fn set_waker(&mut self, wake: SyncSender<()>) {
        self.wake = Some(wake);
    }
    /// The entry's details, read once until forgotten.
    pub fn info(&self, path: &Path) -> Option<Info> {
        self.infos
            .borrow_mut()
            .entry(path.to_path_buf())
            .or_insert_with(|| Info::read(path))
            .clone()
    }
    /// A directory's entry count, requested from the worker when unknown or
    /// stale; a stale count is shown until the new one arrives.
    pub fn count(&self, path: &Path) -> Option<Count> {
        let mut counts = self.counts.borrow_mut();
        let entry = counts.entry(path.to_path_buf()).or_insert((None, false));
        if !entry.1 {
            entry.1 = true;
            self.request(path);
        }
        entry.0
    }
    fn request(&self, path: &Path) {
        let mut worker = self.worker.borrow_mut();
        let worker = worker.get_or_insert_with(|| self.spawn());
        let _ = worker
            .requests
            .send((self.generation.load(Ordering::Relaxed), path.to_path_buf()));
    }
    fn spawn(&self) -> Worker {
        let (requests, incoming) = mpsc::channel::<(u64, PathBuf)>();
        let (outgoing, results) = mpsc::channel();
        let generation = self.generation.clone();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            for (asked, path) in incoming {
                if asked != generation.load(Ordering::Relaxed) {
                    continue;
                }
                let count = count_entries(&path);
                if outgoing.send((asked, path, count)).is_err() {
                    return;
                }
                if let Some(wake) = &wake {
                    let _ = wake.try_send(());
                }
            }
        });
        Worker { requests, results }
    }
    /// Takes arrived counts; returns whether any changed what is shown.
    pub fn drain(&self) -> bool {
        let worker = self.worker.borrow();
        let Some(worker) = worker.as_ref() else {
            return false;
        };
        let current = self.generation.load(Ordering::Relaxed);
        let mut counts = self.counts.borrow_mut();
        let mut changed = false;
        while let Ok((asked, path, count)) = worker.results.try_recv() {
            if asked == current
                && let Some(entry) = counts.get_mut(&path)
            {
                changed |= entry.0 != Some(count);
                *entry = (Some(count), true);
            }
        }
        changed
    }
    /// Everything may have changed (a new scan): read details again as rows
    /// are drawn, keeping old counts on screen until new ones arrive.
    pub fn forget(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.infos.borrow_mut().clear();
        let mut counts = self.counts.borrow_mut();
        if counts.len() > COUNT_CACHE {
            counts.clear();
        }
        for entry in counts.values_mut() {
            entry.1 = false;
        }
    }
    /// One entry changed on disk (written to, or its attributes).
    pub fn touched(&self, path: &Path) {
        self.infos.borrow_mut().remove(path);
        if let Some(entry) = self.counts.borrow_mut().get_mut(path) {
            entry.1 = false;
        }
    }
    /// `user:group`, by name where the system knows one.
    pub fn owner(&self, info: &Info) -> String {
        let mut names = self.names.borrow_mut();
        let user = names
            .entry((true, info.uid))
            .or_insert_with(|| account_name(true, info.uid))
            .clone();
        let group = names
            .entry((false, info.gid))
            .or_insert_with(|| account_name(false, info.gid))
            .clone();
        format!("{user}:{group}")
    }
}

fn count_entries(path: &Path) -> Count {
    match fs::read_dir(path) {
        Ok(entries) => Count::Entries(entries.take(COUNT_LIMIT).count()),
        Err(_) => Count::Unreadable,
    }
}

fn account_name(user: bool, id: u32) -> String {
    let mut buffer = vec![0 as libc::c_char; 4096];
    let mut name = None;
    // SAFETY: the reentrant lookups write only into the buffers passed here,
    // and the returned name points into `buffer`, read before it is dropped.
    unsafe {
        if user {
            let mut entry: libc::passwd = std::mem::zeroed();
            let mut result = std::ptr::null_mut();
            if libc::getpwuid_r(
                id,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            ) == 0
                && !result.is_null()
            {
                name = Some(std::ffi::CStr::from_ptr(entry.pw_name));
            }
            name.map(|n| n.to_string_lossy().into_owned())
        } else {
            let mut entry: libc::group = std::mem::zeroed();
            let mut result = std::ptr::null_mut();
            if libc::getgrgid_r(
                id,
                &mut entry,
                buffer.as_mut_ptr(),
                buffer.len(),
                &mut result,
            ) == 0
                && !result.is_null()
            {
                name = Some(std::ffi::CStr::from_ptr(entry.gr_name));
            }
            name.map(|n| n.to_string_lossy().into_owned())
        }
    }
    .unwrap_or_else(|| id.to_string())
}

/// `ls -l` permissions: type, then rwx for user, group and others.
pub fn permissions(mode: u32) -> String {
    let kind = match mode & libc::S_IFMT {
        libc::S_IFDIR => 'd',
        libc::S_IFLNK => 'l',
        libc::S_IFIFO => 'p',
        libc::S_IFSOCK => 's',
        libc::S_IFCHR => 'c',
        libc::S_IFBLK => 'b',
        _ => '-',
    };
    let mut text = String::from(kind);
    for (shift, special, set, unset) in [
        (6, libc::S_ISUID, 's', 'S'),
        (3, libc::S_ISGID, 's', 'S'),
        (0, libc::S_ISVTX, 't', 'T'),
    ] {
        let bits = (mode >> shift) & 7;
        text.push(if bits & 4 != 0 { 'r' } else { '-' });
        text.push(if bits & 2 != 0 { 'w' } else { '-' });
        text.push(match (bits & 1 != 0, mode & special != 0) {
            (true, true) => set,
            (false, true) => unset,
            (true, false) => 'x',
            (false, false) => '-',
        });
    }
    text
}

/// A compact size: `812B`, `4.2K`, `13M`.
pub fn size(bytes: u64) -> String {
    if bytes < 1000 {
        return format!("{bytes}B");
    }
    let mut value = bytes as f64;
    for unit in ["K", "M", "G", "T", "P"] {
        value /= 1024.0;
        if value < 9.95 {
            return format!("{value:.1}{unit}");
        }
        if value < 999.5 || unit == "P" {
            return format!("{value:.0}{unit}");
        }
    }
    unreachable!()
}

/// A directory's entry count for the size column.
pub fn entries(count: Count) -> String {
    match count {
        Count::Entries(n) if n >= COUNT_LIMIT => format!("{}+", COUNT_LIMIT - 1),
        Count::Entries(n) => n.to_string(),
        Count::Unreadable => "?".into(),
    }
}

/// Time since `modified`: `now`, `5m`, `3h`, `2d`, `3w`, `4mo`, `2y`.
pub fn age(modified: SystemTime, now: SystemTime) -> String {
    let seconds = now
        .duration_since(modified)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (minute, hour, day) = (60, 3600, 86_400);
    match seconds {
        s if s < minute => "now".into(),
        s if s < hour => format!("{}m", s / minute),
        s if s < day => format!("{}h", s / hour),
        s if s < 7 * day => format!("{}d", s / day),
        s if s < 30 * day => format!("{}w", s / (7 * day)),
        s if s < 365 * day => format!("{}mo", (s / (30 * day)).max(1)),
        s => format!("{}y", s / (365 * day)),
    }
}

/// Bytes with thousands separators: `4,404,019`.
pub fn exact(bytes: u64) -> String {
    let digits = bytes.to_string();
    let mut text = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            text.push(',');
        }
        text.push(c);
    }
    text
}

/// Local date and time to the minute: `2026-11-01 14:03`.
pub fn timestamp(time: SystemTime) -> String {
    let seconds = match time.duration_since(SystemTime::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as libc::time_t,
        Err(e) => -(e.duration().as_secs() as libc::time_t),
    };
    // SAFETY: localtime_r only writes the struct passed to it.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&seconds, &mut tm).is_null() {
            return String::new();
        }
        tm
    };
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        tm.tm_year + 1900,
        tm.tm_mon + 1,
        tm.tm_mday,
        tm.tm_hour,
        tm.tm_min
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn sizes_ages_and_permissions_are_compact() {
        assert_eq!(size(0), "0B");
        assert_eq!(size(999), "999B");
        assert_eq!(size(1000), "1.0K");
        assert_eq!(size(4_404_019), "4.2M");
        assert_eq!(size(13 * 1024 * 1024), "13M");
        assert_eq!(size(1023 * 1024), "1.0M");
        assert_eq!(entries(Count::Entries(12)), "12");
        assert_eq!(entries(Count::Entries(COUNT_LIMIT)), "9999+");
        assert_eq!(exact(4_404_019), "4,404,019");
        assert_eq!(exact(999), "999");
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000_000);
        let ago = |s| age(now - Duration::from_secs(s), now);
        assert_eq!(ago(5), "now");
        assert_eq!(ago(300), "5m");
        assert_eq!(ago(3 * 3600), "3h");
        assert_eq!(ago(2 * 86_400), "2d");
        assert_eq!(ago(15 * 86_400), "2w");
        assert_eq!(ago(130 * 86_400), "4mo");
        assert_eq!(ago(800 * 86_400), "2y");
        assert_eq!(age(now + Duration::from_secs(9), now), "now");
        assert_eq!(permissions(libc::S_IFDIR | 0o755), "drwxr-xr-x");
        assert_eq!(permissions(libc::S_IFREG | 0o4754), "-rwsr-xr--");
        assert_eq!(permissions(libc::S_IFDIR | 0o1777), "drwxrwxrwt");
        assert_eq!(permissions(libc::S_IFLNK | 0o777), "lrwxrwxrwx");
    }
    #[test]
    fn links_counts_and_forgetting() {
        let dir = std::env::temp_dir().join(format!("fff-details-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/a"), "abc").unwrap();
        fs::write(dir.join("sub/b"), "").unwrap();
        std::os::unix::fs::symlink("sub", dir.join("good")).unwrap();
        std::os::unix::fs::symlink("missing", dir.join("bad")).unwrap();
        let details = Details::default();
        let good = details.info(&dir.join("good")).unwrap();
        assert_eq!(good.link, Some(("sub".into(), true)));
        assert!(good.directory && !good.broken());
        assert!(details.info(&dir.join("bad")).unwrap().broken());
        assert_eq!(details.info(&dir.join("sub/a")).unwrap().size, 3);
        assert_eq!(details.info(&dir.join("nothing")), None);
        assert_eq!(details.count(&dir.join("sub")), None);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !details.drain() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(details.count(&dir.join("sub")), Some(Count::Entries(2)));
        fs::write(dir.join("sub/a"), "abcdef").unwrap();
        assert_eq!(details.info(&dir.join("sub/a")).unwrap().size, 3);
        details.touched(&dir.join("sub/a"));
        assert_eq!(details.info(&dir.join("sub/a")).unwrap().size, 6);
        // Forgotten counts stay visible until the new count arrives.
        details.forget();
        assert_eq!(details.count(&dir.join("sub")), Some(Count::Entries(2)));
        fs::remove_dir_all(&dir).unwrap();
    }
}
