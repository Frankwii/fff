//! What the results list: hidden entries, exclusions and file filters
//! (`Policy`), the kinds of entry shown (`Kinds`), and the depth and scope
//! checks deciding whether a found path is a row.
use crate::Candidate;
use globset::{GlobBuilder, GlobMatcher};
use std::path::{Path, PathBuf};

#[derive(Clone, Default)]
pub struct Policy {
    pub hidden: bool,
    pub ignore: bool,
    pub rules: Vec<String>,
    pub exclusions: Vec<GlobMatcher>,
    /// File filters (`--glob`, Alt+f): with any, only files whose basename
    /// matches one of them are listed; directories are still traversed.
    pub filters: Vec<String>,
    pub globs: Vec<GlobMatcher>,
}

pub fn glob(pattern: &str) -> Result<GlobMatcher, globset::Error> {
    Ok(GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()?
        .compile_matcher())
}

impl Policy {
    pub fn set_rules(&mut self, rules: Vec<String>) -> Result<(), globset::Error> {
        let compiled = rules
            .iter()
            .map(|s| glob(s))
            .collect::<Result<Vec<_>, _>>()?;
        self.rules = rules;
        self.exclusions = compiled;
        Ok(())
    }
    /// Like `set_rules`, atomic: every filter compiles before any replaces the list.
    pub fn set_filters(&mut self, filters: Vec<String>) -> Result<(), globset::Error> {
        let compiled = filters
            .iter()
            .map(|s| glob(s))
            .collect::<Result<Vec<_>, _>>()?;
        self.filters = filters;
        self.globs = compiled;
        Ok(())
    }
    /// Whether a file filter admits `name`, a basename; true when there are none.
    pub fn admits(&self, name: &Path) -> bool {
        self.globs.is_empty() || self.globs.iter().any(|g| g.is_match(name))
    }
    /// Byte-level `allows` for `path` below `root`, used on every refresh.
    /// `None` (use `allows`) when exclusion rules apply or the bytes do not
    /// show a plain `root/…` prefix, so Path's component rules decide instead.
    pub fn allows_below(&self, root: &Path, path: &Path) -> Option<bool> {
        if self.ignore && !self.exclusions.is_empty() {
            return None;
        }
        let root = root.as_os_str().as_encoded_bytes();
        let path = path.as_os_str().as_encoded_bytes();
        let rest = path.strip_prefix(root)?;
        let rest = if root.ends_with(b"/") {
            rest
        } else {
            rest.strip_prefix(b"/")?
        };
        // Empty, "." and ".." segments are normalized by Path; leave them to it.
        let mut hidden = false;
        for part in rest.split(|&b| b == b'/') {
            if matches!(part, b"" | b"." | b"..") {
                return None;
            }
            hidden |= part[0] == b'.';
        }
        Some(self.hidden || !hidden)
    }
    pub fn allows(&self, relative: &Path) -> bool {
        // Ancestor prefixes are only needed to match exclusion rules.
        let excluding = self.ignore && !self.exclusions.is_empty();
        let mut prefix = PathBuf::new();
        for part in relative.components() {
            if !self.hidden && part.as_os_str().as_encoded_bytes().first() == Some(&b'.') {
                return false;
            }
            if excluding {
                prefix.push(part);
                if self
                    .exclusions
                    .iter()
                    .any(|g| g.is_match(&prefix) || g.is_match(Path::new(part.as_os_str())))
                {
                    return false;
                }
            }
        }
        true
    }
}

/// Whether `c` is within `depth` of its root, allowed by `policy`, and not `cwd`.
pub(crate) fn eligible(
    depth: usize,
    scope: &[PathBuf],
    cwd: &Path,
    policy: &Policy,
    c: &Candidate,
) -> bool {
    if c.depth > depth {
        return false;
    }
    let Some(root) = scope.get(c.root) else {
        return false;
    };
    if let Some(allowed) = policy.allows_below(root, &c.path) {
        return allowed && c.path.as_os_str() != cwd.as_os_str();
    }
    c.path != cwd && c.path.strip_prefix(root).is_ok_and(|p| policy.allows(p))
}
/// Which kinds of entry the results list.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Kinds {
    #[default]
    All,
    /// Alt+d / `d`, and always in `fff seek`.
    Files,
    /// Alt+D / `D`: folders only. File filters do not apply.
    Directories,
}
impl Kinds {
    /// `kinds` after toggling `only` on or off.
    pub fn toggled(self, only: Kinds) -> Kinds {
        if self == only { Kinds::All } else { only }
    }
}
/// Whether an eligible `c` is of the listed `kinds` and passes the file
/// filters, which hide directories whenever any is set.
pub(crate) fn shown(kinds: Kinds, policy: &Policy, c: &Candidate) -> bool {
    match kinds {
        Kinds::Directories => c.directory,
        Kinds::Files if c.directory => false,
        _ => {
            policy.globs.is_empty()
                || !c.directory
                    && c.path.file_name().is_some_and(|name| {
                        policy.admits(Path::new(name.to_string_lossy().as_ref()))
                    })
        }
    }
}
/// `App::listed` with its own copy of what it depends on.
pub(crate) struct Listing {
    pub(crate) depth: usize,
    pub(crate) scope: Vec<PathBuf>,
    pub(crate) cwd: PathBuf,
    pub(crate) policy: Policy,
    pub(crate) kinds: Kinds,
}
impl Listing {
    pub(crate) fn listed(&self, c: &Candidate) -> bool {
        eligible(self.depth, &self.scope, &self.cwd, &self.policy, c)
            && shown(self.kinds, &self.policy, c)
    }
}
