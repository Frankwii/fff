//! Nerd Font filetype glyphs. Presentation only: icons never affect matching,
//! numbering, navigation, or output. Every glyph is a single Basic Multilingual
//! Plane private-use code point (one terminal column), followed by one space.
use std::path::Path;
use unicode_width::UnicodeWidthStr;

/// Semantic group; the view maps it to a palette role so custom and `NO_COLOR`
/// palettes stay consistent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Directory,
    Code,
    Document,
    Media,
    Archive,
    Config,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Icon {
    pub glyph: char,
    pub group: Group,
}
impl Icon {
    /// Glyph plus separator, as drawn before a name.
    pub fn cell(self) -> String {
        format!("{} ", self.glyph)
    }
}

const FOLDER: char = '\u{f07b}'; // nf-fa-folder
const FILE: char = '\u{f016}'; // nf-fa-file_o
const GIT: char = '\u{e702}'; // nf-dev-git

/// Exact (case-insensitive) basenames checked before extensions.
const NAMES: &[(&str, char, Group)] = &[
    (".gitignore", GIT, Group::Config),
    (".gitattributes", GIT, Group::Config),
    (".gitmodules", GIT, Group::Config),
    ("cargo.toml", '\u{e7a8}', Group::Config),
    ("cargo.lock", '\u{f023}', Group::Config),
    ("dockerfile", '\u{e7b0}', Group::Config),
    ("makefile", '\u{f013}', Group::Config),
    ("license", '\u{f02d}', Group::Document),
    ("readme", '\u{f02d}', Group::Document),
    ("readme.md", '\u{f02d}', Group::Document),
];

/// Lowercase extensions.
const EXTENSIONS: &[(&str, char, Group)] = &[
    ("rs", '\u{e7a8}', Group::Code), // nf-dev-rust
    ("py", '\u{e73c}', Group::Code), // nf-dev-python
    ("js", '\u{e74e}', Group::Code), // nf-dev-javascript
    ("mjs", '\u{e74e}', Group::Code),
    ("ts", '\u{e628}', Group::Code),  // nf-seti-typescript
    ("tsx", '\u{e7ba}', Group::Code), // nf-dev-react
    ("jsx", '\u{e7ba}', Group::Code),
    ("go", '\u{e627}', Group::Code), // nf-seti-go
    ("c", '\u{e61e}', Group::Code),  // nf-seti-c
    ("h", '\u{e61e}', Group::Code),
    ("cpp", '\u{e61d}', Group::Code), // nf-seti-cpp
    ("cc", '\u{e61d}', Group::Code),
    ("hpp", '\u{e61d}', Group::Code),
    ("java", '\u{e738}', Group::Code), // nf-dev-java
    ("lua", '\u{e620}', Group::Code),  // nf-seti-lua
    ("vim", '\u{e7c5}', Group::Code),  // nf-dev-vim
    ("sh", '\u{e795}', Group::Code),   // nf-dev-terminal
    ("bash", '\u{e795}', Group::Code),
    ("zsh", '\u{e795}', Group::Code),
    ("fish", '\u{e795}', Group::Code),
    ("html", '\u{e736}', Group::Code),   // nf-dev-html5
    ("css", '\u{e749}', Group::Code),    // nf-dev-css3
    ("json", '\u{e60b}', Group::Config), // nf-seti-json
    ("toml", '\u{e615}', Group::Config), // nf-seti-config
    ("yaml", '\u{e615}', Group::Config),
    ("yml", '\u{e615}', Group::Config),
    ("ini", '\u{e615}', Group::Config),
    ("lock", '\u{f023}', Group::Config),  // nf-fa-lock
    ("md", '\u{e609}', Group::Document),  // nf-seti-markdown
    ("tex", '\u{f15c}', Group::Document), // nf-fa-file_text
    ("bib", '\u{f02d}', Group::Document), // nf-fa-book
    ("txt", '\u{f15c}', Group::Document),
    ("pdf", '\u{f1c1}', Group::Document), // nf-fa-file_pdf_o
    ("doc", '\u{f1c2}', Group::Document), // nf-fa-file_word_o
    ("docx", '\u{f1c2}', Group::Document),
    ("odt", '\u{f1c2}', Group::Document),
    ("xls", '\u{f1c3}', Group::Document), // nf-fa-file_excel_o
    ("xlsx", '\u{f1c3}', Group::Document),
    ("csv", '\u{f1c3}', Group::Document),
    ("ppt", '\u{f1c4}', Group::Document), // nf-fa-file_powerpoint_o
    ("pptx", '\u{f1c4}', Group::Document),
    ("png", '\u{f1c5}', Group::Media), // nf-fa-file_image_o
    ("jpg", '\u{f1c5}', Group::Media),
    ("jpeg", '\u{f1c5}', Group::Media),
    ("gif", '\u{f1c5}', Group::Media),
    ("svg", '\u{f1c5}', Group::Media),
    ("webp", '\u{f1c5}', Group::Media),
    ("mp3", '\u{f1c7}', Group::Media), // nf-fa-file_audio_o
    ("flac", '\u{f1c7}', Group::Media),
    ("wav", '\u{f1c7}', Group::Media),
    ("ogg", '\u{f1c7}', Group::Media),
    ("mp4", '\u{f1c8}', Group::Media), // nf-fa-file_video_o
    ("mkv", '\u{f1c8}', Group::Media),
    ("webm", '\u{f1c8}', Group::Media),
    ("mov", '\u{f1c8}', Group::Media),
    ("zip", '\u{f1c6}', Group::Archive), // nf-fa-file_archive_o
    ("tar", '\u{f1c6}', Group::Archive),
    ("gz", '\u{f1c6}', Group::Archive),
    ("xz", '\u{f1c6}', Group::Archive),
    ("zst", '\u{f1c6}', Group::Archive),
    ("bz2", '\u{f1c6}', Group::Archive),
    ("7z", '\u{f1c6}', Group::Archive),
];

