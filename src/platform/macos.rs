//! macOS: no `openat2`, `statx` or `/proc`. Paths are walked one component
//! at a time with `O_NOFOLLOW`, mount roots are where the device changes,
//! and the mount table and processes are asked of the kernel directly.
use rustix::fs::{self as sys, Mode, OFlags};
use std::{
    ffi::{CStr, OsStr, OsString},
    fs::File,
    io,
    os::unix::{
        ffi::{OsStrExt, OsStringExt},
        net::UnixStream,
    },
    path::{Component, Path, PathBuf},
};

/// No freedesktop.org desktop: no desktop entries, session bus or portal.
pub const FREEDESKTOP: bool = false;
/// macOS: files open with `open`, programs install with Homebrew.
pub const MACOS: bool = true;
/// Where fonts are installed, the user's first.
pub fn font_dirs(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = home
        .map(|h| vec![h.join("Library/Fonts")])
        .unwrap_or_default();
    dirs.extend(["/Library/Fonts", "/System/Library/Fonts"].map(PathBuf::from));
    dirs
}

/// The controlling terminal, for reading and writing. `poll` and `kqueue`
/// refuse `/dev/tty` on macOS (`POLLNVAL`), so the device it stands for is
/// opened instead: that of stdin, stderr or stdout when one is this session's
/// terminal. `/dev/tty` itself only when none is.
pub fn open_terminal() -> io::Result<File> {
    use std::os::unix::fs::OpenOptionsExt;
    let session = unsafe { libc::getsid(0) };
    for fd in [0, 2, 1] {
        if unsafe { libc::isatty(fd) } != 1 || unsafe { libc::tcgetsid(fd) } != session {
            continue;
        }
        let mut name = [0 as libc::c_char; 128];
        if unsafe { libc::ttyname_r(fd, name.as_mut_ptr(), name.len()) } != 0 {
            continue;
        }
        let name = unsafe { CStr::from_ptr(name.as_ptr()) };
        let device = Path::new(OsStr::from_bytes(name.to_bytes()));
        if let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(device)
        {
            return Ok(file);
        }
    }
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
}
/// Writes `file` through to the disk before what depends on it is published:
/// `fsync`, ordering the writes as on Linux. std's `sync_all` is
/// `F_FULLFSYNC`, which empties the drive's own cache for every file (about
/// 3 ms each: minutes for a large tree) and adds safety only against power loss.
pub fn sync(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
/// A file or folder written into a stage: written through as `sync` does,
/// there being no `syncfs` to do it for the whole stage at once.
pub fn sync_staged(file: &File) -> io::Result<()> {
    sync(file)
}
/// The stage `dir` before any of it is published: `sync_staged` already
/// wrote each of its files through.
pub fn sync_stage(_dir: &File) -> io::Result<()> {
    Ok(())
}
/// `O_SEARCH` (macOS 13+): a directory opened only to look names up in it,
/// with search permission alone, like Linux's `O_PATH`.
fn search() -> OFlags {
    OFlags::from_bits_retain(libc::O_SEARCH as u32) | OFlags::CLOEXEC
}
/// `name` in `dir` as a directory to walk through, without following a
/// symlink; read permission is needed only where `O_SEARCH` is unknown.
fn walk(dir: &File, name: &OsStr) -> io::Result<File> {
    let open = |flags| sys::openat(dir, name, flags | OFlags::NOFOLLOW, Mode::empty());
    match open(search()) {
        Err(rustix::io::Errno::INVAL) => {
            open(OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC).map(File::from)
        }
        other => other.map(File::from),
    }
    .map_err(io::Error::from)
}
/// Opens the directory at `path` (absolute, canonical) without following a
/// symlink anywhere along it: each component is opened in the one before
/// with `O_NOFOLLOW`, so a link swapped in meanwhile is refused (`ELOOP`).
pub fn open_dir_nofollow(path: &Path) -> io::Result<File> {
    let listable = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut components = path.components().peekable();
    if components.next() != Some(Component::RootDir) {
        return Err(io::Error::from(io::ErrorKind::InvalidInput));
    }
    if components.peek().is_none() {
        return Ok(File::from(sys::openat(
            sys::CWD,
            "/",
            listable,
            Mode::empty(),
        )?));
    }
    let mut dir = File::from(sys::openat(sys::CWD, "/", search(), Mode::empty())?);
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        };
        dir = if components.peek().is_some() {
            walk(&dir, name)?
        } else {
            File::from(sys::openat(&dir, name, listable, Mode::empty())?)
        };
    }
    Ok(dir)
}
/// The parent of the opened directory `dir`, opened only to be stat'ed and
/// walked through, so one that cannot be listed (mode 0711) opens too.
pub fn open_parent(dir: &File) -> io::Result<File> {
    walk(dir, OsStr::new(".."))
}
/// Where the opened directory `dir` is now, without symlinks.
pub fn dir_path(dir: &File) -> io::Result<PathBuf> {
    Ok(PathBuf::from(OsString::from_vec(
        sys::getpath(dir)?.into_bytes(),
    )))
}
/// Whether `name` in `parent` is the root of a mount: its device differs
/// from its parent's.
pub fn is_mount_root(parent: &File, name: &OsStr) -> io::Result<bool> {
    let entry = super::lstat_at(parent, name)?;
    Ok(entry.is_dir() && entry.dev != super::fstat(parent)?.dev)
}
/// The filesystem and device of an opened directory: a rename works only
/// within both.
pub type Place = (u64, u64);
pub fn place(dir: &File) -> io::Result<Place> {
    // `fsid_t` is two `i32`s, its field private in `libc`.
    let fsid: [u32; 2] = unsafe { std::mem::transmute(sys::fstatfs(dir)?.f_fsid) };
    let fsid = (u64::from(fsid[0]) << 32) | u64::from(fsid[1]);
    Ok((fsid, super::fstat(dir)?.dev))
}
/// The mounted filesystems; empty when they cannot be read.
pub fn mounts() -> crate::mounts::Mounts {
    let mut table: *mut libc::statfs = std::ptr::null_mut();
    // MNT_NOWAIT: what the kernel knows, without asking each filesystem
    // (a hung network mount would block).
    let count = unsafe { libc::getmntinfo(&mut table, libc::MNT_NOWAIT) };
    if count <= 0 || table.is_null() {
        return crate::mounts::Mounts::default();
    }
    let table = unsafe { std::slice::from_raw_parts(table, count as usize) };
    let text = |field: &[libc::c_char]| unsafe { CStr::from_ptr(field.as_ptr()) };
    crate::mounts::Mounts::new(
        table
            .iter()
            .map(|m| crate::mounts::Mount {
                point: PathBuf::from(OsString::from_vec(text(&m.f_mntonname).to_bytes().to_vec())),
                fstype: text(&m.f_fstypename).to_string_lossy().into_owned(),
            })
            .collect(),
    )
}
/// The executable process `pid` runs, if it can be found.
pub fn executable(pid: &str) -> Option<PathBuf> {
    let pid: libc::c_int = pid.parse().ok()?;
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let length =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return None;
    }
    buffer.truncate(length as usize);
    Some(PathBuf::from(OsString::from_vec(buffer)))
}
/// The parent of process `pid`. The short BSD information, unlike the
/// full one, is given for other users' processes too, such as `login`
/// between Terminal.app and the shell.
pub fn parent(pid: u32) -> Option<u32> {
    let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int;
    // SAFETY: the kernel writes at most `size` bytes into `info`.
    let written = unsafe {
        libc::proc_pidinfo(
            pid as libc::c_int,
            libc::PROC_PIDT_SHORTBSDINFO,
            0,
            (&raw mut info).cast(),
            size,
        )
    };
    (written == size).then_some(info.pbsi_ppid)
}
/// Abstract sockets are Linux's own.
pub fn connect_abstract(_name: &[u8]) -> io::Result<UnixStream> {
    Err(io::Error::from(io::ErrorKind::Unsupported))
}
/// Runs the calling thread below the interface: macOS's niceness is the
/// process's, so the thread takes the utility quality of service instead.
pub fn lower_thread(_niceness: i32) {
    // SAFETY: this changes only the calling thread's scheduling class.
    unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_UTILITY, 0);
    }
}
/// The process's umask, read without changing it: `umask(2)` only sets it,
/// and setting it would race other threads creating files. A folder made
/// with every permission keeps what the umask allows. The strictest mask
/// when it cannot be made.
pub fn umask() -> u32 {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt};
    let probe = std::env::temp_dir().join(format!(".fff-umask-{}", crate::operations::unique()));
    let mode = std::fs::DirBuilder::new()
        .mode(0o777)
        .create(&probe)
        .and_then(|()| std::fs::symlink_metadata(&probe));
    let _ = std::fs::remove_dir(&probe);
    mode.map_or(0o077, |m| !m.mode() & 0o777)
}
/// The list piped to fff, read from a copy of its stdin, while the terminal
/// takes stdin's place: crossterm reads keys from stdin when it is a
/// terminal and otherwise opens `/dev/tty`, which kqueue refuses here.
pub fn piped_input() -> Box<dyn io::Read + Send> {
    use std::os::fd::{AsFd, AsRawFd};
    let stdin = io::stdin();
    let Ok(list) = stdin.as_fd().try_clone_to_owned() else {
        return Box::new(stdin);
    };
    let Ok(terminal) = open_terminal() else {
        return Box::new(stdin);
    };
    // SAFETY: dup2 replaces descriptor 0, whose list `list` still reads.
    if unsafe { libc::dup2(terminal.as_raw_fd(), libc::STDIN_FILENO) } == -1 {
        return Box::new(stdin);
    }
    Box::new(File::from(list))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_is_the_kernels() {
        let own = unsafe { libc::getppid() } as u32;
        assert_eq!(parent(std::process::id()), Some(own));
        assert!(executable(&std::process::id().to_string()).is_some());
    }
}
