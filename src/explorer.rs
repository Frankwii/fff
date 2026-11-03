//! `fff explorer`: answering a file dialog (the termfilechooser portal
//! wrapper runs it). The session browses as usual; this decides where it
//! starts, what the query line suggests, and which of the paths a key returned
//! answer the dialog.
use std::path::{Path, PathBuf};

/// What a file dialog asks for.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dialog {
    /// Several files (or folders) may be chosen.
    pub multiple: bool,
    /// Folders are chosen instead of files.
    pub directory: bool,
    /// Saving: the file name to save under in the chosen folder.
    pub save: Option<String>,
}

/// Name for a save dialog that suggests none.
const UNTITLED: &str = "untitled";

impl Dialog {
    /// The dialog, where browsing starts and the entry to highlight there,
    /// from the suggested `path` (a folder, a file, or with `save` the
    /// suggested file, which need not exist).
    pub fn new(
        path: Option<&Path>,
        multiple: bool,
        directory: bool,
        save: bool,
        cwd: &Path,
    ) -> (Self, PathBuf, Option<PathBuf>) {
        let mut dialog = Self {
            multiple,
            directory,
            save: None,
        };
        let path = path.map(|p| cwd.join(p));
        let (start, highlight) = match &path {
            Some(p) if p.is_dir() => (p.clone(), None),
            Some(p) => match p.parent().filter(|d| d.is_dir()) {
                Some(folder) => (folder.to_owned(), p.exists().then(|| p.clone())),
                None => (cwd.to_owned(), None),
            },
            None => (cwd.to_owned(), None),
        };
        if save {
            let name = path
                .filter(|p| !p.is_dir())
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()));
            dialog.save = Some(name.unwrap_or_else(|| UNTITLED.into()));
        }
        (dialog, start, highlight)
    }
    /// How to choose, shown in the empty query line; `typing` in Insert mode,
    /// where `!` is text.
    pub fn hint(&self, typing: bool) -> String {
        match (&self.save, self.directory, self.multiple) {
            (Some(_), _, _) if typing => "Type a custom name, then press Enter or Ctrl+s to save \
                 it here. Press Esc to browse folders."
                .into(),
            (Some(name), _, _) => format!(
                "Press ! or Ctrl+s to save {name} here. Press i to type a custom name. \
                 Press Enter on a file to replace it."
            ),
            (None, true, _) if typing => "Press Ctrl+s to choose the folder you are in.".into(),
            (None, true, _) => "Press ! or Ctrl+s to choose the folder you are in.".into(),
            (None, false, true) => "Press Enter on a file to choose it. Press Space to mark \
                 several, then Ctrl+w to choose them."
                .into(),
            (None, false, false) => "Press Enter on a file to choose it.".into(),
        }
    }
    /// Why `l` (or Alt+number) on a file does not open it in a dialog that
    /// chooses files or where to save, and what to do instead. `None` for
    /// folder dialogs, where files open as in a normal session.
    pub fn file_hint(&self) -> Option<&'static str> {
        match (&self.save, self.directory) {
            (Some(_), _) => Some(
                "Files do not open here. To save over this one, press Enter on it \
                 or Ctrl+number on its row.",
            ),
            (None, true) => None,
            (None, false) => Some(
                "Files do not open here. To choose this one, press Enter on it or \
                 Ctrl+number on its row.",
            ),
        }
    }
    /// The name a save takes: the query, else the suggestion.
    pub fn save_name<'a>(&'a self, query: &'a str) -> Option<&'a str> {
        let typed = query.trim();
        self.save
            .as_deref()
            .map(|suggested| if typed.is_empty() { suggested } else { typed })
    }
    /// The query box title: what Ctrl+s saves as, or `Query`.
    pub fn title(&self, query: &str) -> String {
        match self.save_name(query) {
            Some(name) => format!("Save as {name}"),
            None => "Query".into(),
        }
    }
    /// The answer from the paths the session returned: files for file
    /// dialogs, folders for folder dialogs, the last one unless several are
    /// allowed. Saving into a folder takes the suggested name, numbered when
    /// that is taken; a chosen file is replaced.
    pub fn answer(&self, chosen: Vec<PathBuf>) -> Vec<PathBuf> {
        // The portal reads one path per line.
        let one_line = |p: &PathBuf| !p.as_os_str().as_encoded_bytes().contains(&b'\n');
        if let Some(name) = &self.save {
            return match chosen.last() {
                Some(folder) if folder.is_dir() => vec![free_name(folder, name)],
                Some(file) => vec![file.clone()],
                None => vec![],
            }
            .into_iter()
            .filter(one_line)
            .collect();
        }
        let mut chosen: Vec<PathBuf> = chosen
            .into_iter()
            .filter(|p| p.is_dir() == self.directory)
            .filter(one_line)
            .collect();
        if !self.multiple && chosen.len() > 1 {
            chosen.drain(..chosen.len() - 1);
        }
        chosen
    }
}

