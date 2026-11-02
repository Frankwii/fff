//! Live updates (`[ui] watch`): inotify on the current directory, the pinned
//! roots and the folders whose entries are listed. Entries created, removed or
//! renamed there ask for a quiet rescan; entries written to or changed in
//! attributes only need their details read again.
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use std::{
    collections::HashMap,
    mem::MaybeUninit,
    os::{fd::OwnedFd, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
};

/// Most directories watched at once, leaving the system's inotify watches
/// (often 8192 per user) to other programs too.
pub const MOST: usize = 4096;

pub struct Watch {
    fd: OwnedFd,
    /// Watched directories by watch descriptor, and the reverse.
    watched: HashMap<i32, PathBuf>,
    descriptors: HashMap<PathBuf, i32>,
}

/// What happened since the last poll.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    /// Entries appeared, disappeared or were renamed, or events were lost.
    pub listing: bool,
    /// Entries whose contents or attributes changed.
    pub touched: Vec<PathBuf>,
}

impl Watch {
    /// `None` when inotify is unavailable; fff then works without live updates.
    pub fn new() -> Option<Self> {
        let fd = inotify::init(CreateFlags::NONBLOCK | CreateFlags::CLOEXEC).ok()?;
        Some(Self {
            fd,
            watched: HashMap::new(),
            descriptors: HashMap::new(),
        })
    }
    /// The directories being watched.
    pub fn watched(&self) -> impl Iterator<Item = &Path> {
        self.descriptors.keys().map(PathBuf::as_path)
    }
    /// Watch `dirs`, the first `MOST` of them, instead of what was watched.
    pub fn follow(&mut self, dirs: &[&Path]) {
        let dirs = &dirs[..dirs.len().min(MOST)];
        let wanted: std::collections::HashSet<&Path> = dirs.iter().copied().collect();
        self.descriptors.retain(|dir, wd| {
            let keep = wanted.contains(dir.as_path());
            if !keep {
                let _ = inotify::remove_watch(&self.fd, *wd);
                self.watched.remove(wd);
            }
            keep
        });
        let flags = WatchFlags::CREATE
            | WatchFlags::DELETE
            | WatchFlags::MOVED_FROM
            | WatchFlags::MOVED_TO
            | WatchFlags::DELETE_SELF
            | WatchFlags::MOVE_SELF
            | WatchFlags::MODIFY
            | WatchFlags::ATTRIB
            | WatchFlags::CLOSE_WRITE
            | WatchFlags::ONLYDIR
            | WatchFlags::EXCL_UNLINK;
        for &dir in dirs {
            if self.descriptors.contains_key(dir) {
                continue;
            }
            // An unwatchable directory (permissions) is simply not live; at
            // the system's watch limit, neither are the rest.
            match inotify::add_watch(&self.fd, dir, flags) {
                Ok(wd) => {
                    // Two paths of one directory (a link) share a descriptor.
                    if let Some(other) = self.watched.insert(wd, dir.to_path_buf()) {
                        self.descriptors.remove(&other);
                    }
                    self.descriptors.insert(dir.to_path_buf(), wd);
                }
                Err(rustix::io::Errno::NOSPC) => break,
                Err(_) => {}
            }
        }
    }
    /// Reads waiting events without blocking.
    pub fn poll(&mut self) -> Changes {
        let mut changes = Changes::default();
        if self.watched.is_empty() {
            return changes;
        }
        // A burst (an operation on a large tree) brings thousands of events.
        let mut seen = std::collections::HashSet::new();
        let mut touch = |path: PathBuf| {
            if seen.insert(path.clone()) {
                changes.touched.push(path);
            }
        };
        let mut gone = Vec::new();
        let mut listing = false;
        let mut buffer = [MaybeUninit::uninit(); 4096];
        let mut reader = inotify::Reader::new(&self.fd, &mut buffer);
        while let Ok(event) = reader.next() {
            let flags = event.events();
            if flags.contains(ReadFlags::QUEUE_OVERFLOW) {
                listing = true;
                continue;
            }
            let Some(dir) = self.watched.get(&event.wd()) else {
                continue;
            };
            if flags.contains(ReadFlags::IGNORED) {
                gone.push(event.wd());
            }
            if flags.intersects(
                ReadFlags::CREATE
                    | ReadFlags::DELETE
                    | ReadFlags::MOVED_FROM
                    | ReadFlags::MOVED_TO
                    | ReadFlags::DELETE_SELF
                    | ReadFlags::MOVE_SELF
                    | ReadFlags::IGNORED,
            ) {
                listing = true;
                // The folder's own entry count changed.
                touch(dir.clone());
            }
            if let Some(name) = event.file_name() {
                touch(dir.join(std::ffi::OsStr::from_bytes(name.to_bytes())));
            }
        }
        // The kernel dropped these watches (the directory is gone).
        for wd in gone {
            if let Some(dir) = self.watched.remove(&wd) {
                self.descriptors.remove(&dir);
            }
        }
        changes.listing = listing;
        changes
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn listing_changes_and_writes_are_told_apart() {
        let dir = std::env::temp_dir().join(format!("fff-watch-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("old"), "").unwrap();
        let Some(mut watch) = Watch::new() else {
            eprintln!("inotify unavailable; skipping");
            return;
        };
        watch.follow(&[&dir]);
        assert_eq!(watch.poll(), Changes::default());
        fs::write(dir.join("old"), "more").unwrap();
        let changes = watch.poll();
        assert!(!changes.listing);
        assert_eq!(changes.touched, [dir.join("old")]);
        fs::write(dir.join("new"), "").unwrap();
        assert!(watch.poll().listing);
        fs::rename(dir.join("new"), dir.join("renamed")).unwrap();
        assert!(watch.poll().listing);
        // Following another directory stops reporting the first one.
        watch.follow(&[&dir.join("sub")]);
        let _ = watch.poll();
        fs::write(dir.join("elsewhere"), "").unwrap();
        assert_eq!(watch.poll(), Changes::default());
        fs::write(dir.join("sub/inside"), "").unwrap();
        let changes = watch.poll();
        assert!(changes.listing);
        // A new entry also changes its folder's count.
        assert_eq!(changes.touched, [dir.join("sub"), dir.join("sub/inside")]);
        // Several directories at once: a folder and one inside it.
        fs::create_dir(dir.join("sub/deeper")).unwrap();
        watch.follow(&[&dir, &dir.join("sub/deeper")]);
        let _ = watch.poll();
        fs::write(dir.join("sub/deeper/x"), "").unwrap();
        assert!(watch.poll().listing);
        fs::write(dir.join("top"), "").unwrap();
        assert!(watch.poll().listing);
        // A removed directory's watch is dropped.
        fs::remove_dir_all(dir.join("sub")).unwrap();
        assert!(watch.poll().listing);
        assert_eq!(watch.watched().collect::<Vec<_>>(), [dir.as_path()]);
        fs::remove_dir_all(&dir).unwrap();
    }
}
