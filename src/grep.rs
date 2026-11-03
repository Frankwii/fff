//! `fff grep`: the files whose contents match a second query, one row per
//! match.
//!
//! Text files are read as they are. PDF, DjVu and EPUB documents are read
//! through text extracted once into a cache (`$XDG_CACHE_HOME/fff/text/`),
//! one file per document named from its device, inode, size and modification
//! time, so that an edited file is a new entry and nothing needs
//! invalidating. `pdftotext` and `djvutxt` separate pages with a form feed,
//! which gives each match its page.
//!
//! Searching happens here, on worker threads, in the order of the rows: no
//! program runs per search. A new query stops the old search, but an
//! extraction already running finishes into the cache, since the next query
//! needs the same text.
use crate::Candidate;
use regex_automata::meta::Regex;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::Read,
    ops::Range,
    os::unix::{fs::MetadataExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    time::{Duration, Instant},
};

/// Matches kept per file; a file with more shows these.
const PER_FILE: usize = 200;
/// Matches kept per search, over every file.
const TOTAL: usize = 20_000;
/// Lines kept on each side of a match, for the preview.
pub const CONTEXT: usize = 8;
/// Characters kept of a line around its match.
const LINE_CHARS: usize = 400;
/// Text files larger than this are not searched.
const TEXT_LIMIT: u64 = 64 << 20;
/// Bytes of a text file checked for a NUL, which marks it binary.
const SNIFF: usize = 8192;
/// How long an extraction may take before it is given up.
const EXTRACT_TIME: Duration = Duration::from_secs(120);
/// Extractions run at this niceness, below the interface.
const NICENESS: i32 = 10;
/// Worker threads across searches; a stopped search's workers finish the
/// file they hold.
static WORKERS: AtomicUsize = AtomicUsize::new(0);

/// One match: where it is and the lines around it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    /// Its page, in a document whose text has pages (1 for the first).
    pub page: Option<u32>,
    /// How far into the text it is, in a document without pages (EPUB),
    /// whose lines mean nothing to a reader.
    pub percent: Option<u8>,
    /// How far into the text it is, 0 to 100, in any file.
    pub position: u8,
    /// Its line in the text (1 for the first).
    pub line: u32,
    /// Lines around the match, `at` being the match's own, which keeps the
    /// part around the match when long.
    pub context: Vec<String>,
    pub at: usize,
    /// The match's bytes within `context[at]`, ending there when the match
    /// goes on to the next line.
    pub matched: Range<usize>,
    /// The rest of a match that goes on over line breaks: its bytes within
    /// each following context line.
    pub continued: Vec<(usize, Range<usize>)>,
}
impl Hit {
    /// The match's line with those it goes on to, joined by a space and
    /// with leading space dropped, and the match's parts within it.
    pub fn joined(&self) -> (String, Vec<Range<usize>>) {
        let own = &self.context[self.at];
        let lead = own.len() - own.trim_start().len();
        let mut text = own[lead..].to_owned();
        let mut parts = Vec::with_capacity(1 + self.continued.len());
        parts.push(self.matched.start.max(lead) - lead..self.matched.end.max(lead) - lead);
        for (i, part) in &self.continued {
            let line = &self.context[*i];
            let lead = line.len() - line.trim_start().len();
            text.push(' ');
            let base = text.len();
            text.push_str(&line[lead..]);
            parts.push(base + part.start.max(lead) - lead..base + part.end.max(lead) - lead);
        }
        (text, parts)
    }
    /// The text that matched, as it reads: its parts on each line, joined
    /// by a space.
    pub fn matched_text(&self) -> String {
        let (text, parts) = self.joined();
        let parts: Vec<&str> = parts.iter().filter_map(|p| text.get(p.clone())).collect();
        parts.join(" ")
    }
    /// The match's parts on context line `i`.
    pub fn parts(&self, i: usize) -> Vec<Range<usize>> {
        if i == self.at {
            return vec![self.matched.clone()];
        }
        self.continued
            .iter()
            .filter(|(line, _)| *line == i)
            .map(|(_, part)| part.clone())
            .collect()
    }
    /// Where it is, as rows and the preview show it: `p. 3`, `42%` or `l. 12`.
    pub fn location(&self) -> String {
        match (self.page, self.percent) {
            (Some(page), _) => format!("p. {page}"),
            (None, Some(percent)) => format!("{percent}%"),
            (None, None) => format!("l. {}", self.line),
        }
    }
}

/// What a file is searched as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Pdf,
    Djvu,
    Epub,
    Text,
}
fn kind(path: &Path) -> Kind {
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase());
    match extension.as_deref() {
        Some("pdf") => Kind::Pdf,
        Some("djvu" | "djv") => Kind::Djvu,
        Some("epub") => Kind::Epub,
        _ => Kind::Text,
    }
}

