//! Every discovered path of the current view, sharded so that growing the
//! index never rehashes all of it at once on the input thread.
use crate::Candidate;
use hashbrown::{HashTable, hash_table::Entry as Slot};
use std::{hash::BuildHasher, path::Path, sync::Arc};

/// Shards rehash on their own: a large listing pays for one shard at a time.
const SHARDS: usize = 64;

/// A known path's candidate, the scan generation that last delivered it, and
/// its path's hash, so that a growing shard rehashes without reading paths.
struct Entry {
    candidate: Arc<Candidate>,
    stamp: u64,
    hash: u64,
}

/// The paths are the candidates' own: each is hashed once per lookup, for
/// its shard and its place in it alike, and never copied.
pub struct Known {
    shards: [HashTable<Entry>; SHARDS],
    len: usize,
    hasher: foldhash::fast::RandomState,
}
impl Default for Known {
    fn default() -> Self {
        Self {
            shards: std::array::from_fn(|_| HashTable::new()),
            len: 0,
            hasher: Default::default(),
        }
    }
}
impl Known {
    fn hash(&self, path: &Path) -> u64 {
        self.hasher.hash_one(path)
    }
    /// A hash's shard, from bits a table reads neither for a place (the
    /// lowest) nor for its tags (the highest).
    fn shard(hash: u64) -> usize {
        (hash >> 32) as usize % SHARDS
    }
    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn get(&self, path: &Path) -> Option<&Arc<Candidate>> {
        let hash = self.hash(path);
        self.shards[Self::shard(hash)]
            .find(hash, |e| e.candidate.path == path)
            .map(|e| &e.candidate)
    }
    pub fn contains_key(&self, path: &Path) -> bool {
        self.get(path).is_some()
    }
    pub fn values(&self) -> impl Iterator<Item = &Arc<Candidate>> {
        self.shards
            .iter()
            .flat_map(|s| s.iter().map(|e| &e.candidate))
    }
    /// Keep `candidate`, delivered by scan `delivered` (`None` for a path
    /// replayed from earlier results, unconfirmed until a scan delivers it).
    /// A path known as it is only has its stamp renewed, and gives `None`;
    /// otherwise the candidate kept, and the one it replaces.
    #[allow(clippy::type_complexity)]
    pub(crate) fn store(
        &mut self,
        candidate: Candidate,
        delivered: Option<u64>,
    ) -> Option<(Arc<Candidate>, Option<Arc<Candidate>>)> {
        let hash = self.hash(&candidate.path);
        let shard = &mut self.shards[Self::shard(hash)];
        // Replays are unconfirmed: no scan has generation 0.
        let stamp = delivered.unwrap_or(0);
        match shard.entry(hash, |e| e.candidate.path == candidate.path, |e| e.hash) {
            // Rescans deliver mostly what is already known.
            Slot::Occupied(mut slot) if *slot.get().candidate == candidate => {
                if let Some(generation) = delivered {
                    slot.get_mut().stamp = generation;
                }
                None
            }
            Slot::Occupied(mut slot) => {
                let entry = slot.get_mut();
                let candidate = Arc::new(candidate);
                let old = std::mem::replace(&mut entry.candidate, Arc::clone(&candidate));
                entry.stamp = stamp;
                Some((candidate, Some(old)))
            }
            Slot::Vacant(slot) => {
                let candidate = Arc::new(candidate);
                slot.insert(Entry {
                    candidate: Arc::clone(&candidate),
                    stamp,
                    hash,
                });
                self.len += 1;
                Some((candidate, None))
            }
        }
    }
    /// Forget every path scan `stamp` did not deliver, returning them.
    pub(crate) fn keep_stamped(&mut self, stamp: u64) -> Vec<Arc<Candidate>> {
        let mut forgotten = Vec::new();
        for shard in &mut self.shards {
            shard.retain(|e| {
                e.stamp == stamp || {
                    forgotten.push(Arc::clone(&e.candidate));
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
