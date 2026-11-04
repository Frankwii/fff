//! The mount table: which mount a path is on and its filesystem's type, read
//! from `/proc/self/mounts` by name, so that looking a path up never waits
//! for a mount. Read once and again after `refresh` (each new scan, F5).
//!
//! Kernel filesystems (`/proc`, `/sys`, `/dev`, ...) hold no files of their
//! own: their sizes are made up (`/proc/kcore` claims the address space),
//! their contents change as they are read, entries vanish with the processes
//! they describe, and some reads block or act on hardware. fff lists them,
//! but does not add up their sizes, scan into them from outside, search
//! their contents or watch them, and reads only a little of a file there
//! for the preview.
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// What a filesystem is, as far as fff treats them differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Made up by the kernel: `proc`, `sysfs`, `devtmpfs`, ...
    Kernel,
    /// A local disk (or memory) filesystem known to answer quickly.
    Local,
    /// Anything else: remote, FUSE, unknown.
    Other,
}

/// The kernel's own filesystems: a closed list, fixed by Linux.
const KERNEL: [&str; 19] = [
    "proc",
    "sysfs",
    "devtmpfs",
    "devpts",
    "cgroup",
    "cgroup2",
    "debugfs",
    "tracefs",
    "securityfs",
    "pstore",
    "bpf",
    "configfs",
    "efivarfs",
    "fusectl",
    "mqueue",
    "hugetlbfs",
    "binfmt_misc",
    "nsfs",
    "rpc_pipefs",
];
/// Local filesystems quick enough to read ahead of the highlight (prefetch).
const LOCAL: [&str; 9] = [
    "ext2", "ext3", "ext4", "btrfs", "xfs", "tmpfs", "overlay", "f2fs", "zfs",
];

/// One line of the table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mount {
    pub point: PathBuf,
    pub fstype: String,
}
impl Mount {
    pub fn kind(&self) -> Kind {
        if KERNEL.contains(&self.fstype.as_str()) {
            Kind::Kernel
        } else if LOCAL.contains(&self.fstype.as_str()) {
            Kind::Local
        } else {
            Kind::Other
        }
    }
}

/// The mounts, longest mount point first; of mounts stacked on one point,
/// the last mounted (the one seen there) first.
#[derive(Clone, Debug, Default)]
pub struct Mounts(Vec<Mount>);
impl Mounts {
    /// A `/proc/self/mounts` table, whose fields escape spaces, tabs,
    /// newlines and backslashes in octal.
    pub fn parse(table: &str) -> Self {
        let mut mounts: Vec<Mount> = table
            .lines()
            .filter_map(|line| {
                let mut fields = line.split(' ');
                let point = unescape(fields.nth(1)?);
                let fstype = fields.next()?.to_string();
                Some(Mount { point, fstype })
            })
            .collect();
        mounts.reverse();
        mounts.sort_by_key(|m| std::cmp::Reverse(m.point.as_os_str().len()));
        Self(mounts)
    }
    /// This process's mount table; empty when it cannot be read.
    pub fn read() -> Self {
        std::fs::read_to_string("/proc/self/mounts")
            .map(|table| Self::parse(&table))
            .unwrap_or_default()
    }
    /// The mount `path` is on, by its name; `None` without a table.
    pub fn of(&self, path: &Path) -> Option<&Mount> {
        self.0.iter().find(|m| path.starts_with(&m.point))
    }
    /// The mount point `path` is under; `/` without a table.
    pub fn point(&self, path: &Path) -> PathBuf {
        self.of(path)
            .map_or_else(|| PathBuf::from("/"), |m| m.point.clone())
    }
    pub fn kind(&self, path: &Path) -> Kind {
        self.of(path).map_or(Kind::Other, Mount::kind)
    }
    /// `path` is on a kernel filesystem.
    pub fn kernel(&self, path: &Path) -> bool {
        self.kind(path) == Kind::Kernel
    }
    /// `path` is where a kernel filesystem is mounted: entering it from
    /// outside is entering one.
    pub fn kernel_point(&self, path: &Path) -> bool {
        self.0
            .iter()
            .any(|m| m.point == path && m.kind() == Kind::Kernel)
    }
    /// Both paths are on one mount, of a local filesystem.
    pub fn local_same_mount(&self, parent: &Path, target: &Path) -> bool {
        match (self.of(parent), self.of(target)) {
            (Some(a), Some(b)) => std::ptr::eq(a, b) && a.kind() == Kind::Local,
            _ => false,
        }
    }
}

