//! Fuzzy matching: scoring names and paths below their roots against the
//! query, and the matched characters to highlight.
use crate::Candidate;

/// Case-insensitive basename subsequence score: reward adjacency and word starts,
/// then prefer fewer gaps and earlier matches. Empty queries have equal score.
pub fn score(name: &str, query: &str) -> Option<i64> {
    Matcher::new(query).score(name)
}

/// Path-aware matching: bonus for a query part naming a whole component.
const WHOLE: i64 = 40;
/// Path-aware matching: bonus for consecutive parts matching consecutive
/// components (the last folder part next to the basename included).
const NEXT: i64 = 20;

/// A query lowercased once, for scoring many names.
pub struct Matcher {
    letters: Vec<char>,
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
        let plain = |part: &str| Matcher {
            letters: part.chars().collect(),
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
        if name.is_ascii() {
            return self.score_ascii(name.as_bytes());
        }
        // Whole-string lowercasing is context-sensitive (e.g. final sigma).
        let name: Vec<char> = fold(name).chars().collect();
        self.score_chars(&name)
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
            positions[at] = greedy(&folder.letters, parts[at].as_ref());
        }
        positions[parts.len() - 1] = greedy(&last.letters, parts[parts.len() - 1].as_ref());
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
    fn score_chars(&self, name: &[char]) -> Option<i64> {
        let mut position = 0;
        let mut previous = None;
        let mut score = 0;
        for &letter in &self.letters {
            let found = (position..name.len()).find(|&i| name[i] == letter)?;
            score += 20 - found as i64;
            if previous == Some(found.wrapping_sub(1)) {
                score += 30;
            }
            if found == 0 || !name[found - 1].is_alphanumeric() {
                score += 20;
            }
            previous = Some(found);
            position = found + 1;
        }
        Some(score)
    }
    /// Same rules as `score_chars`, without allocating, for ASCII names.
    fn score_ascii(&self, name: &[u8]) -> Option<i64> {
        let mut position = 0;
        let mut previous = None;
        let mut score = 0;
        for &letter in &self.letters {
            if !letter.is_ascii() {
                return None;
            }
            let letter = letter as u8;
            let found = (position..name.len()).find(|&i| name[i].to_ascii_lowercase() == letter)?;
            score += 20 - found as i64;
            if previous == Some(found.wrapping_sub(1)) {
                score += 30;
            }
            if found == 0 || !name[found - 1].is_ascii_alphanumeric() {
                score += 20;
            }
            previous = Some(found);
            position = found + 1;
        }
        Some(score)
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
/// Char indices of `name` matching `letters` (lowercase) in order, earliest
/// first, as drawn.
pub(crate) fn greedy(letters: &[char], name: &str) -> Vec<usize> {
    let mut wanted = letters.iter().peekable();
    name.chars()
        .enumerate()
        .filter(|(_, c)| {
            let matched = wanted.peek().is_some_and(|&&q| {
                c.to_lowercase()
                    .any(|lower| lower == q || (lower == 'ς' && q == 'σ'))
            });
            if matched {
                wanted.next();
            }
            matched
        })
        .map(|(i, _)| i)
        .collect()
}
#[cfg(test)]
#[test]
fn final_sigma_is_highlighted_for_either_sigma() {
    assert_eq!(greedy(&['σ'], "ΟΔΟΣ"), [3]);
    assert_eq!(greedy(&['ο', 'σ'], "οδος"), [0, 3]);
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