/// Icon for a path from its name alone; never touches the filesystem.
pub fn icon(path: &Path, directory: bool) -> Icon {
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_lowercase();
    if directory {
        let glyph = if name == ".git" { GIT } else { FOLDER };
        return Icon {
            glyph,
            group: Group::Directory,
        };
    }
    let found = NAMES
        .iter()
        .find(|(n, ..)| *n == name)
        .or_else(|| {
            let extension = name.rsplit_once('.').map(|(_, e)| e)?;
            EXTENSIONS.iter().find(|(e, ..)| *e == extension)
        })
        .map(|&(_, glyph, group)| Icon { glyph, group });
    found.unwrap_or(Icon {
        glyph: FILE,
        group: Group::Other,
    })
}

/// The two-column marker drawn before a result name: an icon, or the plain
/// `▸`/`·` kind markers when icons are disabled.
pub fn marker(path: &Path, directory: bool, enabled: bool) -> (String, Group) {
    if enabled {
        let icon = icon(path, directory);
        (icon.cell(), icon.group)
    } else if directory {
        ("▸ ".into(), Group::Directory)
    } else {
        ("· ".into(), Group::Other)
    }
}

/// Columns used by `marker`; all variants are equal so rows align either way.
pub fn marker_width() -> usize {
    "▸ ".width()
}

#[cfg(test)]
mod tests {
    use super::*;
    use unicode_width::UnicodeWidthChar;

    #[test]
    fn every_glyph_is_a_single_column_bmp_private_use_code_point() {
        let all = NAMES
            .iter()
            .chain(EXTENSIONS)
            .map(|(_, glyph, _)| *glyph)
            .chain([FOLDER, FILE, GIT]);
        for glyph in all {
            assert!(('\u{e000}'..='\u{f8ff}').contains(&glyph), "{glyph:?}");
            assert_eq!(glyph.width(), Some(1), "{glyph:?}");
        }
        for (path, directory) in [("a.rs", false), ("dir", true), ("x", false)] {
            for enabled in [true, false] {
                let (cell, _) = marker(Path::new(path), directory, enabled);
                assert_eq!(cell.width(), marker_width());
            }
        }
    }

    #[test]
    fn names_extensions_case_and_directories() {
        let of = |p: &str, d: bool| icon(Path::new(p), d);
        assert_eq!(of("src/main.rs", false).glyph, '\u{e7a8}');
        assert_eq!(of("Paper.PDF", false).glyph, '\u{f1c1}');
        assert_eq!(of("archive.tar.gz", false).group, Group::Archive);
        assert_eq!(of("Makefile", false).group, Group::Config);
        assert_eq!(of("README.md", false).glyph, '\u{f02d}');
        assert_eq!(of(".gitignore", false).glyph, GIT);
        assert_eq!(of("notes", false).glyph, FILE);
        assert_eq!(of("weird.", false).glyph, FILE);
        // Directory status wins over a file-like name.
        assert_eq!(of("site.html", true).glyph, FOLDER);
        assert_eq!(of("repo/.git", true).glyph, GIT);
        assert_eq!(marker(Path::new("d"), true, false).0, "▸ ");
        assert_eq!(marker(Path::new("f"), false, false).0, "· ");
    }
}
