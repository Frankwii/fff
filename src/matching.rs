//! Fuzzy matching: scoring names and paths below their roots against the
//! query, and the matched characters to highlight.
//!
//! A name is scored by nucleo, fzf's algorithm: the best alignment of the
//! query's letters, in order, rewarding runs and word starts (after `_`, `-`,
//! `.`, a space or a lowercase-to-uppercase step) and penalizing gaps. A
//! result is scored by its path below its root, a `/` also starting a word:
//! a gap that crosses a `/` costs `CROSSING` however long it is, and each
//! letter matched in the basename earns `BASENAME`.
use crate::Candidate;
use nucleo_matcher::{
    Config, Utf32Str,
    pattern::{Atom, AtomKind, CaseMatching, Normalization},
};
use std::{borrow::Cow, cell::RefCell};

/// Case-insensitive basename subsequence score: the best alignment of the
/// query's letters, rewarding runs and word starts and penalizing gaps.
/// Latin accents are ignored unless the query has some. Empty queries have
/// equal score.
pub fn score(name: &str, query: &str) -> Option<i64> {
    Matcher::new(query).score(name)
}

/// nucleo's working memory (about 135 KB each, allocated at once): one for
/// names, one with bonuses after `/` for paths.
struct Scratch {
    names: nucleo_matcher::Matcher,
    paths: nucleo_matcher::Matcher,
    /// A haystack's characters.
    chars: Vec<char>,
}
thread_local! {
    /// Reused by every match on this thread.
    static SCRATCH: RefCell<Scratch> = RefCell::new(Scratch {
        names: nucleo_matcher::Matcher::new(Config::DEFAULT),
        paths: nucleo_matcher::Matcher::new(Config::DEFAULT.match_paths()),
        chars: Vec::new(),
    });
}

/// Points for each letter of the query matched in the basename: half a
/// level of depth (`rows::LEVEL`), so that among matches as good the folder
/// a query names ranks before what is inside it.
const BASENAME: i64 = 4;
/// What a gap between matched letters costs up to its last `/`, instead of
/// nucleo's 3 plus 1 per skipped letter: skipping the rest of a folder's
/// name and the folders after it is how a path is typed, not a scattered
/// match. Twice what starting a gap costs. Only a gap landing on a word
/// start gets it, and the letters skipped after the `/` still cost 1 each.
const CROSSING: i64 = 6;

