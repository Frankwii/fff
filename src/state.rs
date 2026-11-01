//! Remembered locations between sessions: the last browsed directory and each
//! directory's highlighted entry. Best effort: a missing, unreadable or corrupt
//! file is treated as empty, and saving failures are ignored.
use std::{
    fs,
    io::Write,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

/// Directories remembered at most; the least recently left are dropped.
pub const CAPACITY: usize = 500;
const HEADER: &[u8] = b"fff-state 1\0";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct State {
    /// Where the last session that may resume was browsing.
    pub last: Option<PathBuf>,
    /// (directory, highlighted entry), least recently used first.
    pub positions: Vec<(PathBuf, PathBuf)>,
}

/// `$XDG_STATE_HOME/fff/state`, or `~/.local/state/fff/state`.
pub fn path() -> Option<PathBuf> {
    let absolute = |v: std::ffi::OsString| Some(PathBuf::from(v)).filter(|p| p.is_absolute());
    std::env::var_os("XDG_STATE_HOME")
        .and_then(absolute)
        .or_else(|| {
            std::env::var_os("HOME")
                .and_then(absolute)
                .map(|h| h.join(".local/state"))
        })
        .map(|base| base.join("fff/state"))
}

/// NUL-separated fields keep any path bytes: the last directory (possibly
/// empty), then directory/entry pairs.
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
    let last = fields.next().filter(|p| !p.as_os_str().is_empty());
    let mut positions = vec![];
    while let (Some(dir), Some(entry)) = (fields.next(), fields.next()) {
        if dir.is_absolute() && entry.is_absolute() {
            positions.push((dir, entry));
        }
    }
    State { last, positions }
}

/// Writes atomically: a private temporary file renamed over the old state.
pub fn save(path: &Path, state: &State) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(dir)?;
    let mut bytes = HEADER.to_vec();
    if let Some(last) = &state.last {
        bytes.extend_from_slice(last.as_os_str().as_bytes());
    }
    bytes.push(0);
    let start = state.positions.len().saturating_sub(CAPACITY);
    for (dir, entry) in &state.positions[start..] {
        for p in [dir, entry] {
            bytes.extend_from_slice(p.as_os_str().as_bytes());
            bytes.push(0);
        }
    }
    let temporary = dir.join(format!(".state-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)
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
            last: Some(odd.clone()),
            positions: vec![(PathBuf::from("/a"), odd.clone())],
        };
        save(&file, &state).unwrap();
        assert_eq!(load(&file), state);
        fs::write(&file, b"garbage").unwrap();
        assert_eq!(load(&file), State::default());
        assert_eq!(load(&dir.join("missing")), State::default());
        let many = State {
            last: None,
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
