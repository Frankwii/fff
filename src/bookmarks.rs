//! Live bookmark edits and lossless, atomic updates to the active TOML file.
use crate::{
    config::{Bookmark, bookmarks_from_text},
    editor::Field,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditField {
    Name,
    Path,
    Icon,
}
impl EditField {
    pub fn next(self, reverse: bool) -> Self {
        match (self, reverse) {
            (Self::Name, false) | (Self::Icon, true) => Self::Path,
            (Self::Path, false) | (Self::Name, true) => Self::Icon,
            _ => Self::Name,
        }
    }
    pub fn editor(self) -> Option<Field> {
        match self {
            Self::Name => Some(Field::BookmarkName),
            Self::Path => Some(Field::BookmarkPath),
            Self::Icon => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Draft {
    pub editing: Option<usize>,
    pub name: String,
    pub path: String,
    pub icon: Option<String>,
    pub field: EditField,
    pub query: String,
    pub index: usize,
}
impl Draft {
    pub fn new(
        cwd: &Path,
        existing: Option<(usize, &Bookmark)>,
        query: String,
        index: usize,
    ) -> Result<Self, String> {
        let (editing, name, path, icon) = match existing {
            Some((i, b)) => (Some(i), b.name.clone(), b.path.as_path(), b.icon.clone()),
            None => (
                None,
                cwd.file_name()
                    .unwrap_or(cwd.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                cwd,
                None,
            ),
        };
        Ok(Self {
            editing,
            name,
            path: path
                .to_str()
                .ok_or("Bookmark paths must be valid UTF-8 to edit")?
                .into(),
            icon,
            field: EditField::Name,
            query,
            index,
        })
    }
    pub fn bookmark(&self, cwd: &Path, home: Option<&Path>) -> Result<Bookmark, String> {
        if self.name.trim().is_empty() {
            return Err("Bookmark name must not be empty".into());
        }
        if self.path.is_empty() {
            return Err("Bookmark path must not be empty".into());
        }
        // Neovim's linewise edits (dd then P) can add line breaks to a field.
        if self
            .name
            .chars()
            .chain(self.path.chars())
            .any(char::is_control)
        {
            return Err(
                "Bookmark names and paths cannot contain line breaks or control characters".into(),
            );
        }
        Ok(Bookmark {
            name: self.name.clone(),
            path: crate::config::expand_path(&self.path, cwd, home)?,
            icon: self.icon.clone(),
        })
    }
}
#[derive(Clone, Debug)]
pub enum Change {
    Put {
        index: Option<usize>,
        bookmark: Bookmark,
    },
    Remove(usize),
    /// Exchange two entries, e.g. adjacent ones to reorder the list.
    Swap(usize, usize),
}
impl Change {
    pub fn apply(&self, current: &[Bookmark]) -> Result<Vec<Bookmark>, String> {
        let mut next = current.to_vec();
        match self {
            Self::Put {
                index: Some(index),
                bookmark,
            } => *next.get_mut(*index).ok_or("Bookmark no longer exists")? = bookmark.clone(),
            Self::Put {
                index: None,
                bookmark,
            } => next.push(bookmark.clone()),
            Self::Remove(index) => {
                if *index >= next.len() {
                    return Err("Bookmark no longer exists".into());
                }
                next.remove(*index);
            }
            Self::Swap(a, b) => {
                if (*a).max(*b) >= next.len() {
                    return Err("Bookmark no longer exists".into());
                }
                next.swap(*a, *b);
            }
        }
        let mut names = std::collections::HashSet::new();
        for b in &next {
            if b.name.trim().is_empty() || !names.insert(&b.name) {
                return Err("Bookmark names must be nonempty and unique".into());
            }
            if b.path.as_os_str().is_empty() {
                return Err("Bookmark path must not be empty".into());
            }
            if let Some(icon) = &b.icon
                && !(1..=2).contains(&unicode_width::UnicodeWidthStr::width(icon.as_str()))
            {
                return Err("Bookmark icon must be one or two terminal columns wide".into());
            }
        }
        Ok(next)
    }
}
#[derive(Default)]
pub struct Store {
    pub path: Option<PathBuf>,
    expected: Vec<Bookmark>,
    home: Option<PathBuf>,
}
impl Store {
    pub fn new(path: Option<PathBuf>, expected: Vec<Bookmark>, home: Option<PathBuf>) -> Self {
        Self {
            path,
            expected,
            home,
        }
    }
    /// Nothing changes in memory when validation or persistence fails.
    pub fn commit(
        &mut self,
        current: &[Bookmark],
        change: &Change,
    ) -> Result<Vec<Bookmark>, String> {
        let next = change.apply(current)?;
        let Some(path) = &self.path else {
            return Ok(next);
        };
        let before = read(path)?;
        let text = before.as_deref().unwrap_or("");
        let on_disk = bookmarks_from_text(
            text,
            path.parent().unwrap_or(Path::new(".")),
            self.home.as_deref(),
        )?;
        if on_disk != self.expected || current != self.expected {
            return Err(
                "Bookmarks changed on disk; cancel this edit and use Alt+r to reload".into(),
            );
        }
        let mut document: DocumentMut = text
            .parse()
            .map_err(|e| format!("Cannot edit configuration: {e}"))?;
        update_document(&mut document, change, current)?;
        let updated = document.to_string();
        // Verify the exact output before replacing the user's configuration.
        let decoded = bookmarks_from_text(
            &updated,
            path.parent().unwrap_or(Path::new(".")),
            self.home.as_deref(),
        )?;
        if decoded != next {
            return Err("Bookmark serialization did not preserve the requested values".into());
        }
        atomic_write(path, before.as_deref(), &updated)?;
        self.expected = next.clone();
        Ok(next)
    }
    pub fn reload(&mut self) -> Result<Vec<Bookmark>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("Session-only bookmarks have no configuration to reload")?;
        let text = read(path)?.unwrap_or_default();
        let bookmarks = bookmarks_from_text(
            &text,
            path.parent().unwrap_or(Path::new(".")),
            self.home.as_deref(),
        )?;
        self.expected = bookmarks.clone();
        Ok(bookmarks)
    }
}
fn read(path: &Path) -> Result<Option<String>, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}
fn update_document(
    document: &mut DocumentMut,
    change: &Change,
    current: &[Bookmark],
) -> Result<(), String> {
    if !document.contains_key("bookmarks") {
        document["bookmarks"] = Item::ArrayOfTables(Default::default());
    }
    let item = &mut document["bookmarks"];
    if let Some(tables) = item.as_array_of_tables_mut() {
        match change {
            Change::Put {
                index: None,
                bookmark,
            } => {
                let mut t = Table::new();
                update_table(&mut t, None, bookmark)?;
                tables.push(t);
            }
            Change::Put {
                index: Some(i),
                bookmark,
            } => update_table(
                tables.get_mut(*i).ok_or("Bookmark table missing")?,
                current.get(*i),
                bookmark,
            )?,
            Change::Remove(i) => {
                tables.remove(*i);
            }
            Change::Swap(a, b) => {
                // Entries carry their comments along; the document keeps its
                // table positions so output order follows the new list order.
                let (mut first, mut second) = (
                    tables.get(*a).ok_or("Bookmark table missing")?.clone(),
                    tables.get(*b).ok_or("Bookmark table missing")?.clone(),
                );
                let positions = (first.position(), second.position());
                first.set_position(positions.1);
                second.set_position(positions.0);
                tables.replace(*a, second);
                tables.replace(*b, first);
            }
        }
    } else if let Some(array) = item.as_array_mut() {
        match change {
            Change::Put {
                index: None,
                bookmark,
            } => {
                let mut t = toml_edit::InlineTable::new();
                update_table(&mut t, None, bookmark)?;
                array.push(t);
            }
            Change::Put {
                index: Some(i),
                bookmark,
            } => {
                let t = array
                    .get_mut(*i)
                    .and_then(Value::as_inline_table_mut)
                    .ok_or("Bookmark table missing")?;
                update_table(t, current.get(*i), bookmark)?;
            }
            Change::Remove(i) => {
                array.remove(*i);
            }
            Change::Swap(a, b) => {
                // Values trade places; the surrounding whitespace stays put.
                let (mut first, mut second) = (
                    array.get(*a).ok_or("Bookmark table missing")?.clone(),
                    array.get(*b).ok_or("Bookmark table missing")?.clone(),
                );
                std::mem::swap(first.decor_mut(), second.decor_mut());
                array.replace_formatted(*a, second);
                array.replace_formatted(*b, first);
            }
        }
    } else {
        return Err("Unsupported bookmarks configuration".into());
    }
    Ok(())
}
fn update_table(
    table: &mut dyn TableLike,
    old: Option<&Bookmark>,
    new: &Bookmark,
) -> Result<(), String> {
    if old.is_none_or(|b| b.name != new.name) {
        set(table, "name", &new.name);
    }
    if old.is_none_or(|b| b.path != new.path) {
        set(
            table,
            "path",
            new.path
                .to_str()
                .ok_or("Bookmark paths must be valid UTF-8 to save")?,
        );
    }
    if old.is_none_or(|b| b.icon != new.icon) {
        if let Some(icon) = &new.icon {
            set(table, "icon", icon);
        } else {
            table.remove("icon");
        }
    }
    Ok(())
}
fn set(table: &mut dyn TableLike, key: &str, text: &str) {
    if let Some(value) = table.get_mut(key).and_then(Item::as_value_mut) {
        let decor = value.decor().clone();
        *value = Value::from(text);
        *value.decor_mut() = decor;
    } else {
        table.insert(key, toml_edit::value(text));
    }
}
fn atomic_write(path: &Path, before: Option<&str>, text: &str) -> Result<(), String> {
    // Follow an existing config symlink, preserving dotfile-manager links.
    let target = match fs::symlink_metadata(path) {
        Ok(_) => path.canonicalize().map_err(|e| e.to_string())?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => return Err(error.to_string()),
    };
    let parent = target
        .parent()
        .ok_or("Configuration has no parent directory")?;
    fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = parent.join(format!(
        ".fff-bookmarks-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let result = (|| {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary).map_err(|e| e.to_string())?;
        if let Ok(metadata) = fs::metadata(&target) {
            file.set_permissions(metadata.permissions())
                .map_err(|e| e.to_string())?;
        }
        file.write_all(text.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        if read(path)?.as_deref() != before {
            return Err("Configuration changed while saving; try again".into());
        }
        fs::rename(&temporary, &target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(|e| format!("Cannot save bookmarks to {}: {e}", path.display()))
}
