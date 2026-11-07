use super::*;
use std::os::unix::{ffi::OsStringExt, fs::symlink};
struct Fixture {
    root: PathBuf,
    work: PathBuf,
    dest: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        // Canonical, as the paths operations report are (macOS's temporary
        // directory is below the `/var` symlink).
        let root = std::env::temp_dir()
            .canonicalize()
            .unwrap()
            .join(format!("fff-ops-test-{}", unique()));
        fs::create_dir(&root).unwrap();
        let work = root.join("work");
        let dest = root.join("dest");
        fs::create_dir(&work).unwrap();
        fs::create_dir(&dest).unwrap();
        Self { root, work, dest }
    }
    fn engine(&self) -> Engine {
        Engine::default()
    }
    fn file(&self, name: &str, data: &[u8]) -> PathBuf {
        let p = self.work.join(name);
        fs::write(&p, data).unwrap();
        p
    }
}
/// Whether `dir`'s filesystem takes names that are not UTF-8 (APFS refuses
/// them with `EILSEQ`).
fn takes_any_bytes(dir: &Path) -> bool {
    let probe = dir.join(OsString::from_vec(b"probe-\xff".to_vec()));
    let taken = fs::write(&probe, b"").is_ok();
    let _ = fs::remove_file(probe);
    taken
}
/// A socket listening at `path`, bound at a short path and moved there
/// when `path` is longer than a socket address holds (104 bytes on macOS).
fn socket_at(path: &Path) -> std::os::unix::net::UnixListener {
    use std::os::unix::net::UnixListener;
    match UnixListener::bind(path) {
        Err(e) if e.kind() == std::io::ErrorKind::InvalidInput => {
            let short = Path::new("/tmp").join(format!("fff-{}", unique()));
            let listener = UnixListener::bind(&short).unwrap();
            fs::rename(&short, path).unwrap();
            listener
        }
        bound => bound.unwrap(),
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
#[test]
fn copy_tree_preserves_hidden_contents_permissions_and_links_without_following() {
    let f = Fixture::new();
    let dir = f.work.join("tree");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join(".hidden"), b"hidden").unwrap();
    symlink("/does/not/exist", dir.join("broken")).unwrap();
    fs::set_permissions(dir.join(".hidden"), fs::Permissions::from_mode(0o751)).unwrap();
    let _ = f
        .engine()
        .execute(Request::Copy {
            sources: vec![dir.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    {
        let root = f.dest.join("tree");
        assert_eq!(fs::read(root.join(".hidden")).unwrap(), b"hidden");
        assert_eq!(
            fs::read_link(root.join("broken")).unwrap(),
            Path::new("/does/not/exist")
        );
        assert_eq!(
            fs::metadata(root.join(".hidden")).unwrap().mode() & 0o777,
            0o751
        );
    }
    assert!(dir.exists());
}
#[test]
fn copies_and_moves_reach_folders_below_ones_that_cannot_be_listed() {
    let f = Fixture::new();
    // Like a home folder of mode 0711: entered, never listed.
    let closed = f.root.join("closed");
    let inside = closed.join("inside");
    fs::create_dir_all(&inside).unwrap();
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o300)).unwrap();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let copied = f.engine().execute(Request::Copy {
        sources: vec![a.clone()],
        directory: inside.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    });
    let moved = f.engine().execute(Request::Move {
        sources: vec![b.clone()],
        directory: inside.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    });
    fs::set_permissions(&closed, fs::Permissions::from_mode(0o755)).unwrap();
    copied.unwrap();
    moved.unwrap();
    assert_eq!(fs::read(inside.join("a")).unwrap(), b"a");
    assert_eq!(fs::read(inside.join("b")).unwrap(), b"b");
    assert!(a.exists() && !b.exists());
}
#[test]
fn move_renames_file_and_directory() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.work.join("dir");
    fs::create_dir(&b).unwrap();
    fs::write(b.join("b"), b"b").unwrap();
    let out = f
        .engine()
        .execute(Request::Move {
            sources: vec![a.clone(), b.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    assert!(!a.exists() && !b.exists());
    assert_eq!(out.moved.len(), 2);
    assert_eq!(fs::read(f.dest.join("dir/b")).unwrap(), b"b");
}
#[test]
fn symlink_uses_real_absolute_paths_and_keeps_source_unchanged() {
    let f = Fixture::new();
    let src = f.file("a", b"a");
    let _ = f
        .engine()
        .execute(Request::Symlink {
            sources: vec![src.clone()],
            directory: f.dest.clone(),
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    assert_eq!(fs::read_link(f.dest.join("a")).unwrap(), src);
    assert_eq!(fs::read(src).unwrap(), b"a");
}
#[test]
fn non_utf8_paths_work_for_copy_move_delete_and_symlink() {
    let f = Fixture::new();
    if !takes_any_bytes(&f.work) {
        return;
    }
    let name = OsString::from_vec(b"bad-\xff".to_vec());
    let src = f.work.join(&name);
    fs::write(&src, b"bytes").unwrap();
    let out = f
        .engine()
        .execute(Request::Move {
            sources: vec![src.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    assert_eq!(fs::read(f.dest.join(&name)).unwrap(), b"bytes");
    assert!(out.changed);
    assert!(rename_text(&[f.dest.join(&name)], &f.dest).is_err());
    f.engine()
        .execute(Request::Delete {
            sources: vec![f.dest.join(&name)],
            confirmed: vec![],
            trash: vec![],
        })
        .unwrap();
    assert!(!f.dest.join(&name).exists());
}
#[test]
fn all_collisions_are_checked_before_any_copy_or_move() {
    for moving in [false, true] {
        let f = Fixture::new();
        let a = f.file("a", b"a");
        let b = f.file("b", b"b");
        fs::write(f.dest.join("b"), b"existing").unwrap();
        let sources = vec![a.clone(), b.clone()];
        let r = if moving {
            Request::Move {
                sources,
                directory: f.dest.clone(),
                replace: vec![],
                confirmed: vec![],
                into: None,
            }
        } else {
            Request::Copy {
                sources,
                directory: f.dest.clone(),
                replace: vec![],
                confirmed: vec![],
                into: None,
            }
        };
        assert!(
            f.engine()
                .execute(r)
                .unwrap_err()
                .contains("already exists")
        );
        assert!(!f.dest.join("a").exists());
        assert_eq!(fs::read(f.dest.join("b")).unwrap(), b"existing");
        assert!(a.exists() && b.exists());
    }
}
#[test]
fn dangling_destination_symlink_is_a_collision() {
    let f = Fixture::new();
    let p = f.file("a", b"a");
    symlink("missing", f.dest.join("a")).unwrap();
    assert!(
        f.engine()
            .execute(Request::Copy {
                sources: vec![p],
                directory: f.dest.clone(),
                replace: vec![],
                confirmed: vec![],
                into: None,
            })
            .is_err()
    );
    assert!(fs::symlink_metadata(f.dest.join("a")).unwrap().is_symlink());
}
#[test]
fn rejects_overlapping_sources_and_destination_in_source() {
    let f = Fixture::new();
    let dir = f.work.join("dir");
    fs::create_dir(&dir).unwrap();
    let file = dir.join("file");
    fs::write(&file, b"x").unwrap();
    assert!(
        f.engine()
            .execute(Request::Copy {
                sources: vec![dir.clone(), file],
                directory: f.dest.clone(),
                replace: vec![],
                confirmed: vec![],
                into: None,
            })
            .is_err()
    );
    assert!(
        f.engine()
            .execute(Request::Copy {
                sources: vec![dir.clone()],
                directory: dir,
                replace: vec![],
                confirmed: vec![],
                into: None,
            })
            .is_err()
    );
}
#[test]
fn missing_source_and_special_files_abort_everything() {
    let f = Fixture::new();
    let p = f.file("good", b"ok");
    assert!(
        f.engine()
            .execute(Request::Delete {
                sources: vec![p.clone(), f.work.join("missing")],
                confirmed: vec![],
                trash: vec![],
            })
            .is_err()
    );
    let socket = f.work.join("socket");
    let _socket = socket_at(&socket);
    assert!(
        f.engine()
            .execute(Request::Delete {
                sources: vec![p.clone(), socket],
                confirmed: vec![],
                trash: vec![],
            })
            .is_err()
    );
    assert!(p.exists());
}
#[test]
fn rename_row_count_empty_parent_change_directory_change_collisions_are_atomic() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let dir = f.work.join("dir");
    fs::create_dir(&dir).unwrap();
    assert!(rename_request(vec![a.clone(), b.clone()], &f.work, "one").is_err());
    assert!(rename_request(vec![a.clone()], &f.work, "").is_err());
    for (sources, targets) in [
        (
            vec![a.clone(), b.clone()],
            vec![f.work.join("new"), f.dest.join("b")],
        ),
        (
            vec![a.clone(), dir.clone()],
            vec![f.work.join("new"), f.work.join("b")],
        ),
        (
            vec![a.clone(), b.clone()],
            vec![f.work.join("new"), f.work.join("new")],
        ),
        (vec![a.clone(), b.clone()], vec![b.clone(), a.clone()]),
    ] {
        assert!(
            f.engine()
                .execute(Request::Rename { sources, targets })
                .is_err()
        );
        assert!(a.exists() && b.exists() && dir.exists());
        assert!(!f.work.join("new").exists());
    }
}
#[test]
fn rename_success_and_unchanged_changes_nothing() {
    let f = Fixture::new();
    let a = f.file("space ü", b"a");
    let text = rename_text(std::slice::from_ref(&a), &f.work).unwrap();
    let _ = f
        .engine()
        .execute(rename_request(vec![a.clone()], &f.work, &text).unwrap())
        .unwrap();
    let _ = f
        .engine()
        .execute(rename_request(vec![a.clone()], &f.work, "new name").unwrap())
        .unwrap();
    assert!(!a.exists());
    assert_eq!(fs::read(f.work.join("new name")).unwrap(), b"a");
}
fn docs_tree(f: &Fixture) -> Vec<PathBuf> {
    let docs = f.work.join("docs");
    fs::create_dir(&docs).unwrap();
    fs::write(docs.join("myfile"), b"m").unwrap();
    vec![docs.clone(), docs.join("myfile")]
}
#[test]
fn renaming_a_directory_through_a_descendant_row_aborts() {
    for text in [
        "docs\ndocuments/myfile",
        "documents\nDocuments/myfile",
        "docs\ndocuments/my_file",
        "docs\n/elsewhere/myfile",
        "docs\nmyfile",
        "docs\ndocs/../myfile",
    ] {
        let f = Fixture::new();
        let sources = docs_tree(&f);
        let result = rename_request(sources, &f.work, text).and_then(|r| f.engine().execute(r));
        assert!(result.is_err(), "{text:?}");
        assert_eq!(fs::read(f.work.join("docs/myfile")).unwrap(), b"m");
    }
}
#[test]
fn directory_and_descendant_rows_rename_together_in_either_spelling() {
    for text in ["documents\ndocs/my_file", "documents\ndocuments/my_file"] {
        let f = Fixture::new();
        let sources = docs_tree(&f);
        let out = f
            .engine()
            .execute(rename_request(sources, &f.work, text).unwrap())
            .unwrap();
        assert!(!f.work.join("docs").exists());
        assert_eq!(fs::read(f.work.join("documents/my_file")).unwrap(), b"m");
        let documents = f.work.join("documents");
        assert_eq!(
            out.moved,
            vec![
                (f.work.join("docs"), documents.clone()),
                (f.work.join("docs/myfile"), documents.join("my_file")),
            ]
        );
    }
}
#[test]
fn nested_renames_follow_every_renamed_ancestor_and_roll_back() {
    let f = Fixture::new();
    fs::create_dir_all(f.work.join("a/b")).unwrap();
    fs::write(f.work.join("a/b/c"), b"c").unwrap();
    let rows = |names: [&str; 3]| names.map(|p| f.work.join(p)).to_vec();
    // Directory rows alone carry their descendants along.
    let out = f
        .engine()
        .execute(rename_request(rows(["a", "a/b", "a/b/c"]), &f.work, "A\nA/B\nA/B/c").unwrap())
        .unwrap();
    assert_eq!(fs::read(f.work.join("A/B/c")).unwrap(), b"c");
    assert_eq!(out.moved.len(), 2);
    let current = rows(["A", "A/B", "A/B/c"]);
    // A mixed spelling is neither the original nor the final parent.
    assert!(rename_request(current.clone(), &f.work, "x\nx/y\nA/y/z").is_err());
    // A late collision is found before anything is renamed.
    fs::write(f.work.join("A/B/taken"), b"t").unwrap();
    let request = rename_request(current.clone(), &f.work, "x\nx/y\nx/y/taken").unwrap();
    assert!(f.engine().execute(request).is_err());
    // A racing entry rolls back the parents renamed first.
    let mut e = f.engine();
    let racer = f.work.join("x/y/z");
    e.hook = Some(Box::new(move |phase| {
        if phase == 2 {
            fs::write(&racer, b"race").unwrap();
        }
    }));
    let error = e
        .execute(rename_request(current, &f.work, "x\nx/y\nx/y/z").unwrap())
        .unwrap_err();
    assert!(error.contains("rolled back"), "{error}");
    assert_eq!(fs::read(f.work.join("A/B/c")).unwrap(), b"c");
    assert_eq!(fs::read(f.work.join("A/B/z")).unwrap(), b"race");
}
#[test]
fn unsafe_new_names_are_refused_before_any_change() {
    for name in [
        "a\r",
        "a\tb",
        "\u{1b}[2Jx",
        "a\u{7f}",
        "a\0b",
        " a",
        "a ",
        "a\u{a0}",
        "-rf",
        "~",
        "~x",
        "bad\u{fffd}",
        "",
    ] {
        assert!(check_name(OsStr::new(name)).is_err(), "{name:?}");
    }
    for name in ["a b", "a-", "a~", "ü.txt", ".hidden", "x\u{301}"] {
        assert!(check_name(OsStr::new(name)).is_ok(), "{name:?}");
    }
    let f = Fixture::new();
    let a = f.file("a", b"a");
    for text in ["a2\r", "\u{1b}[31mx", "x ", "-rf", "~", "a\u{fffd}"] {
        let request = rename_request(vec![a.clone()], &f.work, text).unwrap();
        assert!(f.engine().execute(request).is_err(), "{text:?}");
    }
    for name in ["~", " ", "-rf", "a\u{1b}[31m", "a\0b"] {
        let draft = Draft {
            kind: EditKind::New(true),
            cwd: f.work.clone(),
            text: name.into(),
        };
        assert!(
            draft.request().and_then(|r| f.engine().execute(r)).is_err(),
            "{name:?}"
        );
    }
    let entries = fs::read_dir(&f.work).unwrap().count();
    assert_eq!(entries, 1);
    let draft = Draft {
        kind: EditKind::Extract(vec![a.clone()]),
        cwd: f.work.clone(),
        text: "~/out".into(),
    };
    assert!(draft.request().is_err());
}
#[test]
fn rename_skips_unchanged_directories_that_contain_renamed_rows() {
    let f = Fixture::new();
    let dir = f.work.join("sub");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("f"), b"f").unwrap();
    let sources = vec![dir.clone(), dir.join("f")];
    let text = rename_text(&sources, &f.work).unwrap();
    assert_eq!(text, "sub\nsub/f");
    let out = f
        .engine()
        .execute(rename_request(sources, &f.work, "sub\nsub/g").unwrap())
        .unwrap();
    assert_eq!(fs::read(dir.join("g")).unwrap(), b"f");
    assert_eq!(out.moved, vec![(dir.join("f"), dir.join("g"))]);
    // Only the renamed file is backed up, not its unchanged parent.
}
#[test]
fn copies_keep_modification_times() {
    let f = Fixture::new();
    let tree = f.work.join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("file"), b"x").unwrap();
    symlink("file", tree.join("link")).unwrap();
    let old = std::time::UNIX_EPOCH + std::time::Duration::new(1_000_000_000, 123_456_789);
    let set = |path: &Path| {
        let times = sys::Timestamps {
            last_access: sys::Timespec {
                tv_sec: 1_000_000_000,
                tv_nsec: 123_456_789,
            },
            last_modification: sys::Timespec {
                tv_sec: 1_000_000_000,
                tv_nsec: 123_456_789,
            },
        };
        sys::utimensat(sys::CWD, path, &times, AtFlags::SYMLINK_NOFOLLOW).unwrap();
    };
    for path in [tree.join("file"), tree.join("link"), tree.clone()] {
        set(&path);
    }
    let _ = f
        .engine()
        .execute(Request::Copy {
            sources: vec![tree.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap();
    {
        let root = f.dest.join("tree");
        for path in [root.join("file"), root.join("link"), root.clone()] {
            let modified = fs::symlink_metadata(&path).unwrap().modified().unwrap();
            assert_eq!(modified, old, "{path:?}");
        }
    }
}
#[test]
fn create_file_and_directory_refuse_overwrites_and_record_absence() {
    let f = Fixture::new();
    for directory in [false, true] {
        let path = f.dest.join(if directory { "folder" } else { "file" });
        let out = f
            .engine()
            .execute(Request::Create {
                path: path.clone(),
                directory,
            })
            .unwrap();
        assert!(path.exists());
        assert_eq!(path.is_dir(), directory);
        assert!(out.changed);
        assert!(
            f.engine()
                .execute(Request::Create { path, directory })
                .is_err()
        );
    }
}
#[test]
fn delete_tree_and_links_preserves_external_target() {
    let f = Fixture::new();
    let dir = f.work.join("dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("child"), b"12345").unwrap();
    let external = f.dest.join("outside");
    fs::write(&external, b"untouched").unwrap();
    symlink(&external, dir.join("link")).unwrap();
    let out = f
        .engine()
        .execute(Request::Delete {
            sources: vec![dir.clone()],
            confirmed: vec![],
            trash: vec![],
        })
        .unwrap();
    assert!(!dir.exists());
    assert_eq!(fs::read(external).unwrap(), b"untouched");
    assert!(out.removed == vec![dir]);
}
#[test]
fn weights_group_in_descending_order_and_do_not_follow_links() {
    let f = Fixture::new();
    let a = f.file("a", b"123");
    let dir = f.work.join("dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("b"), b"12345").unwrap();
    assert_eq!(weight(&dir).unwrap(), 5);
    let rows = vec![
        (a, 3),
        (dir, 5),
        (f.work.join("c"), 2),
        (f.work.join("d"), 1),
    ];
    let grouped = deletion_rows(&rows, 3);
    assert_eq!(
        grouped.iter().map(|(_, n)| *n).collect::<Vec<_>>(),
        vec![5, 3, 3]
    );
    assert_eq!(grouped[2].0, Listed::More(2));
    assert_eq!(deletion_rows(&rows, 1)[0].1, 11);
    assert!(deletion_rows(&rows, 0).is_empty());
    assert_eq!(human_size(1024 * 1024), "1.0 MiB");
}
#[test]
fn confirmation_refuses_a_changed_tree() {
    let f = Fixture::new();
    let p = f.file("a", b"before");
    let plan = DeletePlan::new(vec![p.clone()]).unwrap();
    fs::write(&p, b"after").unwrap();
    let error = f.engine().execute(plan.request().unwrap()).unwrap_err();
    assert!(error.contains("changed since confirmation"), "{error}");
    assert_eq!(fs::read(&p).unwrap(), b"after");
}
#[test]
fn confirmation_binds_to_the_confirmed_files_not_their_paths() {
    // A path whose parent symlink is repointed after confirmation names a
    // different file; the confirmed snapshot no longer matches it.
    let f = Fixture::new();
    for dir in ["one", "two"] {
        fs::create_dir(f.work.join(dir)).unwrap();
        fs::write(f.work.join(dir).join("f"), b"same").unwrap();
    }
    symlink("one", f.work.join("link")).unwrap();
    let plan = DeletePlan::new(vec![f.work.join("link/f")]).unwrap();
    let request = plan.request().unwrap();
    fs::remove_file(f.work.join("link")).unwrap();
    symlink("two", f.work.join("link")).unwrap();
    assert!(f.engine().execute(request).is_err());
    assert!(f.work.join("one/f").exists() && f.work.join("two/f").exists());
}
#[test]
fn racing_destination_rolls_back_earlier_moves() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let target = f.dest.join("b");
    let mut engine = f.engine();
    engine.hook = Some(Box::new(move |index| {
        if index == 1 {
            fs::write(&target, b"intruder").unwrap();
        }
    }));
    assert!(
        engine
            .execute(Request::Move {
                sources: vec![a.clone(), b.clone()],
                directory: f.dest.clone(),
                replace: vec![],
                confirmed: vec![],
                into: None,
            })
            .unwrap_err()
            .contains("rolled back")
    );
    assert!(a.exists() && b.exists());
    assert!(!f.dest.join("a").exists());
    assert_eq!(fs::read(f.dest.join("b")).unwrap(), b"intruder");
}
#[test]
fn source_changed_after_validation_never_gets_deleted() {
    let f = Fixture::new();
    let p = f.file("a", b"before");
    let source = p.clone();
    let mut engine = f.engine();
    engine.hook = Some(Box::new(move |phase| {
        if phase == usize::MAX {
            fs::write(&source, b"after").unwrap();
        }
    }));
    assert!(
        engine
            .execute(Request::Delete {
                sources: vec![p.clone()],
                confirmed: vec![],
                trash: vec![],
            })
            .unwrap_err()
            .contains("Source changed")
    );
    assert_eq!(fs::read(p).unwrap(), b"after");
}
#[test]
fn delete_rolls_back_detached_sources_if_next_source_changes() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let source = b.clone();
    let mut e = f.engine();
    e.hook = Some(Box::new(move |index| {
        if index == 1 {
            fs::write(&source, b"changed").unwrap();
        }
    }));
    assert!(
        e.execute(Request::Delete {
            sources: vec![a.clone(), b.clone()],
            confirmed: vec![],
            trash: vec![],
        })
        .is_err()
    );
    assert_eq!(fs::read(a).unwrap(), b"a");
    assert_eq!(fs::read(b).unwrap(), b"changed");
}
// Minimal USTAR fixtures keep malicious path bytes and entry types explicit.
fn tar(path: &Path, entries: &[(&str, u8, &[u8], &str)]) {
    let mut bytes = vec![];
    for (name, kind, data, link) in entries {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"0000644\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        h[136..148].copy_from_slice(b"00000000000\0");
        h[148..156].fill(b' ');
        h[156] = *kind;
        h[157..157 + link.len()].copy_from_slice(link.as_bytes());
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let sum = h.iter().map(|b| *b as u32).sum::<u32>();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        bytes.extend(h);
        bytes.extend_from_slice(data);
        while bytes.len() % 512 != 0 {
            bytes.push(0);
        }
    }
    bytes.extend([0; 1024]);
    fs::write(path, bytes).unwrap();
}
#[test]
fn extracts_tar_to_new_or_existing_directory_and_backs_up_archive() {
    let f = Fixture::new();
    let archive = f.work.join("a.tar");
    tar(&archive, &[("nested/file", b'0', b"contents", "")]);
    let dest = f.dest.join("new");
    let out = f
        .engine()
        .execute(Request::Extract {
            archives: vec![archive.clone()],
            destination: Some(dest.clone()),
        })
        .unwrap();
    assert_eq!(fs::read(dest.join("nested/file")).unwrap(), b"contents");
    assert_eq!(fs::metadata(&dest).unwrap().mode() & 0o777, 0o755);
    assert!(out.changed && archive.exists());
    f.engine()
        .execute(Request::Extract {
            archives: vec![archive],
            destination: Some(f.dest.clone()),
        })
        .unwrap();
    assert_eq!(fs::read(f.dest.join("nested/file")).unwrap(), b"contents");
}
#[test]
fn archives_refuse_traversal_absolute_links_special_duplicate_and_colliding_entries() {
    for entries in [
        vec![("../escape", b'0', b"bad".as_slice(), "")],
        vec![("/tmp/escape", b'0', b"bad".as_slice(), "")],
        vec![
            ("ok", b'0', b"ok".as_slice(), ""),
            ("link", b'2', b"".as_slice(), "../escape"),
        ],
        vec![("hard", b'1', b"".as_slice(), "outside")],
        vec![("device", b'3', b"".as_slice(), "")],
        vec![
            ("a", b'0', b"one".as_slice(), ""),
            ("a", b'0', b"two".as_slice(), ""),
        ],
        // C1 controls such as CSI (U+009B) drive terminals like ESC does.
        vec![("csi\u{9b}31m", b'0', b"bad".as_slice(), "")],
    ] {
        let f = Fixture::new();
        let archive = f.work.join("a.tar");
        tar(&archive, &entries);
        assert!(
            f.engine()
                .execute(Request::Extract {
                    archives: vec![archive],
                    destination: Some(f.dest.clone()),
                })
                .is_err()
        );
        assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 0);
        assert!(!f.root.join("escape").exists());
    }
    let f = Fixture::new();
    let archive = f.work.join("a.tar");
    tar(
        &archive,
        &[("new", b'0', b"new", ""), ("existing", b'0', b"bad", "")],
    );
    fs::write(f.dest.join("existing"), b"keep").unwrap();
    assert!(
        f.engine()
            .execute(Request::Extract {
                archives: vec![archive],
                destination: Some(f.dest.clone()),
            })
            .is_err()
    );
    assert!(!f.dest.join("new").exists());
    assert_eq!(fs::read(f.dest.join("existing")).unwrap(), b"keep");
}
#[test]
fn corrupt_archive_and_destination_symlink_leave_disk_untouched() {
    let f = Fixture::new();
    let archive = f.file("bad.zip", b"not an archive");
    assert!(
        f.engine()
            .execute(Request::Extract {
                archives: vec![archive.clone()],
                destination: Some(f.dest.clone()),
            })
            .is_err()
    );
    let target = f.work.join("target");
    symlink(&f.dest, &target).unwrap();
    assert!(
        f.engine()
            .execute(Request::Extract {
                archives: vec![archive],
                destination: Some(target),
            })
            .is_err()
    );
    assert_eq!(fs::read_dir(f.dest.clone()).unwrap().count(), 0);
}
#[test]
fn safe_archive_paths_and_new_name_validation() {
    for p in [
        "../a",
        "/a",
        "a\\b",
        "a/../../b",
        "a\nb",
        "x\u{1b}[2J",
        ".fff-stage-1-2-3/x",
    ] {
        assert!(crate::archive::safe_path(Path::new(p)).is_err(), "{p:?}");
    }
    // Colons are ordinary on Linux, as in timestamps.
    assert!(crate::archive::safe_path(Path::new("logs/12:30:00.txt")).is_ok());
    assert_eq!(
        crate::archive::safe_path(Path::new("./a/b")).unwrap(),
        Path::new("a/b")
    );
    let f = Fixture::new();
    for text in ["", "..", "../x", "a/b", "/absolute", "a\nb"] {
        assert!(
            Draft {
                kind: EditKind::New(false),
                cwd: f.work.clone(),
                text: text.into()
            }
            .request()
            .is_err()
        );
    }
}

#[test]
fn racing_copy_destination_rolls_back_every_published_copy() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    let target = f.dest.join("b");
    let mut e = f.engine();
    e.hook = Some(Box::new(move |index| {
        if index == 1 {
            fs::write(&target, b"other").unwrap();
        }
    }));
    assert!(
        e.execute(Request::Copy {
            sources: vec![a.clone(), b.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap_err()
        .contains("rolled back")
    );
    assert!(a.exists() && b.exists());
    assert!(!f.dest.join("a").exists());
    assert_eq!(fs::read(f.dest.join("b")).unwrap(), b"other");
}
#[test]
fn unreadable_files_can_be_deleted_but_not_copied() {
    // Deleting needs only the directory; copying must read the contents.
    let f = Fixture::new();
    let a = f.file("a", b"a");
    let b = f.file("b", b"b");
    fs::set_permissions(&b, fs::Permissions::from_mode(0o0)).unwrap();
    let copy = f.engine().execute(Request::Copy {
        sources: vec![a.clone(), b.clone()],
        directory: f.dest.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    });
    assert!(copy.is_err());
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 0);
    let delete = f.engine().execute(Request::Delete {
        sources: vec![a.clone(), b.clone()],
        confirmed: vec![],
        trash: vec![],
    });
    assert!(delete.is_ok());
    assert!(!a.exists() && !b.exists());
}
#[test]
fn zip_gzip_and_7zip_extract_without_external_commands_in_product() {
    let f = Fixture::new();
    let content = f.file("data", b"payload");
    let code = "import sys,zipfile,gzip,tarfile; p=sys.argv[1]; z=zipfile.ZipFile(p+'/x.zip','w',compression=zipfile.ZIP_DEFLATED); z.writestr('nested/data',b'payload'); z.close(); t=tarfile.open(p+'/x.tar.gz','w:gz'); t.add(p+'/data',arcname='nested/data'); t.close()";
    assert!(
        std::process::Command::new("python3")
            .args(["-c", code])
            .arg(&f.work)
            .status()
            .unwrap()
            .success()
    );
    for name in ["x.zip", "x.tar.gz"] {
        let dest = f.dest.join(name);
        f.engine()
            .execute(Request::Extract {
                archives: vec![f.work.join(name)],
                destination: Some(dest.clone()),
            })
            .unwrap();
        assert_eq!(fs::read(dest.join("nested/data")).unwrap(), b"payload");
    }
    if std::process::Command::new("bsdtar")
        .arg("--version")
        .output()
        .is_ok()
    {
        let archive = f.work.join("x.7z");
        assert!(
            std::process::Command::new("bsdtar")
                .args(["--format=7zip", "-cf"])
                .arg(&archive)
                .arg("-C")
                .arg(&f.work)
                .arg("data")
                .status()
                .unwrap()
                .success()
        );
        let dest = f.dest.join("7z");
        f.engine()
            .execute(Request::Extract {
                archives: vec![archive],
                destination: Some(dest.clone()),
            })
            .unwrap();
        assert_eq!(
            fs::read(dest.join("data")).unwrap(),
            fs::read(content).unwrap()
        );
    }
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in bytes {
        c ^= *b as u32;
        for _ in 0..8 {
            c = (c >> 1) ^ if c & 1 != 0 { 0xedb88320 } else { 0 };
        }
    }
    !c
}
fn rar_header(kind: u8, flags: u16, extra: &[u8]) -> Vec<u8> {
    let mut h = vec![kind];
    h.extend(flags.to_le_bytes());
    h.extend(((7 + extra.len()) as u16).to_le_bytes());
    h.extend(extra);
    let mut result = (crc32(&h) as u16).to_le_bytes().to_vec();
    result.extend(h);
    result
}
#[test]
fn extracts_stored_rar_without_using_unrar() {
    let f = Fixture::new();
    let data = b"rar payload";
    let name = b"data.txt";
    let mut bytes = b"Rar!\x1a\x07\x00".to_vec();
    bytes.extend(rar_header(0x73, 0, &[0; 6]));
    let mut extra = vec![];
    extra.extend((data.len() as u32).to_le_bytes());
    extra.extend((data.len() as u32).to_le_bytes());
    extra.push(3);
    extra.extend(crc32(data).to_le_bytes());
    extra.extend([0; 4]);
    extra.extend([20, 0x30]);
    extra.extend((name.len() as u16).to_le_bytes());
    extra.extend(0o100644u32.to_le_bytes());
    extra.extend(name);
    bytes.extend(rar_header(0x74, 0x8000, &extra));
    bytes.extend(data);
    bytes.extend(rar_header(0x7b, 0, &[]));
    let archive = f.file("a.rar", &bytes);
    f.engine()
        .execute(Request::Extract {
            archives: vec![archive],
            destination: Some(f.dest.clone()),
        })
        .unwrap();
    assert_eq!(fs::read(f.dest.join("data.txt")).unwrap(), data);
}
#[test]
fn extraction_limits_reject_oversized_headers_and_too_many_entries() {
    let f = Fixture::new();
    let archive = f.work.join("many.tar");
    let entries = (0..10_001).map(|i| format!("file-{i}")).collect::<Vec<_>>();
    let refs = entries
        .iter()
        .map(|s| (s.as_str(), b'0', b"".as_slice(), ""))
        .collect::<Vec<_>>();
    tar(&archive, &refs);
    assert!(
        f.engine()
            .execute(Request::Extract {
                archives: vec![archive],
                destination: Some(f.dest.clone()),
            })
            .unwrap_err()
            .contains("10,000")
    );
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 0);
}

#[test]
fn changes_after_detaching_delete_roots_are_restored_without_removal() {
    let f = Fixture::new();
    let dir = f.work.join("dir");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("original"), b"keep").unwrap();
    let work = f.work.clone();
    let mut e = f.engine();
    e.hook = Some(Box::new(move |phase| {
        if phase == usize::MAX - 1 {
            let stage = fs::read_dir(&work)
                .unwrap()
                .find(|e| {
                    e.as_ref()
                        .unwrap()
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".fff-stage-")
                })
                .unwrap()
                .unwrap()
                .path();
            fs::write(stage.join("entry/new"), b"new data").unwrap();
        }
    }));
    assert!(
        e.execute(Request::Delete {
            sources: vec![dir.clone()],
            confirmed: vec![],
            trash: vec![],
        })
        .unwrap_err()
        .contains("nothing changed")
    );
    assert_eq!(fs::read(dir.join("new")).unwrap(), b"new data");
    assert_eq!(fs::read(dir.join("original")).unwrap(), b"keep");
}

#[test]
fn failed_move_rollback_never_overwrites_a_new_source_entry() {
    let f = Fixture::new();
    let a = f.file("a", b"original a");
    let b = f.file("b", b"b");
    let source = a.clone();
    let target = f.dest.join("b");
    let mut e = f.engine();
    e.hook = Some(Box::new(move |index| {
        if index == 1 {
            fs::write(&source, b"new source").unwrap();
            fs::write(&target, b"new target").unwrap();
        }
    }));
    let error = e
        .execute(Request::Move {
            sources: vec![a.clone(), b.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap_err();
    assert!(error.contains("rollback incomplete"));
    // The message names where the moved entry stays.
    let left = format!(
        "left in place: {:?}",
        f.dest.canonicalize().unwrap().join("a")
    );
    assert!(error.contains(&left), "{error}");
    assert_eq!(fs::read(a).unwrap(), b"new source");
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"original a");
    assert!(b.exists());
}
#[test]
fn failed_delete_rollback_retains_original_in_private_stage() {
    let f = Fixture::new();
    let a = f.file("a", b"original a");
    let b = f.file("b", b"b");
    let source = a.clone();
    let later = b.clone();
    let mut e = f.engine();
    e.hook = Some(Box::new(move |index| {
        if index == 1 {
            fs::write(&source, b"new source").unwrap();
            fs::write(&later, b"changed").unwrap();
        }
    }));
    let error = e
        .execute(Request::Delete {
            sources: vec![a.clone(), b],
            confirmed: vec![],
            trash: vec![],
        })
        .unwrap_err();
    // The staging path holding the original is named.
    assert!(
        error.contains("could not restore") && error.contains(".fff-stage-"),
        "{error}"
    );
    assert_eq!(fs::read(a).unwrap(), b"new source");
    let stage = fs::read_dir(&f.work)
        .unwrap()
        .find(|p| {
            p.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".fff-stage-")
        })
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(fs::read(stage.join("entry")).unwrap(), b"original a");
    assert_eq!(fs::metadata(stage).unwrap().mode() & 0o777, 0o700);
}
#[test]
fn readonly_destination_failure_leaves_sources_and_destinations_intact() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    fs::set_permissions(&f.dest, fs::Permissions::from_mode(0o500)).unwrap();
    let error = f
        .engine()
        .execute(Request::Copy {
            sources: vec![a.clone()],
            directory: f.dest.clone(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        })
        .unwrap_err();
    fs::set_permissions(&f.dest, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(error.contains("Permission denied"), "{error}");
    assert_eq!(fs::read(a).unwrap(), b"a");
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 0);
}
/// A directory without owner write permission, like `cp -r` and `rm -r` handle.
fn locked_dir(path: &Path) {
    fs::create_dir(path).unwrap();
    fs::write(path.join("x"), b"x").unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o555)).unwrap();
}
fn unlock(path: &Path) {
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
}
#[test]
fn readonly_directories_copy_with_their_mode_and_times() {
    let f = Fixture::new();
    let ro = f.work.join("ro");
    locked_dir(&ro);
    let plan = TransferPlan::new(Transfer::Copy, vec![ro.clone()], f.dest.clone()).unwrap();
    let result = f.engine().execute(plan.request().unwrap());
    let copy = f.dest.join("ro");
    let (was, now) = (fs::metadata(&ro).unwrap(), fs::metadata(&copy));
    unlock(&copy);
    unlock(&ro);
    result.unwrap();
    let now = now.unwrap();
    assert_eq!(now.mode() & 0o7777, 0o555);
    assert_eq!(
        (now.mtime(), now.mtime_nsec()),
        (was.mtime(), was.mtime_nsec())
    );
    assert_eq!(fs::read(copy.join("x")).unwrap(), b"x");
    assert_eq!(fs::read(ro.join("x")).unwrap(), b"x");
}
#[test]
fn readonly_directories_can_be_deleted_and_overwritten() {
    let f = Fixture::new();
    let (ro, empty) = (f.work.join("ro"), f.work.join("empty"));
    locked_dir(&ro);
    fs::create_dir(&empty).unwrap();
    fs::set_permissions(&empty, fs::Permissions::from_mode(0o555)).unwrap();
    let plan = DeletePlan::new(vec![ro.clone(), empty.clone()]).unwrap();
    let result = f.engine().execute(plan.request().unwrap());
    unlock(&ro);
    unlock(&empty);
    assert!(result.unwrap().warning.is_none());
    assert!(!ro.exists() && !empty.exists());
    let a = f.file("a", b"new");
    locked_dir(&f.dest.join("a"));
    let plan = TransferPlan::new(Transfer::Move, vec![a], f.dest.clone()).unwrap();
    let result = f.engine().execute(plan.request().unwrap());
    unlock(&f.dest.join("a"));
    assert!(result.unwrap().warning.is_none());
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"new");
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 1);
}
#[test]
fn readonly_directories_move_to_another_folder_with_their_mode() {
    let f = Fixture::new();
    let ro = f.work.join("ro");
    locked_dir(&ro);
    let plan = TransferPlan::new(Transfer::Move, vec![ro.clone()], f.dest.clone()).unwrap();
    let result = f.engine().execute(plan.request().unwrap());
    let moved = f.dest.join("ro");
    let mode = fs::metadata(&moved).map(|m| m.mode() & 0o7777);
    unlock(&moved);
    unlock(&ro);
    assert!(result.unwrap().warning.is_none());
    assert!(!ro.exists());
    assert_eq!(mode.unwrap(), 0o555);
    assert_eq!(fs::read(moved.join("x")).unwrap(), b"x");
}
#[test]
fn a_cancelled_delete_puts_readonly_directories_back_with_their_mode() {
    let f = Fixture::new();
    let ro = f.work.join("ro");
    locked_dir(&ro);
    let other = f.file("other", b"other");
    let plan = DeletePlan::new(vec![ro.clone(), other.clone()]).unwrap();
    let mut e = f.engine();
    e.hook = Some(Box::new(move |phase| {
        if phase == 1 {
            fs::write(&other, b"changed").unwrap();
        }
    }));
    let result = e.execute(plan.request().unwrap());
    let mode = fs::metadata(&ro).map(|m| m.mode() & 0o7777);
    unlock(&ro);
    let error = result.unwrap_err();
    // The directory was detached and then put back.
    assert!(
        error.contains("Source changed before deletion; nothing changed"),
        "{error}"
    );
    assert_eq!(mode.unwrap(), 0o555);
    assert_eq!(fs::read(ro.join("x")).unwrap(), b"x");
}
#[test]
fn cross_filesystem_moves_refuse_when_available() {
    let f = Fixture::new();
    let other = Path::new("/dev/shm");
    if fs::metadata(other).is_ok_and(|m| m.dev() != fs::metadata(&f.work).unwrap().dev()) {
        let a = f.file(&format!("fff-cross-device-{}", unique()), b"a");
        assert!(
            f.engine()
                .execute(Request::Move {
                    sources: vec![a.clone()],
                    directory: other.into(),
                    replace: vec![],
                    confirmed: vec![],
                    into: None,
                })
                .unwrap_err()
                .contains("across filesystems")
        );
        assert!(a.exists());
    }
}

#[test]
fn archive_preflight_reads_contents_and_creates_no_output() {
    let f = Fixture::new();
    let text = f.file("fake.zip", b"plain text");
    assert!(validate_archive(&text).is_err());
    assert!(validate_archive(&f.work).is_err());
    assert!(validate_archive(&f.work.join("missing")).is_err());
    let archive = f.work.join("not-an-extension.txt");
    tar(&archive, &[("file", b'0', b"valid", "")]);
    validate_archive(&archive).unwrap();
    assert!(!f.work.join("file").exists());
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 0);
    symlink(&archive, f.work.join("link.zip")).unwrap();
    assert!(validate_archive(&f.work.join("link.zip")).is_err());
    tar(
        &archive,
        &[("ok", b'0', b"ok", ""), ("../outside", b'0', b"bad", "")],
    );
    assert!(
        validate_archive(&archive)
            .unwrap_err()
            .contains("unsafe path")
    );
    assert!(!f.work.join("ok").exists());
}
#[test]
fn archive_preflight_rejects_file_directory_conflicts_in_either_order() {
    for reverse in [false, true] {
        let f = Fixture::new();
        let path = f.work.join("conflict.tar");
        let mut entries = vec![
            ("parent", b'0', b"file".as_slice(), ""),
            ("parent/child", b'0', b"child".as_slice(), ""),
        ];
        if reverse {
            entries.reverse();
        }
        tar(&path, &entries);
        assert!(validate_archive(&path).is_err());
        assert!(
            f.engine()
                .execute(Request::Extract {
                    archives: vec![path],
                    destination: Some(f.dest.clone()),
                })
                .is_err()
        );
        assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 0);
    }
}
#[test]
fn move_overwrites_only_confirmed_destinations_and_backs_them_up() {
    let f = Fixture::new();
    let a = f.file("a", b"new a");
    let tree = f.work.join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("new"), b"new tree").unwrap();
    let c = f.file("c", b"c");
    fs::write(f.dest.join("a"), b"old a").unwrap();
    fs::create_dir(f.dest.join("tree")).unwrap();
    fs::write(f.dest.join("tree/old"), b"old tree").unwrap();
    let sources = vec![a.clone(), tree.clone(), c.clone()];
    // Without confirmation any collision aborts everything.
    let unconfirmed = Request::Move {
        sources: sources.clone(),
        directory: f.dest.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    };
    assert!(f.engine().execute(unconfirmed).is_err());
    let plan = TransferPlan::new(Transfer::Move, sources, f.dest.clone()).unwrap();
    assert_eq!(
        plan.overwrites,
        vec![(f.dest.join("a"), 5), (f.dest.join("tree"), 8)]
    );
    let out = f.engine().execute(plan.request().unwrap()).unwrap();
    assert!(out.warning.is_none());
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"new a");
    // Directories are replaced as a whole, never merged.
    assert!(!f.dest.join("tree/old").exists());
    assert_eq!(fs::read(f.dest.join("tree/new")).unwrap(), b"new tree");
    assert_eq!(fs::read(f.dest.join("c")).unwrap(), b"c");
    assert!(!a.exists() && !tree.exists() && !c.exists());
    // No staging directory is left behind.
    let mut names = fs::read_dir(&f.dest)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["a", "c", "tree"]);
}
#[test]
fn move_overwrite_refuses_changed_self_and_containing_destinations() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    fs::write(f.dest.join("a"), b"old").unwrap();
    let plan = TransferPlan::new(Transfer::Move, vec![a.clone()], f.dest.clone()).unwrap();
    fs::write(f.dest.join("a"), b"changed").unwrap();
    assert!(f.engine().execute(plan.request().unwrap()).is_err());
    assert_eq!(fs::read(&a).unwrap(), b"a");
    // Moving a path into its own directory would replace itself.
    assert!(TransferPlan::new(Transfer::Move, vec![a.clone()], f.work.clone()).is_err());
    // A destination that contains a moved path cannot be replaced.
    let inner = f.work.join("x");
    fs::create_dir(&inner).unwrap();
    let nested = inner.join("x");
    fs::write(&nested, b"n").unwrap();
    let request = Request::Move {
        sources: vec![nested.clone()],
        directory: f.work.clone(),
        replace: vec![inner.clone()],
        confirmed: vec![],
        into: None,
    };
    assert!(f.engine().execute(request).is_err());
    assert_eq!(fs::read(&nested).unwrap(), b"n");
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"changed");
}
#[test]
fn failed_overwriting_move_restores_replaced_entries() {
    let f = Fixture::new();
    let a = f.file("a", b"new a");
    let b = f.file("b", b"new b");
    fs::write(f.dest.join("a"), b"old a").unwrap();
    let plan =
        TransferPlan::new(Transfer::Move, vec![a.clone(), b.clone()], f.dest.clone()).unwrap();
    let mut e = f.engine();
    let racer = f.dest.join("b");
    e.hook = Some(Box::new(move |phase| {
        if phase == 1 {
            fs::write(&racer, b"race").unwrap();
        }
    }));
    let error = e.execute(plan.request().unwrap()).unwrap_err();
    assert!(error.contains("rolled back"), "{error}");
    assert_eq!(fs::read(&a).unwrap(), b"new a");
    assert_eq!(fs::read(&b).unwrap(), b"new b");
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"old a");
    assert_eq!(fs::read(f.dest.join("b")).unwrap(), b"race");
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 2);
}
#[test]
fn copy_overwrites_only_confirmed_destinations_and_keeps_sources() {
    let f = Fixture::new();
    let a = f.file("a", b"new a");
    let tree = f.work.join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("new"), b"new tree").unwrap();
    fs::write(f.dest.join("a"), b"old a").unwrap();
    fs::create_dir(f.dest.join("tree")).unwrap();
    fs::write(f.dest.join("tree/old"), b"old tree").unwrap();
    let sources = vec![a.clone(), tree.clone()];
    let unconfirmed = Request::Copy {
        sources: sources.clone(),
        directory: f.dest.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    };
    assert!(f.engine().execute(unconfirmed).is_err());
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"old a");
    let plan = TransferPlan::new(Transfer::Copy, sources, f.dest.clone()).unwrap();
    assert_eq!(plan.overwrites.len(), 2);
    let out = f.engine().execute(plan.request().unwrap()).unwrap();
    assert!(out.warning.is_none());
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"new a");
    assert!(!f.dest.join("tree/old").exists());
    assert_eq!(fs::read(f.dest.join("tree/new")).unwrap(), b"new tree");
    // Copies keep their sources.
    assert_eq!(fs::read(&a).unwrap(), b"new a");
    assert_eq!(fs::read(tree.join("new")).unwrap(), b"new tree");
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 2);
    // Copying into the source's own directory would overwrite the source.
    assert!(TransferPlan::new(Transfer::Copy, vec![a.clone()], f.work.clone()).is_err());
}
#[test]
fn failed_overwriting_copy_restores_overwritten_entries() {
    let f = Fixture::new();
    let a = f.file("a", b"new a");
    let b = f.file("b", b"new b");
    fs::write(f.dest.join("a"), b"old a").unwrap();
    let plan =
        TransferPlan::new(Transfer::Copy, vec![a.clone(), b.clone()], f.dest.clone()).unwrap();
    let mut e = f.engine();
    let racer = f.dest.join("b");
    e.hook = Some(Box::new(move |phase| {
        if phase == 1 {
            fs::write(&racer, b"race").unwrap();
        }
    }));
    let error = e.execute(plan.request().unwrap()).unwrap_err();
    assert!(error.contains("rolled back"), "{error}");
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"old a");
    assert_eq!(fs::read(f.dest.join("b")).unwrap(), b"race");
    assert_eq!(fs::read(&a).unwrap(), b"new a");
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 2);
}
/// Runs `test` again as root of a new user and mount namespace, where bind
/// mounts are allowed; returns false (skip) where namespaces are unavailable.
fn in_namespace(test: &str) -> bool {
    if std::env::var_os("FFF_IN_NAMESPACE").is_some() {
        return false;
    }
    let exe = std::env::current_exe().unwrap();
    let Ok(status) = std::process::Command::new("unshare")
        .args(["-rm"])
        .arg(exe)
        .args(["--exact", test, "--ignored", "--nocapture"])
        .env("FFF_IN_NAMESPACE", "1")
        .status()
    else {
        eprintln!("unshare unavailable; skipping {test}");
        return true;
    };
    assert!(status.success(), "{test} failed in a namespace");
    true
}
fn bind(from: &Path, to: &Path) {
    let status = std::process::Command::new("mount")
        .arg("--bind")
        .args([from, to])
        .status()
        .unwrap();
    assert!(status.success());
}
#[test]
fn same_device_bind_mounts_are_never_deleted_or_copied_through() {
    if !in_namespace("operations::tests::bind_mounts_in_namespace") {
        bind_mounts_in_namespace();
    }
}
#[test]
#[ignore = "runs inside a user and mount namespace"]
fn bind_mounts_in_namespace() {
    if std::env::var_os("FFF_IN_NAMESPACE").is_none() {
        return;
    }
    let f = Fixture::new();
    let outside = f.root.join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("precious"), b"keep").unwrap();
    let a = f.work.join("A");
    fs::create_dir_all(a.join("m")).unwrap();
    bind(&outside, &a.join("m"));
    // Same filesystem, same device number: still refused everywhere.
    assert!(
        DeletePlan::new(vec![a.clone()])
            .err()
            .unwrap()
            .contains("mount point")
    );
    let delete = Request::Delete {
        sources: vec![a.clone()],
        confirmed: vec![],
        trash: vec![],
    };
    assert!(f.engine().execute(delete).is_err());
    let copy = Request::Copy {
        sources: vec![a.clone()],
        directory: f.dest.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    };
    assert!(f.engine().execute(copy).is_err());
    assert_eq!(fs::read(outside.join("precious")).unwrap(), b"keep");
    // An alias of a source directory is still that source.
    let src = f.work.join("src");
    fs::create_dir_all(src.join("sub")).unwrap();
    let alias = f.work.join("alias");
    fs::create_dir(&alias).unwrap();
    bind(&src, &alias);
    let into_itself = Request::Copy {
        sources: vec![src.clone()],
        directory: alias.join("sub"),
        replace: vec![],
        confirmed: vec![],
        into: None,
    };
    let error = f.engine().execute(into_itself).unwrap_err();
    assert!(error.contains("inside a source"), "{error}");
    // Something mounted after validation stops the deletion; every root not
    // yet deleted, and what remains of the failing one, is put back.
    let roots = ["r0", "r1", "r2"].map(|n| f.work.join(n));
    for root in &roots {
        fs::create_dir_all(root.join("m")).unwrap();
        fs::write(root.join("data"), b"data").unwrap();
    }
    let mut e = f.engine();
    let (work, outside2) = (f.work.clone(), outside.clone());
    e.hook = Some(Box::new(move |phase| {
        if phase == usize::MAX - 2 {
            // r1's detached copy lives in the only stage holding "r1" data.
            let stage = fs::read_dir(&work)
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.join("entry/data").exists())
                .nth(1)
                .unwrap();
            bind(&outside2, &stage.join("entry/m"));
        }
    }));
    let plan = DeletePlan::new(roots.to_vec()).unwrap();
    // Partly done: reported as a warning, with what was removed listed.
    let out = e.execute(plan.request().unwrap()).unwrap();
    let error = out.warning.unwrap();
    assert!(
        error.contains("Deleted 1 of 3") && error.contains("back in place"),
        "{error}"
    );
    assert_eq!(out.removed, vec![roots[0].clone()]);
    assert_eq!(fs::read(outside.join("precious")).unwrap(), b"keep");
    // r0 was deleted; r1 failed partway and is back with whatever remained;
    // r2 was never touched.
    assert!(!roots[0].exists() && roots[1].is_dir(), "{error}");
    assert_eq!(fs::read(roots[2].join("data")).unwrap(), b"data");
    assert!(stranded(&f.work).is_empty());
}
#[test]
fn termination_signals_wait_for_the_operation_to_finish() {
    if std::env::var_os("FFF_SIGNAL_CHILD").is_some() {
        let marker = PathBuf::from(std::env::var_os("FFF_SIGNAL_CHILD").unwrap());
        let deferred = interrupt::Deferred::new();
        unsafe {
            libc::raise(libc::SIGTERM);
        }
        // Still running: the operation would finish here.
        fs::write(&marker, b"finished").unwrap();
        drop(deferred);
        unreachable!("SIGTERM is delivered once the operation ends");
    }
    let f = Fixture::new();
    let marker = f.root.join("marker");
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "operations::tests::termination_signals_wait_for_the_operation_to_finish",
            "--nocapture",
        ])
        .env("FFF_SIGNAL_CHILD", &marker)
        .status()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(libc::SIGTERM));
    assert_eq!(fs::read(&marker).unwrap(), b"finished");
}
#[test]
fn stranded_stages_of_dead_processes_are_reported() {
    let f = Fixture::new();
    fs::create_dir(f.work.join(".fff-stage-4194305-1-0")).unwrap();
    let live = format!(".fff-stage-{}-1-0", std::process::id());
    fs::create_dir(f.work.join(&live)).unwrap();
    assert_eq!(
        stranded(&f.work),
        vec![f.work.join(".fff-stage-4194305-1-0")]
    );
}
#[test]
fn copy_rollback_never_pulls_back_an_entry_it_did_not_publish() {
    let f = Fixture::new();
    let a = f.file("a", b"mine");
    let b = f.file("b", b"mine too");
    let mut e = f.engine();
    let dest = f.dest.clone();
    e.hook = Some(Box::new(move |phase| {
        if phase == 1 {
            // Someone replaces the published copy and takes the next name.
            fs::remove_file(dest.join("a")).unwrap();
            fs::write(dest.join("a"), b"theirs").unwrap();
            fs::write(dest.join("b"), b"theirs too").unwrap();
        }
    }));
    let request = Request::Copy {
        sources: vec![a, b],
        directory: f.dest.clone(),
        replace: vec![],
        confirmed: vec![],
        into: None,
    };
    let error = e.execute(request).unwrap_err();
    assert!(error.contains("rollback incomplete"), "{error}");
    let left = format!(
        "left in place: {:?}",
        f.dest.canonicalize().unwrap().join("a")
    );
    assert!(error.contains(&left), "{error}");
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"theirs");
    assert_eq!(fs::read(f.dest.join("b")).unwrap(), b"theirs too");
}
#[test]
fn extraction_into_an_existing_directory_needs_no_writable_parent() {
    let f = Fixture::new();
    let archive = f.work.join("a.tar");
    tar(&archive, &[("file", b'0', b"contents", "")]);
    let locked = f.root.join("locked");
    fs::create_dir_all(locked.join("into")).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o500)).unwrap();
    let result = f.engine().execute(Request::Extract {
        archives: vec![archive],
        destination: Some(locked.join("into")),
    });
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o700)).unwrap();
    result.unwrap();
    assert_eq!(fs::read(locked.join("into/file")).unwrap(), b"contents");
    assert_eq!(fs::read_dir(locked.join("into")).unwrap().count(), 1);
}
#[test]
fn new_files_get_ordinary_permissions_and_plans_refuse_repeated_names() {
    let f = Fixture::new();
    let reference = f.work.join("reference");
    fs::File::create(&reference).unwrap();
    let draft = Draft {
        kind: EditKind::New(false),
        cwd: f.work.clone(),
        text: "made".into(),
    };
    f.engine().execute(draft.request().unwrap()).unwrap();
    let mode = |p: &Path| fs::metadata(p).unwrap().mode() & 0o777;
    assert_eq!(mode(&f.work.join("made")), mode(&reference));
    let one = f.work.join("x");
    fs::create_dir_all(f.dest.join("x")).unwrap();
    fs::write(&one, b"1").unwrap();
    let error = TransferPlan::new(Transfer::Copy, vec![one, f.dest.join("x")], f.root.clone())
        .err()
        .unwrap();
    assert!(error.contains("Two selected entries"), "{error}");
}
#[test]
fn a_failed_delete_restores_exact_directory_modes() {
    if !in_namespace("operations::tests::modes_in_namespace") {
        modes_in_namespace();
    }
}
#[test]
#[ignore = "runs inside a user and mount namespace"]
fn modes_in_namespace() {
    if std::env::var_os("FFF_IN_NAMESPACE").is_none() {
        return;
    }
    let f = Fixture::new();
    let root = f.work.join("A");
    fs::create_dir_all(root.join("sub/m")).unwrap();
    let mut e = f.engine();
    let (work, outside) = (f.work.clone(), f.root.join("outside"));
    fs::create_dir(&outside).unwrap();
    e.hook = Some(Box::new(move |phase| {
        if phase == usize::MAX - 2 {
            let stage = fs::read_dir(&work).unwrap().next().unwrap().unwrap().path();
            fs::set_permissions(stage.join("entry/sub"), fs::Permissions::from_mode(0o2555))
                .unwrap();
            bind(&outside, &stage.join("entry/sub/m"));
        }
    }));
    let plan = DeletePlan::new(vec![root.clone()]).unwrap();
    let out = e.execute(plan.request().unwrap()).unwrap();
    assert!(out.warning.is_some());
    // The read-only, setgid directory is back exactly as it was.
    assert_eq!(
        fs::metadata(root.join("sub")).unwrap().mode() & 0o7777,
        0o2555
    );
}
#[test]
fn overwritten_entries_changed_while_detached_are_kept() {
    let f = Fixture::new();
    let a = f.file("a", b"new");
    fs::write(f.dest.join("a"), b"old").unwrap();
    let plan = TransferPlan::new(Transfer::Move, vec![a], f.dest.clone()).unwrap();
    let mut e = f.engine();
    let dest = f.dest.clone();
    e.hook = Some(Box::new(move |phase| {
        if phase == 0 {
            // A writer appends to the detached entry before it is deleted.
            let stage = fs::read_dir(&dest)
                .unwrap()
                .map(|e| e.unwrap().path())
                .find(|p| p.join("0").exists())
                .unwrap();
            fs::write(stage.join("0"), b"old, then written").unwrap();
        }
    }));
    let out = e.execute(plan.request().unwrap()).unwrap();
    let warning = out.warning.unwrap();
    assert!(
        warning.contains("changed while being overwritten"),
        "{warning}"
    );
    assert_eq!(fs::read(f.dest.join("a")).unwrap(), b"new");
    let kept = fs::read_dir(&f.dest)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.join("0").exists())
        .unwrap();
    assert_eq!(fs::read(kept.join("0")).unwrap(), b"old, then written");
}
#[test]
fn transfers_bind_the_confirmed_destination_directory() {
    let f = Fixture::new();
    let a = f.file("a", b"a");
    fs::create_dir(f.root.join("other")).unwrap();
    symlink(&f.dest, f.root.join("here")).unwrap();
    let plan = TransferPlan::new(Transfer::Move, vec![a.clone()], f.root.join("here")).unwrap();
    fs::remove_file(f.root.join("here")).unwrap();
    symlink(f.root.join("other"), f.root.join("here")).unwrap();
    let error = f.engine().execute(plan.request().unwrap()).unwrap_err();
    assert!(error.contains("destination directory changed"), "{error}");
    assert!(a.exists());
}
#[test]
fn extracted_files_respect_the_umask_and_delete_plans_refuse_overlaps() {
    let f = Fixture::new();
    let archive = f.work.join("a.tar");
    tar(&archive, &[("open", b'0', b"x", "")]);
    f.engine()
        .execute(Request::Extract {
            archives: vec![archive],
            destination: Some(f.dest.join("out")),
        })
        .unwrap();
    let reference = f.work.join("reference");
    fs::File::create(&reference).unwrap();
    // tar() writes mode 0644; never more permissive than the umask allows.
    let mode = fs::metadata(f.dest.join("out/open")).unwrap().mode() & 0o777;
    assert_eq!(
        mode & !(fs::metadata(&reference).unwrap().mode() & 0o777),
        0
    );
    let dir = f.work.join("d");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("x"), b"x").unwrap();
    assert!(DeletePlan::new(vec![dir.clone(), dir.join("x")]).is_err());
}
#[test]
fn a_new_extraction_destination_respects_a_restrictive_umask() {
    if std::env::var_os("FFF_UMASK_CHILD").is_some() {
        // Alone in this process: the umask is the process's.
        unsafe {
            libc::umask(0o077);
        }
        let f = Fixture::new();
        let archive = f.work.join("a.tar");
        tar(&archive, &[("docs/a", b'0', b"a", "")]);
        let existing = f.dest.join("existing");
        fs::create_dir(&existing).unwrap();
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o751)).unwrap();
        for destination in [f.dest.join("new"), existing.clone()] {
            f.engine()
                .execute(Request::Extract {
                    archives: vec![archive.clone()],
                    destination: Some(destination),
                })
                .unwrap();
        }
        let mode = |p: PathBuf| fs::metadata(p).unwrap().mode() & 0o777;
        assert_eq!(mode(f.dest.join("new")), 0o700);
        assert_eq!(mode(f.dest.join("new/docs")), 0o700);
        assert_eq!(mode(f.dest.join("new/docs/a")), 0o600);
        // An existing destination keeps its own mode.
        assert_eq!(mode(existing.clone()), 0o751);
        assert_eq!(mode(existing.join("docs")), 0o700);
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "operations::tests::a_new_extraction_destination_respects_a_restrictive_umask",
            "--nocapture",
        ])
        .env("FFF_UMASK_CHILD", "1")
        .status()
        .unwrap();
    assert!(status.success());
}
#[test]
fn several_archives_extract_together_or_not_at_all() {
    let f = Fixture::new();
    let one = f.work.join("one.tar");
    let two = f.work.join("two.tar");
    tar(&one, &[("docs/a", b'0', b"a", ""), ("one", b'0', b"1", "")]);
    tar(&two, &[("docs/b", b'0', b"b", ""), ("two", b'0', b"2", "")]);
    // A shared destination receives both; common directories combine.
    f.engine()
        .execute(Request::Extract {
            archives: vec![one.clone(), two.clone()],
            destination: Some(f.dest.join("both")),
        })
        .unwrap();
    for (path, data) in [("docs/a", "a"), ("docs/b", "b"), ("one", "1"), ("two", "2")] {
        assert_eq!(
            fs::read(f.dest.join("both").join(path)).unwrap(),
            data.as_bytes()
        );
    }
    // A file in two archives aborts everything; nothing is published.
    let clash = f.work.join("clash.tar");
    tar(&clash, &[("one", b'0', b"other", "")]);
    let error = f
        .engine()
        .execute(Request::Extract {
            archives: vec![one.clone(), clash],
            destination: Some(f.dest.clone()),
        })
        .unwrap_err();
    assert!(error.contains("clash.tar"), "{error}");
    assert_eq!(fs::read_dir(&f.dest).unwrap().count(), 1);
    // Without a destination each archive extracts beside itself.
    let elsewhere = f.root.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let three = elsewhere.join("three.tar");
    tar(&three, &[("three", b'0', b"3", "")]);
    f.engine()
        .execute(Request::Extract {
            archives: vec![two.clone(), three],
            destination: None,
        })
        .unwrap();
    assert_eq!(fs::read(f.work.join("two")).unwrap(), b"2");
    assert_eq!(fs::read(elsewhere.join("three")).unwrap(), b"3");
    assert!(!f.work.join("three").exists() && !elsewhere.join("two").exists());
    // Also in a folder browsed through a symlink, beside one that is not.
    let link = f.root.join("link");
    symlink(&elsewhere, &link).unwrap();
    tar(&elsewhere.join("four.tar"), &[("four", b'0', b"4", "")]);
    tar(&f.work.join("five.tar"), &[("five", b'0', b"5", "")]);
    f.engine()
        .execute(Request::Extract {
            archives: vec![link.join("four.tar"), f.work.join("five.tar")],
            destination: None,
        })
        .unwrap();
    assert_eq!(fs::read(elsewhere.join("four")).unwrap(), b"4");
    assert_eq!(fs::read(f.work.join("five")).unwrap(), b"5");
    assert!(!f.work.join("four").exists() && !elsewhere.join("five").exists());
    assert_eq!(fs::read_link(&link).unwrap(), elsewhere);
}
#[test]
fn a_late_extraction_collision_publishes_nothing() {
    let f = Fixture::new();
    let elsewhere = f.root.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    let one = f.work.join("one.tar");
    let two = elsewhere.join("two.tar");
    tar(&one, &[("fine", b'0', b"1", "")]);
    tar(&two, &[("taken", b'0', b"2", "")]);
    fs::write(elsewhere.join("taken"), b"mine").unwrap();
    let error = f
        .engine()
        .execute(Request::Extract {
            archives: vec![one, two],
            destination: None,
        })
        .unwrap_err();
    assert!(error.contains("already contains"), "{error}");
    assert!(!f.work.join("fine").exists());
    assert_eq!(fs::read(elsewhere.join("taken")).unwrap(), b"mine");
    assert!(stranded(&f.work).is_empty() && stranded(&elsewhere).is_empty());
}
#[path = "trash_tests.rs"]
mod trash;

#[test]
fn a_stop_is_heard_before_an_unlink_never_after_it() {
    let f = Fixture::new();
    for name in ["a", "b"] {
        fs::write(f.work.join(name), b"x").unwrap();
    }
    let dir = File::open(&f.work).unwrap();
    // The stop arrives while "a" is being counted, after its unlink.
    let stopped = std::cell::Cell::new(false);
    let tick = |n: u64| {
        if n > 0 {
            stopped.set(true);
        }
        if stopped.get() {
            Err(CANCELLED.to_string())
        } else {
            Ok(())
        }
    };
    // "a" is gone, so its removal succeeded; "b" is never touched.
    assert_eq!(remove_counting(&dir, OsStr::new("a"), &tick), Ok(()));
    assert!(!f.work.join("a").exists());
    assert_eq!(
        remove_counting(&dir, OsStr::new("b"), &tick),
        Err(CANCELLED.to_string())
    );
    assert!(f.work.join("b").exists());
}