/// A query lowercased once, for scoring many names.
pub struct Matcher {
    letters: Vec<char>,
    atom: Atom,
}
/// Lowercase for matching. Whole-string lowercasing turns a word-final Σ
/// into ς but a lone one into σ: both become σ, so either spelling matches.
pub(crate) fn fold(text: &str) -> String {
    text.to_lowercase().replace('ς', "σ")
}
impl Matcher {
    pub fn new(query: &str) -> Self {
        let folded = fold(query);
        Self {
            // Smart normalization folds accents only for a query without any.
            atom: Atom::new(
                &folded,
                CaseMatching::Ignore,
                Normalization::Smart,
                AtomKind::Fuzzy,
                false,
            ),
            letters: folded.chars().collect(),
        }
    }
    /// How many letters the query has: what a sort by a depth penalty takes
    /// scores per.
    pub fn letters(&self) -> usize {
        self.letters.len()
    }
    /// Score `name` as a whole: a basename, a workspace or palette name.
    pub fn score(&self, name: &str) -> Option<i64> {
        if self.letters.is_empty() {
            return Some(0);
        }
        self.nucleo(name, false, None).map(i64::from)
    }
    /// The char indices of `name` the query matches, in order, as drawn:
    /// the alignment that scored best. `None` when it does not match.
    pub fn positions(&self, name: &str) -> Option<Vec<usize>> {
        self.indices(name, false)
    }
    /// The char indices of a result's path below its root (`relative`) the
    /// query matches, as `score_candidate` aligns them.
    pub fn path_positions(&self, path: &str) -> Option<Vec<usize>> {
        self.indices(path, true)
    }
    fn indices(&self, text: &str, paths: bool) -> Option<Vec<usize>> {
        if self.letters.is_empty() {
            return Some(Vec::new());
        }
        let mut indices = Vec::new();
        self.nucleo(text, paths, Some(&mut indices))?;
        indices.sort_unstable();
        indices.dedup();
        Some(indices.into_iter().map(|i| i as usize).collect())
    }
    /// nucleo's score of `text`, and its matched indices when asked for.
    /// The text is passed as chars (not graphemes), so indices are chars.
    fn nucleo(&self, text: &str, paths: bool, indices: Option<&mut Vec<u32>>) -> Option<u16> {
        SCRATCH.with_borrow_mut(|scratch| {
            let Scratch {
                names,
                paths: path_matcher,
                chars,
            } = scratch;
            let matcher = if paths { path_matcher } else { names };
            let haystack = if text.is_ascii() {
                Utf32Str::Ascii(text.as_bytes())
            } else {
                // nucleo case-folds only uppercase letters while matching
                // (and every letter when ignoring accents), so a lowercase
                // letter with another folding, like ς (σ), is folded here.
                // Uppercase stays: it marks word starts.
                chars.clear();
                chars.extend(text.chars().map(|c| {
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
    /// Score a result by its path below its root (see `score_path`).
    pub fn score_candidate(&self, c: &Candidate) -> Option<i64> {
        self.score_path(&relative(c))
    }
    /// Score a path below a root (components joined by `/`, the basename
    /// last): nucleo's score of its best alignment, with each gap crossing a
    /// `/` costing `CROSSING` instead, plus `BASENAME` for each letter the
    /// alignment puts in the basename.
    pub fn score_path(&self, path: &str) -> Option<i64> {
        if self.letters.is_empty() {
            return Some(0);
        }
        let mut at = Vec::new();
        let mut score = i64::from(self.nucleo(path, true, Some(&mut at))?);
        at.sort_unstable();
        // The path's characters, read from its bytes when it is ASCII.
        let ascii = path.is_ascii();
        let wide: Vec<char> = if ascii {
            Vec::new()
        } else {
            path.chars().collect()
        };
        let char_at = |i: usize| -> char {
            if ascii {
                char::from(path.as_bytes()[i])
            } else {
                wide[i]
            }
        };
        for pair in at.windows(2) {
            let (from, to) = (pair[0] as usize + 1, pair[1] as usize);
            let Some(slash) = (from..to).rev().find(|&i| char_at(i) == '/') else {
                continue;
            };
            if word_start(char_at(to - 1), char_at(to)) {
                // nucleo charged 3 for the gap's first letter, 1 for each other.
                let charged = (to - from) as i64 + 2;
                let landing = (to - slash - 1) as i64;
                score += (charged - CROSSING - landing).max(0);
            }
        }
        let basename = path.rsplit('/').next().unwrap_or(path);
        let start = if ascii {
            path.len() - basename.len()
        } else {
            wide.len() - basename.chars().count()
        };
        let in_basename = at.iter().filter(|&&i| i as usize >= start).count();
        Some(score + BASENAME * in_basename as i64)
    }
}

/// Whether `c` after `before` starts a word: after a separator or a space,
/// or an uppercase letter after a lowercase one, as nucleo's bonuses have it.
fn word_start(before: char, c: char) -> bool {
    !before.is_alphanumeric() || (before.is_lowercase() && c.is_uppercase())
}
/// A result's path below its root, the basename last: its last `depth`
/// components joined by `/`, borrowed from its path when that is UTF-8.
pub fn relative(c: &Candidate) -> Cow<'_, str> {
    match c.path.to_string_lossy() {
        Cow::Borrowed(path) => Cow::Borrowed(tail(path, c.depth)),
        Cow::Owned(path) => Cow::Owned(tail(&path, c.depth).to_owned()),
    }
}
/// The last `depth` (at least one) components of `path`.
fn tail(path: &str, depth: usize) -> &str {
    let mut at = path.len();
    for _ in 0..depth.max(1) {
        match path[..at].rfind('/') {
            Some(slash) => at = slash,
            None => return path,
        }
    }
    &path[at + 1..]
}
/// Whether every match of the (lowercased) query `longer` is among those of
/// `shorter`: it appends letters.
pub(crate) fn narrows(shorter: &str, longer: &str) -> bool {
    longer.starts_with(shorter)
}
#[cfg(test)]
#[test]
fn appended_letters_narrow() {
    assert!(narrows("rep", "repo") && narrows("", "a") && narrows("rep", "rep"));
    assert!(narrows("rep", "rep/") && narrows("pde", "pde/stab") && narrows("", "/s"));
    assert!(!narrows("repo", "rep") && !narrows("abc", "xbc"));
}
#[cfg(test)]
#[test]
fn a_path_below_the_root_is_its_last_components() {
    assert_eq!(tail("/home/me/pde/notes/a.tex", 3), "pde/notes/a.tex");
    assert_eq!(tail("/home/me/a.tex", 1), "a.tex");
    assert_eq!(tail("/home/me/a.tex", 0), "a.tex");
    assert_eq!(tail("a.tex", 2), "a.tex");
}
#[cfg(test)]
#[test]
fn skipped_folders_cost_less_than_scattered_letters() {
    // Two runs across folders beat letters scattered through one name.
    let m = Matcher::new("turinpdf");
    let folders = m.score_path("phd/presentations/turing_patterns_uib_2026/figures/heat.pdf");
    let scattered =
        m.score_path("Desktop/books/coding/the_C_programming_language_kernighan_ritchie.pdf");
    assert!(folders > scattered, "{folders:?} {scattered:?}");
    assert!(folders > m.score_path("Desktop/documents/trabajo/fueib/titularidad_banco.pdf"));
    // Skipping into the middle of a long name, or of a word, is scattered.
    for stray in [
        "Desktop/documents/phd/contractes/fpi/Fitxa dades personals contractació-Ficha datos personales contratación-Personal data sheet recruitment.pdf",
        "testdir/phd/courses/summer_school_on_pattern_formation/handwritten_notes/Spatial_patterns_in_biology.pdf",
    ] {
        assert!(folders > m.score_path(stray), "{stray}");
    }
    // The query in the basename is better still.
    let named =
        m.score_path("phd/presentations/turing_patterns_uib_2026/references/turing_paper.pdf");
    assert!(named > folders);
    // Among matches as good, the letters in the basename win.
    let notes = Matcher::new("notes");
    assert!(notes.score_path("pde/notes") > notes.score_path("notes/a.tex"));
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
