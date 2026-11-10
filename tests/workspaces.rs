use fuzzy_file_finder::{
    config::Setting,
    config::{Settings, Workspace},
    workspaces::{Change, Draft, Store},
};
use std::{fs, path::PathBuf};
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "fff-workspaces-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    fn path(&self) -> PathBuf {
        self.0.join("config.toml")
    }
    fn load(&self) -> Vec<Workspace> {
        Settings::load(Some(&self.path()), false, None)
            .unwrap()
            .workspaces
    }
    fn workspace(&self, name: &str) -> Workspace {
        Workspace {
            name: name.into(),
            path: Some(self.0.join(name)),
            icon: Some("\u{f015}".into()),
            ..Default::default()
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
    fs::write(f.path(), "# user config\n[ui]\npalette = 'nord' # keep colors\n\n[[workspaces]] # favourite\nname = 'Old' # name note\npath = 'notes' # relative path\n\n[cache]\nmax_entries = 8 # memory\n").unwrap();
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
                workspace: renamed.clone(),
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
    let second = f.workspace("two");
    let next = store
        .commit(
            &next,
            &Change::Put {
                index: None,
                workspace: second.clone(),
            },
        )
        .unwrap();
    assert_eq!(f.load(), next);
    fs::create_dir(second.path.as_ref().unwrap()).unwrap();
    fs::write(second.path.as_ref().unwrap().join("file"), "keep").unwrap();
    let next = store.commit(&next, &Change::Remove(1)).unwrap();
    assert_eq!(f.load(), next);
    assert!(second.path.as_ref().unwrap().join("file").exists());
    let next = store.commit(&next, &Change::Remove(0)).unwrap();
    assert!(next.is_empty() && f.load().is_empty());
    assert!(fs::read_to_string(f.path()).unwrap().contains("# memory"));
}

#[test]
fn session_only_edits_validate_names_and_do_not_write_a_config() {
    let f = Fixture::new();
    let mut store = Store::default();
    let first = f.workspace("one");
    let next = store
        .commit(
            &[],
            &Change::Put {
                index: None,
                workspace: first.clone(),
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
                    workspace: first
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
                        workspace: f.workspace(name)
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
    let workspace = f.workspace("one");
    store
        .commit(
            &[],
            &Change::Put {
                index: None,
                workspace: workspace.clone(),
            },
        )
        .unwrap();
    assert_eq!(
        Settings::load(Some(&path), false, None).unwrap().workspaces,
        vec![workspace]
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
fn inline_workspaces_and_icon_removal_preserve_other_configuration() {
    let f = Fixture::new();
    fs::write(f.path(), "workspaces = [{ name = 'One', path = 'one', icon = '★' }] # collection\n[ui]\nicons = false # plain\n").unwrap();
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
                workspace: edited,
            },
        )
        .unwrap();
    let next = store
        .commit(
            &next,
            &Change::Put {
                index: None,
                workspace: f.workspace("Two"),
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
        "[ui]\npalette = 'nord'\n\n# first\n[[workspaces]]\nname = 'One'\npath = 'one'\n\n\
         # second\n[[workspaces]]\nname = 'Two'\npath = 'two' # trailing\n\n[cache]\nenabled = true\n",
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
        "workspaces = [\n  { name = 'One', path = 'one' },\n  { name = 'Two', path = 'two' },\n] # list\n",
    )
    .unwrap();
    let initial = f.load();
    let mut store = Store::new(Some(f.path()), initial.clone(), None);
    let next = store.commit(&initial, &Change::Swap(0, 1)).unwrap();
    assert_eq!(f.load(), next);
    assert_eq!(
        fs::read_to_string(f.path()).unwrap(),
        "workspaces = [\n  { name = 'Two', path = 'two' },\n  { name = 'One', path = 'one' },\n] # list\n"
    );
}

#[test]
fn unrelated_disk_edits_merge_but_changed_workspaces_require_reload() {
    let f = Fixture::new();
    fs::write(f.path(), "[ui]\npalette = 'nord'\n").unwrap();
    let mut store = Store::new(Some(f.path()), vec![], None);
    fs::write(f.path(), "[ui]\npalette = 'forest' # external edit\n").unwrap();
    let next = store
        .commit(
            &[],
            &Change::Put {
                index: None,
                workspace: f.workspace("one"),
            },
        )
        .unwrap();
    assert!(
        fs::read_to_string(f.path())
            .unwrap()
            .contains("palette = 'forest' # external edit")
    );
    let external = "[[workspaces]]\nname='external'\npath='/external'\n";
    fs::write(f.path(), external).unwrap();
    let error = store.commit(&next, &Change::Remove(0)).unwrap_err();
    assert_eq!(error, fuzzy_file_finder::workspaces::CHANGED);
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
                    workspace: f.workspace("one")
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
                    workspace: f.workspace("one")
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
                workspace: f.workspace("one"),
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
    let mut draft = Draft::new(&f.0, None, Workspace::default(), "find".into(), 2).unwrap();
    draft.path = "relative/notes".into();
    assert_eq!(
        draft.workspace(&f.0, None).unwrap().path,
        Some(f.0.join("relative/notes"))
    );
    draft.path = "~/notes".into();
    assert_eq!(
        draft.workspace(&f.0, Some(&f.0)).unwrap().path,
        Some(f.0.join("notes"))
    );
    assert!(draft.workspace(&f.0, None).is_err());
}
#[test]
fn drafts_refuse_line_breaks_and_control_characters() {
    let f = Fixture::new();
    let mut draft = Draft::new(&f.0, None, Workspace::default(), String::new(), 0).unwrap();
    draft.name = "Notes".into();
    assert!(draft.workspace(&f.0, None).is_ok());
    // Linewise Neovim edits (dd then P) leave a trailing line break.
    draft.name = "Notes\n".into();
    assert!(
        draft
            .workspace(&f.0, None)
            .unwrap_err()
            .contains("line breaks")
    );
    draft.name = "Notes".into();
    draft.path = format!("{}\n", f.0.display());
    assert!(draft.workspace(&f.0, None).is_err());
    draft.path = format!("{}/a\tb", f.0.display());
    assert!(draft.workspace(&f.0, None).is_err());
}
#[test]
fn drafts_keep_the_ticked_settings_and_an_empty_path_keeps_the_folder() {
    let f = Fixture::new();
    let now = Workspace {
        path: Some(f.0.clone()),
        roots: Some(vec![f.0.join("papers")]),
        query: Some("thesis".into()),
        contents: Some(String::new()),
        depth: Some(4),
        filters: Some(vec!["*.pdf".into()]),
        hidden: Some(true),
        kinds: Some(fuzzy_file_finder::Kinds::Files),
        preview: Some(false),
        exclusions_on: Some(true),
        filters_on: Some(true),
        ..Workspace::default()
    };
    let mut draft = Draft::new(&f.0, None, now.clone(), String::new(), 0).unwrap();
    // A new workspace keeps the folder and nothing else.
    let folder = draft.workspace(&f.0, None).unwrap();
    assert_eq!(folder.path, Some(f.0.clone()));
    assert!(!Setting::ALL.iter().any(|s| s.is_in(&folder)));
    // A setting the session has no value for cannot be ticked.
    assert!(!draft.toggle(Setting::Program));
    assert!(draft.toggle(Setting::Depth) && draft.toggle(Setting::Toggles));
    // The queries tick separately: an empty contents query alone is kept.
    assert!(draft.toggle(Setting::Contents));
    draft.path.clear();
    let kept = draft.workspace(&f.0, None).unwrap();
    assert_eq!((kept.path, kept.depth), (None, Some(4)));
    assert_eq!(kept.query, None);
    assert_eq!(kept.contents.as_deref(), Some(""));
    assert_eq!((kept.hidden, kept.preview), (Some(true), Some(false)));
    assert_eq!((kept.roots, kept.filters), (None, None));
    // Unticking drops it again; with nothing left there is nothing to save.
    for setting in [Setting::Depth, Setting::Toggles, Setting::Contents] {
        assert!(draft.toggle(setting));
    }
    assert!(draft.workspace(&f.0, None).is_err());
    // Editing offers the workspace's own values first, the session's for
    // the rest.
    let saved = Workspace {
        name: "notes".into(),
        depth: Some(9),
        ..Workspace::default()
    };
    let mut draft = Draft::new(&f.0, Some((0, &saved)), now, String::new(), 0).unwrap();
    assert!(draft.path.is_empty() && Setting::Depth.is_in(&draft.kept));
    assert!(draft.toggle(Setting::Depth) && draft.toggle(Setting::Depth));
    assert!(draft.toggle(Setting::Filters));
    let edited = draft.workspace(&f.0, None).unwrap();
    assert_eq!(edited.depth, Some(9));
    assert_eq!(edited.filters, Some(vec!["*.pdf".to_string()]));
}

#[test]
fn every_setting_is_saved_read_back_and_removed_with_its_keys() {
    let f = Fixture::new();
    fs::write(
        f.path(),
        "# notes\n[[workspaces]]\nname = 'plain' # just a folder\npath = 'p'\n",
    )
    .unwrap();
    let initial = f.load();
    let mut store = Store::new(Some(f.path()), initial.clone(), None);
    let full = Workspace {
        name: "findnotes".into(),
        icon: None,
        path: Some(f.0.join("phd")),
        program: Some(fuzzy_file_finder::program::Program::Pick),
        roots: Some(vec![f.0.join("phd"), f.0.join("books")]),
        query: Some("lemma".into()),
        contents: Some("compact".into()),
        depth: Some(10),
        filters: Some(vec!["*.pdf".into(), "*.djvu".into()]),
        exclusions: Some(vec!["target".into()]),
        sort: Some(Some(fuzzy_file_finder::rows::Sort {
            by: fuzzy_file_finder::rows::Criterion::Modified,
            descending: false,
        })),
        hidden: Some(false),
        kinds: Some(fuzzy_file_finder::Kinds::Files),
        preview: Some(true),
        exclusions_on: Some(true),
        filters_on: Some(false),
    };
    let next = store
        .commit(
            &initial,
            &Change::Put {
                index: None,
                workspace: full.clone(),
            },
        )
        .unwrap();
    assert_eq!(f.load(), next);
    assert_eq!(next[1], full);
    let text = fs::read_to_string(f.path()).unwrap();
    for key in [
        "program = \"pick\"",
        "depth = 10",
        "sort = \"modified\"",
        "sort_descending = false",
        "kinds = \"files\"",
    ] {
        assert!(text.contains(key), "{key} missing from {text}");
    }
    // Keeping fewer settings removes their keys and leaves the rest alone.
    let fewer = Workspace {
        name: "findnotes".into(),
        path: None,
        depth: Some(10),
        ..Workspace::default()
    };
    let next = store
        .commit(
            &next,
            &Change::Put {
                index: Some(1),
                workspace: fewer.clone(),
            },
        )
        .unwrap();
    assert_eq!(f.load(), next);
    let text = fs::read_to_string(f.path()).unwrap();
    assert!(text.contains("# just a folder") && text.contains("depth = 10"));
    for key in ["program", "roots", "sort", "kinds", "path = \"/"] {
        assert!(!text.contains(key), "{key} left in {text}");
    }
}
