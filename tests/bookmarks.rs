use fuzzy_file_finder::{
    bookmarks::{Change, Draft, Store},
    config::{Bookmark, Settings},
};
use std::{fs, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fff-bookmarks-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("config.toml")
    }
    fn load(&self) -> Vec<Bookmark> {
        Settings::load(Some(&self.path()), false, None)
            .unwrap()
            .bookmarks
    }
    fn bookmark(&self, name: &str) -> Bookmark {
        Bookmark {
            name: name.into(),
            path: self.0.join(name),
            icon: Some("\u{f015}".into()),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn create_add_edit_and_delete_preserve_settings_comments_and_relative_paths() {
    let f = Fixture::new();
    fs::write(f.path(), "# user config\n[ui]\npalette = 'nord' # keep colors\n\n[[bookmarks]] # favourite\nname = 'Old' # name note\npath = 'notes' # relative path\n\n[cache]\nmax_entries = 8 # memory\n").unwrap();
    let initial = f.load();
    let mut store = Store::new(Some(f.path()), initial.clone(), None);
    let mut renamed = initial[0].clone();
    renamed.name = "New \"notes\"".into();
    renamed.icon = Some("\u{f02d}".into());
    let next = store
        .commit(
            &initial,
            &Change::Put {
                index: Some(0),
                bookmark: renamed.clone(),
            },
        )
        .unwrap();
    let text = fs::read_to_string(f.path()).unwrap();
    for kept in [
        "# user config",
        "palette = 'nord' # keep colors",
        "# favourite",
        "# name note",
        "path = 'notes' # relative path",
        "max_entries = 8 # memory",
    ] {
        assert!(text.contains(kept), "lost {kept}: {text}");
    }
    assert_eq!(f.load(), vec![renamed]);
    let second = f.bookmark("two");
    let next = store
        .commit(
            &next,
            &Change::Put {
                index: None,
                bookmark: second.clone(),
            },
        )
        .unwrap();
    assert_eq!(f.load(), next);
    fs::create_dir(&second.path).unwrap();
    fs::write(second.path.join("file"), "keep").unwrap();
    let next = store.commit(&next, &Change::Remove(1)).unwrap();
    assert_eq!(f.load(), next);
    assert!(second.path.join("file").exists());
    let next = store.commit(&next, &Change::Remove(0)).unwrap();
    assert!(next.is_empty() && f.load().is_empty());
    assert!(fs::read_to_string(f.path()).unwrap().contains("# memory"));
}

#[test]
fn session_only_edits_validate_names_and_do_not_write_a_config() {
    let f = Fixture::new();
    let mut store = Store::default();
    let first = f.bookmark("one");
    let next = store
        .commit(
            &[],
            &Change::Put {
                index: None,
                bookmark: first.clone(),
            },
        )
        .unwrap();
    assert!(!f.path().exists());
    assert!(
        store
            .commit(
                &next,
                &Change::Put {
                    index: None,
                    bookmark: first
                }
            )
            .is_err()
    );
    for name in ["", "   "] {
        assert!(
            store
                .commit(
                    &next,
                    &Change::Put {
                        index: None,
                        bookmark: f.bookmark(name)
                    }
                )
                .is_err()
        );
    }
    assert!(store.reload().is_err());
}

#[test]
fn missing_default_destination_is_created_only_on_save() {
    let f = Fixture::new();
    let path = f.0.join("new/fff/config.toml");
    let mut store = Store::new(Some(path.clone()), vec![], None);
    assert!(!path.exists());
    let bookmark = f.bookmark("one");
    store
        .commit(
            &[],
            &Change::Put {
                index: None,
                bookmark: bookmark.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        Settings::load(Some(&path), false, None).unwrap().bookmarks,
        vec![bookmark]
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn inline_bookmarks_and_icon_removal_preserve_other_configuration() {
    let f = Fixture::new();
    fs::write(f.path(), "bookmarks = [{ name = 'One', path = 'one', icon = '★' }] # collection\n[ui]\nicons = false # plain\n").unwrap();
    let initial = f.load();
    let mut store = Store::new(Some(f.path()), initial.clone(), None);
    let mut edited = initial[0].clone();
    edited.icon = None;
    edited.name = "Edited".into();
    let next = store
        .commit(
            &initial,
            &Change::Put {
                index: Some(0),
                bookmark: edited,
            },
        )
        .unwrap();
    let next = store
        .commit(
            &next,
            &Change::Put {
                index: None,
                bookmark: f.bookmark("Two"),
            },
        )
        .unwrap();
    assert_eq!(f.load(), next);
    let text = fs::read_to_string(f.path()).unwrap();
    assert!(text.contains("# collection") && text.contains("icons = false # plain"));
}

#[test]
fn swapping_reorders_tables_with_their_comments_and_inline_arrays_in_place() {
    let f = Fixture::new();
    fs::write(
        f.path(),
        "[ui]\npalette = 'nord'\n\n# first\n[[bookmarks]]\nname = 'One'\npath = 'one'\n\n\
         # second\n[[bookmarks]]\nname = 'Two'\npath = 'two' # trailing\n\n[cache]\nenabled = true\n",
    )
    .unwrap();
    let initial = f.load();
    let mut store = Store::new(Some(f.path()), initial.clone(), None);
    let next = store.commit(&initial, &Change::Swap(1, 0)).unwrap();
    assert_eq!(next, vec![initial[1].clone(), initial[0].clone()]);
    assert_eq!(f.load(), next);
    let text = fs::read_to_string(f.path()).unwrap();
    let (two, one) = (
        text.find("# second").unwrap(),
        text.find("# first").unwrap(),
    );
    assert!(two < one, "{text}");
    assert!(text.contains("path = 'two' # trailing") && text.contains("[cache]\nenabled = true"));
    assert!(text.find("[ui]").unwrap() < two && text.find("[cache]").unwrap() > one);
    assert!(store.commit(&next, &Change::Swap(1, 2)).is_err());
    assert_eq!(f.load(), next);

    let f = Fixture::new();
    fs::write(
        f.path(),
        "bookmarks = [\n  { name = 'One', path = 'one' },\n  { name = 'Two', path = 'two' },\n] # list\n",
    )
    .unwrap();
    let initial = f.load();
    let mut store = Store::new(Some(f.path()), initial.clone(), None);
    let next = store.commit(&initial, &Change::Swap(0, 1)).unwrap();
    assert_eq!(f.load(), next);
    assert_eq!(
        fs::read_to_string(f.path()).unwrap(),
        "bookmarks = [\n  { name = 'Two', path = 'two' },\n  { name = 'One', path = 'one' },\n] # list\n"
    );
}

#[test]
fn unrelated_disk_edits_merge_but_changed_bookmarks_require_reload() {
    let f = Fixture::new();
    fs::write(f.path(), "[ui]\npalette = 'nord'\n").unwrap();
    let mut store = Store::new(Some(f.path()), vec![], None);
    fs::write(f.path(), "[ui]\npalette = 'forest' # external edit\n").unwrap();
    let next = store
        .commit(
            &[],
            &Change::Put {
                index: None,
                bookmark: f.bookmark("one"),
            },
        )
        .unwrap();
    assert!(
        fs::read_to_string(f.path())
            .unwrap()
            .contains("palette = 'forest' # external edit")
    );
    let external = "[[bookmarks]]\nname='external'\npath='/external'\n";
    fs::write(f.path(), external).unwrap();
    let error = store.commit(&next, &Change::Remove(0)).unwrap_err();
    assert!(error.contains("Alt+r"));
    assert_eq!(fs::read_to_string(f.path()).unwrap(), external);
    let refreshed = store.reload().unwrap();
    assert_eq!(refreshed[0].name, "external");
    store.commit(&refreshed, &Change::Remove(0)).unwrap();
    assert!(f.load().is_empty());
}

#[test]
fn invalid_config_or_write_failure_leaves_state_and_disk_untouched() {
    let f = Fixture::new();
    fs::write(f.path(), "[ui\n").unwrap();
    let mut store = Store::new(Some(f.path()), vec![], None);
    assert!(
        store
            .commit(
                &[],
                &Change::Put {
                    index: None,
                    bookmark: f.bookmark("one")
                }
            )
            .is_err()
    );
    assert_eq!(fs::read_to_string(f.path()).unwrap(), "[ui\n");
    let blocked = f.0.join("blocked");
    fs::write(&blocked, "keep").unwrap();
    let mut store = Store::new(Some(blocked.join("config.toml")), vec![], None);
    assert!(
        store
            .commit(
                &[],
                &Change::Put {
                    index: None,
                    bookmark: f.bookmark("one")
                }
            )
            .is_err()
    );
    assert_eq!(fs::read_to_string(blocked).unwrap(), "keep");
}

#[cfg(unix)]
#[test]
fn saving_follows_config_symlinks_and_preserves_permissions() {
    use std::os::unix::{fs::PermissionsExt, fs::symlink};
    let f = Fixture::new();
    let actual = f.0.join("dotfiles.toml");
    fs::write(&actual, "# dotfiles\n").unwrap();
    fs::set_permissions(&actual, fs::Permissions::from_mode(0o640)).unwrap();
    symlink(&actual, f.path()).unwrap();
    let mut store = Store::new(Some(f.path()), vec![], None);
    store
        .commit(
            &[],
            &Change::Put {
                index: None,
                bookmark: f.bookmark("one"),
            },
        )
        .unwrap();
    assert!(
        fs::symlink_metadata(f.path())
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::metadata(&actual).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(fs::read_to_string(actual).unwrap().contains("# dotfiles"));
    assert_eq!(f.load()[0].name, "one");
}

#[test]
fn draft_paths_expand_home_and_use_the_browsing_directory() {
    let f = Fixture::new();
    let mut draft = Draft::new(&f.0, None, "find".into(), 2).unwrap();
    draft.path = "relative/notes".into();
    assert_eq!(
        draft.bookmark(&f.0, None).unwrap().path,
        f.0.join("relative/notes")
    );
    draft.path = "~/notes".into();
    assert_eq!(
        draft.bookmark(&f.0, Some(&f.0)).unwrap().path,
        f.0.join("notes")
    );
    assert!(draft.bookmark(&f.0, None).is_err());
}
