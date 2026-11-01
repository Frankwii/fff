//! Bounded LRU snapshots of successful completed discovery, never query state.
use crate::{App, Candidate, config::CacheConfig};
use std::{
    collections::{HashSet, VecDeque},
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Key {
    cwd: PathBuf,
    scope: Vec<PathBuf>,
    depth: usize,
    hidden: bool,
    ignore: bool,
    rules: Vec<String>,
}
impl Key {
    pub(crate) fn of(app: &App) -> Self {
        Self {
            cwd: app.cwd.clone(),
            scope: app.scope.clone(),
            depth: app.depth,
            hidden: app.policy.hidden,
            ignore: app.policy.ignore,
            rules: app.policy.rules.clone(),
        }
    }
    pub(crate) fn for_directory(app: &App, cwd: PathBuf) -> Self {
        let mut key = Self::of(app);
        key.cwd = cwd.clone();
        key.scope = app.roots.clone();
        if !key.scope.contains(&cwd) {
            key.scope.push(cwd);
        }
        key
    }
    pub(crate) fn single_level(&self) -> bool {
        self.depth == 1 && self.scope.len() == 1 && self.scope[0] == self.cwd
    }
}
#[derive(Clone)]
pub(crate) struct Snapshot {
    pub candidates: Arc<[Candidate]>,
    pub complete: bool,
    pub stale: bool,
    /// A completed scan may have been truncated to the cache's path limit.
    pub exhaustive: bool,
    pub physical_empty: bool,
    pub verified_at: Instant,
}
struct Entry {
    key: Key,
    snapshot: Snapshot,
    created: Instant,
}
#[derive(Default, Debug, Clone, Copy)]
pub struct Stats {
    pub hits: u64,
    pub misses: u64,
    pub stores: u64,
    pub evictions: u64,
}
pub(crate) struct Cache {
    options: CacheConfig,
    entries: VecDeque<Entry>,
    pub stats: Stats,
}
impl Cache {
    pub fn new(options: CacheConfig) -> Self {
        Self {
            options,
            entries: VecDeque::new(),
            stats: Stats::default(),
        }
    }
    pub fn get(&mut self, key: &Key, now: Instant) -> Option<Snapshot> {
        if !self.options.enabled {
            return None;
        }

        let Some(index) = self.entries.iter().position(|e| &e.key == key) else {
            self.stats.misses += 1;
            return None;
        };
        let entry = self.entries.remove(index).unwrap();
        let mut snapshot = entry.snapshot.clone();
        snapshot.stale = now.saturating_duration_since(entry.created)
            >= Duration::from_secs(self.options.ttl_seconds);
        self.entries.push_back(entry);
        self.stats.hits += 1;
        Some(snapshot)
    }
    pub fn invalidate(&mut self, key: &Key) {
        self.entries.retain(|e| &e.key != key);
    }
    pub fn recently_verified(&self, key: &Key, now: Instant) -> bool {
        self.entries.iter().any(|entry| {
            &entry.key == key
                && entry.snapshot.complete
                && entry.snapshot.exhaustive
                && now.saturating_duration_since(entry.created)
                    < Duration::from_secs(self.options.fresh_seconds.min(self.options.ttl_seconds))
        })
    }
    pub fn capture(&mut self, key: Key, app: &App, now: Instant) {
        self.capture_visit(key, app, now, true);
    }
    pub fn capture_visit(&mut self, key: Key, app: &App, now: Instant, complete: bool) {
        if !self.options.enabled
            || self.options.max_entries == 0
            || self.options.max_candidates == 0
        {
            return;
        }
        if !complete
            && (app.rows.is_empty()
                || self
                    .entries
                    .iter()
                    .any(|e| e.key == key && e.snapshot.complete))
        {
            return;
        }
        // Preserve the first matches of the completed view, then fill spare
        // capacity with known paths so another query can benefit as well.
        let mut candidates: Vec<_> = app
            .rows
            .iter()
            .take(self.options.max_candidates)
            .cloned()
            .collect();
        if candidates.len() < self.options.max_candidates {
            let used: HashSet<_> = candidates.iter().map(|c| c.path.clone()).collect();
            // Fill only bounded spare capacity; never sort the full discovery index
            // on the input/navigation thread.
            let mut rest: Vec<_> = app
                .known
                .values()
                .filter(|c| app.eligible(c) && !used.contains(&c.path))
                .take(self.options.max_candidates - candidates.len())
                .collect();
            let order = |a: &&Candidate, b: &&Candidate| {
                a.depth
                    .cmp(&b.depth)
                    .then(b.directory.cmp(&a.directory))
                    .then(a.root.cmp(&b.root))
                    .then(a.path.cmp(&b.path))
            };
            rest.sort_by(order);
            candidates.extend(rest.into_iter().cloned());
        }
        self.invalidate(&key);
        while self.entries.len() >= self.options.max_entries {
            self.entries.pop_front();
            self.stats.evictions += 1;
        }
        self.entries.push_back(Entry {
            key,
            snapshot: Snapshot {
                candidates: candidates.into(),
                complete,
                stale: false,
                exhaustive: complete && app.known.len() <= self.options.max_candidates,
                physical_empty: app.physical_empty,
                verified_at: now,
            },
            created: now,
        });
        self.stats.stores += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Policy;
    fn app(path: &str) -> App {
        let mut app = App::browse(path.into(), 1, Policy::default());
        app.insert(
            0,
            Candidate {
                path: app.cwd.join("file"),
                directory: false,
                depth: 1,
                root: 0,
            },
        );
        app.refresh();
        app
    }
    #[test]
    fn lru_hits_touch_recency_and_expired_snapshots_remain_available_as_stale() {
        let mut cache = Cache::new(CacheConfig {
            max_entries: 2,
            ttl_seconds: 10,
            ..CacheConfig::default()
        });
        let now = Instant::now();
        let a = app("/a");
        let b = app("/b");
        let c = app("/c");
        cache.capture(Key::of(&a), &a, now);
        cache.capture(Key::of(&b), &b, now);
        assert!(
            cache
                .get(&Key::of(&a), now + Duration::from_secs(1))
                .is_some()
        );
        cache.capture(Key::of(&c), &c, now + Duration::from_secs(2));
        assert!(
            cache
                .get(&Key::of(&b), now + Duration::from_secs(2))
                .is_none()
        );
        assert!(
            cache
                .get(&Key::of(&a), now + Duration::from_secs(9))
                .is_some()
        );
        assert!(
            cache
                .get(&Key::of(&a), now + Duration::from_secs(10))
                .unwrap()
                .stale
        );
        assert_eq!(cache.stats.evictions, 1);
        assert!(cache.entries.len() <= 2);
    }
    #[test]
    fn cache_key_covers_discovery_options_but_not_fuzzy_query_marks_or_filters() {
        let base = app("/a");
        let key = Key::of(&base);
        let mut other = app("/a");
        other.query = "abc".into();
        other.marks.push("/elsewhere".into());
        // File filters only choose which discovered files are listed.
        other.policy.set_filters(vec!["*.tex".into()]).unwrap();
        assert_eq!(key, Key::of(&other));
        for change in 0..7 {
            let mut other = app("/a");
            match change {
                0 => other.cwd = "/b".into(),
                1 => other.scope.push("/b".into()),
                2 => other.depth = 2,
                3 => other.policy.hidden = true,
                4 => other.policy.ignore = true,
                5 => other.policy.set_rules(vec!["build".into()]).unwrap(),
                6 => {
                    other.scope = vec!["/b".into(), "/a".into()];
                }
                _ => unreachable!(),
            }
            assert_ne!(
                key,
                Key::of(&other),
                "option {change} must separate snapshots"
            );
        }
    }
}
