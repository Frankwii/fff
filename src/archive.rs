//! Read archives through libarchive, but create entries ourselves. No shell or
//! external extractor is invoked. Links, devices and escaping paths are refused.
use crate::operations::{Result, child_dir, err};
use rustix::fs::{self as sys, Mode, OFlags};
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, OsString, c_char, c_int, c_void},
    fs::File,
    io::Write,
    os::{fd::AsRawFd, unix::ffi::OsStringExt},
    path::{Component, Path, PathBuf},
    ptr,
};
#[link(name = "archive")]
unsafe extern "C" {
    fn archive_read_new() -> *mut c_void;
    fn archive_read_support_filter_none(a: *mut c_void) -> c_int;
    fn archive_read_support_filter_gzip(a: *mut c_void) -> c_int;
    fn archive_read_support_filter_bzip2(a: *mut c_void) -> c_int;
    fn archive_read_support_filter_xz(a: *mut c_void) -> c_int;
    fn archive_read_support_filter_zstd(a: *mut c_void) -> c_int;
    fn archive_read_support_format_all(a: *mut c_void) -> c_int;
    fn archive_read_open_fd(a: *mut c_void, fd: c_int, block: usize) -> c_int;
    fn archive_read_next_header(a: *mut c_void, entry: *mut *mut c_void) -> c_int;
    fn archive_read_data(a: *mut c_void, buffer: *mut c_void, size: usize) -> isize;
    fn archive_read_free(a: *mut c_void) -> c_int;
    fn archive_error_string(a: *mut c_void) -> *const c_char;
    fn archive_entry_pathname(e: *mut c_void) -> *const c_char;
    fn archive_entry_filetype(e: *mut c_void) -> libc::mode_t;
    fn archive_entry_hardlink(e: *mut c_void) -> *const c_char;
    fn archive_entry_size(e: *mut c_void) -> i64;
    fn archive_entry_perm(e: *mut c_void) -> libc::mode_t;
}
struct Reader(*mut c_void);
impl Drop for Reader {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                archive_read_free(self.0);
            }
        }
    }
}
impl Reader {
    fn error(&self) -> String {
        unsafe {
            let p = archive_error_string(self.0);
            if p.is_null() {
                "Invalid or unsupported archive".into()
            } else {
                format!("Archive: {}", CStr::from_ptr(p).to_string_lossy())
            }
        }
    }
}
/// Absolute, parent-traversing and backslash-separated names are refused even on
/// Linux, as are control characters, C1 ones included in UTF-8 names (a newline
/// would forge a second name, a CSI drive the terminal), and
/// names fff reserves for its own staging directories.
pub fn safe_path(path: &Path) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&b'\\')
        || bytes.iter().any(|b| *b < 0x20 || *b == 0x7f)
        || path
            .to_str()
            .is_some_and(|t| t.chars().any(char::is_control))
        || path.components().any(|c| {
            c.as_os_str()
                .as_bytes()
                .starts_with(crate::operations::STAGE_PREFIX.as_bytes())
        })
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err("Archive entry has an unsafe path".into());
    }
    let path = path
        .components()
        .filter_map(|c| {
            if let Component::Normal(s) = c {
                Some(s)
            } else {
                None
            }
        })
        .collect::<PathBuf>();
    if path.components().count() > 64 {
        return Err("Archive paths are too deep".into());
    }
    Ok(path)
}
/// `tick` hears of every chunk of bytes written; an error from it stops.
pub fn extract(input: File, stage: &File, tick: &dyn Fn(u64) -> Result<()>) -> Result<()> {
    scan(input, Some(stage), tick)
}
pub fn validate(input: File) -> Result<()> {
    scan(input, None, &|_| Ok(()))
}
/// An archive entry's name, whether it is a directory, and its size.
pub type Entry = (PathBuf, bool, u64);
/// The first `limit` entries' names, whether each is a directory, and their
/// sizes, for the preview pane; true when more follow. Nothing is extracted or
/// validated, and `stop` is checked between entries.
pub fn list(input: File, limit: usize, stop: &dyn Fn() -> bool) -> Result<(Vec<Entry>, bool)> {
    let a = open(&input)?;
    let mut entries = Vec::new();
    loop {
        if stop() {
            return Err("Stopped".into());
        }
        let mut entry = ptr::null_mut();
        let code = unsafe { archive_read_next_header(a.0, &mut entry) };
        if code == 1 {
            return Ok((entries, false));
        }
        if code != 0 || entry.is_null() {
            // A damaged tail still lists what came before it.
            return if entries.is_empty() {
                Err(a.error())
            } else {
                Ok((entries, true))
            };
        }
        if entries.len() == limit {
            return Ok((entries, true));
        }
        let (path, kind, size) = unsafe {
            let p = archive_entry_pathname(entry);
            if p.is_null() {
                continue;
            }
            (
                PathBuf::from(OsString::from_vec(CStr::from_ptr(p).to_bytes().to_vec())),
                archive_entry_filetype(entry),
                archive_entry_size(entry),
            )
        };
        entries.push((path, kind == 0o040000, size.max(0) as u64));
    }
}
type Filter = unsafe extern "C" fn(*mut c_void) -> c_int;
/// The decompression filters fff reads with, where libarchive has them
/// built in.
const FILTERS: [Filter; 4] = [
    archive_read_support_filter_gzip,
    archive_read_support_filter_bzip2,
    archive_read_support_filter_xz,
    archive_read_support_filter_zstd,
];
/// Which of `FILTERS` this libarchive has built in. Without one it
/// answers `ARCHIVE_WARN` and would run an external program instead
/// (macOS's libarchive has no zstd), so that filter stays off.
fn built_in() -> &'static [bool; 4] {
    static BUILT_IN: std::sync::OnceLock<[bool; 4]> = std::sync::OnceLock::new();
    BUILT_IN.get_or_init(|| {
        FILTERS.map(|filter| {
            let probe = Reader(unsafe { archive_read_new() });
            !probe.0.is_null() && unsafe { filter(probe.0) } == 0
        })
    })
}
/// A reader for `input` with the built-in filters and every format.
fn open(input: &File) -> Result<Reader> {
    let a = Reader(unsafe { archive_read_new() });
    if a.0.is_null() {
        return Err("Cannot allocate archive reader".into());
    }
    // These built-in filters avoid libarchive's optional external helper
    // programs: one this libarchive lacks is left out.
    unsafe {
        for code in [
            archive_read_support_filter_none(a.0),
            archive_read_support_format_all(a.0),
        ] {
            if code != 0 {
                return Err(a.error());
            }
        }
        for (filter, built_in) in FILTERS.iter().zip(built_in()) {
            if *built_in && filter(a.0) != 0 {
                return Err(a.error());
            }
        }
        if archive_read_open_fd(a.0, input.as_raw_fd(), 64 * 1024) != 0 {
            return Err(a.error());
        }
    }
    Ok(a)
}
/// The directory `path` (relative, already checked by `safe_path`) below
/// `stage`, made with the default mode where missing, like `mkdir -p`.
fn directories(stage: &File, path: &Path) -> Result<File> {
    let mut dir = stage.try_clone().map_err(err)?;
    for component in path.components() {
        let Component::Normal(name) = component else {
            return Err("Archive path is not relative".into());
        };
        match sys::mkdirat(&dir, name, Mode::from_bits_truncate(0o777)) {
            Ok(()) | Err(rustix::io::Errno::EXIST) => {}
            Err(e) => return Err(err(e)),
        }
        dir = child_dir(&dir, name)?;
    }
    Ok(dir)
}
/// Validation and extraction share entry checks and read every payload, including
/// integrity checks, without writing anything when no staging directory is given.
fn scan(input: File, stage: Option<&File>, tick: &dyn Fn(u64) -> Result<()>) -> Result<()> {
    let a = open(&input)?;
    let mut names = HashSet::new();
    let mut types = HashMap::new();
    let mut count = 0;
    let mut total = 0u64;
    loop {
        let mut entry = ptr::null_mut();
        let code = unsafe { archive_read_next_header(a.0, &mut entry) };
        if code == 1 {
            break;
        }
        if code != 0 || entry.is_null() {
            return Err(a.error());
        }
        count += 1;
        if count > 10_000 {
            return Err("Archive exceeds 10,000 entries".into());
        }
        // `mode_t` is `u16` on macOS.
        #[allow(clippy::unnecessary_cast)]
        let (path, kind, size, mode, hardlink) = unsafe {
            let p = archive_entry_pathname(entry);
            if p.is_null() {
                return Err("Archive entry has no pathname".into());
            }
            (
                PathBuf::from(OsString::from_vec(CStr::from_ptr(p).to_bytes().to_vec())),
                archive_entry_filetype(entry) as u32,
                archive_entry_size(entry),
                archive_entry_perm(entry) as u32,
                !archive_entry_hardlink(entry).is_null(),
            )
        };
        let path = safe_path(&path)?;
        if hardlink || !matches!(kind, 0o100000 | 0o040000) {
            return Err("Archive links and special files are not allowed".into());
        }
        if path.as_os_str().is_empty() {
            if kind == 0o040000 {
                continue;
            }
            return Err("Archive has an empty filename".into());
        }
        if !names.insert(path.clone()) {
            return Err("Archive has duplicate paths".into());
        }
        if size < 0 || size as u64 > 2 * 1024 * 1024 * 1024 {
            return Err("Archive entry exceeds 2 GiB".into());
        }
        let is_dir = kind == 0o040000;
        for ancestor in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if types.insert(ancestor.to_owned(), true) == Some(false) {
                return Err("Archive uses a file as a directory".into());
            }
        }
        if types
            .insert(path.clone(), is_dir)
            .is_some_and(|was_dir| was_dir != is_dir)
        {
            return Err("Archive has conflicting file and directory paths".into());
        }
        let mut file = if let Some(stage) = stage {
            // Stage is private and archive links are refused.
            if is_dir {
                directories(stage, &path)?;
                None
            } else {
                let parent = directories(stage, path.parent().unwrap_or(Path::new("")))?;
                let name = path.file_name().ok_or("Archive has an empty filename")?;
                Some(File::from(
                    sys::openat(
                        &parent,
                        name,
                        OFlags::WRONLY
                            | OFlags::CREATE
                            | OFlags::EXCL
                            | OFlags::NOFOLLOW
                            | OFlags::CLOEXEC,
                        // The archive's permission bits, limited by the umask.
                        crate::platform::mode(mode & 0o777),
                    )
                    .map_err(err)?,
                ))
            }
        } else {
            None
        };
        if is_dir {
            continue;
        }
        let mut bytes = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = unsafe { archive_read_data(a.0, buffer.as_mut_ptr().cast(), buffer.len()) };
            if n < 0 {
                return Err(a.error());
            }
            if n == 0 {
                break;
            }
            bytes += n as u64;
            total += n as u64;
            if bytes > 2 * 1024 * 1024 * 1024 || total > 4 * 1024 * 1024 * 1024 {
                return Err("Archive exceeds the extraction size limit".into());
            }
            if let Some(file) = &mut file {
                file.write_all(&buffer[..n as usize])
                    .map_err(|e| e.to_string())?;
                tick(n as u64)?;
            }
        }
        if let Some(file) = file {
            crate::platform::sync(&file).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
