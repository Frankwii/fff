//! Row details for the metadata column, the footer, link markers and the
//! marked list. Only entries on screen are inspected: one lstat each, cached
//! until the next scan. Drawing never touches the filesystem: a slow network
//! or FUSE mount would stall the frame, so workers read entries and count
//! directories, and the frame shows what they found once it arrives (until
//! then, what they found before). Each mount has its own workers, so a stuck
//! one holds up only its own entries. Owner names, which may come from a
//! network directory service, are looked up by a worker too.
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

/// `st_mode` bits as the `u32` that `MetadataExt::mode` returns. POSIX fixes
/// their values; libc declares them `u32` on Linux and `u16` on macOS.
pub mod bits {
    pub const S_IFMT: u32 = 0o170000;
    pub const S_IFSOCK: u32 = 0o140000;
    pub const S_IFLNK: u32 = 0o120000;
    pub const S_IFREG: u32 = 0o100000;
    pub const S_IFBLK: u32 = 0o060000;
    pub const S_IFDIR: u32 = 0o040000;
    pub const S_IFCHR: u32 = 0o020000;
    pub const S_IFIFO: u32 = 0o010000;
    pub const S_ISUID: u32 = 0o4000;
    pub const S_ISGID: u32 = 0o2000;
    pub const S_ISVTX: u32 = 0o1000;
}

/// `[ui] columns`: what the right-hand side of each result row shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Column {
    /// Permissions, like `ls -l`: `drwxr-xr-x`.
    Permissions,
    /// A file's size (`4.2K`), or the size of the files in a directory.
    Size,
    /// Time since the last modification (`3d`).
    Modified,
}
pub const DEFAULT_COLUMNS: [Column; 2] = [Column::Size, Column::Modified];

