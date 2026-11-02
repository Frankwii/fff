//! The Alt+g prompt: a typed path, or words picking one of your bookmarks or
//! zoxide's frequently used directories.
use std::{
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

/// Where a suggestion comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Bookmark(String),
    Zoxide,
}

#[derive(Clone, Debug)]
pub struct Location {
    pub text: String,
    /// Bookmarks first, then zoxide's directories by frecency; no duplicates.
    pub places: Vec<(PathBuf, Source)>,
    /// The chosen suggestion among the current matches, if the user moved.
    pub selected: Option<usize>,
}

/// Directories zoxide knows, best first; empty when zoxide is not installed.
/// zoxide leaves out the directory it runs in: that is `cwd`, the one place
/// not worth suggesting, rather than where fff was started.
pub fn zoxide(cwd: &Path) -> Vec<PathBuf> {
    let Ok(program) = crate::run::resolve("zoxide", cwd) else {
        return vec![];
    };
    let Ok(output) = Command::new(program)
        .args(["query", "--list"])
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return vec![];
    };
    output
        .stdout
        .split(|b| *b == b'\n')
        .filter(|line| !line.is_empty())
        .map(|line| PathBuf::from(OsString::from_vec(line.to_vec())))
        .collect()
}

/// Words must all appear, in order, in the path (case-insensitively), like
/// `zoxide query`: `bu ff` matches `~/builds/fff`.
fn keywords_match(path: &Path, words: &[String]) -> bool {
    let haystack = String::from_utf8_lossy(path.as_os_str().as_bytes()).to_lowercase();
    let mut from = 0;
    words
        .iter()
        .all(|word| match haystack[from..].find(word.as_str()) {
            Some(at) => {
                from += at + word.len();
                true
            }
            None => false,
        })
}

impl Location {
    pub fn new(bookmarks: &[(String, PathBuf)], frequent: Vec<PathBuf>, cwd: &Path) -> Self {
        let mut places: Vec<(PathBuf, Source)> = vec![];
        let candidates = bookmarks
            .iter()
            .map(|(name, path)| (path.clone(), Source::Bookmark(name.clone())))
            .chain(frequent.into_iter().map(|p| (p, Source::Zoxide)));
        for (path, source) in candidates {
            if path != cwd && !places.iter().any(|(p, _)| *p == path) {
                places.push((path, source));
            }
        }
        Self {
            text: String::new(),
            places,
            selected: None,
        }
    }
    /// Suggestions for the typed words, bookmarks first; all when empty.
    pub fn matches(&self) -> Vec<&(PathBuf, Source)> {
        let words = self
            .text
            .split_whitespace()
            .map(str::to_lowercase)
            .collect::<Vec<_>>();
        self.places
            .iter()
            .filter(|(path, source)| {
                keywords_match(path, &words)
                    || matches!(source, Source::Bookmark(name)
                        if !words.is_empty() && keywords_match(Path::new(name), &words))
            })
            .collect()
    }
    /// ↑ / ↓: move through the suggestions, wrapping at either end.
    pub fn choose(&mut self, down: bool) {
        let count = self.matches().len();
        if count == 0 {
            self.selected = None;
            return;
        }
        self.selected = Some(match (self.selected, down) {
            (None, true) => 0,
            (None, false) => count - 1,
            (Some(i), true) => (i + 1) % count,
            (Some(i), false) => i.checked_sub(1).unwrap_or(count - 1),
        });
    }
    fn typed(&self, cwd: &Path, home: Option<&Path>) -> PathBuf {
        resolve(&self.text, cwd, home)
    }
    /// Enter: the chosen suggestion, else the typed directory if it exists,
    /// else the best suggestion.
    pub fn target(&self, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, String> {
        let matches = self.matches();
        if let Some((path, _)) = self.selected.and_then(|i| matches.get(i)) {
            return Ok(path.clone());
        }
        let typed = self.typed(cwd, home);
        if !self.text.trim().is_empty() && typed.is_dir() {
            return Ok(typed);
        }
        matches
            .first()
            .map(|(path, _)| path.clone())
            .ok_or_else(|| format!("No directory matches {:?}", self.text.trim()))
    }
    /// Tab: complete the typed path's last component among directories.
    pub fn complete(&mut self, cwd: &Path, home: Option<&Path>) {
        // Split the text as typed: completing replaces what follows the last
        // `/`. A lone `~` or a `..` names a folder to complete inside.
        let (mut head, mut prefix) = match self.text.rfind('/') {
            Some(i) => (self.text[..=i].to_owned(), self.text[i + 1..].to_owned()),
            None => (String::new(), self.text.clone()),
        };
        if (head.is_empty() && prefix == "~") || prefix == ".." {
            head = format!("{}/", self.text);
            prefix.clear();
        }
        let dir = resolve(&head, cwd, home);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return;
        };
        let mut names = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.starts_with(&prefix) && (prefix.starts_with('.') || !n.starts_with('.')))
            .collect::<Vec<_>>();
        names.sort();
        let completion = match names.as_slice() {
            [] => return,
            [only] => format!("{only}/"),
            many => many.iter().skip(1).fold(many[0].clone(), |common, n| {
                common
                    .chars()
                    .zip(n.chars())
                    .take_while(|(a, b)| a == b)
                    .map(|(a, _)| a)
                    .collect()
            }),
        };
        self.text = format!("{head}{completion}");
        self.selected = None;
    }
}

