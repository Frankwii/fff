//! Live workspace edits and lossless, atomic updates to the active TOML file.
use crate::{
    config::{Setting, Workspace, workspaces_from_text},
    editor::Field,
};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
use toml_edit::{DocumentMut, Item, Table, TableLike, Value};

/// A write refused because the file's workspaces changed meanwhile; the
/// session adds how to reload them.
pub const CHANGED: &str = "Workspaces changed on disk; cancel this edit and reload them";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditField {
    Name,
    Path,
    Icon,
    /// A setting's tick.
    Setting(Setting),
}
impl EditField {
    /// The form's rows in order: the text fields, the icon, the settings.
    fn all() -> impl Iterator<Item = Self> {
        [Self::Name, Self::Path, Self::Icon]
            .into_iter()
            .chain(Setting::ALL.map(Self::Setting))
    }
    pub fn next(self, reverse: bool) -> Self {
        let all: Vec<_> = Self::all().collect();
        let i = all.iter().position(|&f| f == self).unwrap_or(0);
        let n = all.len();
        all[if reverse {
            (i + n - 1) % n
        } else {
            (i + 1) % n
        }]
    }
    pub fn editor(self) -> Option<Field> {
        match self {
            Self::Name => Some(Field::WorkspaceName),
            Self::Path => Some(Field::WorkspacePath),
            Self::Icon | Self::Setting(_) => None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct Draft {
    pub editing: Option<usize>,
    pub name: String,
    /// The folder; empty leaves the folder as it is.
    pub path: String,
    pub icon: Option<String>,
    /// The settings kept, as ticked (its name, icon and path unused).
    pub kept: Workspace,
    /// What ticking a setting keeps: the workspace's own value, else the
    /// session's. One it lacks cannot be ticked.
    pub offered: Workspace,
    pub field: EditField,
    pub query: String,
    pub index: usize,
}
impl Draft {
    /// A form for a new workspace (the current directory and no setting
    /// ticked) or for `existing`, offering the session's settings in `now`.
    pub fn new(
        cwd: &Path,
        existing: Option<(usize, &Workspace)>,
        now: Workspace,
        query: String,
        index: usize,
    ) -> Result<Self, String> {
        let utf8 = |path: &Path| -> Result<String, String> {
            Ok(path
                .to_str()
                .ok_or("Workspace paths must be valid UTF-8 to edit")?
                .into())
        };
        let mut offered = now;
        let (editing, name, path, icon, kept) = match existing {
            Some((i, w)) => {
                for setting in Setting::ALL {
                    if setting.is_in(w) {
                        setting.copy(w, &mut offered);
                    }
                }
                let path = w.path.as_deref().map(utf8).transpose()?;
                (
                    Some(i),
                    w.name.clone(),
                    path.unwrap_or_default(),
                    w.icon.clone(),
                    w.clone(),
                )
            }
            None => (
                None,
                cwd.file_name()
                    .unwrap_or(cwd.as_os_str())
                    .to_string_lossy()
                    .into_owned(),
                utf8(cwd)?,
                None,
                Workspace::default(),
            ),
        };
        Ok(Self {
            editing,
            name,
            path,
            icon,
            kept,
            offered,
            field: EditField::Name,
            query,
            index,
        })
    }
    /// `setting` can be ticked: there is a value to keep.
    pub fn offers(&self, setting: Setting) -> bool {
        setting.is_in(&self.offered)
    }
    /// Ticks or unticks `setting`; false when there is nothing to tick.
    pub fn toggle(&mut self, setting: Setting) -> bool {
        if setting.is_in(&self.kept) {
            setting.clear(&mut self.kept);
        } else if self.offers(setting) {
            setting.copy(&self.offered, &mut self.kept);
        } else {
            return false;
        }
        true
    }
    pub fn workspace(&self, cwd: &Path, home: Option<&Path>) -> Result<Workspace, String> {
        if self.name.trim().is_empty() {
            return Err("Workspace name must not be empty".into());
        }
        // Neovim's linewise edits (dd then P) can add line breaks to a field.
        if self
            .name
            .chars()
            .chain(self.path.chars())
            .any(char::is_control)
        {
            return Err(
                "Workspace names and paths cannot contain line breaks or control characters".into(),
            );
        }
        let path = (!self.path.is_empty())
            .then(|| crate::config::expand_path(&self.path, cwd, home))
            .transpose()?;
        if path.is_none() && !Setting::ALL.iter().any(|s| s.is_in(&self.kept)) {
            return Err("A workspace needs a path or a ticked setting".into());
        }
        let mut workspace = Workspace {
            name: self.name.clone(),
            icon: self.icon.clone(),
            path,
            ..Workspace::default()
        };
        for setting in Setting::ALL {
            setting.copy(&self.kept, &mut workspace);
        }
        Ok(workspace)
    }
}
// A change lives for one save; boxing the workspace would gain nothing.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug)]
pub enum Change {
    Put {
        index: Option<usize>,
        workspace: Workspace,
    },
    Remove(usize),
    /// Exchange two entries, e.g. adjacent ones to reorder the list.
    Swap(usize, usize),
}
impl Change {
    pub fn apply(&self, current: &[Workspace]) -> Result<Vec<Workspace>, String> {
        let mut next = current.to_vec();
        match self {
            Self::Put {
                index: Some(index),
                workspace,
            } => *next.get_mut(*index).ok_or("Workspace no longer exists")? = workspace.clone(),
            Self::Put {
                index: None,
                workspace,
            } => next.push(workspace.clone()),
            Self::Remove(index) => {
                if *index >= next.len() {
                    return Err("Workspace no longer exists".into());
                }
                next.remove(*index);
            }
            Self::Swap(a, b) => {
                if (*a).max(*b) >= next.len() {
                    return Err("Workspace no longer exists".into());
                }
                next.swap(*a, *b);
            }
        }
        let mut names = std::collections::HashSet::new();
        for b in &next {
            if b.name.trim().is_empty() || !names.insert(&b.name) {
                return Err("Workspace names must be nonempty and unique".into());
            }
            if b.path.as_ref().is_some_and(|p| p.as_os_str().is_empty()) {
                return Err("Workspace path must not be empty".into());
            }
            if let Some(icon) = &b.icon
                && !(1..=2).contains(&unicode_width::UnicodeWidthStr::width(icon.as_str()))
            {
                return Err("Workspace icon must be one or two terminal columns wide".into());
            }
        }
        Ok(next)
    }
}
#[derive(Default)]
pub struct Store {
    pub path: Option<PathBuf>,
    expected: Vec<Workspace>,
    home: Option<PathBuf>,
}
impl Store {
    pub fn new(path: Option<PathBuf>, expected: Vec<Workspace>, home: Option<PathBuf>) -> Self {
        Self {
            path,
            expected,
            home,
        }
    }
    /// Nothing changes in memory when validation or persistence fails.
    pub fn commit(
        &mut self,
        current: &[Workspace],
        change: &Change,
    ) -> Result<Vec<Workspace>, String> {
        let next = change.apply(current)?;
        let Some(path) = &self.path else {
            return Ok(next);
        };
        let before = read(path)?;
        let text = before.as_deref().unwrap_or("");
        let on_disk = workspaces_from_text(
            text,
            path.parent().unwrap_or(Path::new(".")),
            self.home.as_deref(),
        )?;
        if on_disk != self.expected || current != self.expected {
            return Err(CHANGED.into());
        }
        let mut document: DocumentMut = text
            .parse()
            .map_err(|e| format!("Cannot edit configuration: {e}"))?;
        update_document(&mut document, change, current)?;
        let updated = document.to_string();
        // Verify the exact output before replacing the user's configuration.
        let decoded = workspaces_from_text(
            &updated,
            path.parent().unwrap_or(Path::new(".")),
            self.home.as_deref(),
        )?;
        if decoded != next {
            return Err("Workspace serialization did not preserve the requested values".into());
        }
        atomic_write(path, before.as_deref(), &updated)
            .map_err(|e| format!("Cannot save workspaces to {}: {e}", path.display()))?;
        self.expected = next.clone();
        Ok(next)
    }
    pub fn reload(&mut self) -> Result<Vec<Workspace>, String> {
        let path = self
            .path
            .as_ref()
            .ok_or("Session-only workspaces have no configuration to reload")?;
        let text = read(path)?.unwrap_or_default();
        let workspaces = workspaces_from_text(
            &text,
            path.parent().unwrap_or(Path::new(".")),
            self.home.as_deref(),
        )?;
        self.expected = workspaces.clone();
        Ok(workspaces)
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
    current: &[Workspace],
) -> Result<(), String> {
    if !document.contains_key("workspaces") {
        document["workspaces"] = Item::ArrayOfTables(Default::default());
    }
    let item = &mut document["workspaces"];
    if let Some(tables) = item.as_array_of_tables_mut() {
        match change {
            Change::Put {
                index: None,
                workspace,
            } => {
                let mut t = Table::new();
                update_table(&mut t, None, workspace)?;
                tables.push(t);
            }
            Change::Put {
                index: Some(i),
                workspace,
            } => update_table(
                tables.get_mut(*i).ok_or("Workspace table missing")?,
                current.get(*i),
                workspace,
            )?,
            Change::Remove(i) => {
                tables.remove(*i);
            }
            Change::Swap(a, b) => {
                // Entries carry their comments along; the document keeps its
                // table positions so output order follows the new list order.
                let (mut first, mut second) = (
                    tables.get(*a).ok_or("Workspace table missing")?.clone(),
                    tables.get(*b).ok_or("Workspace table missing")?.clone(),
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
                workspace,
            } => {
                let mut t = toml_edit::InlineTable::new();
                update_table(&mut t, None, workspace)?;
                array.push(t);
            }
            Change::Put {
                index: Some(i),
                workspace,
            } => {
                let t = array
                    .get_mut(*i)
                    .and_then(Value::as_inline_table_mut)
                    .ok_or("Workspace table missing")?;
                update_table(t, current.get(*i), workspace)?;
            }
            Change::Remove(i) => {
                array.remove(*i);
            }
            Change::Swap(a, b) => {
                // Values trade places; the surrounding whitespace stays put.
                let (mut first, mut second) = (
                    array.get(*a).ok_or("Workspace table missing")?.clone(),
                    array.get(*b).ok_or("Workspace table missing")?.clone(),
                );
                std::mem::swap(first.decor_mut(), second.decor_mut());
                array.replace_formatted(*a, second);
                array.replace_formatted(*b, first);
            }
        }
    } else {
        return Err("Unsupported workspaces configuration".into());
    }
    Ok(())
}
/// Each key `workspace` writes, with its value (`None` when it has none).
fn keys(workspace: &Workspace) -> Result<Vec<(&'static str, Option<Value>)>, String> {
    let text = |s: &str| Value::from(s);
    let path = |p: &Path| {
        p.to_str()
            .map(Value::from)
            .ok_or("Workspace paths must be valid UTF-8 to save")
    };
    let strings = |items: &[String]| Value::Array(items.iter().map(String::as_str).collect());
    let roots = match &workspace.roots {
        Some(roots) => Some(Value::Array(
            roots
                .iter()
                .map(|r| path(r))
                .collect::<Result<toml_edit::Array, _>>()?,
        )),
        None => None,
    };
    let sort = workspace.sort.map(|sort| match sort {
        Some(sort) => (sort.by.name(), sort.descending),
        None => ("ranking", false),
    });
    Ok(vec![
        ("name", Some(text(&workspace.name))),
        ("icon", workspace.icon.as_deref().map(text)),
        ("path", workspace.path.as_deref().map(path).transpose()?),
        ("program", workspace.program.map(|p| text(p.title()))),
        ("roots", roots),
        ("query", workspace.query.as_deref().map(text)),
        ("contents", workspace.contents.as_deref().map(text)),
        ("depth", workspace.depth.map(|d| Value::from(d as i64))),
        ("filters", workspace.filters.as_deref().map(strings)),
        ("exclusions", workspace.exclusions.as_deref().map(strings)),
        ("sort", sort.map(|(name, _)| text(name))),
        (
            "sort_descending",
            sort.filter(|&(_, descending)| descending)
                .map(|_| Value::from(true)),
        ),
        ("hidden", workspace.hidden.map(Value::from)),
        (
            "kinds",
            workspace.kinds.map(|k| {
                text(match k {
                    crate::Kinds::All => "all",
                    crate::Kinds::Files => "files",
                    crate::Kinds::Directories => "directories",
                })
            }),
        ),
        ("preview", workspace.preview.map(Value::from)),
        ("exclusions_on", workspace.exclusions_on.map(Value::from)),
        ("filters_on", workspace.filters_on.map(Value::from)),
    ])
}
/// Writes the keys that changed from `old`: an unchanged path keeps its
/// spelling, and an unchanged value its formatting.
fn update_table(
    table: &mut dyn TableLike,
    old: Option<&Workspace>,
    new: &Workspace,
) -> Result<(), String> {
    let before = old.map(keys).transpose()?;
    for (i, (key, value)) in keys(new)?.into_iter().enumerate() {
        let was = before.as_ref().map(|keys| &keys[i].1);
        if was.is_some_and(|was| same(was.as_ref(), value.as_ref())) {
            continue;
        }
        match value {
            Some(value) => set(table, key, value),
            None => {
                table.remove(key);
            }
        }
    }
    Ok(())
}
/// The same value, whatever its formatting.
fn same(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a.to_string().trim() == b.to_string().trim(),
        (a, b) => a.is_none() && b.is_none(),
    }
}
fn set(table: &mut dyn TableLike, key: &str, new: Value) {
    if let Some(value) = table.get_mut(key).and_then(Item::as_value_mut) {
        let decor = value.decor().clone();
        *value = new;
        *value.decor_mut() = decor;
    } else {
        table.insert(key, toml_edit::value(new));
    }
}
/// Writes `text` to `path` whole or not at all, if the file still holds
/// `before`: a temporary file beside it, renamed over it, keeping its
/// permissions and a symlink to it.
pub(crate) fn atomic_write(path: &Path, before: Option<&str>, text: &str) -> Result<(), String> {
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
        ".fff-save-{}-{}.tmp",
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
            return Err("the file changed while saving; try again".into());
        }
        fs::rename(&temporary, &target).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
