//! Bare `fff` with paths on stdin (`rg -l TODO | fff`): the list another
//! program wrote is the rows, read once on a thread of its own, and the
//! choice is printed as the lines spelled it. Nothing else lists, rescans
//! or navigates there (`Program::Piped`).
use crate::Candidate;
use std::{
    collections::HashMap,
    ffi::OsString,
    io::{self, BufRead},
    os::unix::{ffi::OsStringExt, fs::FileTypeExt},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Whether stdin is a list to read: a pipe or a redirected file. A
/// terminal, `/dev/null` or a closed stdin is not, and fff browses.
pub fn stdin_lists() -> bool {
    use std::os::fd::AsFd;
    let Ok(fd) = io::stdin().as_fd().try_clone_to_owned() else {
        return false;
    };
    std::fs::File::from(fd)
        .metadata()
        .is_ok_and(|meta| meta.file_type().is_fifo() || meta.is_file())
}

/// What a session reading a list keeps: the input until a scan reads it,
/// and each listed path's line as it was read, to print.
pub struct Piped {
    input: Option<Box<dyn io::Read + Send>>,
    /// NUL-separated lines (`-0`), printed NUL-terminated.
    pub null: bool,
    pub spelled: Spellings,
}
/// Each listed path's line as the list spelled it: the first one, when
/// several name the same path.
#[derive(Clone, Default)]
pub struct Spellings(Arc<Mutex<HashMap<PathBuf, OsString>>>);
impl Spellings {
    /// `paths` as the list spelled them, for printing.
    pub fn of(&self, paths: &[PathBuf]) -> Vec<PathBuf> {
        let spelled = self.0.lock().unwrap();
        paths
            .iter()
            .map(|p| spelled.get(p).map_or_else(|| p.clone(), PathBuf::from))
            .collect()
    }
    /// Keeps `line` as the spelling of `path`; false when another line
    /// named it before.
    fn keep(&self, path: &Path, line: &[u8]) -> bool {
        let mut spelled = self.0.lock().unwrap();
        if spelled.contains_key(path) {
            return false;
        }
        spelled.insert(path.to_owned(), OsString::from_vec(line.to_vec()));
        true
    }
}
impl Piped {
    pub fn new(input: Box<dyn io::Read + Send>, null: bool) -> Self {
        Self {
            input: Some(input),
            null,
            spelled: Spellings::default(),
        }
    }
    /// The input, the first time: it is read once.
    pub fn take(&mut self) -> Option<Box<dyn io::Read + Send>> {
        self.input.take()
    }
}

/// What reading a list found.
pub enum Read {
    Paths(Vec<Candidate>),
    /// Lines naming no file or folder (or the current directory itself).
    Skipped(usize),
    Failed(io::Error),
}
/// Paths sent at most this many at once.
const BATCH: usize = 256;
/// Reads `input` to its end, sending what it lists as it comes: a batch
/// once `BATCH` paths wait or nothing more is buffered (the producer is
/// slow), then the lines skipped, if any. A path named again is listed
/// where it was first. Stops when `send` fails.
pub fn read(
    input: Box<dyn io::Read + Send>,
    null: bool,
    cwd: &Path,
    spelled: &Spellings,
    mut send: impl FnMut(Read) -> bool,
) {
    let mut input = io::BufReader::new(input);
    let separator = if null { b'\0' } else { b'\n' };
    let (mut batch, mut skipped, mut line) = (Vec::new(), 0, Vec::new());
    let mut order = 0u32;
    loop {
        line.clear();
        match input.read_until(separator, &mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                if !batch.is_empty() && !send(Read::Paths(std::mem::take(&mut batch))) {
                    return;
                }
                send(Read::Failed(e));
                return;
            }
        }
        if line.last() == Some(&separator) {
            line.pop();
        }
        if line.is_empty() {
            continue;
        }
        match listed(&line, cwd, order) {
            Some(candidate) => {
                if spelled.keep(&candidate.path, &line) {
                    batch.push(candidate);
                    order = order.saturating_add(1);
                }
            }
            None => skipped += 1,
        }
        if (batch.len() >= BATCH || input.buffer().is_empty() && !batch.is_empty())
            && !send(Read::Paths(std::mem::take(&mut batch)))
        {
            return;
        }
    }
    if !batch.is_empty() && !send(Read::Paths(batch)) {
        return;
    }
    if skipped > 0 {
        send(Read::Skipped(skipped));
    }
}
/// The row a line lists: the file or folder it names, absolute, below
/// `cwd` when relative. `.` parts go; `..` resolves through the real
/// folder above it. The depth is the parts below `cwd`, or of the whole
/// path outside it, and every row has the one root, `/`; `order` is its
/// place among the paths listed.
fn listed(line: &[u8], cwd: &Path, order: u32) -> Option<Candidate> {
    let given = PathBuf::from(OsString::from_vec(line.to_vec()));
    let mut path = cwd.join(given);
    if path.components().any(|c| c == Component::ParentDir) {
        let name = path.file_name()?.to_owned();
        path = path.parent()?.canonicalize().ok()?.join(name);
    }
    let path: PathBuf = path.components().collect();
    let meta = path.symlink_metadata().ok()?;
    if path == cwd {
        return None;
    }
    let directory = meta.is_dir() || meta.is_symlink() && path.is_dir();
    let depth = match path.strip_prefix(cwd) {
        Ok(below) => below.components().count(),
        Err(_) => path.components().count() - 1,
    };
    Some(Candidate {
        path,
        directory,
        depth,
        root: 0,
        order,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("fff-piped-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("dir/sub")).unwrap();
        std::fs::write(root.join("dir/a.txt"), "").unwrap();
        std::fs::write(root.join("b.txt"), "").unwrap();
        root.canonicalize().unwrap()
    }
    fn read_all(text: &[u8], null: bool, cwd: &Path) -> (Vec<Candidate>, usize, Spellings) {
        let spelled = Spellings::default();
        let (mut paths, mut skipped) = (Vec::new(), 0);
        read(
            Box::new(io::Cursor::new(text.to_vec())),
            null,
            cwd,
            &spelled,
            |read| {
                match read {
                    Read::Paths(batch) => paths.extend(batch),
                    Read::Skipped(n) => skipped = n,
                    Read::Failed(e) => panic!("{e}"),
                }
                true
            },
        );
        (paths, skipped, spelled)
    }
    #[test]
    fn lines_name_paths_below_the_directory_or_anywhere() {
        let root = fixture("lines");
        let text = format!(
            "dir/a.txt\n./b.txt\n\ndir/sub/\nmissing\nb.txt\n{}\n",
            root.display()
        );
        let (paths, skipped, spelled) = read_all(text.as_bytes(), false, &root);
        let found: Vec<_> = paths
            .iter()
            .map(|c| (c.path.clone(), c.directory, c.depth, c.order))
            .collect();
        assert_eq!(
            found,
            [
                (root.join("dir/a.txt"), false, 2, 0),
                (root.join("b.txt"), false, 1, 1),
                (root.join("dir/sub"), true, 2, 2),
            ]
        );
        // The missing path and the directory itself; the empty line is no
        // path, and b.txt named again is listed once, spelled as first.
        assert_eq!(skipped, 2);
        // Printed as the list spelled them.
        assert_eq!(
            spelled.of(&[root.join("b.txt"), root.join("dir/sub")]),
            [PathBuf::from("./b.txt"), PathBuf::from("dir/sub/")]
        );
        // Outside the directory, a path keeps every part.
        let (paths, ..) = read_all(
            format!("{}\n", root.display()).as_bytes(),
            false,
            &root.join("dir"),
        );
        assert_eq!(paths[0].depth, root.components().count() - 1);
        let _ = std::fs::remove_dir_all(&root);
    }
    #[test]
    fn nul_separated_lines_and_parent_parts() {
        let root = fixture("nul");
        let (paths, skipped, spelled) = read_all(b"dir/sub/../a.txt\0b.txt", true, &root);
        assert_eq!(skipped, 0);
        assert_eq!(paths.len(), 2);
        assert_eq!(paths[0].path, root.join("dir/a.txt"));
        assert_eq!(
            spelled.of(&[root.join("dir/a.txt")]),
            [PathBuf::from("dir/sub/../a.txt")]
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