#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// `st_mode`, file type bits included.
    pub mode: u32,
    /// The size in bytes; a link's is its target's (0 when broken).
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
        let resolved = m.is_symlink().then(|| fs::metadata(path).ok()).flatten();
        let link = m.is_symlink().then(|| {
            let target = fs::read_link(path).unwrap_or_default();
            (target, resolved.is_some())
        });
        Some(Self {
            mode: m.mode(),
            size: match (&link, resolved) {
                (None, _) => m.len(),
                (Some(_), resolved) => resolved.map_or(0, |r| r.len()),
            },
            modified: m.modified().ok(),
            uid: m.uid(),
            gid: m.gid(),
            directory: m.is_dir() || (link.is_some() && path.is_dir()),
            link,
        })
    }
    /// The entry itself is a directory (a link to one is not).
    pub fn is_dir(&self) -> bool {
        self.mode & bits::S_IFMT == bits::S_IFDIR
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

struct Worker<K, T> {
    requests: Sender<(u64, K)>,
    results: Receiver<(u64, K, T)>,
}
/// Workers by the mount point of the entries they read.
type Workers<T> = RefCell<HashMap<PathBuf, Worker<PathBuf, T>>>;
/// A user's (`true`) or group's id.
type Account = (bool, u32);

/// What a worker found for an entry (`None` until it answers once), and
/// whether that is current; stale values show until replaced.
type Found<T> = (Option<T>, bool);

#[derive(Default)]
pub struct Details {
    /// Entries' details; `Some(None)` for an entry that does not exist.
    infos: RefCell<HashMap<PathBuf, Found<Option<Info>>>>,
    counts: RefCell<HashMap<PathBuf, Found<Count>>>,
    info_workers: Workers<Option<Info>>,
    count_workers: Workers<Count>,
    /// The mount table, read again after each `forget`.
    mounts: RefCell<Option<Arc<crate::mounts::Mounts>>>,
    /// Requests made before the last `forget` are skipped by the workers.
    generation: Arc<AtomicU64>,
    /// Account names; `None` while being looked up.
    names: RefCell<HashMap<Account, Option<String>>>,
    name_worker: RefCell<Option<Worker<Account, String>>>,
    /// Folders' sizes, added up by workers of their own.
    sizes: crate::sizes::Sizes,
    wake: Option<SyncSender<()>>,
}
impl Details {
    /// Coalesced notification for arriving counts, shared with terminal input.
    pub fn set_waker(&mut self, wake: SyncSender<()>) {
        self.sizes.set_waker(wake.clone());
        self.wake = Some(wake);
    }
    /// The entry's details, or `None` while unknown or missing.
    pub fn info(&self, path: &Path) -> Option<Info> {
        self.lookup(path).flatten()
    }
    /// The entry's details as last read: `Some(None)` when it does not
    /// exist, `None` until a worker has read it once. Unknown or stale entries
    /// are read again in the background; without a waker (no session to
    /// redraw), they are read here.
    pub fn lookup(&self, path: &Path) -> Option<Option<Info>> {
        let mut infos = self.infos.borrow_mut();
        let entry = infos.entry(path.to_path_buf()).or_insert((None, false));
        if !entry.1 {
            entry.1 = true;
            if self.wake.is_none() {
                entry.0 = Some(Info::read(path));
            } else {
                self.request(&self.info_workers, path, |p| Info::read(p));
            }
        }
        entry.0.clone()
    }
    /// A directory's entry count, requested from the worker when unknown or
    /// stale; a stale count is shown until the new one arrives.
    pub fn count(&self, path: &Path) -> Option<Count> {
        let mut counts = self.counts.borrow_mut();
        let entry = counts.entry(path.to_path_buf()).or_insert((None, false));
        if !entry.1 {
            entry.1 = true;
            self.request(&self.count_workers, path, |p| count_entries(p));
        }
        entry.0
    }
    /// A folder's size: the bytes of the files below it on its filesystem,
    /// as far as a worker has added them up; asked for in this frame.
    pub fn size(&self, path: &Path) -> Option<crate::sizes::Size> {
        if self.on_kernel(path) {
            return None;
        }
        self.want_size(path);
        self.sizes.get(path)
    }
    /// A folder near the view whose size is added up after those drawn;
    /// never one on a kernel filesystem, whose sizes are made up.
    pub fn want_size(&self, path: &Path) {
        if !self.on_kernel(path) {
            self.sizes.want(path, self.mount_of(path));
        }
    }
    /// `path` is on a kernel filesystem (`/proc`, `/sys`, `/dev`): it has
    /// no size, by its name (drawing never reads a link).
    pub fn on_kernel(&self, path: &Path) -> bool {
        self.mounts().kernel(path)
    }
    /// A frame was drawn: workers add up the sizes it asked for, in order,
    /// and give up on others.
    pub fn commit_sizes(&self) {
        self.sizes.commit();
    }
    /// Folders' totals as they are found, for sorting by size.
    pub fn totals(&self) -> crate::sizes::Totals {
        self.sizes.totals()
    }
    /// The rows sorted by size: their folders are added up after those the
    /// frames want, but for those on a kernel filesystem; none with no runs.
    pub fn list_sizes(&self, runs: Vec<std::sync::Arc<crate::rows::Run>>) {
        self.sizes.list(runs, self.mounts());
    }
    /// Whether totals were added up since the last call.
    pub fn sizes_added(&self) -> bool {
        self.sizes.added()
    }
    /// F5: folders' sizes are added up again.
    pub fn refresh_sizes(&self) {
        self.sizes.refresh();
    }
    /// Folders' sizes are kept in `path` between sessions.
    pub fn keep_sizes(&mut self, path: PathBuf) {
        self.sizes.keep(path);
    }
    /// Asks the worker for `path`'s mount to `read` it.
    fn request<T: Send + 'static>(
        &self,
        workers: &Workers<T>,
        path: &Path,
        read: fn(&PathBuf) -> T,
    ) {
        let mount = self.mount_of(path);
        let mut workers = workers.borrow_mut();
        let worker = workers
            .entry(mount)
            .or_insert_with(|| self.spawn(self.generation.clone(), read));
        let _ = worker
            .requests
            .send((self.generation.load(Ordering::Relaxed), path.to_path_buf()));
    }
    /// The mount point `path` is under, by its name: reading the mount table
    /// never waits for a mount.
    fn mount_of(&self, path: &Path) -> PathBuf {
        self.mounts().point(path)
    }
    fn mounts(&self) -> Arc<crate::mounts::Mounts> {
        self.mounts
            .borrow_mut()
            .get_or_insert_with(crate::mounts::current)
            .clone()
    }
    fn spawn<K: Send + 'static, T: Send + 'static>(
        &self,
        generation: Arc<AtomicU64>,
        read: fn(&K) -> T,
    ) -> Worker<K, T> {
        let (requests, incoming) = mpsc::channel::<(u64, K)>();
        let (outgoing, results) = mpsc::channel();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            for (asked, path) in incoming {
                if asked != generation.load(Ordering::Relaxed) {
                    continue;
                }
                let found = read(&path);
                if outgoing.send((asked, path, found)).is_err() {
                    return;
                }
                if let Some(wake) = &wake {
                    let _ = wake.try_send(());
                }
            }
        });
        Worker { requests, results }
    }
    /// Takes arrived details and counts; returns whether any changed what is
    /// shown.
    pub fn drain(&self) -> bool {
        let current = self.generation.load(Ordering::Relaxed);
        let mut changed = false;
        for worker in self.info_workers.borrow().values() {
            changed |= drain_into(worker, &self.infos, current);
        }
        for worker in self.count_workers.borrow().values() {
            changed |= drain_into(worker, &self.counts, current);
        }
        changed |= self.sizes.drain();
        if let Some(worker) = self.name_worker.borrow().as_ref() {
            let mut names = self.names.borrow_mut();
            while let Ok((_, account, name)) = worker.results.try_recv() {
                names.insert(account, Some(name));
                changed = true;
            }
        }
        changed
    }
    /// Everything may have changed (a new scan): read details again as rows
    /// are drawn, keeping old counts on screen until new ones arrive.
    pub fn forget(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        *self.mounts.borrow_mut() = None;
        crate::mounts::refresh();
        let mut infos = self.infos.borrow_mut();
        if infos.len() > COUNT_CACHE {
            infos.clear();
        }
        for entry in infos.values_mut() {
            entry.1 = false;
        }
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
        if let Some(entry) = self.infos.borrow_mut().get_mut(path) {
            entry.1 = false;
        }
        if let Some(entry) = self.counts.borrow_mut().get_mut(path) {
            entry.1 = false;
        }
        self.sizes.touched(path);
    }
    /// `user:group`, by name where the system knows one; by number while
    /// the name is being looked up.
    pub fn owner(&self, info: &Info) -> String {
        let user = self.name((true, info.uid));
        let group = self.name((false, info.gid));
        format!("{user}:{group}")
    }
    fn name(&self, account: Account) -> String {
        let mut names = self.names.borrow_mut();
        let name = names.entry(account).or_insert_with(|| {
            if self.wake.is_none() {
                return Some(account_name(&account));
            }
            let mut worker = self.name_worker.borrow_mut();
            // Names do not change with a scan: no generation to skip.
            let worker = worker.get_or_insert_with(|| self.spawn(Arc::default(), account_name));
            let _ = worker.requests.send((0, account));
            None
        });
        name.clone().unwrap_or_else(|| account.1.to_string())
    }
}

