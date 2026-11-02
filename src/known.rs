//! Every discovered path of the current view, sharded so that growing the
//! index never rehashes all of it at once on the input thread.
use crate::Candidate;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
};

/// Shards rehash on their own: a large listing pays for one shard at a time.
const SHARDS: usize = 64;

/// A known path's candidate and the scan generation that last delivered it.
type Entry = (Arc<Candidate>, u64);

pub struct Known {
    shards: Vec<HashMap<PathBuf, Entry>>,
    len: usize,
}
impl Default for Known {
    fn default() -> Self {
        Self {
            shards: (0..SHARDS).map(|_| HashMap::new()).collect(),
            len: 0,
        }
    }
}
impl Known {
    fn shard(path: &Path) -> usize {
        // FNV-1a: cheap, and spread well enough for choosing a shard.
        let bytes = path.as_os_str().as_encoded_bytes();
        let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
            (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3)
        });
        hash as usize % SHARDS
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get(&self, path: &Path) -> Option<&Arc<Candidate>> {
        self.shards[Self::shard(path)].get(path).map(|(c, _)| c)
    }
    pub fn contains_key(&self, path: &Path) -> bool {
        self.get(path).is_some()
    }
    pub fn values(&self) -> impl Iterator<Item = &Arc<Candidate>> {
        self.shards.iter().flat_map(|s| s.values().map(|(c, _)| c))
    }
    /// The entry for the candidate's path, delivered by scan `stamp`; returns
    /// the candidate it replaces.
    pub(crate) fn insert(
        &mut self,
        candidate: Arc<Candidate>,
        stamp: u64,
    ) -> Option<Arc<Candidate>> {
        let shard = &mut self.shards[Self::shard(&candidate.path)];
        let old = shard.insert(candidate.path.clone(), (candidate, stamp));
        if old.is_none() {
            self.len += 1;
        }
        old.map(|(c, _)| c)
    }
    /// Record that scan `stamp` delivered `path` again, unchanged.
    pub(crate) fn confirm(&mut self, path: &Path, stamp: u64) {
        if let Some(entry) = self.shards[Self::shard(path)].get_mut(path) {
            entry.1 = stamp;
        }
    }
    /// Forget every path scan `stamp` did not deliver, returning them.
    pub(crate) fn keep_stamped(&mut self, stamp: u64) -> Vec<Arc<Candidate>> {
        let mut forgotten = Vec::new();
        for shard in &mut self.shards {
            shard.retain(|_, (c, s)| {
                *s == stamp || {
                    forgotten.push(Arc::clone(c));
                    false
                }
            });
        }
        self.len -= forgotten.len();
        forgotten
    }
    /// Empty the index, freeing a large one on another thread.
    pub fn clear(&mut self) {
        let len = self.len;
        discard(std::mem::take(self), len);
    }
}
impl<P: AsRef<Path> + ?Sized> std::ops::Index<&P> for Known {
    type Output = Arc<Candidate>;
    fn index(&self, path: &P) -> &Arc<Candidate> {
        self.get(path.as_ref()).expect("path is known")
    }
}

/// Paths at which a value is freed on its own thread: freeing many paths takes
/// long enough to delay a frame.
const DISCARD_AT: usize = 16_384;
/// Drop `value`, which holds about `paths` paths, away from the input thread
/// when they are many.
pub(crate) fn discard<T: Send + 'static>(value: T, paths: usize) {
    if paths >= DISCARD_AT {
        // Should no thread start, the value is freed here after all.
        let _ = std::thread::Builder::new().spawn(move || drop(value));
    }
}
