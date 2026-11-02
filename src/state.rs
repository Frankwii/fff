//! What fff remembers between sessions: each directory's highlighted entry
//! (`state`), and the last `fff roam` session for `--resume` (`session`).
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
/// Earlier versions led with the last directory (1), or where the last
/// session started and ended (2); their positions are still read.
const HEADERS: [(&[u8], usize); 2] = [(b"fff-state 1\0", 1), (b"fff-state 2\0", 2)];
const SESSION_HEADER: &[u8] = b"fff-session 1\0";

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
/// The last `fff roam` session: `$XDG_STATE_HOME/fff/session`.
pub fn session_path() -> Option<PathBuf> {
    file("session")
}

/// NUL-separated fields keep any path bytes: directory/entry pairs.
pub fn load(path: &Path) -> State {
    let Ok(bytes) = fs::read(path) else {
        return State::default();
    };
    let (body, skip) = match bytes.strip_prefix(HEADER) {
        Some(body) => (body, 0),
        None => match HEADERS
            .iter()
            .find_map(|(header, skip)| Some((bytes.strip_prefix(*header)?, *skip)))
        {
            Some(found) => found,
            None => return State::default(),
        },
    };
    let mut fields = body
        .split(|b| *b == 0)
        .skip(skip)
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

/// Writes atomically: a private temporary file renamed over the old one.
fn write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(dir)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = dir.join(format!(".{name}-{}", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .open(&temporary)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(&temporary, path)
}

/// A `fff roam` session as it ended, for `--resume`: where it was, what was
/// highlighted and marked, and the query and settings it had.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    pub roots: Vec<PathBuf>,
    /// Every tab, and which one was active.
    pub tabs: Vec<crate::Tab>,
    pub tab: usize,
    pub highlight: Option<PathBuf>,
    pub marks: Vec<PathBuf>,
    pub query: String,
    pub depth: usize,
    pub filters: Vec<String>,
    pub rules: Vec<String>,
    /// Exclusions enforced (Alt+H).
    pub ignore: bool,
    pub hidden: bool,
    /// Directories hidden (Alt+d).
    pub files_only: bool,
    /// The preview pane shown (Alt+v).
    pub preview: bool,
}

/// NUL-separated key/value pairs, so values keep any bytes. A tab's `back`
/// and `forward` entries follow its `tab`.
pub fn save_session(path: &Path, session: &Session) -> std::io::Result<()> {
    let mut bytes = SESSION_HEADER.to_vec();
    // Paths cannot contain NUL; text values lose any, keeping the pairs intact.
    let mut put = |key: &str, value: &[u8]| {
        bytes.extend_from_slice(key.as_bytes());
        bytes.push(0);
        bytes.extend(value.iter().filter(|&&b| b != 0));
        bytes.push(0);
    };
    let flag = |on: bool| if on { b"1" } else { b"0" };
    for root in &session.roots {
        put("root", root.as_os_str().as_bytes());
    }
    for tab in &session.tabs {
        put("tab", tab.cwd.as_os_str().as_bytes());
        for dir in &tab.back {
            put("back", dir.as_os_str().as_bytes());
        }
        for dir in &tab.forward {
            put("forward", dir.as_os_str().as_bytes());
        }
    }
    put("active", session.tab.to_string().as_bytes());
    if let Some(highlight) = &session.highlight {
        put("highlight", highlight.as_os_str().as_bytes());
    }
    for mark in &session.marks {
        put("mark", mark.as_os_str().as_bytes());
    }
    put("query", session.query.as_bytes());
    put("depth", session.depth.to_string().as_bytes());
    for filter in &session.filters {
        put("filter", filter.as_bytes());
    }
    for rule in &session.rules {
        put("exclude", rule.as_bytes());
    }
    put("ignore", flag(session.ignore));
    put("hidden", flag(session.hidden));
    put("files_only", flag(session.files_only));
    put("preview", flag(session.preview));
    write(path, &bytes)
}

/// The saved session, if there is a readable one with a tab.
pub fn load_session(path: &Path) -> Option<Session> {
    let bytes = fs::read(path).ok()?;
    let body = bytes.strip_prefix(SESSION_HEADER)?;
    let mut fields = body.split(|b| *b == 0);
    let mut session = Session {
        depth: 2,
        ignore: true,
        ..Session::default()
    };
    let path = |v: &[u8]| PathBuf::from(std::ffi::OsString::from_vec(v.to_vec()));
    let text = |v: &[u8]| String::from_utf8_lossy(v).into_owned();
    while let (Some(key), Some(value)) = (fields.next(), fields.next()) {
        match key {
            b"root" => session.roots.push(path(value)),
            b"tab" => session.tabs.push(crate::Tab {
                cwd: path(value),
                ..crate::Tab::default()
            }),
            b"back" | b"forward" => {
                if let Some(tab) = session.tabs.last_mut() {
                    let list = if key == b"back" {
                        &mut tab.back
                    } else {
                        &mut tab.forward
                    };
                    list.push(path(value));
                }
            }
            b"active" => session.tab = text(value).parse().unwrap_or(0),
            b"highlight" => session.highlight = Some(path(value)),
            b"mark" => session.marks.push(path(value)),
            b"query" => session.query = text(value),
            b"depth" => session.depth = text(value).parse().unwrap_or(2),
            b"filter" => session.filters.push(text(value)),
            b"exclude" => session.rules.push(text(value)),
            b"ignore" => session.ignore = value == b"1",
            b"hidden" => session.hidden = value == b"1",
            b"files_only" => session.files_only = value == b"1",
            b"preview" => session.preview = value == b"1",
            _ => {}
        }
    }
    (!session.tabs.is_empty()).then_some(session)
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
        // Earlier versions keep their positions.
        let expected = State {
            positions: vec![(PathBuf::from("/a"), PathBuf::from("/a/b"))],
        };
        fs::write(&file, b"fff-state 1\0/last\0/a\0/a/b\0").unwrap();
        assert_eq!(load(&file), expected);
        fs::write(&file, b"fff-state 2\0/from\0/last\0/a\0/a/b\0").unwrap();
        assert_eq!(load(&file), expected);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sessions_round_trip_and_need_a_tab() {
        let dir = std::env::temp_dir().join(format!("fff-session-{}", std::process::id()));
        let file = dir.join("fff/session");
        let odd = PathBuf::from(std::ffi::OsString::from_vec(b"/a/b\xff\nc".to_vec()));
        let session = Session {
            roots: vec!["/r".into()],
            tabs: vec![
                crate::Tab {
                    cwd: "/one".into(),
                    back: vec!["/zero".into()],
                    forward: vec![],
                },
                crate::Tab {
                    cwd: odd.clone(),
                    back: vec![],
                    forward: vec!["/x".into(), "/y".into()],
                },
            ],
            tab: 1,
            highlight: Some(odd.join("papers")),
            marks: vec!["/m1".into(), odd],
            query: "pap\0er".into(),
            depth: 4,
            filters: vec!["*.pdf".into()],
            rules: vec!["target".into(), "build".into()],
            ignore: false,
            hidden: true,
            files_only: true,
            preview: true,
        };
        save_session(&file, &session).unwrap();
        let loaded = load_session(&file).unwrap();
        // A NUL is dropped; everything else survives.
        assert_eq!(
            Session {
                query: "paper".into(),
                ..session
            },
            loaded
        );
        fs::write(&file, b"fff-session 1\0query\0x\0").unwrap();
        assert_eq!(load_session(&file), None);
        fs::write(&file, b"garbage").unwrap();
        assert_eq!(load_session(&file), None);
        fs::remove_dir_all(&dir).unwrap();
    }
}
