//! Fuzzy matching: scoring names and paths below their roots against the
//! query, and the matched characters to highlight.
//!
//! A name is scored by nucleo, fzf's algorithm: the best alignment of the
//! query's letters, in order, rewarding runs and word starts (after `_`, `-`,
//! `.`, a space or a lowercase-to-uppercase step) and penalizing gaps.
use crate::Candidate;
use nucleo_matcher::{
    Utf32Str,
    pattern::{Atom, AtomKind, CaseMatching, Normalization},
};
use std::cell::RefCell;

/// Case-insensitive basename subsequence score: the best alignment of the
/// query's letters, rewarding runs and word starts and penalizing gaps.
/// Latin accents are ignored unless the query has some. Empty queries have
/// equal score.
pub fn score(name: &str, query: &str) -> Option<i64> {
    Matcher::new(query).score(name)
}

thread_local! {
    /// nucleo's working memory (about 135 KB, allocated at once) and a
    /// name's characters, reused by every match on this thread.
    static SCRATCH: RefCell<(nucleo_matcher::Matcher, Vec<char>)> =
        RefCell::new((nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT), Vec::new()));
}

/// Path-aware matching: bonus for a query part naming a whole component.
const WHOLE: i64 = 40;
/// Path-aware matching: bonus for consecutive parts matching consecutive
/// components (the last folder part next to the basename included).
const NEXT: i64 = 20;

/// A query lowercased once, for scoring many names.
pub struct Matcher {
    letters: Vec<char>,
    atom: Atom,
    /// A query containing `/` matches the path below a result's root: the
    /// parts before the last (empty ones dropped) match ancestors in order,
    /// the last matches the basename.
    path: Option<(Vec<Matcher>, Box<Matcher>)>,
}
/// Lowercase for matching. Whole-string lowercasing turns a word-final Σ
/// into ς but a lone one into σ: both become σ, so either spelling matches.
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase().replace('ς', "σ")
}
impl Matcher {
    pub fn new(query: &str) -> Self {
        let folded = fold(query);
        let atom = |part: &str| {
            // Smart normalization folds accents only for a query without any.
            Atom::new(
                part,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
                false,
            )
        };
        let plain = |part: &str| Matcher {
            letters: part.chars().collect(),
            atom: atom(part),
            path: None,
        };
        let path = folded.rsplit_once('/').map(|(folders, last)| {
            let folders = folders
                .split('/')
                .filter(|part| !part.is_empty())
                .map(plain)
                .collect();
            (folders, Box::new(plain(last)))
        });
        Self {
            letters: folded.chars().collect(),
            atom: atom(&folded),
            path,
        }
    }
    /// Whether the query matches paths rather than basenames.
    pub fn paths(&self) -> bool {
        self.path.is_some()
    }
    /// Score `name` as a whole: a basename, a bookmark or palette name.
    pub fn score(&self, name: &str) -> Option<i64> {
        if self.letters.is_empty() {
            return Some(0);
        }
        self.nucleo(name, None).map(i64::from)
    }
    /// The char indices of `name` the query matches, in order, as drawn:
    /// the alignment that scored best. `None` when it does not match.
    pub fn positions(&self, name: &str) -> Option<Vec<usize>> {
        if self.letters.is_empty() {
            return Some(Vec::new());
        }
        let mut indices = Vec::new();
        self.nucleo(name, Some(&mut indices))?;
        indices.sort_unstable();
        indices.dedup();
        Some(indices.into_iter().map(|i| i as usize).collect())
    }
    /// nucleo's score of `name`, and its matched indices when asked for.
    /// The name is passed as chars (not graphemes), so indices are chars.
    fn nucleo(&self, name: &str, indices: Option<&mut Vec<u32>>) -> Option<u16> {
        SCRATCH.with_borrow_mut(|(matcher, chars)| {
            let haystack = if name.is_ascii() {
                Utf32Str::Ascii(name.as_bytes())
            } else {
                // nucleo case-folds only uppercase letters while matching
                // (and every letter when ignoring accents), so a lowercase
                // letter with another folding, like ς (σ), is folded here.
                // Uppercase stays: it marks word starts.
                chars.clear();
                chars.extend(name.chars().map(|c| {
                    if c.is_lowercase() {
                        nucleo_matcher::chars::to_lower_case(c)
                    } else {
                        c
                    }
                }));
                Utf32Str::Unicode(chars)
            };
            match indices {
                Some(indices) => self.atom.indices(haystack, matcher, indices),
                None => self.atom.score(haystack, matcher),
            }
        })
    }
    /// Score a result: its basename, or with a `/` in the query, its path
    /// below its root.
    pub fn score_candidate(&self, c: &Candidate) -> Option<i64> {
        if self.path.is_none() {
            return self.score(&c.path.file_name()?.to_string_lossy());
        }
        self.score_path(&relative_parts(c))
    }
    /// Score a path's components below its root, the basename last.
    pub fn score_path<S: AsRef<str>>(&self, parts: &[S]) -> Option<i64> {
        match &self.path {
            None => self.score(parts.last()?.as_ref()),
            Some(_) => self.assign(parts).map(|(score, _)| score),
        }
    }
    /// The letters of `parts` (a path's components below its root) a path
    /// query matches, as char indices within each component; `None` when it
    /// does not match.
    pub fn path_positions<S: AsRef<str>>(&self, parts: &[S]) -> Option<Vec<Vec<usize>>> {
        let (folders, last) = self.path.as_ref()?;
        let (_, chosen) = self.assign(parts)?;
        let mut positions = vec![Vec::new(); parts.len()];
        for (folder, &at) in folders.iter().zip(&chosen) {
            positions[at] = folder.positions(parts[at].as_ref())?;
        }
        positions[parts.len() - 1] = last.positions(parts[parts.len() - 1].as_ref())?;
        Some(positions)
    }
    /// The best path score and, for each folder part, the component it
    /// matches: parts match components in order, the last part the basename.
    fn assign<S: AsRef<str>>(&self, parts: &[S]) -> Option<(i64, Vec<usize>)> {
        let (folders, last) = self.path.as_ref()?;
        let (basename, ancestors) = parts.split_last()?;
        let basename = basename.as_ref();
        let mut total = last.score(basename)? + last.whole(basename);
        if folders.is_empty() {
            return Some((total, Vec::new()));
        }
        if folders.len() > ancestors.len() {
            return None;
        }
        // best[i][j]: the best score of folder parts 0..=i with part i on
        // ancestor j, and the ancestor part i - 1 is on.
        let mut best: Vec<Vec<Option<(i64, usize)>>> = Vec::with_capacity(folders.len());
        for (i, folder) in folders.iter().enumerate() {
            let mut row = vec![None; ancestors.len()];
            for (j, part) in ancestors.iter().enumerate() {
                // Leave room for the parts still to place.
                if j < i || ancestors.len() - j < folders.len() - i {
                    continue;
                }
                let Some(own) = folder.score(part.as_ref()) else {
                    continue;
                };
                let own = own + folder.whole(part.as_ref());
                row[j] = if i == 0 {
                    Some((own, 0))
                } else {
                    (0..j)
                        .filter_map(|k| {
                            let (score, _) = best[i - 1][k]?;
                            Some((score + own + if k + 1 == j { NEXT } else { 0 }, k))
                        })
                        .max_by_key(|&(score, k)| (score, std::cmp::Reverse(k)))
                };
            }
            best.push(row);
        }
        let (end, score) = best[folders.len() - 1]
            .iter()
            .enumerate()
            .filter_map(|(j, cell)| {
                let (score, _) = (*cell)?;
                Some((j, score + if j + 1 == ancestors.len() { NEXT } else { 0 }))
            })
            .max_by_key(|&(j, score)| (score, std::cmp::Reverse(j)))?;
        total += score;
        let mut chosen = vec![end; folders.len()];
        for i in (1..folders.len()).rev() {
            chosen[i - 1] = best[i][chosen[i]].expect("on the best path").1;
        }
        Some((total, chosen))
    }
    /// `WHOLE` when this part names all of `name`, ignoring case.
    fn whole(&self, name: &str) -> i64 {
        let same = if name.is_ascii() {
            name.len() == self.letters.len()
                && name
                    .bytes()
                    .zip(&self.letters)
                    .all(|(b, &l)| b.to_ascii_lowercase() as u32 == l as u32)
        } else {
            fold(name).chars().eq(self.letters.iter().copied())
        };
        if same && !self.letters.is_empty() {
            WHOLE
        } else {
            0
        }
    }
}

