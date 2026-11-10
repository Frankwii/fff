//! Short labels in `fff pick` and `fff seek`: a row shows its name, with
//! only the parents that tell it apart from listed entries sharing that name.
//!
//! The listed entries are kept by name (`Names`), and a label is worked out
//! when its row is drawn: a scan adding paths costs a lookup per path, not
//! labelling them, and that lookup is made on a thread of its own
//! (`Labeller`). A group is sorted by its paths read from the end, so the
//! path ending most like a row's is beside it, and a row's label comes from
//! its two neighbours. A group changed since is sorted again first, which the
//! next frame showing one of its rows does.
use crate::{Candidate, known};
use std::{
    cmp::Ordering,
    collections::{HashMap, VecDeque},
    ffi::{OsStr, OsString},
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, PoisonError, mpsc},
};

/// Listed entries by basename.
#[derive(Default)]
pub(crate) struct Names {
    groups: HashMap<OsString, Group, foldhash::fast::RandomState>,
    /// How many candidates the groups hold.
    len: usize,
}

struct Group {
    /// Never empty: a group is removed with its last member.
    members: Vec<Arc<Candidate>>,
    /// How many of `members`, from the first, are in `from_end` order;
    /// those inserted since follow them.
    sorted: usize,
}

impl Group {
    /// Sort the members inserted since the last time and merge them in.
    fn sort(&mut self) {
        if self.sorted == self.members.len() {
            return;
        }
        self.members[self.sorted..].sort_unstable_by(|a, b| from_end(&a.path, &b.path));
        // Two sorted runs, which a stable sort merges in one pass.
        self.members.sort_by(|a, b| from_end(&a.path, &b.path));
        self.sorted = self.members.len();
    }
}

impl Names {
    /// Group the listed candidates by basename, each group sorted, which a
    /// ranking does on its own thread.
    pub fn new(listed: impl IntoIterator<Item = Arc<Candidate>>) -> Self {
        let mut names = Self::default();
        for c in listed {
            names.insert(c);
        }
        for group in names.groups.values_mut() {
            group.sort();
        }
        names
    }
    pub fn insert(&mut self, c: Arc<Candidate>) {
        let Some(name) = c.path.file_name() else {
            return;
        };
        self.len += 1;
        // A name is copied only for a group it starts.
        match self.groups.get_mut(name) {
            Some(group) => group.members.push(c),
            None => {
                let name = name.to_os_string();
                let members = vec![c];
                self.groups.insert(name, Group { members, sorted: 0 });
            }
        }
    }
    /// Forget `c`, the very candidate inserted.
    pub fn remove(&mut self, c: &Arc<Candidate>) {
        let Some(name) = c.path.file_name() else {
            return;
        };
        if let Some(group) = self.groups.get_mut(name)
            && let Some(at) = group.members.iter().position(|m| Arc::ptr_eq(m, c))
        {
            // Removing keeps the order of the rest.
            group.members.remove(at);
            self.len -= 1;
            if at < group.sorted {
                group.sorted -= 1;
            }
            if group.members.is_empty() {
                self.groups.remove(name);
            }
        }
    }
    /// How many candidates it holds.
    pub fn len(&self) -> usize {
        self.len
    }
    /// The label of `path`, listed: its last parts, one more than it ends in
    /// common with another entry of the same name, and at least its parent,
    /// short of the whole path. `None` for a name no other entry has (or a
    /// path not listed), which shows bare.
    pub fn label(&mut self, path: &Path) -> Option<PathBuf> {
        self.groups.get_mut(path.file_name()?)?.sort();
        self.sorted_label(path)
    }
    /// `label`, its group sorted already (as `new` and `sort` leave them),
    /// so that threads ranking rows share the names.
    fn sorted_label(&self, path: &Path) -> Option<PathBuf> {
        let group = self.groups.get(path.file_name()?)?;
        if group.members.len() < 2 {
            return None;
        }
        debug_assert_eq!(group.sorted, group.members.len());
        let members = &group.members;
        let at = members.binary_search_by(|m| from_end(&m.path, path)).ok()?;
        let shared = |other: Option<&Arc<Candidate>>| {
            other.map_or(0, |o| {
                path.components()
                    .rev()
                    .zip(o.path.components().rev())
                    .take_while(|(a, b)| a == b)
                    .count()
            })
        };
        let before = shared(at.checked_sub(1).and_then(|i| members.get(i)));
        let after = shared(members.get(at + 1));
        let parts: Vec<Component> = path.components().collect();
        let length = (before.max(after) + 1).max(2).min(parts.len());
        Some(parts[parts.len() - length..].iter().collect())
    }
    /// Sort every group, for `text`.
    pub fn sort(&mut self) {
        self.groups.values_mut().for_each(Group::sort);
    }
    /// What a listed row shows and a query matches while labels are shown:
    /// its label, else its name. Groups are sorted (`new`, `sort`).
    pub fn text(&self, c: &Candidate) -> String {
        match self.sorted_label(&c.path) {
            Some(label) => label.to_string_lossy().into_owned(),
            None => c
                .path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
        }
    }
    /// The listed entries named `name`.
    pub fn members(&self, name: &OsStr) -> &[Arc<Candidate>] {
        self.groups.get(name).map_or(&[], |g| g.members.as_slice())
    }
    /// Every listed entry's label (those that have one), by path.
    pub fn all(&mut self) -> HashMap<PathBuf, PathBuf> {
        let paths: Vec<PathBuf> = self
            .groups
            .values()
            .filter(|g| g.members.len() > 1)
            .flat_map(|g| g.members.iter().map(|c| c.path.clone()))
            .collect();
        paths
            .into_iter()
            .filter_map(|p| Some((self.label(&p)?, p)))
            .map(|(label, p)| (p, label))
            .collect()
    }
}

