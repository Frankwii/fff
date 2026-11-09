//! What differs between Linux and macOS, behind one interface: the
//! controlling terminal, syncing a file or a stage to disk, opening a directory without
//! following symlinks, where an opened directory is,
//! mount roots and identities, the mount table, the program a process runs
//! and its parent, and abstract sockets. The rest of fff is the same code on both systems,
//! so `cfg(target_os)` stays in this module.
//!
//! Entries are read relative to directory descriptors (`lstat_at`,
//! `read_dir`), never through a path that names a descriptor: such paths
//! (`/proc/self/fd/N`) exist only on Linux, and need `/proc` mounted there.
use rustix::fs::{self as sys, AtFlags, Dir};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    io,
    os::unix::ffi::OsStrExt,
};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::*;

/// The fields of an entry's `stat` that fff compares, with the types
/// `std::os::unix::fs::MetadataExt` gives them on every system.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Meta {
    pub dev: u64,
    pub ino: u64,
    pub len: u64,
    pub atime: i64,
    pub atime_nsec: i64,
    pub mtime: i64,
    pub mtime_nsec: i64,
    pub ctime: i64,
    pub ctime_nsec: i64,
    pub mode: u32,
    pub uid: u32,
}
impl Meta {
    // The widths of `stat`'s fields differ between systems (`st_dev` is `i32`
    // on macOS, `st_mode` `u16`); these are the conversions std makes.
    #[allow(clippy::unnecessary_cast)]
    fn of(s: &sys::Stat) -> Self {
        Self {
            dev: s.st_dev as u64,
            ino: s.st_ino as u64,
            len: s.st_size as u64,
            atime: s.st_atime as i64,
            atime_nsec: s.st_atime_nsec as i64,
            mtime: s.st_mtime as i64,
            mtime_nsec: s.st_mtime_nsec as i64,
            ctime: s.st_ctime as i64,
            ctime_nsec: s.st_ctime_nsec as i64,
            mode: s.st_mode as u32,
            uid: s.st_uid as u32,
        }
    }
    fn kind(&self) -> u32 {
        self.mode & crate::details::bits::S_IFMT
    }
    pub fn is_dir(&self) -> bool {
        self.kind() == crate::details::bits::S_IFDIR
    }
    pub fn is_file(&self) -> bool {
        self.kind() == crate::details::bits::S_IFREG
    }
    pub fn is_symlink(&self) -> bool {
        self.kind() == crate::details::bits::S_IFLNK
    }
}

/// `name` in `dir`, without following a final symlink.
pub fn lstat_at(dir: &File, name: &OsStr) -> io::Result<Meta> {
    Ok(Meta::of(&sys::statat(
        dir,
        name,
        AtFlags::SYMLINK_NOFOLLOW,
    )?))
}
/// The opened entry itself.
pub fn fstat(file: &File) -> io::Result<Meta> {
    Ok(Meta::of(&sys::fstat(file)?))
}

/// One entry of a directory listing.
pub struct DirEntry {
    pub name: OsString,
    pub ino: u64,
}
/// The entries of the opened directory `dir`, without `.` and `..`, in the
/// order the filesystem gives them.
pub fn read_dir(dir: &File) -> io::Result<Vec<DirEntry>> {
    let mut entries = Vec::new();
    for entry in Dir::read_from(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        entries.push(DirEntry {
            name: OsStr::from_bytes(name).to_owned(),
            ino: entry.ino(),
        });
    }
    Ok(entries)
}

/// File permission bits as `rustix` takes them (`mode_t` is `u16` on macOS).
#[allow(clippy::unnecessary_cast)]
pub fn mode(bits: u32) -> sys::Mode {
    sys::Mode::from_bits_truncate(bits as sys::RawMode)
}
