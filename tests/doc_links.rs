//! Every link between the README, the guides and the development notes names a file that exists
//! and, after `#`, one of its headings, as GitHub spells their anchors.
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// GitHub's anchor for a heading: lower case, punctuation dropped but for
/// `-` and `_`, spaces made `-`.
fn anchor(heading: &str) -> String {
    heading
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}

/// The anchors of a Markdown file's headings, outside code blocks.
fn anchors(text: &str) -> Vec<String> {
    let mut fenced = false;
    let mut found = vec![];
    for line in text.lines() {
        if line.starts_with("```") {
            fenced = !fenced;
        } else if !fenced && line.starts_with('#') {
            found.push(anchor(line.trim_start_matches('#')));
        }
    }
    found
}

/// The targets of a file's inline links, `[text](target)`.
fn links(text: &str) -> Vec<&str> {
    text.match_indices("](")
        .filter_map(|(at, _)| {
            let rest = &text[at + 2..];
            let end = rest.find(')')?;
            Some(&rest[..end]).filter(|t| !t.contains(char::is_whitespace))
        })
        .collect()
}

#[test]
fn links_between_the_guides_reach_their_files_and_headings() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = vec![root.join("README.md"), root.join("CHANGELOG.md")];
    for folder in ["docs", "docs/dev"] {
        for entry in fs::read_dir(root.join(folder)).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "md") {
                files.push(path);
            }
        }
    }
    let texts: HashMap<PathBuf, String> = files
        .iter()
        .map(|f| (f.canonicalize().unwrap(), fs::read_to_string(f).unwrap()))
        .collect();
    let mut broken = vec![];
    for (file, text) in &texts {
        for target in links(text) {
            if target.contains("://") || target.starts_with("mailto:") {
                continue;
            }
            let (path, fragment) = target.split_once('#').unwrap_or((target, ""));
            let linked = if path.is_empty() {
                file.clone()
            } else {
                match file.parent().unwrap().join(path).canonicalize() {
                    Ok(linked) => linked,
                    Err(_) => {
                        broken.push(format!("{}: {target}", file.display()));
                        continue;
                    }
                }
            };
            if let Some(linked_text) = texts.get(&linked)
                && !fragment.is_empty()
                && !anchors(linked_text).iter().any(|a| a == fragment)
            {
                broken.push(format!("{}: {target}", file.display()));
            }
        }
    }
    broken.sort();
    assert!(broken.is_empty(), "broken links:\n{}", broken.join("\n"));
}