/// A result's components below its root, the basename last: its last
/// `depth` components.
pub fn relative_parts(c: &Candidate) -> Vec<std::borrow::Cow<'_, str>> {
    let mut parts: Vec<_> = c
        .path
        .components()
        .rev()
        .take(c.depth.max(1))
        .map(|part| part.as_os_str().to_string_lossy())
        .collect();
    parts.reverse();
    parts
}
/// Whether every match of the (lowercased) query `longer` is among those of
/// `shorter`: it appends letters, but no `/`, which turns the part before it
/// into a folder (or a basename query into a path query).
pub(crate) fn narrows(shorter: &str, longer: &str) -> bool {
    longer
        .strip_prefix(shorter)
        .is_some_and(|added| !added.contains('/'))
}
#[cfg(test)]
#[test]
fn appended_letters_narrow_but_a_slash_does_not() {
    assert!(narrows("rep", "repo") && narrows("", "a") && narrows("rep", "rep"));
    assert!(!narrows("repo", "rep") && !narrows("abc", "xbc"));
    // A slash turns the basename query into a path query, or the last part
    // into a folder; letters after one narrow again.
    assert!(!narrows("rep", "rep/") && !narrows("pde", "pde/stab") && !narrows("", "/s"));
    assert!(narrows("pde/", "pde/stab"));
}
#[cfg(test)]
#[test]
fn final_sigma_is_highlighted_for_either_sigma() {
    assert_eq!(Matcher::new("σ").positions("ΟΔΟΣ"), Some(vec![3]));
    assert_eq!(Matcher::new("ος").positions("οδος"), Some(vec![0, 3]));
}
#[cfg(test)]
#[test]
fn the_best_alignment_is_scored_and_highlighted() {
    // A whole word deep in a long name beats letters scattered from the start.
    let canard = "krupa_szmolyan_relaxation_oscillation_canard_explosion.pdf";
    let scattered = "minicozzi_colding_a_course_in_minimal_surfaces.djvu";
    assert!(score(canard, "canard") > score(scattered, "canard"));
    let at = canard.find("canard").unwrap();
    assert_eq!(
        Matcher::new("canard").positions(canard),
        Some((at..at + 6).collect())
    );
    // Accents are ignored unless the query has some.
    assert!(score("Evaluación.pdf", "evaluacion").is_some());
    assert!(score("Evaluacion.pdf", "evaluación").is_none());
    assert!(score("Evaluación.pdf", "evaluación").is_some());
}
