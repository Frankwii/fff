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
    /// How to choose, shown in the empty query line.
    pub fn hint(&self) -> String {
        match (&self.save, self.directory, self.multiple) {
            (Some(name), _, _) => {
                format!("! saves {name} here · Enter on a file replaces it · q cancels")
            }
            (None, true, _) => "! chooses the folder you are in · q cancels".into(),
            (None, false, true) => {
                "Enter chooses a file · Tab marks several · Ctrl+w the marks · q cancels".into()
            }
            (None, false, false) => "Enter chooses a file · q cancels".into(),
        }
    }
    /// The answer from the paths the session returned: files for file
    /// dialogs, folders for folder dialogs, the last one unless several are
    /// allowed. Saving into a folder takes the suggested name, numbered when
    /// that is taken; a chosen file is replaced.
    pub fn answer(&self, chosen: Vec<PathBuf>) -> Vec<PathBuf> {
        if let Some(name) = &self.save {
            return match chosen.last() {
                Some(folder) if folder.is_dir() => vec![free_name(folder, name)],
                Some(file) => vec![file.clone()],
                None => vec![],
            };
        }
        let mut chosen: Vec<PathBuf> = chosen
            .into_iter()
            .filter(|p| p.is_dir() == self.directory)
            // The portal reads one path per line.
            .filter(|p| !p.as_os_str().as_encoded_bytes().contains(&b'\n'))
            .collect();
        if !self.multiple && chosen.len() > 1 {
            chosen.drain(..chosen.len() - 1);
        }
        chosen
    }
}

/// `folder/name`, or the first of `name (1).ext`, `name (2).ext`, ... that
/// does not exist yet.
fn free_name(folder: &Path, name: &str) -> PathBuf {
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