/// `Names` brought up to date on a thread of its own: changes are queued
/// for it, and reading a label first applies whatever it has not applied
/// yet, so a label is never out of date. The thread holds the names only
/// while it applies one change (a round of a scan's paths), which is the
/// most a frame waits for it; queueing a change waits for neither.
#[derive(Default)]
pub(crate) struct Labeller {
    state: Arc<State>,
    /// Wakes the thread, started with the first change; none should it not
    /// start, and changes are applied when read.
    wake: Option<mpsc::SyncSender<()>>,
}

#[derive(Default)]
struct State {
    /// Changes are taken from `queue` only with `names` held, so they are
    /// applied in order.
    names: Mutex<Names>,
    queue: Mutex<VecDeque<Change>>,
}

enum Change {
    Insert(Vec<Arc<Candidate>>),
    Remove(Vec<Arc<Candidate>>),
    Replace(Names),
}

impl Change {
    fn apply(self, to: &mut Names) {
        match self {
            Change::Insert(listed) => listed.into_iter().for_each(|c| to.insert(c)),
            Change::Remove(gone) => gone.iter().for_each(|c| to.remove(c)),
            Change::Replace(names) => {
                let old = std::mem::replace(to, names);
                let len = old.len();
                known::discard(old, len);
            }
        }
    }
}

