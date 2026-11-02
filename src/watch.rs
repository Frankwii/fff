//! Live updates (`[ui] watch`): inotify on the current directory. Entries
//! created, removed or renamed there ask for a quiet rescan; entries written to
//! or changed in attributes only need their details read again.
use rustix::fs::inotify::{self, CreateFlags, ReadFlags, WatchFlags};
use std::{
    mem::MaybeUninit,
    os::{fd::OwnedFd, unix::ffi::OsStrExt},
    path::{Path, PathBuf},
};

pub struct Watch {
    fd: OwnedFd,
    watched: Option<(PathBuf, i32)>,
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
        Some(Self { fd, watched: None })
    }
    /// Watch `dir` instead of the previous directory.
    pub fn follow(&mut self, dir: &Path) {
        if self.watched.as_ref().is_some_and(|(d, _)| d == dir) {
            return;
        }
        if let Some((_, wd)) = self.watched.take() {
            let _ = inotify::remove_watch(&self.fd, wd);
        }
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
        // An unwatchable directory (permissions, watch limit) is simply not live.
        if let Ok(wd) = inotify::add_watch(&self.fd, dir, flags) {
            self.watched = Some((dir.to_path_buf(), wd));
        }
    }
    /// Reads waiting events without blocking.
    pub fn poll(&mut self) -> Changes {
        let mut changes = Changes::default();
        let Some((dir, wd)) = self.watched.clone() else {
            return changes;
        };
        let mut buffer = [MaybeUninit::uninit(); 4096];
        let mut reader = inotify::Reader::new(&self.fd, &mut buffer);
        while let Ok(event) = reader.next() {
            let flags = event.events();
            if flags.contains(ReadFlags::QUEUE_OVERFLOW) {
                changes.listing = true;
                continue;
            }
            if event.wd() != wd {
                continue;
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
                changes.listing = true;
            }
            if let Some(name) = event.file_name() {
                let path = dir.join(std::ffi::OsStr::from_bytes(name.to_bytes()));
                if !changes.touched.contains(&path) {
                    changes.touched.push(path);
                }
            }
        }
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
        watch.follow(&dir);
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
        watch.follow(&dir.join("sub"));
        let _ = watch.poll();
        fs::write(dir.join("elsewhere"), "").unwrap();
        assert_eq!(watch.poll(), Changes::default());
        fs::write(dir.join("sub/inside"), "").unwrap();
        assert!(watch.poll().listing);
        fs::remove_dir_all(&dir).unwrap();
    }
}
