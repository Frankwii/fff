//! The trash, undo, progress and stopping, through the engine.
use super::*;
use crate::operations::undo::Record;
use std::time::{Duration, SystemTime};

/// Bins under the fixture's own data directory.
fn trash(f: &Fixture, ask_above: u64) -> Trash {
    Trash::new(&f.root.join("data"), 30, ask_above)
}
fn engine(f: &Fixture) -> Engine {
    Engine::new(Some(trash(f, u64::MAX)), None)
}
fn bin(f: &Fixture) -> PathBuf {
    f.root.join("data/fff/trash")
}
fn listing(dir: &Path) -> Vec<String> {
    let mut names = fs::read_dir(dir)
        .map(|d| {
            d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_else(|_| vec![]);
    names.sort();
    names
}
fn record(outcome: Outcome) -> Record {
    Record {
        summary: "test".into(),
        steps: outcome.undo,
        lost: outcome.lost,
    }
}
fn trash_all(f: &Fixture, sources: Vec<PathBuf>) -> Outcome {
    let plan = DeletePlan::trash(sources, &trash(f, u64::MAX)).unwrap();
    assert!(plan.fates.iter().all(|fate| *fate == Fate::Trash));
    engine(f).execute(plan.trash_request(false)).unwrap()
}
/// Every entry's identity and metadata except the root's change time, which
/// a rename updates.
fn contents(path: &Path) -> Snapshot {
    let mut snap = snapshot(path).unwrap().0;
    if let Some(root) = snap.0.get_mut(Path::new("")) {
        root.5 = 0;
        root.6 = 0;
    }
    snap
}

#[test]
fn trashed_entries_keep_everything_and_undo_puts_them_back() {
    let f = Fixture::new();
    let a = f.file("a", b"alpha");
    let dir = f.work.join("tree");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join(".hidden"), b"hidden").unwrap();
    symlink("/does/not/exist", dir.join("broken")).unwrap();
    locked_dir(&dir.join("sub"));
    let odd = f.work.join(OsString::from_vec(b"bad-\xff name".to_vec()));
    fs::write(&odd, b"bytes").unwrap();
    let before = [&a, &dir, &odd].map(|p| contents(p));
    let out = trash_all(&f, vec![a.clone(), dir.clone(), odd.clone()]);
    assert_eq!(out.trashed, 3);
    assert_eq!(out.lost, 0);
    assert_eq!(out.removed, vec![a.clone(), dir.clone(), odd.clone()]);
    assert!(out.warning.is_none());
    assert!(!a.exists() && !dir.exists() && !odd.exists());
    let files = bin(&f).join("files");
    assert_eq!(fs::read(files.join("a")).unwrap(), b"alpha");
    assert_eq!(fs::read(files.join("tree/.hidden")).unwrap(), b"hidden");
    // Undoing moves the slots out of the trash: browsing inside one refuses.
    let r = Record {
        summary: "test".into(),
        steps: out.undo.clone(),
        lost: 0,
    };
    let moving: Vec<_> = r.moving().map(|p| p.into_owned()).collect();
    assert!(moving.contains(&files.join("tree")), "{moving:?}");
    assert_eq!(fs::read(files.join("tree/sub/x")).unwrap(), b"x");
    assert_eq!(
        fs::read_link(files.join("tree/broken")).unwrap(),
        Path::new("/does/not/exist")
    );
    // The bin is private, and each entry has its record.
    assert_eq!(fs::metadata(bin(&f)).unwrap().mode() & 0o777, 0o700);
    let info = fs::read_to_string(bin(&f).join("info/a.trashinfo")).unwrap();
    assert!(
        info.starts_with(&format!(
            "[Trash Info]\nPath={}\nDeletionDate=20",
            a.display()
        )),
        "{info}"
    );
    let info = fs::read_to_string(bin(&f).join("info/bad-\u{fffd} name.trashinfo"));
    assert!(info.is_err(), "the record keeps the name's bytes");
    let info = fs::read(
        bin(&f)
            .join("info")
            .join(OsString::from_vec(b"bad-\xff name.trashinfo".to_vec())),
    )
    .unwrap();
    assert!(
        String::from_utf8(info)
            .unwrap()
            .contains("bad-%FF%20name\n")
    );
    assert!(stranded(&f.work).is_empty());
    let back = engine(&f).undo(&record(out)).unwrap();
    assert!(back.moved.is_empty() && back.removed.is_empty());
    assert_eq!([&a, &dir, &odd].map(|p| contents(p)), before);
    assert_eq!(fs::metadata(dir.join("sub")).unwrap().mode() & 0o777, 0o555);
    assert!(listing(&files).is_empty());
    assert!(listing(&bin(&f).join("info")).is_empty());
    unlock(&dir.join("sub"));
}
#[test]
fn equal_names_get_their_own_slots_and_come_back_to_their_own_places() {
    let f = Fixture::new();
    let one = f.file("same", b"one");
    fs::create_dir(f.work.join("other")).unwrap();
    let two = f.work.join("other/same");
    fs::write(&two, b"two").unwrap();
    let first = trash_all(&f, vec![one.clone()]);
    let second = trash_all(&f, vec![two.clone()]);
    assert_eq!(listing(&bin(&f).join("files")), ["same", "same.2"]);
    assert_eq!(fs::read(bin(&f).join("files/same.2")).unwrap(), b"two");
    // Undone in any order, each goes back where it came from.
    engine(&f).undo(&record(first)).unwrap();
    assert_eq!(fs::read(&one).unwrap(), b"one");
    engine(&f).undo(&record(second)).unwrap();
    assert_eq!(fs::read(&two).unwrap(), b"two");
    // A long name is shortened in the trash, never on the way back.
    let long = "n".repeat(250);
    let path = f.file(&long, b"long");
    let out = trash_all(&f, vec![path.clone()]);
    assert_eq!(listing(&bin(&f).join("files")), ["n".repeat(200)]);
    engine(&f).undo(&record(out)).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"long");
}
#[test]
fn large_entries_ask_and_answers_trash_or_delete_them() {
    let f = Fixture::new();
    let small = f.file("small", b"tiny");
    let large = f.file("large", &[7; 100]);
    let t = trash(&f, 10);
    let plan = DeletePlan::trash(vec![small.clone(), large.clone()], &t).unwrap();
    assert_eq!(plan.fates, [Fate::Trash, Fate::Large]);
    assert_eq!(plan.asking(), [(large.clone(), 100)]);
    // d: the large one is deleted for good, the small one trashed.
    let out = Engine::new(Some(t.clone()), None)
        .execute(plan.trash_request(false))
        .unwrap();
    assert_eq!((out.trashed, out.lost), (1, 1));
    assert!(!small.exists() && !large.exists());
    assert_eq!(listing(&bin(&f).join("files")), ["small"]);
    // Undoing brings back what the trash kept.
    let r = record(out);
    assert_eq!(r.lost, 1);
    Engine::new(Some(t.clone()), None).undo(&r).unwrap();
    assert!(small.exists() && !large.exists());
    // t: both go to the trash.
    let large = f.file("large", &[7; 100]);
    let plan = DeletePlan::trash(vec![small.clone(), large.clone()], &t).unwrap();
    let out = Engine::new(Some(t), None)
        .execute(plan.trash_request(true))
        .unwrap();
    assert_eq!((out.trashed, out.lost), (2, 0));
    assert_eq!(listing(&bin(&f).join("files")), ["large", "small"]);
}
#[test]
fn entries_in_or_around_the_trash_cannot_go_there() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    trash_all(&f, vec![a]);
    let inside = bin(&f).join("files/a");
    let around = f.root.join("data");
    let t = trash(&f, u64::MAX);
    let plan = DeletePlan::trash(vec![inside.clone()], &t).unwrap();
    assert_eq!(
        plan.fates,
        [Fate::Unavailable("already in the trash".into())]
    );
    // Nothing moves into the trash; asked, it is deleted permanently.
    let error = engine(&f)
        .execute(Request::Delete {
            sources: vec![inside.clone()],
            confirmed: vec![],
            trash: vec![true],
        })
        .unwrap_err();
    assert!(error.contains("cannot go to the trash"), "{error}");
    assert!(inside.exists());
    let out = engine(&f).execute(plan.trash_request(false)).unwrap();
    assert_eq!((out.trashed, out.lost), (0, 1));
    assert!(out.undo.is_empty());
    assert!(!inside.exists());
    let plan = DeletePlan::trash(vec![around.clone()], &t).unwrap();
    assert_eq!(
        plan.fates,
        [Fate::Unavailable("contains the trash folder".into())]
    );
    engine(&f).execute(plan.trash_request(false)).unwrap();
    assert!(!around.exists());
}
#[test]
fn without_a_trash_nothing_is_trashed_or_undone_through_it() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let error = Engine::default()
        .execute(Request::Delete {
            sources: vec![a.clone()],
            confirmed: vec![],
            trash: vec![true],
        })
        .unwrap_err();
    assert!(error.contains("The trash is off"), "{error}");
    assert!(a.exists());
    // A copy cannot be undone without somewhere to put it.
    let out = Engine::default()
        .execute(Request::Copy {
            sources: vec![a.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    let error = Engine::default().undo(&record(out)).unwrap_err();
    assert!(
        error.contains("trash is off") && error.contains("nothing changed"),
        "{error}"
    );
    assert!(f.dest.join("a").exists());
}
#[test]
fn a_failure_while_trashing_puts_every_entry_back() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let c = f.file("c", b"c");
    // b is deleted permanently, a and c trashed; c changes before it moves.
    let plan = DeletePlan::trash(vec![a.clone(), b.clone(), c.clone()], &trash(&f, 0)).unwrap();
    let Request::Delete {
        sources, confirmed, ..
    } = plan.trash_request(true)
    else {
        unreachable!()
    };
    let request = Request::Delete {
        sources,
        confirmed,
        trash: vec![true, false, true],
    };
    let mut e = engine(&f);
    let changing = c.clone();
    e.hook = Some(Box::new(move |phase| {
        if phase == 2 {
            fs::write(&changing, b"changed").unwrap();
        }
    }));
    let error = e.execute(request).unwrap_err();
    assert!(error.contains("nothing changed"), "{error}");
    assert_eq!(fs::read(&a).unwrap(), b"a");
    assert_eq!(fs::read(&b).unwrap(), b"b");
    assert!(listing(&bin(&f).join("files")).is_empty());
    assert!(listing(&bin(&f).join("info")).is_empty());
    assert!(stranded(&f.work).is_empty());
    // A change inside the trash before deletion starts also puts all back.
    let mut e = engine(&f);
    let slot = bin(&f).join("files/a");
    e.hook = Some(Box::new(move |phase| {
        if phase == usize::MAX - 1 {
            fs::write(&slot, b"changed in the trash").unwrap();
        }
    }));
    let error = e
        .execute(Request::Delete {
            sources: vec![a.clone(), b.clone()],
            confirmed: vec![],
            trash: vec![true, false],
        })
        .unwrap_err();
    assert!(error.contains("changed while preparing"), "{error}");
    assert_eq!(fs::read(&a).unwrap(), b"changed in the trash");
    assert_eq!(fs::read(&b).unwrap(), b"b");
    assert!(listing(&bin(&f).join("files")).is_empty());
    assert!(listing(&bin(&f).join("info")).is_empty());
}
#[test]
fn undoing_a_trashing_checks_every_entry_first() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let out = trash_all(&f, vec![a.clone(), b.clone()]);
    let r = record(out);
    // Something new where a was: the undo stops, and b goes back to the trash.
    fs::write(&a, b"new").unwrap();
    let error = engine(&f).undo(&r).unwrap_err();
    assert!(
        error.contains("a cannot come back") && error.contains("nothing changed"),
        "{error}"
    );
    assert_eq!(fs::read(&a).unwrap(), b"new");
    assert!(!b.exists());
    assert_eq!(listing(&bin(&f).join("files")), ["a", "b"]);
    assert_eq!(
        listing(&bin(&f).join("info")),
        ["a.trashinfo", "b.trashinfo"]
    );
    fs::remove_file(&a).unwrap();
    // An entry replaced inside the trash is not the one that was trashed.
    fs::remove_file(bin(&f).join("files/b")).unwrap();
    fs::write(bin(&f).join("files/b"), b"b").unwrap();
    let error = engine(&f).undo(&r).unwrap_err();
    assert!(error.contains("b is no longer in the trash"), "{error}");
    assert!(!a.exists() && !b.exists());
}
#[test]
fn moves_and_renames_are_undone_by_identity() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let dir = f.work.join("dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("inner"), b"inner").unwrap();
    let out = engine(&f)
        .execute(Request::Move {
            sources: vec![a.clone(), dir.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    let r = record(out);
    assert_eq!(r.steps.len(), 2);
    assert_eq!(
        r.moving().collect::<Vec<_>>(),
        [f.dest.join("a"), f.dest.join("dir")]
    );
    // Taken: the undo changes nothing.
    fs::write(&a, b"taken").unwrap();
    assert!(engine(&f).undo(&r).is_err());
    assert_eq!(listing(&f.dest), ["a", "dir"]);
    fs::remove_file(&a).unwrap();
    let back = engine(&f).undo(&r).unwrap();
    assert_eq!(
        back.moved,
        [
            (f.dest.join("dir"), dir.clone()),
            (f.dest.join("a"), a.clone())
        ]
    );
    assert_eq!(fs::read(&a).unwrap(), b"a");
    assert_eq!(fs::read(dir.join("inner")).unwrap(), b"inner");
    assert!(listing(&f.dest).is_empty());
    // A second undo of the same record finds nothing where it was left.
    let error = engine(&f).undo(&r).unwrap_err();
    assert!(error.contains("no longer where it was left"), "{error}");
    // Nested renames go back deepest first, each to its own old name.
    let rows = docs_tree(&f);
    let request = rename_request(rows.clone(), &f.work, "documents\ndocuments/my_file").unwrap();
    let out = engine(&f).execute(request).unwrap();
    assert!(f.work.join("documents/my_file").exists());
    let back = engine(&f).undo(&record(out)).unwrap();
    assert!(f.work.join("docs/myfile").exists() && !f.work.join("documents").exists());
    assert_eq!(back.moved.len(), 2);
}
#[test]
fn overwritten_entries_go_to_the_trash_and_come_back_with_undo() {
    for moving in [true, false] {
        let f = Fixture::new();
        let a = f.file("a", b"new");
        fs::write(f.dest.join("a"), b"old").unwrap();
        let plan = TransferPlan::new(
            if moving {
                Transfer::Move
            } else {
                Transfer::Copy
            },
            vec![a.clone()],
            f.dest.clone(),
        )
        .unwrap();
        let out = engine(&f).execute(plan.request().unwrap()).unwrap();
        assert_eq!((out.trashed, out.lost), (1, 0));
        assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"new");
        assert_eq!(fs::read(bin(&f).join("files/a")).unwrap(), b"old");
        assert!(stranded(&f.dest).is_empty());
        let r = record(out);
        engine(&f).undo(&r).unwrap();
        assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"old");
        assert_eq!(fs::read(&a).unwrap(), b"new");
        // Undoing a copy trashes the copy, beside the overwritten entry
        // still in the trash at that moment.
        assert_eq!(
            listing(&bin(&f).join("files")),
            if moving {
                vec![]
            } else {
                vec!["a.2".to_string()]
            }
        );
        assert!(listing(&bin(&f).join("info")).len() <= 1);
        // Without a trash, the overwritten entry is gone for good.
        let a2 = f.file("b", b"newer");
        fs::write(f.dest.join("b"), b"older").unwrap();
        let plan = TransferPlan::new(Transfer::Move, vec![a2], f.dest.clone()).unwrap();
        let out = Engine::default().execute(plan.request().unwrap()).unwrap();
        assert_eq!((out.trashed, out.lost), (0, 1));
        assert_eq!(record(out).lost, 1);
    }
}
#[test]
fn created_entries_are_undone_into_the_trash() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let copied = engine(&f)
        .execute(Request::Copy {
            sources: vec![a.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    let linked = engine(&f)
        .execute(Request::Symlink {
            sources: vec![a.clone()],
            directory: f.root.clone(),
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    let created = engine(&f)
        .execute(Request::Create {
            path: f.work.join("new"),
            directory: true,
        })
        .unwrap();
    let archive = f.work.join("x.tar");
    tar(&archive, &[("unpacked/file", b'0', b"payload", "")]);
    let extracted = engine(&f)
        .execute(Request::Extract {
            archives: vec![archive.clone()],
            destination: None,
        })
        .unwrap();
    for (out, path) in [
        (copied, f.dest.join("a")),
        (linked, f.root.join("a")),
        (created, f.work.join("new")),
        (extracted, f.work.join("unpacked")),
    ] {
        assert!(path.symlink_metadata().is_ok(), "{path:?}");
        let back = engine(&f).undo(&record(out)).unwrap();
        assert_eq!(back.removed, std::slice::from_ref(&path));
        assert!(path.symlink_metadata().is_err(), "{path:?}");
    }
    assert_eq!(fs::read(&a).unwrap(), b"a");
    assert!(archive.exists());
    assert_eq!(
        listing(&bin(&f).join("files")),
        ["a", "a.2", "new", "unpacked"]
    );
    assert_eq!(fs::read_link(bin(&f).join("files/a.2")).unwrap(), a);
    // A created entry replaced since is not undone.
    let out = engine(&f)
        .execute(Request::Create {
            path: f.work.join("again"),
            directory: false,
        })
        .unwrap();
    fs::remove_file(f.work.join("again")).unwrap();
    fs::write(f.work.join("again"), b"someone else's").unwrap();
    let error = engine(&f).undo(&record(out)).unwrap_err();
    assert!(error.contains("no longer where it was left"), "{error}");
    assert!(f.work.join("again").exists());
}
#[test]
fn a_failing_step_rolls_back_the_steps_undone_before_it() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let out = engine(&f)
        .execute(Request::Move {
            sources: vec![a.clone(), b.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    let r = record(out);
    // b is undone first; then a's way back is taken.
    let mut e = engine(&f);
    let taken = a.clone();
    e.hook = Some(Box::new(move |step| {
        if step == 0 {
            fs::write(&taken, b"taken").unwrap();
        }
    }));
    let error = e.undo(&r).unwrap_err();
    assert!(error.contains("nothing changed"), "{error}");
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"a");
    assert_eq!(fs::read(f.dest.join("b")).unwrap(), b"b");
    assert!(!b.exists());
    // The trash keeps its records when restoring is rolled back.
    let c = f.file("c", b"c");
    let d = f.file("d", b"d");
    let r = record(trash_all(&f, vec![c.clone(), d.clone()]));
    let mut e = engine(&f);
    let taken = c.clone();
    e.hook = Some(Box::new(move |step| {
        if step == 0 {
            fs::write(&taken, b"taken").unwrap();
        }
    }));
    assert!(e.undo(&r).is_err());
    assert!(!d.exists());
    assert_eq!(listing(&bin(&f).join("files")), ["c", "d"]);
    assert_eq!(
        listing(&bin(&f).join("info")),
        ["c.trashinfo", "d.trashinfo"]
    );
}
#[test]
fn the_purge_deletes_only_what_waited_long_enough() {
    let f = Fixture::new();
    let t = trash(&f, u64::MAX);
    let old = f.work.join("old");
    fs::create_dir(&old).unwrap();
    fs::write(old.join("file"), b"x").unwrap();
    locked_dir(&old.join("deep"));
    let outside = f.file("outside", b"keep");
    let link = f.work.join("link");
    symlink(&outside, &link).unwrap();
    trash_all(&f, vec![old, link]);
    let now = SystemTime::now();
    // Undated and unreadable records are left alone.
    fs::write(bin(&f).join("files/mystery"), b"?").unwrap();
    fs::write(bin(&f).join("info/mystery.trashinfo"), b"[Trash Info]\n").unwrap();
    fs::write(bin(&f).join("files/stray"), b"no record").unwrap();
    let purged = t.purge(now + Duration::from_secs(29 * 86_400));
    assert_eq!((purged.removed, purged.errors.len()), (0, 0));
    assert_eq!(listing(&bin(&f).join("files")).len(), 4);
    let purged = t.purge(now + Duration::from_secs(31 * 86_400));
    assert_eq!(
        (purged.removed, purged.errors.len()),
        (2, 0),
        "{:?}",
        purged.errors
    );
    assert_eq!(listing(&bin(&f).join("files")), ["mystery", "stray"]);
    assert_eq!(listing(&bin(&f).join("info")), ["mystery.trashinfo"]);
    // A link's target is never followed.
    assert_eq!(fs::read(&outside).unwrap(), b"keep");
    // Records can only name entries inside files/, never the bin itself.
    for name in [".trashinfo", "..trashinfo", "...trashinfo"] {
        fs::write(
            bin(&f).join("info").join(name),
            b"[Trash Info]\nDeletionDate=2000-01-01T00:00:00\n",
        )
        .unwrap();
    }
    t.purge(now + Duration::from_secs(31 * 86_400));
    assert_eq!(listing(&bin(&f).join("files")), ["mystery", "stray"]);
    for name in [".trashinfo", "..trashinfo", "...trashinfo"] {
        fs::remove_file(bin(&f).join("info").join(name)).unwrap();
    }
    // A record whose entry is gone is dropped once it is old.
    let gone = f.file("gone", b"gone");
    trash_all(&f, vec![gone]);
    fs::remove_file(bin(&f).join("files/gone")).unwrap();
    let purged = t.purge(now + Duration::from_secs(31 * 86_400));
    assert_eq!(purged.removed, 1);
    assert_eq!(listing(&bin(&f).join("info")), ["mystery.trashinfo"]);
    // The shorter the setting, the sooner.
    let soon = f.file("soon", b"soon");
    trash_all(&f, vec![soon]);
    let mut short = t.clone();
    short.days = 1;
    assert_eq!(
        short.purge(now + Duration::from_secs(2 * 86_400)).removed,
        1
    );
    // A registry of unusable paths is harmless.
    fs::write(&t.registry, b"relative\n/does/not/exist\n\n").unwrap();
    assert_eq!(t.bins(), [bin(&f), PathBuf::from("/does/not/exist")]);
    assert_eq!(t.purge(now).removed, 0);
    // Purging in the background reports the same way.
    let gone = f.file("bg", b"bg");
    trash_all(&f, vec![gone]);
    assert_eq!(t.purge_in_background().join().unwrap().removed, 0);
}
#[test]
fn copies_report_progress_and_stop_unchanged() {
    let f = Fixture::new();
    let big = f.file("big", &vec![1; (CHUNK as usize) * 2 + 5]);
    let small = f.file("small", b"12345");
    let progress = Arc::new(Progress::default());
    let e = Engine::new(None, Some(progress.clone()));
    e.execute(Request::Copy {
        sources: vec![big.clone(), small.clone()],
        directory: f.dest.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    })
    .unwrap();
    let total = 2 * CHUNK + 10;
    assert_eq!(progress.total.load(Ordering::Relaxed), total);
    assert_eq!(progress.done.load(Ordering::Relaxed), total);
    assert!(!progress.stoppable.load(Ordering::Relaxed));
    // Stopped while staging: nothing is published or left behind.
    let into = f.root.join("into");
    fs::create_dir(&into).unwrap();
    let progress = Arc::new(Progress::default());
    progress.cancel.store(true, Ordering::Relaxed);
    let e = Engine::new(None, Some(progress.clone()));
    let error = e
        .execute(Request::Copy {
            sources: vec![big.clone(), small],
            directory: into.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap_err();
    assert_eq!(error, CANCELLED);
    assert!(listing(&into).is_empty(), "{:?}", listing(&into));
    // Every operation can stop while its trees are checked...
    let e = Engine::new(Some(trash(&f, u64::MAX)), Some(progress.clone()));
    for request in [
        Request::Move {
            sources: vec![big.clone()],
            directory: into.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        },
        Request::Delete {
            sources: vec![big.clone()],
            confirmed: vec![],
            trash: vec![true],
        },
    ] {
        assert_eq!(e.execute(request).unwrap_err(), CANCELLED);
    }
    assert!(big.exists() && listing(&into).is_empty());
    assert!(listing(&bin(&f).join("files")).is_empty());
    // ...but once a move has begun, Ctrl+c is ignored.
    let progress = Arc::new(Progress::default());
    let mut e = Engine::new(None, Some(progress.clone()));
    e.hook = Some(Box::new(move |phase| {
        if phase == 0 {
            progress.cancel.store(true, Ordering::Relaxed);
        }
    }));
    e.execute(Request::Move {
        sources: vec![big],
        directory: into.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    })
    .unwrap();
    assert_eq!(listing(&into), ["big"]);
}
#[test]
fn extractions_and_deletions_stop_safely() {
    let f = Fixture::new();
    let archive = f.work.join("x.tar");
    tar(&archive, &[("unpacked/file", b'0', b"payload", "")]);
    let progress = Arc::new(Progress::default());
    progress.cancel.store(true, Ordering::Relaxed);
    let e = Engine::new(None, Some(progress.clone()));
    let error = e
        .execute(Request::Extract {
            archives: vec![archive.clone()],
            destination: Some(f.dest.clone()),
        })
        .unwrap_err();
    assert_eq!(error, CANCELLED);
    assert!(listing(&f.dest).is_empty());
    // An extraction counts what it writes, with no known total.
    let progress = Arc::new(Progress::default());
    Engine::new(None, Some(progress.clone()))
        .execute(Request::Extract {
            archives: vec![archive],
            destination: Some(f.dest.clone()),
        })
        .unwrap();
    assert_eq!(progress.done.load(Ordering::Relaxed), 7);
    assert_eq!(progress.total.load(Ordering::Relaxed), 0);
    // A deletion counts entries; stopped, it puts back what remains.
    let tree = f.work.join("tree");
    fs::create_dir(&tree).unwrap();
    for n in 0..5 {
        fs::write(tree.join(n.to_string()), b"x").unwrap();
    }
    let last = f.file("last", b"last");
    let progress = Arc::new(Progress::default());
    let e = Engine::new(None, Some(progress.clone()));
    let stop = progress.clone();
    let out = {
        let mut e = e;
        e.hook = Some(Box::new(move |phase| {
            if phase == usize::MAX - 2 {
                stop.cancel.store(true, Ordering::Relaxed);
            }
        }));
        e.execute(Request::Delete {
            sources: vec![tree.clone(), last.clone()],
            confirmed: vec![],
            trash: vec![],
        })
        .unwrap()
    };
    let warning = out.warning.unwrap();
    // The stop came before anything was unlinked: nothing is.
    assert_eq!(
        warning,
        format!(
            "Stopped after deleting 0 of 2; {tree:?} was not deleted. It, and the entry after \
             it, is back in place."
        )
    );
    assert_eq!(listing(&tree).len(), 5);
    assert_eq!(fs::read(&last).unwrap(), b"last");
    assert!(stranded(&f.work).is_empty());
    assert_eq!(progress.total.load(Ordering::Relaxed), 7);
    // Uninterrupted, every entry is counted: the tree, its five, and last.
    let progress = Arc::new(Progress::default());
    Engine::new(None, Some(progress.clone()))
        .execute(Request::Delete {
            sources: vec![tree, last],
            confirmed: vec![],
            trash: vec![],
        })
        .unwrap();
    assert_eq!(progress.done.load(Ordering::Relaxed), 7);
    assert_eq!(progress.total.load(Ordering::Relaxed), 7);
}
#[test]
fn other_filesystems_get_a_bin_at_their_top() {
    if !in_namespace("operations::tests::trash::volume_bins_in_namespace") {
        volume_bins_in_namespace();
    }
}
fn mount_tmpfs(at: &Path, options: &str) {
    let status = std::process::Command::new("mount")
        .args(["-t", "tmpfs", "-o", options, "none"])
        .arg(at)
        .status()
        .unwrap();
    assert!(status.success());
}
#[test]
#[ignore = "runs inside a user and mount namespace"]
fn volume_bins_in_namespace() {
    if std::env::var_os("FFF_IN_NAMESPACE").is_none() {
        return;
    }
    let f = Fixture::new();
    let volume = f.root.join("volume");
    fs::create_dir(&volume).unwrap();
    mount_tmpfs(&volume, "mode=0755");
    fs::create_dir(volume.join("deep")).unwrap();
    let file = volume.join("deep/file");
    fs::write(&file, b"elsewhere").unwrap();
    let out = trash_all(&f, vec![file.clone()]);
    let uid = rustix::process::geteuid().as_raw();
    let volume_bin = volume.join(format!(".fff-trash-{uid}"));
    assert_eq!(
        fs::read(volume_bin.join("files/file")).unwrap(),
        b"elsewhere"
    );
    assert_eq!(fs::metadata(&volume_bin).unwrap().mode() & 0o777, 0o700);
    // The home bin stays empty; the registry lets the purge find this one.
    assert!(listing(&bin(&f).join("files")).is_empty());
    let t = trash(&f, u64::MAX);
    assert_eq!(t.bins(), [bin(&f), volume_bin.clone()]);
    engine(&f).undo(&record(out)).unwrap();
    assert_eq!(fs::read(&file).unwrap(), b"elsewhere");
    trash_all(&f, vec![file.clone()]);
    let later = SystemTime::now() + Duration::from_secs(31 * 86_400);
    assert_eq!(t.purge(later).removed, 1);
    assert!(listing(&volume_bin.join("files")).is_empty());
    // Registered once, however often it is used.
    let again = volume.join("again");
    fs::write(&again, b"x").unwrap();
    trash_all(&f, vec![again]);
    assert_eq!(t.bins().len(), 2);
    // A bin that is a symlink, or where none can be made, is refused.
    let linked = f.root.join("linked");
    fs::create_dir(&linked).unwrap();
    mount_tmpfs(&linked, "mode=0755");
    fs::create_dir(f.root.join("elsewhere")).unwrap();
    symlink(
        f.root.join("elsewhere"),
        linked.join(format!(".fff-trash-{uid}")),
    )
    .unwrap();
    fs::write(linked.join("x"), b"x").unwrap();
    let readonly = f.root.join("readonly");
    fs::create_dir(&readonly).unwrap();
    mount_tmpfs(&readonly, "mode=0755");
    fs::write(readonly.join("y"), b"y").unwrap();
    let status = std::process::Command::new("mount")
        .args(["-o", "remount,bind,ro"])
        .arg(&readonly)
        .status()
        .unwrap();
    assert!(status.success());
    let plan = DeletePlan::trash(vec![linked.join("x"), readonly.join("y")], &t).unwrap();
    for fate in &plan.fates {
        assert!(
            matches!(fate, Fate::Unavailable(reason) if reason.contains("no trash folder can be made")),
            "{fate:?}"
        );
    }
    assert!(listing(&f.root.join("elsewhere")).is_empty());
    // Undoing a copy onto that filesystem trashes the copy in its own bin.
    let copy = engine(&f)
        .execute(Request::Copy {
            sources: vec![f.file("c", b"c")],
            directory: volume.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    engine(&f).undo(&record(copy)).unwrap();
    assert_eq!(fs::read(volume_bin.join("files/c")).unwrap(), b"c");
    // Mount points cannot be removed while mounted.
    for mounted in [&volume, &linked, &readonly] {
        let status = std::process::Command::new("umount")
            .arg(mounted)
            .status()
            .unwrap();
        assert!(status.success());
    }
}