/// Why a file has no text to search.
#[derive(Debug, PartialEq, Eq)]
enum Skip {
    /// Unreadable, binary, too large, or its extraction failed.
    NoText,
    /// The program that extracts it is not installed.
    Missing(&'static str),
}
/// How a text's matches say where they are.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Units {
    /// Form feeds separate its pages (PDF, DjVu).
    Pages,
    /// A position through it (EPUB).
    Percent,
    /// Its lines (text files).
    Lines,
}
/// A file's text, and how its matches are placed.
struct Text {
    bytes: Vec<u8>,
    units: Units,
}

/// The extracted-text cache: `$XDG_CACHE_HOME/fff/text`, else
/// `~/.cache/fff/text`.
pub fn cache_dir() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .filter(|h| !h.is_empty())
                .map(|h| PathBuf::from(h).join(".cache"))
        })?;
    Some(base.join("fff/text"))
}
/// A document's cache entry: its identity and version, so that an edited
/// or replaced file is another entry.
fn cache_name(meta: &fs::Metadata) -> String {
    let nanos = meta.mtime() as i128 * 1_000_000_000 + meta.mtime_nsec() as i128;
    format!(
        "{:x}-{:x}-{:x}-{:x}.txt",
        meta.dev(),
        meta.ino(),
        meta.size(),
        nanos
    )
}

/// The text of `path`: read, or for a document taken from the cache in
/// `cache`, extracting it there first.
fn text_of(path: &Path, cache: Option<&Path>) -> Result<Text, Skip> {
    let kind = kind(path);
    let meta = fs::metadata(path).map_err(|_| Skip::NoText)?;
    if !meta.is_file() {
        return Err(Skip::NoText);
    }
    if kind == Kind::Text {
        if meta.len() > TEXT_LIMIT {
            return Err(Skip::NoText);
        }
        // The start tells a binary file, before reading the rest of it.
        let mut file = fs::File::open(path).map_err(|_| Skip::NoText)?;
        let mut bytes = Vec::with_capacity(meta.len() as usize);
        (&mut file)
            .take(SNIFF as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| Skip::NoText)?;
        if memchr::memchr(0, &bytes).is_some() {
            return Err(Skip::NoText);
        }
        file.read_to_end(&mut bytes).map_err(|_| Skip::NoText)?;
        return Ok(Text {
            bytes,
            units: Units::Lines,
        });
    }
    let units = if kind == Kind::Epub {
        Units::Percent
    } else {
        Units::Pages
    };
    let entry = cache.map(|dir| dir.join(cache_name(&meta)));
    if let Some(entry) = &entry
        && let Ok(bytes) = fs::read(entry)
    {
        return Ok(Text { bytes, units });
    }
    let extracted = match kind {
        Kind::Pdf => run_extractor(
            "pdftotext",
            &[
                "-q".as_ref(),
                "-enc".as_ref(),
                "UTF-8".as_ref(),
                path.as_os_str(),
                "-".as_ref(),
            ],
        ),
        Kind::Djvu => run_extractor("djvutxt", &[path.as_os_str()]),
        Kind::Epub => epub_text(path).ok_or(Extraction::Failed),
        Kind::Text => unreachable!("read above"),
    };
    let bytes = match extracted {
        Ok(bytes) => bytes,
        Err(Extraction::Missing(tool)) => return Err(Skip::Missing(tool)),
        // Not cached: a busy machine may let it finish next time.
        Err(Extraction::TimedOut) => return Err(Skip::NoText),
        // Remembered as empty, so it is not tried on every search.
        Err(Extraction::Failed) => Vec::new(),
    };
    if let Some(entry) = &entry {
        store(entry, &bytes);
    }
    Ok(Text { bytes, units })
}
/// Write a cache entry whole or not at all: to a temporary name, then
/// renamed into place.
fn store(entry: &Path, bytes: &[u8]) {
    let Some(dir) = entry.parent() else {
        return;
    };
    let _ = fs::create_dir_all(dir);
    let temporary = dir.join(format!(
        ".{}.{}",
        entry.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    if fs::write(&temporary, bytes).is_ok() && fs::rename(&temporary, entry).is_err() {
        let _ = fs::remove_file(&temporary);
    }
}

#[derive(Debug)]
enum Extraction {
    Missing(&'static str),
    TimedOut,
    Failed,
}
/// Runs an extractor without a shell or a terminal, at low priority, and
/// returns its output; it is killed after `EXTRACT_TIME`.
fn run_extractor(program: &'static str, args: &[&std::ffi::OsStr]) -> Result<Vec<u8>, Extraction> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // SAFETY: setpriority is async-signal-safe and touches only the child.
    unsafe {
        command.pre_exec(|| {
            libc::setpriority(libc::PRIO_PROCESS, 0, NICENESS);
            Ok(())
        });
    }
    let mut child = command.spawn().map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => Extraction::Missing(program),
        _ => Extraction::Failed,
    })?;
    let mut stdout = child.stdout.take().ok_or(Extraction::Failed)?;
    let reader = std::thread::spawn(move || {
        let mut out = Vec::new();
        let _ = stdout.read_to_end(&mut out);
        out
    });
    let deadline = Instant::now() + EXTRACT_TIME;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(Extraction::TimedOut);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => return Err(Extraction::Failed),
        }
    };
    let out = reader.join().map_err(|_| Extraction::Failed)?;
    if status.success() {
        Ok(out)
    } else {
        Err(Extraction::Failed)
    }
}