/// Takes what `worker` found for the current generation into `found`;
/// returns whether any of it changed.
fn drain_into<T: PartialEq>(
    worker: &Worker<PathBuf, T>,
    found: &RefCell<HashMap<PathBuf, Found<T>>>,
    current: u64,
) -> bool {
    let mut found = found.borrow_mut();
    let mut changed = false;
    while let Ok((asked, path, value)) = worker.results.try_recv() {
        if asked == current
            && let Some(entry) = found.get_mut(&path)
        {
            changed |= entry.0.as_ref() != Some(&value);
            *entry = (Some(value), true);
        }
    }
    changed
}

fn count_entries(path: &Path) -> Count {
    match fs::read_dir(path) {
        Ok(entries) => Count::Entries(entries.take(COUNT_LIMIT).count()),
        Err(_) => Count::Unreadable,
    }
}

fn account_name(&(user, id): &Account) -> String {
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
    let kind = match mode & bits::S_IFMT {
        bits::S_IFDIR => 'd',
        bits::S_IFLNK => 'l',
        bits::S_IFIFO => 'p',
        bits::S_IFSOCK => 's',
        bits::S_IFCHR => 'c',
        bits::S_IFBLK => 'b',
        _ => '-',
    };
    let mut text = String::from(kind);
    for (shift, special, set, unset) in [
        (6, bits::S_ISUID, 's', 'S'),
        (3, bits::S_ISGID, 's', 'S'),
        (0, bits::S_ISVTX, 't', 'T'),
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

/// A folder's size for the size column: `…` while it is added up, `?`
/// when it cannot be read.
pub fn folder_size(size: crate::sizes::Size) -> String {
    if size.unreadable {
        "?".into()
    } else if size.counting {
        self::size(size.bytes) + "…"
    } else {
        self::size(size.bytes)
    }
}

/// A directory's entry count for the footer.
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
        assert_eq!(permissions(bits::S_IFDIR | 0o755), "drwxr-xr-x");
        assert_eq!(permissions(bits::S_IFREG | 0o4754), "-rwsr-xr--");
        assert_eq!(permissions(bits::S_IFDIR | 0o1777), "drwxrwxrwt");
        assert_eq!(permissions(bits::S_IFLNK | 0o777), "lrwxrwxrwx");
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
        std::os::unix::fs::symlink("sub/a", dir.join("file-link")).unwrap();
        assert_eq!(details.info(&dir.join("file-link")).unwrap().size, 3);
        assert_eq!(details.info(&dir.join("sub/a")).unwrap().size, 3);
        assert_eq!(details.info(&dir.join("nothing")), None);
        assert_eq!(details.count(&dir.join("sub")), None);
        let deadline = std::time::Instant::now() + Duration::from_secs(300);
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
    #[test]
    fn a_session_reads_entries_in_the_background_showing_stale_ones_meanwhile() {
        let dir = std::env::temp_dir().join(format!("fff-details-bg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a"), "abc").unwrap();
        let mut details = Details::default();
        let (wake, woken) = mpsc::sync_channel(1);
        details.set_waker(wake);
        let settle = |details: &Details| {
            woken.recv_timeout(Duration::from_secs(300)).unwrap();
            assert!(details.drain());
        };
        // Unknown until the worker answers; then known, missing or not.
        assert_eq!(details.lookup(&dir.join("a")), None);
        settle(&details);
        assert_eq!(details.info(&dir.join("a")).unwrap().size, 3);
        assert_eq!(details.lookup(&dir.join("gone")), None);
        settle(&details);
        assert_eq!(details.lookup(&dir.join("gone")), Some(None));
        // After a change, the old details show until the new ones arrive.
        fs::write(dir.join("a"), "abcdef").unwrap();
        details.touched(&dir.join("a"));
        assert_eq!(details.info(&dir.join("a")).unwrap().size, 3);
        settle(&details);
        assert_eq!(details.info(&dir.join("a")).unwrap().size, 6);
        // Answers asked for before a forget are dropped, then asked again.
        fs::remove_file(dir.join("a")).unwrap();
        details.forget();
        assert_eq!(details.info(&dir.join("a")).unwrap().size, 6);
        settle(&details);
        assert_eq!(details.lookup(&dir.join("a")), Some(None));
        // Owners show by number until their names arrive.
        let info = Info::read(&dir).unwrap();
        let ids = format!("{}:{}", info.uid, info.gid);
        assert_eq!(details.owner(&info), ids);
        let names = format!(
            "{}:{}",
            account_name(&(true, info.uid)),
            account_name(&(false, info.gid))
        );
        // The user's and the group's names may arrive separately.
        let deadline = std::time::Instant::now() + Duration::from_secs(300);
        while details.owner(&info) != names && std::time::Instant::now() < deadline {
            let _ = woken.recv_timeout(Duration::from_millis(50));
            details.drain();
        }
        assert_eq!(details.owner(&info), names);
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn entries_are_read_by_their_mounts_workers() {
        let details = Details::default();
        let table = "/dev/sda1 / ext4 rw 0 0\nproc /proc proc rw 0 0\n\
                     a /mnt ext4 rw 0 0\nb /mnt/a\\040b ext4 rw 0 0\n";
        *details.mounts.borrow_mut() = Some(Arc::new(crate::mounts::Mounts::parse(table)));
        assert_eq!(
            details.mount_of(Path::new("/mnt/a b/c")),
            Path::new("/mnt/a b")
        );
        assert_eq!(details.mount_of(Path::new("/mnt/a bc")), Path::new("/mnt"));
        assert_eq!(details.mount_of(Path::new("/home")), Path::new("/"));
        // Kernel filesystems have no sizes to add up.
        assert!(details.on_kernel(Path::new("/proc/1")));
        details.want_size(Path::new("/proc"));
        details.want_size(Path::new("/home"));
        assert_eq!(details.sizes.wanted(), [PathBuf::from("/home")]);
        // Without a table, everything is on `/`.
        *details.mounts.borrow_mut() = Some(Arc::default());
        assert_eq!(details.mount_of(Path::new("/home")), Path::new("/"));
    }
}