fn unescape(field: &str) -> PathBuf {
    use std::os::unix::ffi::OsStringExt;
    let (mut bytes, mut rest) = (Vec::new(), field.as_bytes());
    while let Some((&b, tail)) = rest.split_first() {
        let code = tail
            .get(..3)
            .and_then(|d| std::str::from_utf8(d).ok())
            .and_then(|d| u8::from_str_radix(d, 8).ok());
        match code {
            Some(c) if b == b'\\' => {
                bytes.push(c);
                rest = &tail[3..];
            }
            _ => {
                bytes.push(b);
                rest = tail;
            }
        }
    }
    PathBuf::from(std::ffi::OsString::from_vec(bytes))
}

static CURRENT: Mutex<Option<Arc<Mounts>>> = Mutex::new(None);

/// The table as last read, read now if it was not yet (or since `refresh`).
pub fn current() -> Arc<Mounts> {
    CURRENT
        .lock()
        .unwrap()
        .get_or_insert_with(|| Arc::new(Mounts::read()))
        .clone()
}
/// Mounts may have come and gone: the table is read again when next needed.
pub fn refresh() {
    *CURRENT.lock().unwrap() = None;
}
/// `path`, or what it links to, is on a kernel filesystem. Reads the link,
/// so only for workers, never while drawing.
pub fn kernel_file(path: &Path) -> bool {
    let mounts = current();
    mounts.kernel(path)
        || (path.is_symlink()
            && std::fs::canonicalize(path).is_ok_and(|real| mounts.kernel(&real)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "/dev/sda1 / ext4 rw 0 0\n\
                         proc /proc proc rw 0 0\n\
                         sys /sys sysfs rw 0 0\n\
                         cgroup2 /sys/fs/cgroup cgroup2 rw 0 0\n\
                         dev /dev devtmpfs rw 0 0\n\
                         share /mnt/my\\040disk\\134x nfs rw 0 0\n\
                         /dev/sdb1 /home/child ext4 rw 0 0\n\
                         tmpfs /home/child tmpfs rw 0 0\n";

    #[test]
    fn the_table_decodes_points_and_finds_the_longest() {
        let mounts = Mounts::parse(TABLE);
        let point = |p: &str| mounts.point(Path::new(p));
        assert_eq!(point("/mnt/my disk\\x/a"), Path::new("/mnt/my disk\\x"));
        assert_eq!(point("/sys/fs/cgroup/user"), Path::new("/sys/fs/cgroup"));
        assert_eq!(point("/system"), Path::new("/"));
        assert_eq!(point("/proc"), Path::new("/proc"));
        // Stacked on one point, the last mounted is the one seen.
        assert_eq!(
            mounts.of(Path::new("/home/child/x")).unwrap().fstype,
            "tmpfs"
        );
        // Without a table, everything is on `/`, of no known kind.
        let none = Mounts::default();
        assert_eq!(none.point(Path::new("/home")), Path::new("/"));
        assert_eq!(none.kind(Path::new("/proc")), Kind::Other);
    }

    #[test]
    fn kernel_filesystems_are_known_by_type_not_path() {
        let mounts = Mounts::parse(TABLE);
        for path in ["/proc", "/proc/1/fd", "/sys/class", "/sys/fs/cgroup/a", "/dev/null"] {
            assert!(mounts.kernel(Path::new(path)), "{path}");
        }
        for path in ["/", "/home", "/procfile", "/mnt/my disk\\x"] {
            assert!(!mounts.kernel(Path::new(path)), "{path}");
        }
        assert!(mounts.kernel_point(Path::new("/proc")));
        assert!(mounts.kernel_point(Path::new("/sys/fs/cgroup")));
        assert!(!mounts.kernel_point(Path::new("/proc/1")));
        assert!(!mounts.kernel_point(Path::new("/home/child")));
        // A kernel filesystem mounted elsewhere is one too.
        let moved = Mounts::parse("/dev/sda1 / ext4 rw 0 0\nproc /srv/p proc rw 0 0\n");
        assert!(moved.kernel(Path::new("/srv/p/self")));
        assert!(!moved.kernel(Path::new("/proc")));
    }

    #[test]
    fn prefetch_stays_on_one_local_mount() {
        let mounts = Mounts::parse(TABLE);
        let same = |a: &str, b: &str| mounts.local_same_mount(Path::new(a), Path::new(b));
        assert!(same("/home", "/home/x"));
        assert!(!same("/home", "/home/child"));
        assert!(!same("/mnt/my disk\\x", "/mnt/my disk\\x/a"));
        assert!(!same("/proc", "/proc/1"));
        assert!(!same("/home", "/proc"));
        assert!(!Mounts::default().local_same_mount(Path::new("/a"), Path::new("/a/b")));
    }
}