impl State {
    /// The names, with every queued change applied.
    fn settled(&self) -> MutexGuard<'_, Names> {
        let mut names = lock(&self.names);
        while let Some(change) = self.next() {
            change.apply(&mut names);
        }
        names
    }
    /// The next change; taken with `names` held.
    fn next(&self) -> Option<Change> {
        lock(&self.queue).pop_front()
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Labeller {
    /// Listed candidates to group.
    pub fn insert(&mut self, listed: Vec<Arc<Candidate>>) {
        if !listed.is_empty() {
            self.queue(Change::Insert(listed));
        }
    }
    /// Candidates inserted before and forgotten since.
    pub fn remove(&mut self, gone: Vec<Arc<Candidate>>) {
        if !gone.is_empty() {
            self.queue(Change::Remove(gone));
        }
    }
    /// Names grouped anew, by a ranking: after the changes queued already.
    pub fn replace(&mut self, names: Names) {
        self.queue(Change::Replace(names));
    }
    pub fn label(&self, path: &Path) -> Option<PathBuf> {
        self.state.settled().label(path)
    }
    /// Every label, by path.
    pub fn all(&self) -> HashMap<PathBuf, PathBuf> {
        self.state.settled().all()
    }
    /// A handle on the same names, for ranking on another thread.
    pub fn reader(&self) -> Reader {
        Reader(Arc::clone(&self.state))
    }
    fn queue(&mut self, change: Change) {
        lock(&self.state.queue).push_back(change);
        if self.wake.is_none() {
            let (wake, woken) = mpsc::sync_channel(1);
            let state = Arc::downgrade(&self.state);
            let worker = std::thread::Builder::new()
                .name("fff-labels".into())
                .spawn(move || {
                    // Until the session drops its labeller.
                    while woken.recv().is_ok() {
                        while let Some(state) = state.upgrade() {
                            let mut names = lock(&state.names);
                            match state.next() {
                                Some(change) => change.apply(&mut names),
                                None => break,
                            }
                        }
                    }
                });
            if worker.is_ok() {
                self.wake = Some(wake);
            }
        }
        if let Some(wake) = &self.wake {
            // Already woken when full: it drains the whole queue.
            let _ = wake.try_send(());
        }
    }
}

/// The names a `Labeller` keeps, read while ranking (`Labeller::reader`).
#[derive(Clone)]
pub(crate) struct Reader(Arc<State>);
impl Reader {
    /// The names brought up to date and sorted, for `f`: drawing waits
    /// meanwhile.
    pub fn with<R>(&self, f: impl FnOnce(&Names) -> R) -> R {
        let mut names = self.0.settled();
        names.sort();
        f(&names)
    }
}

/// Paths compared part by part from their ends: their bytes from the last,
/// a separator before any other byte, so that a part ends before any longer
/// one it begins. The paths sharing the most last parts with one are then
/// next to it.
fn from_end(a: &Path, b: &Path) -> Ordering {
    let byte = |&b: &u8| if b == b'/' { 0 } else { u16::from(b) + 1 };
    let a = a.as_os_str().as_encoded_bytes().iter().rev().map(byte);
    let b = b.as_os_str().as_encoded_bytes().iter().rev().map(byte);
    a.cmp(b)
}

#[cfg(test)]
#[test]
fn labels_stay_right_as_entries_come_and_go() {
    // Inserted in rounds, some forgotten, labels read between rounds: the
    // same labels as grouping what is left at once.
    let names = ["a", "b", "c"];
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for _ in 0..200 {
        let mut grouped = Labeller::default();
        let mut listed: Vec<Arc<Candidate>> = Vec::new();
        for _ in 0..6 {
            let mut round = Vec::new();
            for _ in 0..next() % 6 {
                let r = next();
                let mut path = PathBuf::from("/");
                for k in 0..1 + r % 5 {
                    path.push(names[((r >> (8 + 2 * k)) % 3) as usize]);
                }
                if listed.iter().any(|c| c.path == path) {
                    continue;
                }
                let c = Arc::new(Candidate {
                    path,
                    directory: false,
                    depth: 0,
                    root: 0,
                    order: 0,
                });
                round.push(Arc::clone(&c));
                listed.push(c);
            }
            grouped.insert(round);
            if !listed.is_empty() && next() % 3 == 0 {
                let c = listed.remove((next() % listed.len() as u64) as usize);
                grouped.remove(vec![c]);
            }
            if next() % 2 == 0
                && let Some(c) = listed.first()
            {
                grouped.label(&c.path);
            }
            if next() % 5 == 0 {
                grouped.replace(Names::new(listed.iter().cloned()));
            }
            let expected = crate::disambiguate(listed.iter().map(|c| c.path.as_path()));
            assert_eq!(grouped.all(), expected, "{listed:?}");
        }
    }
}
