//! Marked paths in marking order, with constant-time membership: every frame
//! asks whether each visible row is marked, and marks can number in the
//! hundreds of thousands (Normal-mode `m` marks every listed row).
use std::{
    collections::HashMap,
    ops::Deref,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Default)]
pub struct Marks {
    order: Vec<PathBuf>,
    /// How many times each path appears in `order`; normally once.
    counts: HashMap<PathBuf, usize>,
}
impl Marks {
    pub fn contains(&self, path: &Path) -> bool {
        self.counts.contains_key(path)
    }
    pub fn push(&mut self, path: PathBuf) {
        *self.counts.entry(path.clone()).or_default() += 1;
        self.order.push(path);
    }
    pub fn remove(&mut self, index: usize) -> PathBuf {
        let path = self.order.remove(index);
        forget(&mut self.counts, &path);
        path
    }
    pub fn clear(&mut self) {
        self.order.clear();
        self.counts.clear();
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&PathBuf) -> bool) {
        let counts = &mut self.counts;
        self.order.retain(|p| {
            let kept = keep(p);
            if !kept {
                forget(counts, p);
            }
            kept
        });
    }
    /// Change paths in place (e.g. after a move), keeping their order.
    pub fn update(&mut self, change: impl FnMut(&mut PathBuf)) {
        self.order.iter_mut().for_each(change);
        self.counts.clear();
        for path in &self.order {
            *self.counts.entry(path.clone()).or_default() += 1;
        }
    }
    pub fn to_vec(&self) -> Vec<PathBuf> {
        self.order.clone()
    }
}
fn forget(counts: &mut HashMap<PathBuf, usize>, path: &Path) {
    if let Some(n) = counts.get_mut(path) {
        *n -= 1;
        if *n == 0 {
            counts.remove(path);
        }
    }
}
impl Deref for Marks {
    type Target = [PathBuf];
    fn deref(&self) -> &[PathBuf] {
        &self.order
    }
}
impl Extend<PathBuf> for Marks {
    fn extend<I: IntoIterator<Item = PathBuf>>(&mut self, paths: I) {
        paths.into_iter().for_each(|p| self.push(p));
    }
}
impl From<Vec<PathBuf>> for Marks {
    fn from(paths: Vec<PathBuf>) -> Self {
        let mut marks = Self::default();
        marks.extend(paths);
        marks
    }
}
impl<'a> IntoIterator for &'a Marks {
    type Item = &'a PathBuf;
    type IntoIter = std::slice::Iter<'a, PathBuf>;
    fn into_iter(self) -> Self::IntoIter {
        self.order.iter()
    }
}
impl<T: AsRef<[PathBuf]> + ?Sized> PartialEq<T> for Marks {
    fn eq(&self, other: &T) -> bool {
        self.order == other.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn membership_follows_every_change() {
        let p = |s: &str| PathBuf::from(s);
        let mut marks = Marks::from(vec![p("/a"), p("/b"), p("/a")]);
        assert!(marks.contains(Path::new("/a")));
        marks.remove(0);
        assert!(marks.contains(Path::new("/a")), "a duplicate remains");
        marks.retain(|x| x != &p("/a"));
        assert!(!marks.contains(Path::new("/a")));
        assert_eq!(marks, [p("/b")]);
        marks.update(|x| *x = x.with_extension("txt"));
        assert!(marks.contains(Path::new("/b.txt")) && !marks.contains(Path::new("/b")));
        marks.clear();
        assert!(marks.is_empty() && !marks.contains(Path::new("/b.txt")));
    }
}
