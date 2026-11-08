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
    /// The program answering it, for where keys apply.
    pub fn program(&self) -> crate::program::Program {
        use crate::program::Program;
        if self.save.is_some() {
            Program::ExplorerSave
        } else if self.directory && self.multiple {
            Program::ExplorerFolderMultiple
        } else if self.directory {
            Program::ExplorerFolder
        } else if self.multiple {
            Program::ExplorerMultiple
        } else {
            Program::ExplorerOpen
        }
    }
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
    /// where `!` is text. Keys are named as `app` binds them.
    pub fn hint(&self, typing: bool, app: &crate::App) -> String {
        use crate::keymap::Dimension;
        use crate::keymap::{Function, Mode};
        // Up to two keys `function` has in `mode` for this dialog and what
        // has the focus, else any.
        let keys = |function: Function, mode: Mode| {
            let (program, focus) = (app.program(), app.cell_in(mode).focus);
            let bindings = app.keymap.of(function, mode);
            let mut names: Vec<String> = Vec::new();
            let here = bindings.iter().filter(|b| {
                b.reach()
                    .iter()
                    .any(|s| s.programs & program.bit() != 0 && s.focuses & focus.bit() != 0)
            });
            for b in here.chain(bindings.iter()) {
                if !names.contains(&b.display()) && names.len() < 2 {
                    names.push(b.display());
                }
            }
            if names.is_empty() {
                "—".to_string()
            } else {
                names.join(" or ")
            }
        };
        let enter = keys(
            if self.multiple {
                Function::PrintHighlightedAndMarks
            } else {
                Function::PrintHighlighted
            },
            Mode::Normal,
        );
        let here = if self.save.is_some() {
            Function::SaveHere
        } else {
            Function::PrintHere
        };
        match (&self.save, self.directory, self.multiple) {
            (Some(_), _, _) if typing => format!(
                "Type a custom name, then press {} to save it here.",
                keys(here, Mode::Insert),
            ),
            // The query in focus: one key on, the folders.
            (Some(name), _, _) if app.query_in_focus() => format!(
                "Press {} to save {name} here. Press {} to browse folders.",
                keys(here, Mode::Normal),
                keys(Function::ToggleQueryFocus, Mode::Normal)
            ),
            (Some(name), _, _) => format!(
                "Press {} to save {name} here. Press i to type a custom name.",
                keys(here, Mode::Normal),
            ),
            (None, true, _) if typing => format!(
                "Press {} to choose the folder you are in.",
                keys(here, Mode::Insert)
            ),
            (None, true, _) => format!(
                "Press {} to choose the folder you are in.",
                keys(here, Mode::Normal)
            ),
            (None, false, true) => format!(
                "Press {enter} on a file to choose it. Press {} to mark several, then {} to \
                 choose them.",
                app.bound_in(Function::ToggleMark, Mode::Normal)
                    .unwrap_or_else(|| app.bound_or_none(Function::ToggleMark)),
                app.bound_or_none(Function::PrintMarks)
            ),
            (None, false, false) => format!("Press {enter} on a file to choose it."),
        }
    }
    /// Why `l` (or Alt+number) on a file does not open it in a dialog that
    /// chooses files or where to save, and what to do instead. `None` for
    /// folder dialogs, where files open as in a normal session.
    pub fn file_hint(&self, app: &crate::App) -> Option<String> {
        let how = format!(
            "press {} on it",
            app.bound_or_none(crate::keymap::Function::PrintHighlighted)
        );
        match (&self.save, self.directory) {
            (Some(_), _) => {
                Some("Files do not open here. To save over this one, save under its name.".into())
            }
            (None, true) => None,
            (None, false) => Some(format!(
                "Files do not open here. To choose this one, {how}."
            )),
        }
    }
    /// The name a save takes: the query, else the suggestion.
    pub fn save_name<'a>(&'a self, query: &'a str) -> Option<&'a str> {
        let typed = query.trim();
        self.save
            .as_deref()
            .map(|suggested| if typed.is_empty() { suggested } else { typed })
    }
    /// Saving as `name` changes the suggested name's extension (or drops
    /// it): what the status line says before such a save. `None` when the
    /// extension stays, ignoring case, or nothing was suggested.
    pub fn extension_warning(&self, name: &str) -> Option<String> {
        let suggested = self.save.as_deref().filter(|s| *s != UNTITLED)?;
        let (wanted, typed) = (extension(suggested), extension(name));
        if wanted.map(str::to_lowercase) == typed.map(str::to_lowercase) {
            return None;
        }
        let has = match typed {
            Some(typed) => format!("ends in .{typed}"),
            None => "has no extension".into(),
        };
        Some(format!(
            "{name} {has}, unlike the suggested {suggested}: save again to use it anyway"
        ))
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

/// What follows a name's last dot, unless that dot starts it (`.bashrc`),
/// as `free_name` numbers names.
fn extension(name: &str) -> Option<&str> {
    match name.rfind('.') {
        Some(dot) if dot > 0 => Some(&name[dot + 1..]),
        _ => None,
    }
}

/// `folder/name`, or the first of `name (1).ext`, `name (2).ext`, ... that
/// does not exist yet.
pub fn free_name(folder: &Path, name: impl AsRef<std::ffi::OsStr>) -> PathBuf {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    let name = name.as_ref();
    let path = folder.join(name);
    if path.symlink_metadata().is_err() {
        return path;
    }
    // A leading dot is part of the stem (".bashrc"), as is a lone dot.
    let bytes = name.as_bytes();
    let (stem, extension) = match bytes.iter().rposition(|&b| b == b'.') {
        Some(dot) if dot > 0 => bytes.split_at(dot),
        _ => (bytes, &b""[..]),
    };
    (1..)
        .map(|n| {
            let numbered = [stem, format!(" ({n})").as_bytes(), extension].concat();
            folder.join(std::ffi::OsString::from_vec(numbered))
        })
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
        let mut app = crate::App::browse(std::env::temp_dir(), 1, Default::default());
        app.open_dialog(dialog.clone());
        assert!(
            dialog
                .hint(false, &app)
                .starts_with("Press ! or Ctrl+s to save page.html here.")
        );
        assert!(dialog.hint(true, &app).starts_with("Type a custom name"));
        // Keys are named as configured.
        app.keymap = crate::keymap::Keymap::parse("[normal]\nsave_here = '<C-t>'\n").unwrap();
        assert!(
            dialog
                .hint(false, &app)
                .starts_with("Press Ctrl+t to save page.html here."),
            "{}",
            dialog.hint(false, &app)
        );
        for bad in ["a/b", ".", "..", "a\nb"] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
        assert!(check_name(".hidden").is_ok());
    }

    #[test]
    fn a_name_changing_the_suggested_extension_is_warned_about() {
        let save = |name: &str| Dialog {
            save: Some(name.into()),
            ..Dialog::default()
        };
        let pdf = save("a.pdf");
        assert_eq!(pdf.extension_warning("b.pdf"), None);
        assert_eq!(pdf.extension_warning("b.PDF"), None);
        assert_eq!(
            pdf.extension_warning("b.txt").unwrap(),
            "b.txt ends in .txt, unlike the suggested a.pdf: save again to use it anyway"
        );
        assert!(
            pdf.extension_warning("b")
                .unwrap()
                .starts_with("b has no extension")
        );
        assert!(pdf.extension_warning(".pdf").is_some());
        // A suggestion without one warns when a name adds one.
        assert!(save("Makefile").extension_warning("Makefile.bak").is_some());
        assert_eq!(save(".bashrc").extension_warning(".profile"), None);
        // Nothing suggested: any name.
        assert_eq!(save(UNTITLED).extension_warning("notes.txt"), None);
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
