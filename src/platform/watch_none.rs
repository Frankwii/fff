//! Live updates (`[ui] watch`) where inotify is missing: none. The session
//! works as with an unwatchable directory; F5 reads the listing again.
use std::path::{Path, PathBuf};

/// Most directories watched at once.
pub const MOST: usize = 4096;

pub struct Watch;

/// What happened since the last poll.
#[derive(Debug, Default, PartialEq)]
pub struct Changes {
    /// Entries appeared, disappeared or were renamed, or events were lost.
    pub listing: bool,
    /// Entries whose contents or attributes changed.
    pub touched: Vec<PathBuf>,
}

impl Watch {
    /// Always `None`: fff then works without live updates.
    pub fn new() -> Option<Self> {
        None
    }
    pub fn watched(&self) -> impl Iterator<Item = &Path> {
        std::iter::empty()
    }
    pub fn follow(&mut self, _dirs: &[&Path], _roots: usize) {}
    pub fn poll(&mut self) -> Changes {
        Changes::default()
    }
}
