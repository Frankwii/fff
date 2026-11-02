//! Read archives through libarchive, but create entries ourselves. No shell or
//! external extractor is invoked. Links, devices and escaping paths are refused.
use crate::operations::Result;
use std::{
    collections::{HashMap, HashSet},
    ffi::{CStr, OsString, c_char, c_int, c_void},
    fs::{self, File, OpenOptions},
    io::Write,
    os::{
        fd::AsRawFd,
        unix::{ffi::OsStringExt, fs::OpenOptionsExt},
    },
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
    fn archive_entry_filetype(e: *mut c_void) -> u32;
    fn archive_entry_hardlink(e: *mut c_void) -> *const c_char;
    fn archive_entry_size(e: *mut c_void) -> i64;
    fn archive_entry_perm(e: *mut c_void) -> u32;
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
/// Linux, as are control characters (a newline would forge a second name) and
/// names fff reserves for its own staging directories.
pub fn safe_path(path: &Path) -> Result<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    if bytes.contains(&b'\\')
        || bytes.iter().any(|b| *b < 0x20 || *b == 0x7f)
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
pub fn extract(input: File, stage: &File) -> Result<()> {
    scan(input, Some(stage))
}
pub fn validate(input: File) -> Result<()> {
    scan(input, None)
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
/// A reader for `input` with the built-in filters and every format.
fn open(input: &File) -> Result<Reader> {
    let a = Reader(unsafe { archive_read_new() });
    if a.0.is_null() {
        return Err("Cannot allocate archive reader".into());
    }
    // These built-in filters avoid libarchive's optional external helper programs.
    unsafe {
        for code in [
            archive_read_support_filter_none(a.0),
            archive_read_support_filter_gzip(a.0),
            archive_read_support_filter_bzip2(a.0),
            archive_read_support_filter_xz(a.0),
            archive_read_support_filter_zstd(a.0),
            archive_read_support_format_all(a.0),
        ] {
            if code != 0 {
                return Err(a.error());
            }
        }
        if archive_read_open_fd(a.0, input.as_raw_fd(), 64 * 1024) != 0 {
            return Err(a.error());
        }
    }
    Ok(a)
}
/// Validation and extraction share entry checks and read every payload, including
/// integrity checks, without writing anything when no staging directory is given.
fn scan(input: File, stage: Option<&File>) -> Result<()> {
    let a = open(&input)?;
    let root = stage.map(|stage| PathBuf::from(format!("/proc/self/fd/{}", stage.as_raw_fd())));
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
        let (path, kind, size, mode, hardlink) = unsafe {
            let p = archive_entry_pathname(entry);
            if p.is_null() {
                return Err("Archive entry has no pathname".into());
            }
            (
                PathBuf::from(OsString::from_vec(CStr::from_ptr(p).to_bytes().to_vec())),
                archive_entry_filetype(entry),
                archive_entry_size(entry),
                archive_entry_perm(entry),
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
        let mut file = if let Some(root) = &root {
            let target = root.join(&path);
            // Stage is private and archive links are refused.
            if is_dir {
                fs::create_dir_all(&target).map_err(|e| e.to_string())?;
                None
            } else {
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                Some(
                    OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        // The archive's permission bits, limited by the umask.
                        .mode(mode & 0o777)
                        .open(&target)
                        .map_err(|e| e.to_string())?,
                )
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
            }
        }
        if let Some(file) = file {
            file.sync_all().map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}
