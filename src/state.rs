//! What fff remembers between sessions: each directory's highlighted entry.
//! Best effort: a missing, unreadable or corrupt file is treated as empty, and
//! saving failures are ignored.
use std::{
    fs,
    io::Write,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

/// Directories remembered at most; the least recently left are dropped.
pub const CAPACITY: usize = 500;
const HEADER: &[u8] = b"fff-state 3\0";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct State {
    /// (directory, highlighted entry), least recently used first.
    pub positions: Vec<(PathBuf, PathBuf)>,
}

/// `$XDG_STATE_HOME/fff/NAME`, or `~/.local/state/fff/NAME`.
fn file(name: &str) -> Option<PathBuf> {
    let absolute = |v: std::ffi::OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    std::env::var_os("XDG_STATE_HOME")
        .and_then(absolute)
        .or_else(|| {
            std::env::var_os("HOME")
                .and_then(absolute)
                .map(|h| h.join(".local/state"))
        })
        .map(|base| base.join("fff").join(name))
}
/// The remembered highlights: `$XDG_STATE_HOME/fff/state`.
pub fn path() -> Option<PathBuf> {
    file("state")
}

/// NUL-separated fields keep any path bytes: directory/entry pairs.
pub fn load(path: &Path) -> State {
    let Ok(bytes) = fs::read(path) else {
        return State::default();
    };
    let Some(body) = bytes.strip_prefix(HEADER) else {
        return State::default();
    };
    let mut fields = body
        .split(|b| *b == 0)
        .map(|f| PathBuf::from(std::ffi::OsString::from_vec(f.to_vec())));
    let mut positions = vec![];
    while let (Some(dir), Some(entry)) = (fields.next(), fields.next()) {
        if dir.is_absolute() && entry.is_absolute() {
            positions.push((dir, entry));
        }
    }
    State { positions }
}

pub fn save(path: &Path, state: &State) -> std::io::Result<()> {
    let mut bytes = HEADER.to_vec();
    let start = state.positions.len().saturating_sub(CAPACITY);
    for (dir, entry) in &state.positions[start..] {
        for p in [dir, entry] {
            bytes.extend_from_slice(p.as_os_str().as_bytes());
            bytes.push(0);
        }
    }
    write(path, &bytes)
}

/// Writes atomically: a private temporary file (mode 0600: marks, queries
/// and folders are nobody else's business) renamed over the old one.
pub(crate) fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = dir.join(format!(".{name}-{}", std::process::id()));
    let mut file = private_file(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}
/// Opens `path` for writing, emptied and readable only by the user (mode
/// 0600), whatever the umask.
pub(crate) fn private_file(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)?;
    // A leftover file keeps its mode when reopened.
    file.set_permissions(fs::Permissions::from_mode(0o600))?;
    Ok(file)
}
/// Creates `dir` and its missing parents readable only by the user (mode
/// 0700); an existing `dir` that others may read or enter loses that.
pub(crate) fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    let mode = fs::metadata(dir)?.permissions().mode() & 0o7777;
    if mode & 0o077 != 0 {
        fs::set_permissions(dir, fs::Permissions::from_mode(mode & !0o077))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_round_trips_any_path_bytes_and_tolerates_damage() {
        let dir = std::env::temp_dir().join(format!("fff-state-{}", std::process::id()));
        let file = dir.join("fff/state");
        let odd = PathBuf::from(std::ffi::OsString::from_vec(b"/a/b\xff\nc".to_vec()));
        let state = State {
            positions: vec![(PathBuf::from("/a"), odd.clone())],
        };
        save(&file, &state).unwrap();
        assert_eq!(load(&file), state);
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        fs::write(&file, b"garbage").unwrap();
        assert_eq!(load(&file), State::default());
        assert_eq!(load(&dir.join("missing")), State::default());
        let many = State {
            positions: (0..CAPACITY + 7)
                .map(|i| (PathBuf::from(format!("/d{i}")), PathBuf::from("/e")))
                .collect(),
        };
        save(&file, &many).unwrap();
        let loaded = load(&file);
        assert_eq!(loaded.positions.len(), CAPACITY);
        // The most recently used directories are the ones kept.
        assert_eq!(
            loaded.positions.last().unwrap().0,
            PathBuf::from(format!("/d{}", CAPACITY + 6))
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
