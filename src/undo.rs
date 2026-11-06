//! Undoing file operations (`u`). Each operation records how to reverse every
//! change it made; undoing first checks that each entry is still the one the
//! operation left (by identity), then reverses the steps last to first,
//! rolling back the steps already reversed if one fails.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    /// An entry moved or renamed to `now`; it goes back to `back`.
    Return {
        now: PathBuf,
        back: PathBuf,
        id: (u64, u64),
    },
    /// An entry moved into the trash `bin` as `slot`; it goes back to `back`.
    Restore {
        bin: PathBuf,
        slot: OsString,
        back: PathBuf,
        id: (u64, u64),
    },
    /// An entry restored from the trash `bin` to `now`, out of `slot`; it
    /// goes back into the bin with its old `record`.
    Retrash {
        now: PathBuf,
        bin: PathBuf,
        slot: OsString,
        record: Vec<u8>,
        id: (u64, u64),
    },
    /// An entry the operation created (a copy, link, extraction or new
    /// entry); undoing moves it to the trash.
    Discard { path: PathBuf, id: (u64, u64) },
}
/// One operation that can be undone.
#[derive(Clone, Debug)]
pub struct Record {
    /// What the operation did, as its message said.
    pub summary: String,
    pub steps: Vec<Step>,
    /// Entries it deleted permanently, which undoing cannot bring back.
    pub lost: usize,
    /// When the operation started, among this session's: `u` undoes the
    /// latest first, whether it has ended or still runs.
    pub order: u64,
    /// The marks it took on starting, marked again when `u` undoes it.
    pub marked: Vec<PathBuf>,
}
impl Record {
    /// The paths undoing moves away, a trashed entry's slot included; the
    /// current directory and its ancestors must not be among them.
    pub fn moving(&self) -> impl Iterator<Item = std::borrow::Cow<'_, Path>> {
        self.steps.iter().map(|step| match step {
            Step::Return { now: path, .. }
            | Step::Retrash { now: path, .. }
            | Step::Discard { path, .. } => path.into(),
            Step::Restore { bin, slot, .. } => bin.join("files").join(slot).into(),
        })
    }
    /// Every path undoing changes: what it moves away and where it puts
    /// things back.
    pub fn touching(&self) -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = self.moving().map(|p| p.into_owned()).collect();
        paths.extend(self.steps.iter().filter_map(|step| match step {
            Step::Return { back, .. } | Step::Restore { back, .. } => Some(back.clone()),
            Step::Retrash { bin, slot, .. } => Some(bin.join("files").join(slot)),
            Step::Discard { .. } => None,
        }));
        paths
    }
}
/// A reversed step, kept to roll it forward again if a later step fails.
enum Undone {
    Returned {
        now: Entry,
        back: Entry,
        id: (u64, u64),
    },
    Restored {
        bin: Bin,
        slot: OsString,
        back: Entry,
        id: (u64, u64),
    },
    /// Back in the trash: `Retrash` and `Discard` alike.
    Discarded {
        bin: Bin,
        slot: OsString,
        entry: Entry,
    },
}
impl Undone {
    /// Rolls the step forward again; when that fails, where the entry
    /// was left.
    fn redo(self) -> Option<PathBuf> {
        match self {
            Undone::Returned { now, back, id } => {
                let redone = identity(&back.parent, &back.name) == Some(id)
                    && reparent(&back.parent, &back.name, &now.parent, &now.name).is_ok();
                (!redone).then_some(back.path)
            }
            Undone::Restored {
                bin,
                slot,
                back,
                id,
            } => {
                let redone = identity(&back.parent, &back.name) == Some(id)
                    && reparent(&back.parent, &back.name, &bin.files, &slot).is_ok();
                (!redone).then_some(back.path)
            }
            Undone::Discarded { bin, slot, entry } => {
                let back = bin.take(&slot, &entry.parent, &entry.name).is_ok();
                if back {
                    bin.forget(&slot);
                }
                (!back).then(|| bin.path.join("files").join(&slot))
            }
        }
    }
}
fn name(path: &Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
/// `path`, still the entry `id`.
fn still(path: &Path, id: (u64, u64)) -> Result<Entry> {
    let entry = Entry::at(path).map_err(|e| format!("{}: {e}", name(path)))?;
    if identity(&entry.parent, &entry.name) != Some(id) {
        return Err(format!("{} is no longer where it was left", name(path)));
    }
    Ok(entry)
}
impl Engine {
    /// Reverses `record`, or changes nothing.
    pub fn undo(&self, record: &Record) -> Result<Outcome> {
        let _deferred = interrupt::Deferred::new();
        for step in &record.steps {
            match step {
                Step::Return { now: path, id, .. }
                | Step::Retrash { now: path, id, .. }
                | Step::Discard { path, id } => {
                    still(path, *id)?;
                }
                Step::Restore {
                    bin,
                    slot,
                    back,
                    id,
                } => {
                    let bin = Bin::open(bin, false)?;
                    if identity(&bin.files, slot) != Some(*id) {
                        return Err(format!("{} is no longer in the trash", name(back)));
                    }
                }
            }
        }
        let mut done: Vec<Undone> = vec![];
        for (i, step) in record.steps.iter().enumerate().rev() {
            self.checkpoint(i);
            match self.reverse(step) {
                Ok(undone) => done.push(undone),
                Err(e) => {
                    // Every step is rolled forward, even after one fails.
                    let left: Vec<String> = done
                        .into_iter()
                        .rev()
                        .filter_map(Undone::redo)
                        .map(|path| format!("{path:?}"))
                        .collect();
                    return Err(format!(
                        "Cannot undo: {e}; {}",
                        if left.is_empty() {
                            "nothing changed".to_string()
                        } else {
                            format!("undo stopped partway, leaving {}", left.join(", "))
                        }
                    ));
                }
            }
        }
        let mut outcome = Outcome::new(true);
        for undone in done {
            match undone {
                // Records of restored entries go once everything is back.
                Undone::Restored { bin, slot, .. } => bin.forget(&slot),
                Undone::Returned { now, back, .. } => outcome.moved.push((now.path, back.path)),
                Undone::Discarded { entry, .. } => outcome.removed.push(entry.path),
            }
        }
        Ok(outcome)
    }
    fn reverse(&self, step: &Step) -> Result<Undone> {
        match step {
            Step::Return { now, back, id } => {
                let now = still(now, *id)?;
                let back = Entry::at(back).map_err(|e| format!("{}: {e}", name(back)))?;
                reparent(&now.parent, &now.name, &back.parent, &back.name)
                    .map_err(|e| format!("{} cannot go back: {e}", name(&now.path)))?;
                Ok(Undone::Returned { now, back, id: *id })
            }
            Step::Restore {
                bin,
                slot,
                back,
                id,
            } => {
                let bin = Bin::open(bin, false)?;
                if identity(&bin.files, slot) != Some(*id) {
                    return Err(format!("{} is no longer in the trash", name(back)));
                }
                let back = Entry::at(back).map_err(|e| format!("{}: {e}", name(back)))?;
                bin.take(slot, &back.parent, &back.name)
                    .map_err(|e| format!("{} cannot come back: {e}", name(&back.path)))?;
                Ok(Undone::Restored {
                    bin,
                    slot: slot.clone(),
                    back,
                    id: *id,
                })
            }
            Step::Retrash {
                now,
                bin,
                slot,
                record,
                id,
            } => {
                let entry = still(now, *id)?;
                let bin = Bin::open(bin, false)?;
                let slot = bin
                    .put_record(&entry.parent, &entry.name, slot, record)
                    .map_err(|e| format!("{} cannot go back to the trash: {e}", name(now)))?;
                Ok(Undone::Discarded { bin, slot, entry })
            }
            Step::Discard { path, id } => {
                let entry = still(path, *id)?;
                // Only trees fff could have made, and could delete later.
                tree(&entry)?;
                let bin = self
                    .bin(&entry)
                    .map_err(|e| format!("{} cannot go to the trash: {e}", name(path)))?;
                let slot = bin.put(&entry.parent, &entry.name, &entry.path)?;
                Ok(Undone::Discarded { bin, slot, entry })
            }
        }
    }
}
