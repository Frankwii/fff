//! Linux: `openat2`, `statx` (5.8+), `O_PATH` and `/proc`.
use rustix::fs::{self as sys, AtFlags, Mode, OFlags, ResolveFlags, StatxFlags};
use std::{
    ffi::OsStr,
    fs::File,
    io,
    os::{fd::AsRawFd, unix::net::UnixStream},
    path::{Path, PathBuf},
};

/// The freedesktop.org desktop (desktop entries, D-Bus, the portal) is here.
pub const FREEDESKTOP: bool = true;

/// The controlling terminal, for reading and writing.
pub fn open_terminal() -> io::Result<File> {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
}
/// Writes `file` through to the disk before what depends on it is published.
pub fn sync(file: &File) -> io::Result<()> {
    file.sync_all()
}
/// Opens the directory at `path` (absolute, canonical) without following a
/// symlink anywhere along it.
pub fn open_dir_nofollow(path: &Path) -> io::Result<File> {
    Ok(File::from(sys::openat2(
        sys::CWD,
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
        ResolveFlags::NO_SYMLINKS,
    )?))
}
/// The parent of the opened directory `dir`, opened only to be stat'ed and
/// walked through, so one that cannot be listed (mode 0711) opens too.
pub fn open_parent(dir: &File) -> io::Result<File> {
    Ok(File::from(sys::openat(
        dir,
        "..",
        OFlags::PATH | OFlags::DIRECTORY | OFlags::CLOEXEC,
        Mode::empty(),
    )?))
}
/// Where the opened directory `dir` is now, without symlinks.
pub fn dir_path(dir: &File) -> io::Result<PathBuf> {
    std::fs::read_link(format!("/proc/self/fd/{}", dir.as_raw_fd()))
}
/// Whether `name` in `parent` is the root of a mount, bind mounts of the
/// same filesystem included.
pub fn is_mount_root(parent: &File, name: &OsStr) -> io::Result<bool> {
    let x = sys::statx(
        parent,
        name,
        AtFlags::SYMLINK_NOFOLLOW | AtFlags::NO_AUTOMOUNT,
        StatxFlags::BASIC_STATS,
    )?;
    if !x
        .stx_attributes_mask
        .contains(sys::StatxAttributes::MOUNT_ROOT)
    {
        return Err(io::Error::other(
            "Cannot detect mount points; Linux 5.8 or newer is required",
        ));
    }
    Ok(x.stx_attributes.contains(sys::StatxAttributes::MOUNT_ROOT))
}
/// The mount and device of an opened directory: a rename works only within
/// both.
pub type Place = (u64, u32, u32);
pub fn place(dir: &File) -> io::Result<Place> {
    let x = sys::statx(dir, "", AtFlags::EMPTY_PATH, StatxFlags::MNT_ID)?;
    if x.stx_mask & StatxFlags::MNT_ID.bits() == 0 {
        return Err(io::Error::other(
            "Cannot identify mounts; Linux 5.8 or newer is required",
        ));
    }
    Ok((x.stx_mnt_id, x.stx_dev_major, x.stx_dev_minor))
}
/// This process's mount table; empty when it cannot be read.
pub fn mounts() -> crate::mounts::Mounts {
    std::fs::read_to_string("/proc/self/mounts")
        .map(|table| crate::mounts::Mounts::parse(&table))
        .unwrap_or_default()
}
/// The executable process `pid` runs, if it can be found.
pub fn executable(pid: &str) -> Option<PathBuf> {
    std::fs::read_link(Path::new("/proc").join(pid).join("exe")).ok()
}
/// Connects to the socket with this name in the abstract namespace.
pub fn connect_abstract(name: &[u8]) -> io::Result<UnixStream> {
    use std::os::linux::net::SocketAddrExt;
    let address = std::os::unix::net::SocketAddr::from_abstract_name(name)?;
    UnixStream::connect_addr(&address)
}
/// Runs the calling thread at `niceness`: Linux keeps a niceness per thread.
pub fn lower_thread(niceness: i32) {
    // SAFETY: setpriority changes only the calling thread's priority.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, niceness);
    }
}
/// The process's umask, read without changing it: `umask(2)` only sets it,
/// and setting it would race other threads creating files. The strictest
/// mask when it cannot be read.
pub fn umask() -> u32 {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            let mask = status.lines().find_map(|l| l.strip_prefix("Umask:"))?;
            u32::from_str_radix(mask.trim(), 8).ok()
        })
        .map_or(0o077, |mask| mask & 0o777)
}