/// A typed path: `~` is the home folder, a relative one is below `cwd`.
fn resolve(text: &str, cwd: &Path, home: Option<&Path>) -> PathBuf {
    let text = text.trim();
    match (text.strip_prefix('~'), home) {
        (Some(rest), Some(home)) if rest.is_empty() || rest.starts_with('/') => {
            home.join(rest.trim_start_matches('/'))
        }
        _ => cwd.join(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn words_pick_bookmarks_and_frequent_folders_and_paths_still_work() {
        let home = std::env::temp_dir().join(format!("fff-location-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(home.join("builds/fff")).unwrap();
        std::fs::create_dir_all(home.join("builds/dragon")).unwrap();
        let frequent = vec![
            home.join("builds/fff"),
            home.join("notes"),
            home.join("builds/dragon"),
        ];
        let bookmarks = [("Notes".to_string(), home.join("notes"))];
        let mut l = Location::new(&bookmarks, frequent, &home);
        // The bookmark comes first and is not repeated from zoxide.
        assert_eq!(l.places.len(), 3);
        assert_eq!(l.places[0].1, Source::Bookmark("Notes".into()));
        l.text = "bu ff".into();
        assert_eq!(
            l.target(&home, Some(&home)).unwrap(),
            home.join("builds/fff")
        );
        l.text = "notes".into();
        assert_eq!(l.matches().len(), 1);
        l.text = "builds".into();
        l.choose(true);
        l.choose(true);
        assert_eq!(
            l.target(&home, Some(&home)).unwrap(),
            home.join("builds/dragon")
        );
        // A typed existing directory wins over suggestions.
        l.selected = None;
        l.text = "~/builds".into();
        assert_eq!(l.target(&home, Some(&home)).unwrap(), home.join("builds"));
        l.text = "~/builds/f".into();
        l.complete(&home, Some(&home));
        assert_eq!(l.text, "~/builds/fff/");
        l.text = "zzz".into();
        assert!(l.target(&home, Some(&home)).is_err());
        std::fs::remove_dir_all(&home).unwrap();
    }
    #[test]
    fn tab_completes_after_the_text_as_typed() {
        let root = std::env::temp_dir().join(format!("fff-tab-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (home, cwd) = (root.join("héme"), root.join("work"));
        std::fs::create_dir_all(home.join("notes")).unwrap();
        std::fs::create_dir_all(cwd.join("only/.hid")).unwrap();
        let mut l = Location::new(&[], vec![], &cwd);
        for (typed, completed) in [
            ("~", "~/notes/"),
            // `..` holds héme and work: no common prefix to add.
            ("..", "../"),
            ("only/.", "only/.hid/"),
            ("on", "only/"),
            ("on ", "on "),
        ] {
            l.text = typed.into();
            l.complete(&cwd, Some(&home));
            assert_eq!(l.text, completed, "{typed:?}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
