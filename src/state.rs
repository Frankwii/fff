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
const HEADER: &[u8] = b"fff-state 2\0";
/// Before `from` was recorded; its positions are still read.
const HEADER_1: &[u8] = b"fff-state 1\0";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct State {
    /// Where the last session that may resume was started from: the next one
    /// resumes only when started there too.
    pub from: Option<PathBuf>,
    /// Where that session was browsing when it ended.
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

/// NUL-separated fields keep any path bytes: where the last session started
/// and where it ended (either possibly empty), then directory/entry pairs.
pub fn load(path: &Path) -> State {
    let Ok(bytes) = fs::read(path) else {
        return State::default();
    };
    let (body, version) = match (bytes.strip_prefix(HEADER), bytes.strip_prefix(HEADER_1)) {
        (Some(body), _) => (body, 2),
        (_, Some(body)) => (body, 1),
        _ => return State::default(),
    };
    let mut fields = body
        .split(|b| *b == 0)
        .map(|f| PathBuf::from(std::ffi::OsString::from_vec(f.to_vec())));
    let mut field = || fields.next().filter(|p| !p.as_os_str().is_empty());
    // A version 1 file never resumes: it does not say where it started.
    let from = if version == 1 { None } else { field() };
    let last = field();
    let from = from.filter(|_| last.is_some());
    let mut positions = vec![];
    while let (Some(dir), Some(entry)) = (fields.next(), fields.next()) {
        if dir.is_absolute() && entry.is_absolute() {
            positions.push((dir, entry));
        }
    }
    State {
        from,
        last,
        positions,
    }
}

/// Writes atomically: a private temporary file renamed over the old state.
pub fn save(path: &Path, state: &State) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(dir)?;
    let mut bytes = HEADER.to_vec();
    for place in [&state.from, &state.last] {
        if let Some(place) = place {
            bytes.extend_from_slice(place.as_os_str().as_bytes());
        }
        bytes.push(0);
    }
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
            from: Some(PathBuf::from("/home")),
            last: Some(odd.clone()),
            positions: vec![(PathBuf::from("/a"), odd.clone())],
        };
        save(&file, &state).unwrap();
        assert_eq!(load(&file), state);
        fs::write(&file, b"garbage").unwrap();
        assert_eq!(load(&file), State::default());
        assert_eq!(load(&dir.join("missing")), State::default());
        let many = State {
            from: None,
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
        // A version 1 file keeps its positions but does not resume.
        let mut old = HEADER_1.to_vec();
        old.extend_from_slice(b"/last\0/a\0/a/b\0");
        fs::write(&file, old).unwrap();
        assert_eq!(
            load(&file),
            State {
                from: None,
                last: Some(PathBuf::from("/last")),
                positions: vec![(PathBuf::from("/a"), PathBuf::from("/a/b"))],
            }
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