/// An EPUB's text: its documents in reading order (the spine), tags
/// stripped, one paragraph per line.
fn epub_text(path: &Path) -> Option<Vec<u8>> {
    let mut archive = zip::ZipArchive::new(fs::File::open(path).ok()?).ok()?;
    let mut read = |name: &str| -> Option<String> {
        let mut entry = archive.by_name(name).ok()?;
        let mut text = String::new();
        entry.read_to_string(&mut text).ok()?;
        Some(text)
    };
    let container = read("META-INF/container.xml")?;
    let opf = attribute(&container, "rootfile", "full-path")?;
    let package = read(&opf)?;
    let base = opf.rsplit_once('/').map_or("", |(dir, _)| dir);
    // The manifest names each document; the spine orders them.
    let mut hrefs = HashMap::new();
    for item in tags(&package, "item") {
        if let (Some(id), Some(href)) = (
            attribute(item, "item", "id"),
            attribute(item, "item", "href"),
        ) {
            hrefs.insert(id, href);
        }
    }
    let mut out = String::new();
    for itemref in tags(&package, "itemref") {
        let Some(href) = attribute(itemref, "itemref", "idref").and_then(|id| hrefs.get(&id))
        else {
            continue;
        };
        let name = if base.is_empty() {
            href.clone()
        } else {
            format!("{base}/{href}")
        };
        if let Some(document) = read(&percent_decoded(&name)) {
            out.push_str(&html_text(&document));
            out.push('\n');
        }
    }
    Some(out.into_bytes())
}
/// Each `<name ...>` tag of `xml`, from `<` to `>`.
fn tags<'a>(xml: &'a str, name: &str) -> Vec<&'a str> {
    let open = format!("<{name}");
    let mut found = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        let tag = &rest[at..];
        let after = tag[open.len()..].chars().next();
        let end = tag.find('>').map_or(tag.len(), |e| e + 1);
        if after.is_some_and(|c| c.is_whitespace() || c == '/' || c == '>') {
            found.push(&tag[..end]);
        }
        rest = &tag[end.max(1)..];
    }
    found
}
/// The value of `key` in the first `<tag ...>` of `xml`.
fn attribute(xml: &str, tag: &str, key: &str) -> Option<String> {
    let element = tags(xml, tag).into_iter().next()?;
    let mut rest = element;
    loop {
        let at = rest.find(key)?;
        let before = rest[..at].chars().last();
        let after = rest[at + key.len()..].trim_start();
        rest = &rest[at + key.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(after) = after.strip_prefix('=') else {
            continue;
        };
        let after = after.trim_start();
        let quote = after.chars().next().filter(|q| *q == '"' || *q == '\'')?;
        let value = &after[1..];
        return Some(entities(&value[..value.find(quote)?]));
    }
}
fn percent_decoded(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = text
                .get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(h, 16).ok())
        {
            out.push(byte);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}
/// The text of an (X)HTML document: scripts and styles dropped, a line per
/// block, entities decoded.
fn html_text(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut rest = html;
    let mut skipping: Option<&str> = None;
    while let Some(open) = rest.find('<') {
        if skipping.is_none() {
            out.push_str(&entities(&rest[..open]));
        }
        let tag = &rest[open..];
        let end = tag.find('>').map_or(tag.len(), |e| e + 1);
        let name: String = tag[1..]
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_ascii_lowercase();
        let closing = tag[1..].starts_with('/');
        match (skipping, name.as_str()) {
            (Some(skipped), _) if closing && name == skipped => skipping = None,
            (Some(_), _) => {}
            (None, "script" | "style") if !closing => {
                skipping = Some(if name == "script" { "script" } else { "style" })
            }
            (
                None,
                "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6"
                | "blockquote" | "section" | "title",
            ) => out.push('\n'),
            _ => {}
        }
        rest = &tag[end..];
    }
    if skipping.is_none() {
        out.push_str(&entities(rest));
    }
    out
}
/// `text` with the common character references decoded.
fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        // A reference is short: look for its `;` among the next characters.
        let Some(end) = tail
            .char_indices()
            .take(12)
            .find(|&(_, c)| c == ';')
            .map(|(i, _)| i)
        else {
            out.push('&');
            rest = &tail[1..];
            continue;
        };
        let name = &tail[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &tail[end + 1..];
            }
            None => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The base letter of an accented Latin letter (`é` → `e`, `Ñ` → `n`), by
/// nucleo's table, so that contents fold accents as names do.
fn base_letter(c: char) -> Option<char> {
    static TABLE: OnceLock<HashMap<char, char>> = OnceLock::new();
    if c.is_ascii() {
        return None;
    }
    TABLE
        .get_or_init(|| {
            use nucleo_matcher::{
                Utf32Str,
                pattern::{Atom, AtomKind, CaseMatching, Normalization},
            };
            let mut matcher = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
            let atoms: Vec<(char, Atom)> = ('a'..='z')
                .map(|l| {
                    let needle = l.to_string();
                    let atom = Atom::new(
                        &needle,
                        CaseMatching::Ignore,
                        Normalization::Smart,
                        AtomKind::Exact,
                        false,
                    );
                    (l, atom)
                })
                .collect();
            let latin = ('\u{c0}'..='\u{24f}').chain('\u{1e00}'..='\u{1eff}');
            let mut table = HashMap::new();
            for c in latin.filter(|c| c.is_alphabetic()) {
                let chars = [c];
                for (letter, atom) in &atoms {
                    if atom
                        .score(Utf32Str::Unicode(&chars), &mut matcher)
                        .is_some()
                    {
                        table.insert(c, *letter);
                        break;
                    }
                }
            }
            table
        })
        .get(&c)
        .copied()
}
/// Every accented letter with `letter` (ASCII lowercase) as its base.
fn accented(letter: char) -> Vec<char> {
    let mut found: Vec<char> = ('\u{c0}'..='\u{24f}')
        .chain('\u{1e00}'..='\u{1eff}')
        .filter(|&c| base_letter(c) == Some(letter))
        .collect();
    found.sort_unstable();
    found
}

/// A content query, compiled. It is literal and case-smart (an uppercase
/// letter makes it case-sensitive); Latin accents are ignored unless it has
/// some; whitespace matches any whitespace, line breaks included; and a word
/// broken by a hyphen at a line end still matches.
pub struct Pattern {
    regex: Regex,
}
impl Pattern {
    /// `None` for a query with nothing to search for.
    pub fn new(query: &str) -> Option<Self> {
        let query = query.trim();
        if query.is_empty() {
            return None;
        }
        let sensitive = query.chars().any(char::is_uppercase);
        let fold = !query.chars().any(|c| base_letter(c).is_some());
        // A word broken across lines: a hyphen (or soft hyphen) at the end
        // of a line, or a soft hyphen alone.
        let broken = r"(?:[-\x{ad}][ \t]*\r?\n[ \t\x0c]*|\x{ad})?";
        let mut pattern = String::new();
        let mut previous: Option<char> = None;
        let mut space = false;
        for c in query.chars() {
            if c.is_whitespace() {
                space = true;
                continue;
            }
            if space {
                pattern.push_str(r"\s+");
                space = false;
            } else if previous.is_some_and(char::is_alphanumeric) && c.is_alphanumeric() {
                pattern.push_str(broken);
            }
            if fold && c.is_ascii_alphabetic() {
                let lower = c.to_ascii_lowercase();
                let variants: Vec<char> = accented(lower)
                    .into_iter()
                    .filter(|v| !sensitive || v.is_uppercase() == c.is_uppercase())
                    .collect();
                if variants.is_empty() {
                    pattern.push(c);
                } else {
                    pattern.push('[');
                    pattern.push(c);
                    for v in variants {
                        pattern.push_str(&format!(r"\x{{{:x}}}", v as u32));
                    }
                    pattern.push(']');
                }
            } else if c.is_alphanumeric() {
                pattern.push(c);
            } else {
                pattern.push_str(&format!(r"\x{{{:x}}}", c as u32));
            }
            previous = Some(c);
        }
        let regex = Regex::builder()
            .syntax(
                regex_automata::util::syntax::Config::new()
                    .case_insensitive(!sensitive)
                    .utf8(false),
            )
            .build(&pattern)
            .ok()?;
        Some(Self { regex })
    }
    /// The matches in `text`, up to `PER_FILE`.
    fn hits(&self, text: &[u8], units: Units) -> Vec<Hit> {
        let mut hits = Vec::new();
        // Pages and lines are counted on from the previous match.
        let (mut counted, mut page, mut line) = (0, 1u32, 1u32);
        for found in self.regex.find_iter(text) {
            if hits.len() == PER_FILE {
                break;
            }
            let start = found.start();
            let before = &text[counted..start];
            page += memchr::memchr_iter(0x0c, before).count() as u32;
            line += memchr::memchr_iter(b'\n', before).count() as u32;
            counted = start;
            let mut hit = hit(
                text,
                found.range(),
                (units == Units::Pages).then_some(page),
                line,
            );
            hit.position = (start * 100 / text.len().max(1)) as u8;
            if units == Units::Percent {
                hit.percent = Some(hit.position);
            }
            hits.push(hit);
        }
        hits
    }
}
/// Whether every match of the query `longer` is one of `shorter`'s: it adds
/// characters at the end.
pub fn narrows(shorter: &str, longer: &str) -> bool {
    let shorter = shorter.trim();
    !shorter.is_empty() && longer.trim_start().starts_with(shorter)
}

/// A line of `text`: the bytes from `at` to the next line break or page.
fn line_end(text: &[u8], at: usize) -> usize {
    memchr::memchr2(b'\n', 0x0c, &text[at..]).map_or(text.len(), |e| at + e)
}
fn line_start(text: &[u8], at: usize) -> usize {
    memchr::memrchr2(b'\n', 0x0c, &text[..at]).map_or(0, |s| s + 1)
}
/// A line's text as shown: invalid UTF-8 replaced, a carriage return
/// dropped, control characters (tabs too) as spaces.
fn shown_line(bytes: &[u8]) -> String {
    let mut line = shown(bytes);
    if line.ends_with(' ') && bytes.ends_with(b"\r") {
        line.pop();
    }
    line
}
/// Bytes as shown, one character for each: the same on a line's start as
/// on the whole line, so that offsets carry over.
fn shown(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect()
}
/// The part of `line` kept around `matched`, and the match within it.
fn window(line: &str, matched: Range<usize>) -> (String, Range<usize>) {
    if line.chars().count() <= LINE_CHARS {
        return (line.to_owned(), matched);
    }
    let start = line
        .char_indices()
        .map(|(i, _)| i)
        .filter(|&i| i <= matched.start)
        .rev()
        .nth(LINE_CHARS / 4)
        .unwrap_or(0);
    let kept: String = line[start..].chars().take(LINE_CHARS).collect();
    let end = (matched.end - start).min(kept.len());
    (kept, matched.start - start..end)
}
/// The hit for the match at `range` of `text`.
fn hit(text: &[u8], range: Range<usize>, page: Option<u32>, line: u32) -> Hit {
    let start = line_start(text, range.start);
    let end = line_end(text, range.start);
    // Byte offsets survive lossy decoding only where the text is valid;
    // measure the match on the decoded prefix instead.
    let own = shown_line(&text[start..end]);
    let prefix = shown(&text[start..range.start]).len();
    let matched_len = shown(&text[range.start..range.end.min(end)]).len();
    let (own, matched) = window(&own, prefix..(prefix + matched_len).min(own.len()));
    let mut before = Vec::new();
    let mut at = start;
    while before.len() < CONTEXT && at > 0 && text[at - 1] == b'\n' {
        let s = line_start(text, at - 1);
        before.push(cut(&shown_line(&text[s..at - 1])));
        at = s;
    }
    before.reverse();
    let index = before.len();
    let mut after = Vec::new();
    let mut continued = Vec::new();
    let mut at = end;
    while after.len() < CONTEXT && at < text.len() && text[at] == b'\n' {
        let (s, e) = (at + 1, line_end(text, at + 1));
        let line = cut(&shown_line(&text[s..e]));
        // The match goes on here, after a line break.
        if s < range.end {
            let length = shown(&text[s..range.end.min(e)]).len().min(line.len());
            if length > 0 && line.is_char_boundary(length) {
                continued.push((index + 1 + after.len(), 0..length));
            }
        }
        after.push(line);
        at = e;
    }
    let mut context = before;
    context.push(own);
    context.extend(after);
    Hit {
        page,
        percent: None,
        position: 0,
        line,
        context,
        at: index,
        matched,
        continued,
    }
}
fn cut(line: &str) -> String {
    line.chars().take(LINE_CHARS).collect()
}

/// What a worker found in one file.
enum Found {
    Hits(PathBuf, Arc<[Hit]>),
    /// No text: binary, unreadable, or its extractor (named) is missing.
    Skipped(PathBuf, Option<&'static str>),
}

/// One query's search over the rows.
struct Search {
    query: String,
    pattern: Arc<Pattern>,
    queue: Arc<Mutex<VecDeque<PathBuf>>>,
    /// Every file handed to this search, so that new rows are added once.
    queued: HashSet<PathBuf>,
    cancel: Arc<AtomicBool>,
    found: Receiver<Found>,
    sender: Sender<Found>,
    /// The previous query's results, when this one narrows it: files it
    /// did not match cannot match now.
    narrowing: Option<HashMap<PathBuf, Arc<[Hit]>>>,
    total: usize,
    done: usize,
}
impl Drop for Search {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// `fff grep`'s state: the contents query, its search and the rows it shows.
#[derive(Default)]
pub struct Grep {
    /// The contents query, as typed.
    pub text: String,
    /// The contents query is being typed into (else the names query).
    pub editing: bool,
    search: Option<Search>,
    /// The current query's results by file; empty when it does not match.
    hits: HashMap<PathBuf, Arc<[Hit]>>,
    /// Shown rows: one per match, files in the rows' order, and each one's
    /// match.
    pub shown: Vec<Arc<Candidate>>,
    pub shown_hits: Vec<(Arc<[Hit]>, usize)>,
    /// The rows and results `shown` was built from.
    shown_from: Option<(u64, usize)>,
    generation: usize,
    /// Extractors met that are not installed.
    pub missing: Vec<&'static str>,
    waker: Option<SyncSender<()>>,
    pub cache: Option<PathBuf>,
}
impl Grep {
    pub fn new() -> Self {
        Self {
            editing: true,
            cache: cache_dir(),
            ..Self::default()
        }
    }
    pub fn set_waker(&mut self, waker: SyncSender<()>) {
        self.waker = Some(waker);
    }
    /// A contents query is set: rows are matches, not files.
    pub fn active(&self) -> bool {
        !self.text.trim().is_empty()
    }
    /// Files searched and to search, while a search runs.
    pub fn progress(&self) -> Option<(usize, usize)> {
        let search = self.search.as_ref()?;
        (search.done < search.total).then_some((search.done, search.total))
    }
    /// Catch up: start a search for a new query, hand it rows not yet
    /// searched (`rows`, changed when `rows_version` does), take what the
    /// workers found and rebuild the shown rows. True when they changed.
    pub fn poll(&mut self, rows: &[Arc<Candidate>], rows_version: u64) -> bool {
        if !self.active() {
            let had = !self.shown.is_empty() || self.search.is_some();
            self.search = None;
            self.hits.clear();
            self.shown.clear();
            self.shown_hits.clear();
            self.shown_from = None;
            return had;
        }
        let query = self.text.trim().to_owned();
        if self.search.as_ref().is_none_or(|s| s.query != query) {
            let Some(pattern) = Pattern::new(&query) else {
                return false;
            };
            let previous = self.search.take();
            let narrowing = previous
                .filter(|p| narrows(&p.query, &query))
                .map(|_| std::mem::take(&mut self.hits));
            self.hits.clear();
            let (sender, found) = mpsc::channel();
            self.search = Some(Search {
                query,
                pattern: Arc::new(pattern),
                queue: Arc::default(),
                queued: HashSet::new(),
                cancel: Arc::default(),
                found,
                sender,
                narrowing,
                total: 0,
                done: 0,
            });
            self.shown_from = None;
            self.generation += 1;
        }
        let search = self.search.as_mut().expect("started above");
        let mut changed = false;
        if self
            .shown_from
            .is_none_or(|(version, _)| version != rows_version)
        {
            let mut queue = search.queue.lock().unwrap_or_else(|e| e.into_inner());
            for c in rows.iter().filter(|c| !c.directory) {
                if search.queued.contains(&c.path) {
                    continue;
                }
                search.queued.insert(c.path.clone());
                search.total += 1;
                // A file the shorter query searched without a match cannot
                // match a longer one.
                match search.narrowing.as_ref().and_then(|n| n.get(&c.path)) {
                    Some(hits) if hits.is_empty() => {
                        self.hits.insert(c.path.clone(), Arc::from([]));
                        search.done += 1;
                    }
                    _ => queue.push_back(c.path.clone()),
                }
            }
            changed = true;
        }
        let workers = std::thread::available_parallelism()
            .map_or(1, |n| n.get() / 2)
            .clamp(1, 8);
        let waiting = search.queue.lock().map_or(0, |q| q.len());
        for _ in 0..waiting {
            if WORKERS.load(Ordering::Relaxed) >= workers {
                break;
            }
            WORKERS.fetch_add(1, Ordering::Relaxed);
            let (queue, pattern, cancel, sender) = (
                Arc::clone(&search.queue),
                Arc::clone(&search.pattern),
                Arc::clone(&search.cancel),
                search.sender.clone(),
            );
            let (cache, waker) = (self.cache.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new()
                .name("fff-grep".into())
                .spawn(move || work(&queue, &pattern, &cancel, &sender, cache.as_deref(), waker));
            if spawned.is_err() {
                WORKERS.fetch_sub(1, Ordering::Relaxed);
                break;
            }
        }
        let mut kept: usize = self.hits.values().map(|h| h.len()).sum();
        while let Ok(found) = search.found.try_recv() {
            search.done += 1;
            changed = true;
            match found {
                Found::Hits(path, hits) => {
                    // Past the total, files still count as searched.
                    let room = TOTAL.saturating_sub(kept).min(hits.len());
                    kept += room;
                    let hits = if room < hits.len() {
                        Arc::from(&hits[..room])
                    } else {
                        hits
                    };
                    self.hits.insert(path, hits);
                }
                Found::Skipped(path, missing) => {
                    if let Some(tool) = missing
                        && !self.missing.contains(&tool)
                    {
                        self.missing.push(tool);
                    }
                    self.hits.insert(path, Arc::from([]));
                }
            }
        }
        if changed {
            self.shown.clear();
            self.shown_hits.clear();
            for c in rows {
                if let Some(hits) = self.hits.get(&c.path) {
                    for i in 0..hits.len() {
                        self.shown.push(Arc::clone(c));
                        self.shown_hits.push((Arc::clone(hits), i));
                    }
                }
            }
            self.shown_from = Some((rows_version, self.generation));
        }
        changed
    }
    /// The shown matches and the files they are in.
    pub fn summary(&self) -> (usize, usize) {
        let files = self
            .shown
            .iter()
            .zip(self.shown.iter().skip(1).map(Some).chain([None]))
            .filter(|(c, next)| next.is_none_or(|n| !Arc::ptr_eq(c, n)))
            .count();
        (self.shown.len(), files)
    }
    /// The match shown at row `i`.
    pub fn hit(&self, i: usize) -> Option<&Hit> {
        self.shown_hits.get(i).map(|(hits, at)| &hits[*at])
    }
}
/// A worker: take files from `queue` until it is empty or the search is
/// stopped, and send what each holds.
fn work(
    queue: &Mutex<VecDeque<PathBuf>>,
    pattern: &Pattern,
    cancel: &AtomicBool,
    sender: &Sender<Found>,
    cache: Option<&Path>,
    waker: Option<SyncSender<()>>,
) {
    // The slot frees however the worker ends.
    struct Slot;
    impl Drop for Slot {
        fn drop(&mut self) {
            WORKERS.fetch_sub(1, Ordering::Relaxed);
        }
    }
    let _slot = Slot;
    loop {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let Some(path) = queue.lock().ok().and_then(|mut q| q.pop_front()) else {
            break;
        };
        // A file that trips a bug is skipped rather than stopping the search.
        let read = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            text_of(&path, cache).map(|text| pattern.hits(&text.bytes, text.units))
        }));
        let found = match read {
            Ok(Ok(hits)) => Found::Hits(path, hits.into()),
            Ok(Err(Skip::Missing(tool))) => Found::Skipped(path, Some(tool)),
            Ok(Err(Skip::NoText)) | Err(_) => Found::Skipped(path, None),
        };
        if cancel.load(Ordering::Relaxed) || sender.send(found).is_err() {
            break;
        }
        if let Some(waker) = &waker {
            let _ = waker.try_send(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(query: &str, text: &str) -> Vec<(Option<u32>, u32, String)> {
        Pattern::new(query)
            .unwrap()
            .hits(
                text.as_bytes(),
                if text.contains('\x0c') {
                    Units::Pages
                } else {
                    Units::Lines
                },
            )
            .into_iter()
            .map(|h| {
                let line = &h.context[h.at];
                (h.page, h.line, line[h.matched.clone()].to_owned())
            })
            .collect()
    }

    #[test]
    fn queries_are_case_smart_literals() {
        assert_eq!(found("canard", "A Canard; canard.").len(), 2);
        assert_eq!(found("Canard", "A Canard; canard.").len(), 1);
        // Regex syntax is text.
        assert_eq!(found("a.c", "abc a.c").len(), 1);
        assert_eq!(found("(x)", "(x) x").len(), 1);
        assert!(Pattern::new("   ").is_none());
    }
    #[test]
    fn accents_are_ignored_unless_typed() {
        assert_eq!(found("evaluacion", "Evaluación y evaluacion").len(), 2);
        assert_eq!(found("evaluación", "Evaluación y evaluacion").len(), 1);
        assert_eq!(found("munoz", "Muñoz").len(), 1);
    }
    #[test]
    fn phrases_match_across_line_breaks_and_hyphenation() {
        assert_eq!(found("canard explosion", "a canard\nexplosion").len(), 1);
        assert_eq!(found("explosion", "the explo-\nsion of").len(), 1);
        assert_eq!(found("non-hyperbolic", "non-hyperbolic").len(), 1);
        // Each line holds its part of the match.
        assert_eq!(found("explosion", "explo-\nsion")[0].2, "explo-");
        let hit = &Pattern::new("boundary conditions")
            .unwrap()
            .hits(b"coupled with boundary\n  conditions, i.e.", Units::Lines)[0];
        assert_eq!(hit.continued, [(1, 0..12)]);
        let (line, parts) = hit.joined();
        assert_eq!(line, "coupled with boundary conditions, i.e.");
        let shown: Vec<&str> = parts.iter().map(|p| &line[p.clone()]).collect();
        assert_eq!(shown, ["boundary", "conditions"]);
        // As it reads, for `{match}`; and how far in it is, for `{percent}`.
        assert_eq!(hit.matched_text(), "boundary conditions");
        assert_eq!(hit.position, 32);
    }
    #[test]
    fn matches_know_their_page_and_line() {
        let text = "first\ncanard\x0csecond page\nmore canard\n\x0cthird canard";
        assert_eq!(
            found("canard", text),
            [
                (Some(1), 2, "canard".into()),
                (Some(2), 3, "canard".into()),
                (Some(3), 4, "canard".into()),
            ]
        );
        assert_eq!(found("more", "a\nb\nmore")[0].1, 3);
        // A book without pages places a match by how far into it it is.
        let hit = &Pattern::new("end")
            .unwrap()
            .hits(b"start middle end", Units::Percent)[0];
        assert_eq!(hit.location(), "81%");
    }
    #[test]
    fn a_match_keeps_the_lines_around_it() {
        let text: String = (1..=30).map(|i| format!("line {i}\n")).collect();
        let hits = Pattern::new("line 15")
            .unwrap()
            .hits(text.as_bytes(), Units::Lines);
        let hit = &hits[0];
        assert_eq!(hit.context.len(), 2 * CONTEXT + 1);
        assert_eq!(hit.context[hit.at], "line 15");
        assert_eq!(hit.context[0], format!("line {}", 15 - CONTEXT));
        // Context stops at a page break.
        let hits = Pattern::new("b").unwrap().hits(b"a\x0cb\nc", Units::Pages);
        assert_eq!(hits[0].context, ["b", "c"]);
    }
    #[test]
    fn long_lines_keep_the_part_around_the_match() {
        let line = format!("{}needle{}", "x".repeat(1000), "y".repeat(1000));
        let hit = &Pattern::new("needle")
            .unwrap()
            .hits(line.as_bytes(), Units::Lines)[0];
        let shown = &hit.context[hit.at];
        assert!(shown.chars().count() <= LINE_CHARS);
        assert_eq!(&shown[hit.matched.clone()], "needle");
    }
    #[test]
    fn longer_queries_narrow() {
        assert!(narrows("canard", "canard e"));
        assert!(narrows("can", "canard"));
        assert!(!narrows("canard", "can"));
        assert!(!narrows("", "a"));
    }
    #[test]
    fn entities_decode_beside_any_character() {
        assert_eq!(entities("a &amp; b"), "a & b");
        assert_eq!(entities("caf&#233; &#x41;"), "café A");
        // A bare ampersand before wide characters, as in "R&D · más".
        assert_eq!(entities("R&D ··········"), "R&D ··········");
        assert_eq!(entities("&unknown; x"), "&unknown; x");
    }
    #[test]
    fn epub_documents_read_in_spine_order() {
        let dir = std::env::temp_dir().join(format!("fff-grep-epub-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.epub");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        let mut add = |name: &str, text: &str| {
            zip.start_file(name, options).unwrap();
            std::io::Write::write_all(&mut zip, text.as_bytes()).unwrap();
        };
        add(
            "META-INF/container.xml",
            r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#,
        );
        add(
            "OEBPS/content.opf",
            r#"<package><manifest><item id="b" href="two.xhtml"/><item id="a" href="one.xhtml"/></manifest>
               <spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#,
        );
        add(
            "OEBPS/one.xhtml",
            "<html><head><style>p{}</style></head><body><p>First &amp; foremost</p></body></html>",
        );
        add(
            "OEBPS/two.xhtml",
            "<html><body><p>Second</p><p>caf&#233;</p></body></html>",
        );
        zip.finish().unwrap();
        let text = String::from_utf8(epub_text(&path).unwrap()).unwrap();
        let words: Vec<&str> = text.split_whitespace().collect();
        assert_eq!(words, ["First", "&", "foremost", "Second", "café"]);
        fs::remove_dir_all(&dir).unwrap();
    }
    #[test]
    fn documents_are_cached_by_identity() {
        let dir = std::env::temp_dir().join(format!("fff-grep-cache-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let cache = dir.join("cache");
        // An EPUB whose extraction fails is remembered as empty.
        let broken = dir.join("broken.epub");
        fs::write(&broken, "not a zip").unwrap();
        let text = text_of(&broken, Some(&cache)).unwrap();
        assert!(text.bytes.is_empty());
        let entry = cache.join(cache_name(&fs::metadata(&broken).unwrap()));
        assert!(entry.exists());
        // A cached entry is read instead of extracting.
        fs::write(&entry, "cached words").unwrap();
        assert_eq!(
            text_of(&broken, Some(&cache)).unwrap().bytes,
            b"cached words"
        );
        // Text files are read directly; binary ones have no text.
        fs::write(dir.join("notes.txt"), "plain").unwrap();
        assert_eq!(
            text_of(&dir.join("notes.txt"), Some(&cache)).unwrap().bytes,
            b"plain"
        );
        fs::write(dir.join("blob.bin"), b"a\0b").unwrap();
        assert_eq!(
            text_of(&dir.join("blob.bin"), Some(&cache)).err(),
            Some(Skip::NoText)
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
