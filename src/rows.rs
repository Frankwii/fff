//! The results' order: one value that every comparison of rows goes
//! through, so that another order is a variant here rather than a change
//! to ranking and merging.
use crate::Candidate;
use std::cmp::Ordering;

/// How rows are ordered. Each program has one (`Program::order`, read as
/// `App::order`); a sorting picker would add variants and choose among them
/// there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Order {
    /// Depth, directories first, best score, root order, then pathname
    /// bytes, which break every tie.
    #[default]
    Depth,
    /// The best score first, then as `Depth` (`fff pick` and `fff seek`).
    Score,
}
impl Order {
    /// Compares two rows with their scores for the current query.
    pub fn cmp(self, a: &Candidate, score_a: i64, b: &Candidate, score_b: i64) -> Ordering {
        let score = score_b.cmp(&score_a);
        let depth = a.depth.cmp(&b.depth).then(b.directory.cmp(&a.directory));
        let (first, second) = match self {
            Self::Depth => (depth, score),
            Self::Score => (score, depth),
        };
        first
            .then(second)
            .then(a.root.cmp(&b.root))
            .then(a.path.as_os_str().cmp(b.path.as_os_str()))
    }
}