/// Whether `name` can be a file name in a folder.
pub fn check_name(name: &str) -> Result<(), String> {
    match name {
        "." | ".." => Err(format!("{name} is not a file name")),
        _ if name.contains('/') => Err("A file name cannot contain /".into()),
        _ if name.contains(['\n', '\0']) => Err("A file name cannot span lines".into()),
        _ => Ok(()),
    }
}

/// `folder/name`, or the first of `name (1).ext`, `name (2).ext`, ... that
/// does not exist yet.
pub fn free_name(folder: &Path, name: &str) -> PathBuf {
    let path = folder.join(name);
    if path.symlink_metadata().is_err() {
        return path;
    }
    // A leading dot is part of the stem (".bashrc"), as is a lone dot.
    let (stem, extension) = match name.rfind('.') {
        Some(dot) if dot > 0 => name.split_at(dot),
        _ => (name, ""),
    };
    (1..)
        .map(|n| folder.join(format!("{stem} ({n}){extension}")))
        .find(|p| p.symlink_metadata().is_err())
        .expect("some number is free")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fff-explorer-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("folder")).unwrap();
        fs::write(dir.join("a.pdf"), "").unwrap();
        fs::write(dir.join("b"), "").unwrap();
        dir
    }

    #[test]
    fn dialogs_start_beside_the_suggestion() {
        let dir = scratch("start");
        let new = |path: Option<&Path>, save| {
            let (dialog, start, highlight) = Dialog::new(path, false, false, save, &dir);
            (dialog.save, start, highlight)
        };
        assert_eq!(new(None, false), (None, dir.clone(), None));
        assert_eq!(new(Some(&dir.join("folder")), false).1, dir.join("folder"));
        // A file is highlighted in its folder; relative paths start at cwd.
        assert_eq!(
            new(Some(Path::new("a.pdf")), false),
            (None, dir.clone(), Some(dir.join("a.pdf")))
        );
        // Saving: the suggested name, which need not exist yet.
        assert_eq!(
            new(Some(&dir.join("folder/page.html")), true),
            (Some("page.html".into()), dir.join("folder"), None)
        );
        assert_eq!(new(Some(&dir.join("nowhere/x")), true).1, dir);
        assert_eq!(new(Some(&dir), true).0, Some(UNTITLED.into()));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn saves_take_the_typed_name_or_the_suggestion() {
        let dialog = Dialog {
            save: Some("page.html".into()),
            ..Dialog::default()
        };
        assert_eq!(dialog.save_name(""), Some("page.html"));
        assert_eq!(dialog.save_name("  notes.html "), Some("notes.html"));
        assert_eq!(dialog.title("x"), "Save as x");
        assert_eq!(Dialog::default().save_name("x"), None);
        assert_eq!(Dialog::default().title("x"), "Query");
        assert!(
            dialog
                .hint(false)
                .starts_with("Press ! or Ctrl+s to save page.html here.")
        );
        assert!(dialog.hint(true).starts_with("Type a custom name"));
        for bad in ["a/b", ".", "..", "a\nb"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert!(check_name(".hidden").is_ok());
    }

    #[test]
    fn answers_keep_what_the_dialog_asked_for() {
        let dir = scratch("answer");
        let (folder, a, b) = (dir.join("folder"), dir.join("a.pdf"), dir.join("b"));
        let all = vec![folder.clone(), a.clone(), b.clone()];
        let dialog = |multiple, directory| Dialog {
            multiple,
            directory,
            save: None,
        };
        // Enter on a file returns the marks, then that file: it wins.
        assert_eq!(
            dialog(false, false).answer(all.clone()),
            std::slice::from_ref(&b)
        );
        assert_eq!(
            dialog(true, false).answer(all.clone()),
            [a.clone(), b.clone()]
        );
        assert_eq!(
            dialog(false, true).answer(all.clone()),
            std::slice::from_ref(&folder)
        );
        assert!(dialog(false, false).answer(vec![folder.clone()]).is_empty());
        assert!(dialog(false, false).answer(vec![]).is_empty());
        let save = |name: &str| Dialog {
            save: Some(name.into()),
            ..Dialog::default()
        };
        assert_eq!(
            save("new.pdf").answer(vec![dir.clone()]),
            [dir.join("new.pdf")]
        );
        // A taken name is numbered rather than replaced...
        assert_eq!(
            save("a.pdf").answer(vec![dir.clone()]),
            [dir.join("a (1).pdf")]
        );
        assert_eq!(save("b").answer(vec![dir.clone()]), [dir.join("b (1)")]);
        fs::write(dir.join("a (1).pdf"), "").unwrap();
        assert_eq!(
            save("a.pdf").answer(vec![dir.clone()]),
            [dir.join("a (2).pdf")]
        );
        // ...unless that file itself was chosen.
        assert_eq!(save("x.pdf").answer(vec![a.clone()]), [a]);
        fs::remove_dir_all(dir).unwrap();
    }
}
