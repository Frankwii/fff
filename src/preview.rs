//! The preview pane (Alt+v, `P`): the highlighted entry's first lines, or a
//! summary of what it is. Text only, so it looks the same in any terminal.
//!
//! Reading never slows the list: it starts on a thread of its own once the
//! highlight has rested for `DWELL`, is bounded in bytes, lines and time, and
//! moving on cancels it (killing a helper program). The frame shows whatever
//! has arrived for the highlighted entry, and nothing else. A read stuck on a
//! slow mount cannot be interrupted, so at most `READERS` run at once.
use std::{
    collections::VecDeque,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc::{self, Receiver, Sender, SyncSender},
    },
    time::{Duration, Instant, SystemTime},
};

/// How long the highlight rests before its preview is read.
pub const DWELL: Duration = Duration::from_millis(100);
/// Bytes read from the start of a file.
const HEAD: usize = 64 * 1024;
/// Lines kept of a text, and entries of a directory or archive.
const LINES: usize = 300;
/// Characters kept of a line; the pane cuts it to its width.
const LINE_CHARS: usize = 400;
/// Directory entries read before sorting; more are counted as "N+".
const ENTRIES_READ: usize = 5000;
/// How long a helper program (ffprobe, pdfinfo, pdftotext) may take.
const TOOL_TIME: Duration = Duration::from_secs(3);
/// Previews kept for entries visited again.
const CACHE: usize = 32;
/// Reads still running when another waits for one to end: moving on cannot
/// stop a read stuck on a slow mount, only leave it behind.
const READERS: usize = 16;
/// How long a text waits for its syntax colors before it is shown plain;
/// the colors follow when they come, up to `COLORS_LATE`.
const COLORS_WAIT: Duration = Duration::from_millis(50);
const COLORS_LATE: Duration = Duration::from_secs(3);

/// How a line is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Text,
    Muted,
    Directory,
}

/// What the pane shows for one entry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Preview {
    /// What it is, e.g. `PNG image · 1920 × 1080`, in the pane's title.
    pub kind: String,
    /// Facts first, as (label, value): pages, duration, streams, size, ...
    pub facts: Vec<(String, String)>,
    /// Then content: a text's lines, a folder's or an archive's entries.
    pub lines: Vec<(String, Tone)>,
    /// A text file's syntax colors, by line, once they have arrived.
    pub syntax: crate::highlight::Lines,
    /// The lines are the start of a text file, which can have colors.
    pub(crate) text_file: bool,
}
impl Preview {
    fn fact(&mut self, label: &str, value: impl Into<String>) {
        let value = value.into();
        if !value.trim().is_empty() {
            self.facts.push((label.into(), clean(&value)));
        }
    }
    fn note(&mut self, text: &str) {
        self.lines.push((text.into(), Tone::Muted));
    }
}

/// The entry a preview belongs to; a change to its size or modification time,
/// or to whether hidden entries are shown, makes it a different one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub path: PathBuf,
    pub size: u64,
    pub modified: Option<SystemTime>,
    pub hidden: bool,
}

/// The pane's state: whether it is shown, the entry it follows and what has
/// arrived for it.
pub struct Pane {
    pub shown: bool,
    /// `[ui] highlight`: color the syntax of text files.
    pub highlight: bool,
    target: Option<Key>,
    since: Option<Instant>,
    requested: bool,
    /// The followed entry's read waits for one of `READERS` to end.
    waiting: bool,
    /// Reads still running, cancelled or not.
    reading: Arc<AtomicUsize>,
    cancel: Arc<AtomicBool>,
    current: Option<Arc<Preview>>,
    cache: VecDeque<(Key, Arc<Preview>)>,
    /// Content lines scrolled past (Ctrl+j / Ctrl+k), from the top.
    scroll: usize,
    /// Content lines the last frame had room for, and the most it could
    /// scroll: set while drawing.
    pub(crate) room: std::cell::Cell<(usize, usize)>,
    /// A contents query: the match the pane shows, and the line its view starts
    /// at once drawn: the match a third of the way down at first, then
    /// wherever scrolling took it.
    showing_match: Option<u64>,
    pub(crate) match_top: std::cell::Cell<Option<usize>>,
    /// The match's view was scrolled since it was shown.
    match_scrolled: bool,
    /// Long lines wrap onto the next rows (the default) instead of being cut.
    pub wrap: bool,
    sender: Sender<(Key, Preview)>,
    results: Receiver<(Key, Preview)>,
    wake: Option<SyncSender<()>>,
}
impl Default for Pane {
    fn default() -> Self {
        let (sender, results) = mpsc::channel();
        Self {
            shown: false,
            highlight: false,
            target: None,
            since: None,
            requested: false,
            waiting: false,
            reading: Arc::default(),
            cancel: Arc::new(AtomicBool::new(false)),
            current: None,
            cache: VecDeque::new(),
            scroll: 0,
            room: Default::default(),
            showing_match: None,
            match_top: Default::default(),
            match_scrolled: false,
            wrap: true,
            sender,
            results,
            wake: None,
        }
    }
}
impl Pane {
    /// Coalesced notification for arriving previews, shared with input.
    pub fn set_waker(&mut self, wake: SyncSender<()>) {
        self.wake = Some(wake);
    }
    /// Follows `target` (the highlighted entry): takes arrived previews,
    /// starts reading once the highlight has rested, and cancels reading for
    /// an entry left. Returns whether what the pane shows changed.
    pub fn tick(&mut self, target: Option<Key>, now: Instant) -> bool {
        let mut changed = false;
        while let Ok((key, preview)) = self.results.try_recv() {
            let preview = Arc::new(preview);
            if self.target.as_ref() == Some(&key) {
                self.current = Some(preview.clone());
                changed = true;
            }
            self.cache.retain(|(k, _)| *k != key);
            self.cache.push_back((key, preview));
            if self.cache.len() > CACHE {
                self.cache.pop_front();
            }
        }
        let target = target.filter(|_| self.shown);
        if target != self.target {
            self.cancel.store(true, Ordering::Relaxed);
            self.current = target.as_ref().and_then(|key| {
                self.cache
                    .iter()
                    .find(|(k, _)| k == key)
                    .map(|(_, p)| p.clone())
            });
            self.requested = self.current.is_some();
            self.waiting = false;
            self.target = target;
            self.since = Some(now);
            self.scroll = 0;
            changed = true;
        }
        let free = self.reading.load(Ordering::Relaxed) < READERS;
        // A read that waited starts once another ends, which wakes the loop.
        let due = self.remaining(now).is_some_and(|d| d.is_zero()) || (self.waiting && free);
        if due && let Some(key) = self.target.clone() {
            self.requested = true;
            self.waiting = !free;
            if !free {
                let mut busy = Preview::default();
                busy.note("Waiting for earlier previews to finish reading (a slow disk?)");
                self.current = Some(Arc::new(busy));
                return true;
            }
            self.reading.fetch_add(1, Ordering::Relaxed);
            let reading = Reading(self.reading.clone(), self.wake.clone());
            let cancel = Arc::new(AtomicBool::new(false));
            self.cancel = cancel.clone();
            let sender = self.sender.clone();
            let wake = self.wake.clone();
            // A thread per entry: a read stuck on a slow mount holds up only
            // its own preview.
            let highlight = self.highlight;
            std::thread::spawn(move || {
                let _reading = reading;
                let mut preview = build(&key.path, key.hidden, &cancel);
                let send = |preview: Preview| {
                    if !cancel.load(Ordering::Relaxed)
                        && sender.send((key.clone(), preview)).is_ok()
                        && let Some(wake) = &wake
                    {
                        let _ = wake.try_send(());
                    }
                };
                if highlight && preview.text_file {
                    let lines = preview.lines.iter().map(|(l, _)| l.clone()).collect();
                    let colors = crate::highlight::request(&key.path, lines, cancel.clone());
                    // Shown at once in color when they come soon, else
                    // plain first: the colors never hold the text up.
                    match colors.recv_timeout(COLORS_WAIT) {
                        Ok(syntax) => preview.syntax = syntax,
                        Err(mpsc::RecvTimeoutError::Timeout) => {
                            send(preview.clone());
                            match colors.recv_timeout(COLORS_LATE) {
                                Ok(syntax) => preview.syntax = syntax,
                                Err(_) => return,
                            }
                        }
                        Err(mpsc::RecvTimeoutError::Disconnected) => {}
                    }
                }
                send(preview);
            });
        }
        changed
    }
    /// The entry followed while the pane is shown.
    pub fn followed(&self) -> Option<&Key> {
        self.target.as_ref()
    }
    /// How long until reading starts, while it is still to start.
    pub fn remaining(&self, now: Instant) -> Option<Duration> {
        if self.requested || self.target.is_none() {
            return None;
        }
        self.since
            .map(|since| DWELL.saturating_sub(now.saturating_duration_since(since)))
    }
    /// Shows `preview` for `key` at once, for frame tests.
    #[cfg(test)]
    pub(crate) fn set(&mut self, key: Key, preview: Preview) {
        self.shown = true;
        self.target = Some(key);
        self.requested = true;
        self.current = Some(Arc::new(preview));
    }
    /// Follows the contents query's highlighted match (by an identity of it), or
    /// none: another match starts its view at the match again.
    pub fn follow_match(&mut self, id: Option<u64>) {
        if id != self.showing_match {
            self.showing_match = id;
            self.match_top.set(None);
            self.match_scrolled = false;
        }
    }
    /// The match's file arrived, with more lines than its context: its
    /// view starts at the match again, unless already scrolled.
    pub fn more_match_lines(&mut self) {
        if !self.match_scrolled {
            self.match_top.set(None);
        }
    }
    /// Scrolls the content by half the room the last frame had; false when
    /// it was already at that end.
    pub fn scroll(&mut self, down: bool) -> bool {
        let (room, most) = self.room.get();
        let step = (room / 2).max(1);
        // A match's lines scroll from where the match put them.
        if self.showing_match.is_some()
            && let Some(top) = self.match_top.get()
        {
            let moved = if down {
                (top + step).min(most)
            } else {
                top.saturating_sub(step)
            };
            self.match_top.set(Some(moved));
            self.match_scrolled = true;
            return moved != top;
        }
        let scroll = self.scroll.min(most);
        self.scroll = if down {
            (scroll + step).min(most)
        } else {
            scroll.saturating_sub(step)
        };
        self.scroll != scroll
    }
    /// Content lines scrolled past.
    pub fn scrolled(&self) -> usize {
        self.scroll
    }
    /// The preview of the entry followed, once it has arrived.
    pub fn current(&self) -> Option<&Preview> {
        self.current.as_deref()
    }
}

/// A read running: when it ends, its count drops and the loop wakes, for a
/// read that waited.
struct Reading(Arc<AtomicUsize>, Option<SyncSender<()>>);
impl Drop for Reading {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
        if let Some(wake) = &self.1 {
            let _ = wake.try_send(());
        }
    }
}

/// Reads what the pane shows for `path`. `hidden`: list a folder's dot
/// entries. Stops early, with whatever it has, once `cancel` is set.
pub fn build(path: &Path, hidden: bool, cancel: &AtomicBool) -> Preview {
    let mut preview = Preview::default();
    let Ok(lstat) = fs::symlink_metadata(path) else {
        preview.kind = "missing".into();
        preview.note("This entry no longer exists.");
        return preview;
    };
    // At the top of a trash: where it came from, by its record.
    if crate::trash::bin_of(path)
        .and_then(Path::file_name)
        .is_some_and(|n| n == "trash" || n.as_encoded_bytes().starts_with(b".fff-trash-"))
        && let Ok(origin) = crate::trash::origin(path)
    {
        preview.fact("From", origin.path.to_string_lossy());
        if let Some(when) = origin.deleted {
            preview.fact("Trashed", crate::details::timestamp(when));
        }
    }
    let link = lstat
        .is_symlink()
        .then(|| fs::read_link(path).unwrap_or_default());
    let Ok(meta) = fs::metadata(path) else {
        preview.kind = "broken link".into();
        if let Some(target) = &link {
            preview.fact("Link", format!("→ {}", target.to_string_lossy()));
        }
        preview.note("The link's target does not exist.");
        return preview;
    };
    let kind = meta.file_type();
    if meta.is_dir() {
        directory(path, hidden, cancel, &mut preview);
    } else if kind.is_fifo() {
        // Opening a pipe would wait for a writer: never read one.
        preview.kind = "named pipe".into();
    } else if kind.is_socket() {
        preview.kind = "socket".into();
    } else if kind.is_char_device() {
        preview.kind = "character device".into();
    } else if kind.is_block_device() {
        preview.kind = "block device".into();
    } else {
        file(path, &meta, cancel, &mut preview);
    }
    // Size, dates, permissions and a link's target are in the footer below.
    if preview.facts.is_empty() && preview.lines.is_empty() {
        preview.note("Nothing to show as text; the footer has its size and dates.");
    }
    preview
}

/// A folder's entries, folders first, then by name.
fn directory(path: &Path, hidden: bool, cancel: &AtomicBool, preview: &mut Preview) {
    let Ok(entries) = fs::read_dir(path) else {
        preview.kind = "directory".into();
        preview.note("This directory cannot be read.");
        return;
    };
    let mut names = Vec::new();
    let mut more = false;
    for entry in entries.flatten() {
        if cancel.load(Ordering::Relaxed) {
            return;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if !hidden && name.starts_with('.') {
            continue;
        }
        if names.len() == ENTRIES_READ {
            more = true;
            break;
        }
        let is_dir = entry
            .file_type()
            .is_ok_and(|t| t.is_dir() || (t.is_symlink() && entry.path().is_dir()));
        names.push((name, is_dir));
    }
    names.sort_by(|(a, ad), (b, bd)| {
        bd.cmp(ad)
            .then_with(|| a.to_lowercase().cmp(&b.to_lowercase()))
    });
    preview.kind = match (names.len(), more) {
        (_, true) => format!("directory · {}+ entries", names.len()),
        (1, _) => "directory · 1 entry".into(),
        (n, _) => format!("directory · {n} entries"),
    };
    if names.is_empty() {
        preview.note("Empty directory.");
    }
    for (name, is_dir) in names.into_iter().take(LINES) {
        preview.lines.push(if is_dir {
            (clean(&name) + "/", Tone::Directory)
        } else {
            (clean(&name), Tone::Text)
        });
    }
}

/// A regular file: what its first bytes say it is.
fn file(path: &Path, meta: &fs::Metadata, cancel: &AtomicBool, preview: &mut Preview) {
    let head = match read_head(path) {
        Ok(head) => head,
        Err(error) => {
            preview.kind = "file".into();
            preview.note(&format!("Cannot be read: {error}"));
            return;
        }
    };
    if head.is_empty() {
        preview.kind = "empty file".into();
        return;
    }
    match sniff(&head, path) {
        Some(Sniff::Pdf) => pdf(path, &head, cancel, preview),
        Some(Sniff::Image(format)) => image(path, format, &head, cancel, preview),
        Some(Sniff::Media(format)) => media(path, format, cancel, preview),
        Some(Sniff::Archive(format)) => archive(path, format, cancel, preview),
        Some(Sniff::Elf) => elf(&head, meta, preview),
        Some(Sniff::Sqlite) => {
            preview.kind = "SQLite 3 database".into();
            // The page size follows the 16-byte magic, if the file goes on.
            if let Some(&[high, low]) = head.get(16..18) {
                let size = u16::from_be_bytes([high, low]);
                let page = if size == 1 { 65536 } else { size as u64 };
                if let Some(pages) = meta.len().checked_div(page) {
                    preview.fact("Pages", pages.to_string());
                }
            }
        }
        Some(Sniff::Other(name)) => preview.kind = name.into(),
        None => match text(&head) {
            Some(text) => lines(
                &text,
                head.len() < HEAD || meta.len() <= HEAD as u64,
                preview,
            ),
            None => preview.kind = "binary data".into(),
        },
    }
}

fn read_head(path: &Path) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::with_capacity(HEAD);
    File::open(path)?.take(HEAD as u64).read_to_end(&mut head)?;
    Ok(head)
}

/// What a file is, from its first bytes (and, for zip-based documents, its
/// extension).
#[derive(Debug, PartialEq, Eq)]
enum Sniff {
    Pdf,
    Image(&'static str),
    Media(&'static str),
    Archive(&'static str),
    Elf,
    Sqlite,
    Other(&'static str),
}
/// An MPEG audio frame header: its sync bits, then a version, layer,
/// bitrate and sample rate that exist. UTF-16LE text, whose byte-order mark
/// `FF FE` has the sync bits, is not one.
fn mpeg_frame(head: &[u8]) -> bool {
    let [0xff, b, c, ..] = *head else {
        return false;
    };
    let (version, layer) = ((b >> 3) & 3, (b >> 1) & 3);
    let (bitrate, rate) = (c >> 4, (c >> 2) & 3);
    b & 0xe0 == 0xe0 && b != 0xfe && version != 1 && layer != 0 && bitrate != 15 && rate != 3
}
fn sniff(head: &[u8], path: &Path) -> Option<Sniff> {
    let at = |offset: usize, magic: &[u8]| head.get(offset..offset + magic.len()) == Some(magic);
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    Some(if at(0, b"%PDF-") {
        Sniff::Pdf
    } else if at(0, b"\x89PNG\r\n\x1a\n") {
        Sniff::Image("PNG")
    } else if at(0, b"\xff\xd8\xff") {
        Sniff::Image("JPEG")
    } else if at(0, b"GIF87a") || at(0, b"GIF89a") {
        Sniff::Image("GIF")
    } else if at(0, b"RIFF") && at(8, b"WEBP") {
        Sniff::Image("WebP")
    } else if at(0, b"BM") && head.len() > 26 && at(14, &[40, 0, 0, 0]) {
        Sniff::Image("BMP")
    } else if at(0, b"II*\0") || at(0, b"MM\0*") {
        Sniff::Image("TIFF")
    } else if at(0, &[0, 0, 1, 0]) && matches!(extension.as_str(), "ico") {
        Sniff::Image("icon")
    } else if at(4, b"ftyp") {
        match head.get(8..12) {
            Some(b"avif" | b"avis") => Sniff::Image("AVIF"),
            Some(b"heic" | b"heix" | b"mif1" | b"msf1") => Sniff::Image("HEIF"),
            Some(b"M4A " | b"M4B ") => Sniff::Media("MPEG-4 audio"),
            Some(b"qt  ") => Sniff::Media("QuickTime"),
            _ => Sniff::Media("MPEG-4"),
        }
    } else if at(0, b"\x1a\x45\xdf\xa3") {
        Sniff::Media(if extension == "webm" {
            "WebM"
        } else {
            "Matroska"
        })
    } else if at(0, b"ID3") || mpeg_frame(head) {
        Sniff::Media("MP3")
    } else if at(0, b"fLaC") {
        Sniff::Media("FLAC")
    } else if at(0, b"OggS") {
        Sniff::Media("Ogg")
    } else if at(0, b"RIFF") && at(8, b"WAVE") {
        Sniff::Media("WAV")
    } else if at(0, b"RIFF") && at(8, b"AVI ") {
        Sniff::Media("AVI")
    } else if at(0, b"FORM") && (at(8, b"AIFF") || at(8, b"AIFC")) {
        Sniff::Media("AIFF")
    } else if at(0, b"FLV\x01") {
        Sniff::Media("Flash video")
    } else if at(0, &[0x30, 0x26, 0xb2, 0x75]) {
        Sniff::Media("Windows Media")
    } else if at(0, b"MThd") {
        Sniff::Other("MIDI music")
    } else if at(0, b"PK\x03\x04") || at(0, b"PK\x05\x06") {
        Sniff::Archive(match extension.as_str() {
            "docx" => "Word document",
            "xlsx" => "Excel workbook",
            "pptx" => "PowerPoint presentation",
            "odt" => "OpenDocument text",
            "ods" => "OpenDocument spreadsheet",
            "odp" => "OpenDocument presentation",
            "epub" => "EPUB book",
            "jar" => "Java archive",
            "apk" => "Android package",
            _ => "zip archive",
        })
    } else if at(257, b"ustar") {
        Sniff::Archive("tar archive")
    } else if at(0, b"\x1f\x8b") {
        Sniff::Archive("gzip-compressed")
    } else if at(0, b"\xfd7zXZ\0") {
        Sniff::Archive("xz-compressed")
    } else if at(0, b"\x28\xb5\x2f\xfd") {
        Sniff::Archive("zstd-compressed")
    } else if at(0, b"BZh") && head.len() > 3 && head[3].is_ascii_digit() {
        Sniff::Archive("bzip2-compressed")
    } else if at(0, b"7z\xbc\xaf\x27\x1c") {
        Sniff::Archive("7-Zip archive")
    } else if at(0, b"Rar!\x1a\x07") {
        Sniff::Archive("RAR archive")
    } else if at(0, b"!<arch>\n") {
        Sniff::Archive(if extension == "deb" {
            "Debian package"
        } else {
            "ar archive"
        })
    } else if at(0, b"070701") || at(0, b"070707") {
        Sniff::Archive("cpio archive")
    } else if at(0x8001, b"CD001") {
        Sniff::Archive("ISO 9660 disk image")
    } else if at(0, b"\x7fELF") {
        Sniff::Elf
    } else if at(0, b"SQLite format 3\0") {
        Sniff::Sqlite
    } else if at(0, b"MZ") {
        Sniff::Other("Windows executable")
    } else if at(0, b"\0asm") {
        Sniff::Other("WebAssembly module")
    } else if at(0, &[0xca, 0xfe, 0xba, 0xbe]) {
        Sniff::Other("Java class or Mach-O universal binary")
    } else if at(0, &[0xcf, 0xfa, 0xed, 0xfe]) || at(0, &[0xce, 0xfa, 0xed, 0xfe]) {
        Sniff::Other("Mach-O binary")
    } else if at(0, b"OTTO") || at(0, &[0, 1, 0, 0]) && matches!(extension.as_str(), "ttf" | "otf")
    {
        Sniff::Other("font")
    } else if at(0, b"wOFF") || at(0, b"wOF2") {
        Sniff::Other("web font")
    } else if at(0, b"\x89HDF\r\n\x1a\n") {
        Sniff::Other("HDF5 data")
    } else if at(0, b"PAR1") {
        Sniff::Other("Parquet data")
    } else if at(0, b"\x93NUMPY") {
        Sniff::Other("NumPy array")
    } else {
        return None;
    })
}

/// The start of a file as text, or `None` when it looks binary: any NUL, or
/// more than one character in twenty that is invalid or a control character.
fn text(head: &[u8]) -> Option<String> {
    if head.contains(&0) {
        return None;
    }
    // A character cut by the read limit is not an error.
    let head = match std::str::from_utf8(head) {
        Err(e) if e.error_len().is_none() => &head[..e.valid_up_to()],
        _ => head,
    };
    let text = String::from_utf8_lossy(head);
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    let total = text.chars().count();
    let bad = text
        .chars()
        .filter(|&c| {
            c == '\u{fffd}'
                || (c.is_control() && !matches!(c, '\n' | '\t' | '\r' | '\x0c' | '\x1b'))
        })
        .count();
    (bad * 20 <= total).then(|| text.to_owned())
}
/// A text's first lines; `whole` when the read reached the end.
fn lines(text: &str, whole: bool, preview: &mut Preview) {
    preview.text_file = true;
    let mut count = 0;
    for line in text.split_inclusive('\n') {
        count += 1;
        if preview.lines.len() < LINES {
            preview.lines.push((clean(line), Tone::Text));
        }
    }
    // A last line cut by the read limit is still shown, but not counted.
    preview.kind = match (whole, count) {
        (true, 1) => "text · 1 line".into(),
        (true, n) => format!("text · {n} lines"),
        (false, _) => format!("text · first {} KiB", HEAD / 1024),
    };
}

/// One line made safe to draw: no line ending, tabs expanded to every fourth
/// column, other control characters as their Unicode pictures, and long lines
/// cut.
pub fn clean(line: &str) -> String {
    let line = line.trim_end_matches(['\n', '\r']);
    let mut out = String::new();
    let mut column = 0;
    for c in line.chars().take(LINE_CHARS) {
        match c {
            '\t' => {
                let spaces = 4 - column % 4;
                out.extend(std::iter::repeat_n(' ', spaces));
                column += spaces;
            }
            c if (c as u32) < 0x20 => {
                out.push(char::from_u32(0x2400 + c as u32).unwrap_or('?'));
                column += 1;
            }
            '\x7f' => {
                out.push('\u{2421}');
                column += 1;
            }
            c if c.is_control() => {
                out.push('\u{fffd}');
                column += 1;
            }
            c => {
                out.push(c);
                column += unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
            }
        }
    }
    out
}

/// A helper program's failure: not installed, or no usable answer.
#[derive(Debug, PartialEq, Eq)]
enum ToolError {
    Missing,
    Failed,
}
/// Runs `program` without a shell, with no terminal (stdin and stderr closed),
/// and returns the first `HEAD` bytes of its output. It is killed once
/// `cancel` is set or after `TOOL_TIME`.
fn tool(
    program: &str,
    args: &[&std::ffi::OsStr],
    cancel: &AtomicBool,
) -> Result<String, ToolError> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => ToolError::Missing,
            _ => ToolError::Failed,
        })?;
    let mut stdout = child.stdout.take().ok_or(ToolError::Failed)?;
    // Output past the limit is read and dropped, so the program can finish.
    let reader = std::thread::spawn(move || {
        let mut kept = Vec::new();
        let _ = (&mut stdout).take(HEAD as u64).read_to_end(&mut kept);
        let _ = std::io::copy(&mut stdout, &mut std::io::sink());
        kept
    });
    let deadline = Instant::now() + TOOL_TIME;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if cancel.load(Ordering::Relaxed) || Instant::now() > deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ToolError::Failed);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => return Err(ToolError::Failed),
        }
    }
    let out = reader.join().map_err(|_| ToolError::Failed)?;
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// `pdfinfo`'s facts and `pdftotext`'s text of the first pages (poppler).
fn pdf(path: &Path, head: &[u8], cancel: &AtomicBool, preview: &mut Preview) {
    let version = head
        .get(5..8)
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .unwrap_or_default();
    preview.kind = "PDF document".into();
    let info = tool(
        "pdfinfo",
        &["-enc".as_ref(), "UTF-8".as_ref(), path.as_os_str()],
        cancel,
    );
    match info {
        Err(ToolError::Missing) => {
            preview.fact("Version", version);
            preview.note("Install poppler (pdfinfo, pdftotext) to see pages and text.");
            return;
        }
        Err(ToolError::Failed) => preview.fact("Version", version),
        Ok(info) => {
            let facts = pdfinfo(&info);
            if let Some((_, pages)) = facts.iter().find(|(k, _)| k == "Pages") {
                preview.kind = match pages.as_str() {
                    "1" => "PDF document · 1 page".into(),
                    n => format!("PDF document · {n} pages"),
                };
            }
            for (label, value) in facts {
                preview.fact(&label, value);
            }
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return;
    }
    let args = [
        "-f".as_ref(),
        "1".as_ref(),
        "-l".as_ref(),
        "3".as_ref(),
        "-enc".as_ref(),
        "UTF-8".as_ref(),
        "-q".as_ref(),
        path.as_os_str(),
        "-".as_ref(),
    ];
    if let Ok(text) = tool("pdftotext", &args, cancel) {
        pdf_text(&text, preview);
    }
}
/// The facts worth showing from `pdfinfo` output, in order.
fn pdfinfo(output: &str) -> Vec<(String, String)> {
    let mut facts = Vec::new();
    for wanted in [
        "Title",
        "Author",
        "Subject",
        "Pages",
        "Page size",
        "Creator",
        "Producer",
        "Encrypted",
    ] {
        let value = output.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == wanted).then(|| value.trim().to_owned())
        });
        match value {
            Some(v) if wanted == "Encrypted" && v.starts_with("no") => {}
            Some(v) if !v.is_empty() => facts.push((wanted.to_owned(), v)),
            _ => {}
        }
    }
    facts
}
/// Pages are separated by form feeds; runs of blank lines are squeezed.
fn pdf_text(text: &str, preview: &mut Preview) {
    let pages: Vec<&str> = text.split('\x0c').collect();
    let mut blank = true;
    for (number, page) in pages.iter().enumerate() {
        if page.trim().is_empty() {
            continue;
        }
        if number > 0 {
            preview.lines.push((String::new(), Tone::Text));
            preview
                .lines
                .push((format!("── page {} ──", number + 1), Tone::Muted));
            blank = true;
        }
        for line in page.lines() {
            let empty = line.trim().is_empty();
            if !(empty && blank) {
                preview.lines.push((clean(line), Tone::Text));
            }
            blank = empty;
            if preview.lines.len() >= LINES {
                return;
            }
        }
    }
}

/// An image's dimensions, read from its header where that is simple, else
/// from `ffprobe`.
fn image(path: &Path, format: &str, head: &[u8], cancel: &AtomicBool, preview: &mut Preview) {
    let u16le = |i: usize| {
        head.get(i..i + 2)
            .map(|b| u16::from_le_bytes([b[0], b[1]]) as u32)
    };
    let u32be = |i: usize| {
        head.get(i..i + 4)
            .map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    };
    let u24le = |i: usize| {
        head.get(i..i + 3)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], 0]))
    };
    let i32le = |i: usize| {
        head.get(i..i + 4)
            .map(|b| i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    };
    let size = match format {
        "PNG" => u32be(16).zip(u32be(20)),
        "GIF" => u16le(6).zip(u16le(8)),
        "BMP" => i32le(18)
            .zip(i32le(22))
            .map(|(w, h)| (w.unsigned_abs(), h.unsigned_abs())),
        "JPEG" => jpeg_size(path),
        "WebP" => match head.get(12..16) {
            Some(b"VP8 ") => u16le(26)
                .zip(u16le(28))
                .map(|(w, h)| (w & 0x3fff, h & 0x3fff)),
            Some(b"VP8L") => head.get(21..25).map(|b| {
                let bits = u32::from_le_bytes([b[0], b[1], b[2], b[3]]);
                (1 + (bits & 0x3fff), 1 + ((bits >> 14) & 0x3fff))
            }),
            Some(b"VP8X") => u24le(24).zip(u24le(27)).map(|(w, h)| (w + 1, h + 1)),
            _ => None,
        },
        _ => None,
    };
    preview.kind = format!("{format} image");
    match size {
        Some((w, h)) => {
            preview.kind = format!("{format} image · {w} × {h}");
            if format == "PNG"
                && let (Some(&depth), Some(&color)) = (head.get(24), head.get(25))
            {
                let color = match color {
                    0 => "grayscale",
                    2 => "RGB",
                    3 => "indexed",
                    4 => "grayscale + alpha",
                    6 => "RGBA",
                    _ => "",
                };
                preview.fact("Color", format!("{color}, {depth}-bit"));
            }
        }
        None => {
            // ffprobe also reads TIFF, AVIF, HEIF and icons.
            if let Ok(probe) = ffprobe(path, cancel)
                && let Some(video) = probe
                    .streams
                    .iter()
                    .find(|s| s.get("codec_type") == Some("video"))
                && let (Some(w), Some(h)) = (video.get("width"), video.get("height"))
            {
                preview.kind = format!("{format} image · {w} × {h}");
            }
        }
    }
}
/// A JPEG's dimensions from its frame header, walking the segments before
/// it (an EXIF thumbnail can push it far from the start).
fn jpeg_size(path: &Path) -> Option<(u32, u32)> {
    let mut file = File::open(path).ok()?;
    let mut position = 2u64;
    for _ in 0..1000 {
        file.seek(SeekFrom::Start(position)).ok()?;
        let mut marker = [0u8; 9];
        file.read_exact(&mut marker[..4]).ok()?;
        if marker[0] != 0xff {
            return None;
        }
        let kind = marker[1];
        let length = u16::from_be_bytes([marker[2], marker[3]]) as u64;
        // Start-of-frame markers, except DHT (c4), JPG (c8) and DAC (cc).
        if (0xc0..=0xcf).contains(&kind) && !matches!(kind, 0xc4 | 0xc8 | 0xcc) {
            file.read_exact(&mut marker[4..9]).ok()?;
            let height = u16::from_be_bytes([marker[5], marker[6]]) as u32;
            let width = u16::from_be_bytes([marker[7], marker[8]]) as u32;
            return Some((width, height));
        }
        position += 2 + length;
        if position > 64 * 1024 * 1024 {
            return None;
        }
    }
    None
}

/// What `ffprobe` reports: the container's fields and tags, then each stream's.
#[derive(Debug, Default)]
struct Probe {
    format: Fields,
    streams: Vec<Fields>,
}
#[derive(Debug, Default)]
struct Fields(Vec<(String, String)>);
impl Fields {
    fn get(&self, key: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
            .filter(|v| !v.is_empty() && *v != "N/A")
    }
}
fn ffprobe(path: &Path, cancel: &AtomicBool) -> Result<Probe, ToolError> {
    // `file:` keeps ffprobe from reading a name as another protocol.
    let mut target = std::ffi::OsString::from("file:");
    target.push(path.as_os_str());
    let entries = "format=duration,bit_rate\
        :format_tags=title,artist,album,date\
        :stream=codec_type,codec_name,width,height,avg_frame_rate,sample_rate,channels\
        :stream_disposition=attached_pic";
    let output = tool(
        "ffprobe",
        &[
            "-v".as_ref(),
            "error".as_ref(),
            "-show_entries".as_ref(),
            entries.as_ref(),
            "-of".as_ref(),
            "default".as_ref(),
            target.as_os_str(),
        ],
        cancel,
    )?;
    Ok(parse_probe(&output))
}
fn parse_probe(output: &str) -> Probe {
    let mut probe = Probe::default();
    let mut section: Option<&mut Fields> = None;
    for line in output.lines() {
        match line.trim() {
            "[STREAM]" => {
                probe.streams.push(Fields::default());
                section = probe.streams.last_mut();
            }
            "[FORMAT]" => section = Some(&mut probe.format),
            l if l.starts_with("[/") => section = None,
            l => {
                if let (Some(fields), Some((key, value))) =
                    (section.as_deref_mut(), l.split_once('='))
                {
                    let key = key
                        .trim_start_matches("TAG:")
                        .trim_start_matches("DISPOSITION:");
                    fields.0.push((key.to_lowercase(), value.to_owned()));
                }
            }
        }
    }
    probe
}
/// Audio and video: duration, streams and tags from `ffprobe`.
fn media(path: &Path, format: &str, cancel: &AtomicBool, preview: &mut Preview) {
    preview.kind = format.into();
    let probe = match ffprobe(path, cancel) {
        Ok(probe) => probe,
        Err(ToolError::Missing) => {
            preview.note("Install ffprobe (FFmpeg) to see duration and streams.");
            return;
        }
        Err(ToolError::Failed) => return,
    };
    let video: Vec<&Fields> = probe
        .streams
        .iter()
        .filter(|s| s.get("codec_type") == Some("video") && s.get("attached_pic") != Some("1"))
        .collect();
    let audio = probe
        .streams
        .iter()
        .filter(|s| s.get("codec_type") == Some("audio"));
    // The sniffed name is shorter than ffprobe's ("MP2/3 (MPEG audio layer 2/3)").

    preview.kind = match video.first() {
        Some(v) => match (v.get("width"), v.get("height")) {
            (Some(w), Some(h)) => format!("{format} video · {w} × {h}"),
            _ => format!("{format} video"),
        },
        None => format!("{format} audio"),
    };
    if let Some(seconds) = probe
        .format
        .get("duration")
        .and_then(|d| d.parse::<f64>().ok())
    {
        preview.fact("Duration", duration(seconds));
    }
    for key in ["title", "artist", "album", "date"] {
        if let Some(value) = probe.format.get(key) {
            let mut label = key.to_owned();
            label[..1].make_ascii_uppercase();
            preview.fact(&label, value);
        }
    }
    for v in &video {
        let mut parts = vec![v.get("codec_name").unwrap_or("?").to_owned()];
        if let (Some(w), Some(h)) = (v.get("width"), v.get("height")) {
            parts.push(format!("{w} × {h}"));
        }
        if let Some(fps) = v.get("avg_frame_rate").and_then(rate) {
            parts.push(format!("{fps} fps"));
        }
        preview.fact("Video", parts.join(" · "));
    }
    for a in audio {
        let mut parts = vec![a.get("codec_name").unwrap_or("?").to_owned()];
        if let Some(rate) = a.get("sample_rate").and_then(|r| r.parse::<f64>().ok()) {
            parts.push(format!("{} kHz", trim(rate / 1000.0)));
        }
        match a.get("channels") {
            Some("1") => parts.push("mono".into()),
            Some("2") => parts.push("stereo".into()),
            Some(n) => parts.push(format!("{n} channels")),
            None => {}
        }
        preview.fact("Audio", parts.join(" · "));
    }
    let subtitles = probe
        .streams
        .iter()
        .filter(|s| s.get("codec_type") == Some("subtitle"))
        .count();
    if subtitles > 0 {
        preview.fact(
            "Subtitles",
            format!("{subtitles} track{}", if subtitles == 1 { "" } else { "s" }),
        );
    }
    if let Some(cover) = probe
        .streams
        .iter()
        .find(|s| s.get("attached_pic") == Some("1"))
        && let (Some(w), Some(h)) = (cover.get("width"), cover.get("height"))
    {
        preview.fact("Cover art", format!("{w} × {h}"));
    }
    if let Some(bits) = probe
        .format
        .get("bit_rate")
        .and_then(|b| b.parse::<f64>().ok())
    {
        preview.fact("Bit rate", format!("{} kb/s", (bits / 1000.0).round()));
    }
}
/// `1:02:03`, `3:25` or `0:07`.
fn duration(seconds: f64) -> String {
    let total = seconds.round() as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}
/// A frame rate written as a fraction, `30000/1001` → `29.97`.
fn rate(fraction: &str) -> Option<String> {
    let (n, d) = fraction.split_once('/')?;
    let (n, d) = (n.parse::<f64>().ok()?, d.parse::<f64>().ok()?);
    (d > 0.0 && n > 0.0).then(|| trim(n / d))
}
/// Two decimals at most, without trailing zeros.
fn trim(value: f64) -> String {
    let text = format!("{value:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// An archive's first entries, read through libarchive without extracting.
fn archive(path: &Path, format: &str, cancel: &AtomicBool, preview: &mut Preview) {
    preview.kind = format.into();
    let Ok(file) = File::open(path) else {
        return;
    };
    let stop = || cancel.load(Ordering::Relaxed);
    // A compressed file that is not a tar is a single stream, not a listing.
    let Ok((entries, more)) = crate::archive::list(file, LINES, &stop) else {
        return;
    };
    let count = entries.len();
    preview.kind = match (more, count) {
        (true, n) => format!("{format} · {n}+ entries"),
        (false, 1) => format!("{format} · 1 entry"),
        (false, n) => format!("{format} · {n} entries"),
    };
    // Sizes are what the archive declares, so they may be absurd.
    let total = entries
        .iter()
        .fold(0u64, |total, (_, _, size)| total.saturating_add(*size));
    if !more && total > 0 {
        preview.fact("Unpacked", crate::details::size(total));
    }
    for (name, is_dir, _) in entries {
        let name = clean(&name.to_string_lossy());
        preview.lines.push(if is_dir {
            (name.trim_end_matches('/').to_owned() + "/", Tone::Directory)
        } else {
            (name, Tone::Text)
        });
    }
}

/// ELF binaries: class, kind and machine from the header.
fn elf(head: &[u8], meta: &fs::Metadata, preview: &mut Preview) {
    let little = head.get(5) == Some(&1);
    let u16at = |i: usize| {
        head.get(i..i + 2).map(|b| {
            if little {
                u16::from_le_bytes([b[0], b[1]])
            } else {
                u16::from_be_bytes([b[0], b[1]])
            }
        })
    };
    let bits = match head.get(4) {
        Some(1) => "32-bit",
        Some(2) => "64-bit",
        _ => "",
    };
    // Position-independent executables are shared objects too; the
    // executable bit tells them from libraries.
    let kind = match u16at(16) {
        Some(1) => "relocatable object",
        Some(2) => "executable",
        Some(3) if meta.mode() & 0o111 != 0 => "executable",
        Some(3) => "shared library",
        Some(4) => "core dump",
        _ => "file",
    };
    let machine = match u16at(18) {
        Some(3) => "x86",
        Some(62) => "x86-64",
        Some(40) => "ARM",
        Some(183) => "AArch64",
        Some(243) => "RISC-V",
        Some(8) => "MIPS",
        Some(20) => "PowerPC",
        Some(21) => "PowerPC 64",
        _ => "",
    };
    preview.kind = format!("ELF {bits} {kind}").replace("  ", " ");
    preview.fact("Machine", machine);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fff-preview-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
    fn preview(path: &Path) -> Preview {
        build(path, false, &AtomicBool::new(false))
    }
    fn texts(p: &Preview) -> Vec<&str> {
        p.lines.iter().map(|(l, _)| l.as_str()).collect()
    }

    #[test]
    fn text_is_shown_safely_and_binary_is_summarized() {
        let dir = scratch("text");
        let file = dir.join("a.rs");
        fs::write(&file, "fn main() {\r\n\tlet x\t= 1;\x1b[31m\n}\n").unwrap();
        let p = preview(&file);
        assert_eq!(p.kind, "text · 3 lines");
        assert_eq!(texts(&p), ["fn main() {", "    let x   = 1;␛[31m", "}"]);
        // Size and dates are in the footer, not repeated here.
        assert!(p.facts.is_empty());
        let long = dir.join("long");
        fs::write(&long, "x".repeat(HEAD + 10)).unwrap();
        assert_eq!(preview(&long).kind, "text · first 64 KiB");
        assert_eq!(preview(&long).lines[0].0.chars().count(), LINE_CHARS);
        // A file of exactly 64 KiB was read whole.
        fs::write(&long, "x\n".repeat(HEAD / 2)).unwrap();
        assert_eq!(preview(&long).kind, format!("text · {} lines", HEAD / 2));
        // A database cut short after its magic has no page size to read.
        let db = dir.join("db");
        fs::write(&db, b"SQLite format 3\0").unwrap();
        assert_eq!(preview(&db).kind, "SQLite 3 database");
        fs::write(
            &db,
            [&b"SQLite format 3\0"[..], &[16, 0], &[0; 4094]].concat(),
        )
        .unwrap();
        assert_eq!(preview(&db).facts, [("Pages".to_string(), "1".to_string())]);
        let binary = dir.join("blob");
        fs::write(&binary, b"\x00\x01\x02garbage").unwrap();
        let p = preview(&binary);
        assert_eq!(p.kind, "binary data");
        assert!(p.facts.is_empty());
        assert_eq!(p.lines[0].1, Tone::Muted);
        fs::write(&binary, b"").unwrap();
        assert_eq!(preview(&binary).kind, "empty file");
        // UTF-16 text starts like an MPEG frame, which it is not.
        fs::write(&binary, b"\xff\xfeH\x00i\x00\n\x00").unwrap();
        assert_eq!(preview(&binary).kind, "binary data");
        assert!(mpeg_frame(b"\xff\xfb\x90\x64"));
        assert!(!mpeg_frame(b"\xff\xfb\xf0\x64"));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_list_entries_folders_first_and_pipes_are_never_opened() {
        let dir = scratch("dir");
        fs::create_dir(dir.join("zeta")).unwrap();
        for name in ["b", "A", ".hidden"] {
            fs::write(dir.join(name), "").unwrap();
        }
        let p = preview(&dir);
        assert_eq!(p.kind, "directory · 3 entries");
        assert_eq!(texts(&p), ["zeta/", "A", "b"]);
        assert_eq!(p.lines[0].1, Tone::Directory);
        assert_eq!(build(&dir, true, &AtomicBool::new(false)).lines.len(), 4);
        assert_eq!(
            preview(&dir.join("zeta")).lines,
            [("Empty directory.".into(), Tone::Muted)]
        );
        let fifo = dir.join("fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        assert_eq!(preview(&fifo).kind, "named pipe");
        std::os::unix::fs::symlink("nowhere", dir.join("broken")).unwrap();
        let p = preview(&dir.join("broken"));
        assert_eq!(
            (p.kind.as_str(), p.facts[0].1.as_str()),
            ("broken link", "→ nowhere")
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn images_report_their_size_from_the_header() {
        let dir = scratch("image");
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&1920u32.to_be_bytes());
        png.extend_from_slice(&1080u32.to_be_bytes());
        png.extend_from_slice(&[8, 6, 0, 0, 0]);
        fs::write(dir.join("a.png"), &png).unwrap();
        let p = preview(&dir.join("a.png"));
        assert_eq!(p.kind, "PNG image · 1920 × 1080");
        assert_eq!(p.facts[0], ("Color".into(), "RGBA, 8-bit".into()));
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[10, 0, 20, 0, 0, 0, 0]);
        fs::write(dir.join("a.gif"), &gif).unwrap();
        assert_eq!(preview(&dir.join("a.gif")).kind, "GIF image · 10 × 20");
        // A JPEG's frame header may follow other segments.
        let mut jpeg = vec![0xff, 0xd8, 0xff, 0xe1, 0, 6, 1, 2, 3, 4];
        jpeg.extend_from_slice(&[0xff, 0xc0, 0, 17, 8, 0, 48, 0, 64, 3]);
        fs::write(dir.join("a.jpg"), &jpeg).unwrap();
        assert_eq!(preview(&dir.join("a.jpg")).kind, "JPEG image · 64 × 48");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn tool_output_is_parsed_into_facts() {
        let info = "Title:           Notes\nAuthor:          \nPages:           12\n\
                    Encrypted:       no\nPage size:       595.276 x 841.89 pts (A4)\n";
        assert_eq!(
            pdfinfo(info),
            [
                ("Title".to_string(), "Notes".to_string()),
                ("Pages".into(), "12".into()),
                ("Page size".into(), "595.276 x 841.89 pts (A4)".into()),
            ]
        );
        let mut p = Preview::default();
        pdf_text("Intro\n\n\n\nmore\n\x0cSecond\n\x0c", &mut p);
        assert_eq!(
            texts(&p),
            ["Intro", "", "more", "", "── page 2 ──", "Second"]
        );
        let probe = parse_probe(
            "[STREAM]\ncodec_name=h264\ncodec_type=video\nwidth=1920\nheight=1080\n\
             avg_frame_rate=30000/1001\nDISPOSITION:attached_pic=0\n[/STREAM]\n\
             [STREAM]\ncodec_name=aac\ncodec_type=audio\nsample_rate=48000\nchannels=2\n[/STREAM]\n\
             [FORMAT]\nformat_long_name=Matroska / WebM\nduration=3725.4\nbit_rate=N/A\n\
             TAG:title=Film\n[/FORMAT]\n",
        );
        assert_eq!(probe.streams.len(), 2);
        assert_eq!(probe.format.get("title"), Some("Film"));
        assert_eq!(probe.format.get("bit_rate"), None);
        assert_eq!(duration(3725.4), "1:02:05");
        assert_eq!(duration(7.0), "0:07");
        assert_eq!(rate("30000/1001").as_deref(), Some("29.97"));
        assert_eq!(rate("25/1").as_deref(), Some("25"));
        assert_eq!(rate("0/0"), None);
    }

    #[test]
    fn archives_list_their_entries() {
        let dir = scratch("archive");
        fs::create_dir(dir.join("src")).unwrap();
        fs::write(dir.join("src/a.txt"), "hello").unwrap();
        let tar = dir.join("a.tar.gz");
        let made = Command::new("tar")
            .arg("-czf")
            .arg(&tar)
            .arg("-C")
            .arg(&dir)
            .arg("src")
            .status();
        if !made.is_ok_and(|s| s.success()) {
            return;
        }
        let p = preview(&tar);
        assert_eq!(p.kind, "gzip-compressed · 2 entries");
        assert_eq!(texts(&p), ["src/", "src/a.txt"]);
        assert_eq!(p.facts[0], ("Unpacked".into(), crate::details::size(5)));
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_pane_waits_for_the_highlight_to_rest_and_keeps_recent_previews() {
        let dir = scratch("pane");
        let file = dir.join("a");
        fs::write(&file, "one\n").unwrap();
        let key = |path: &Path| Key {
            path: path.to_owned(),
            size: 4,
            modified: None,
            hidden: false,
        };
        let mut pane = Pane {
            shown: true,
            ..Pane::default()
        };
        let start = Instant::now();
        assert!(pane.tick(Some(key(&file)), start));
        assert_eq!(pane.remaining(start), Some(DWELL));
        assert!(pane.current().is_none());
        // Once rested, the preview is read in the background.
        pane.tick(Some(key(&file)), start + DWELL);
        assert_eq!(pane.remaining(start + DWELL), None);
        let deadline = Instant::now() + Duration::from_secs(5);
        while pane.current().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            pane.tick(Some(key(&file)), start + DWELL);
        }
        assert_eq!(pane.current().unwrap().lines[0].0, "one");
        // Moving away and back shows the kept preview at once.
        pane.tick(Some(key(&dir)), start + DWELL);
        assert!(pane.current().is_none());
        pane.tick(Some(key(&file)), start + DWELL);
        assert!(pane.current().is_some());
        assert_eq!(pane.remaining(start + DWELL), None);
        // Hidden, it follows nothing.
        pane.shown = false;
        pane.tick(Some(key(&file)), start + DWELL);
        assert!(pane.current().is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn reads_stuck_on_a_slow_disk_hold_back_new_ones_until_one_ends() {
        let dir = scratch("stuck");
        fs::write(dir.join("a"), "one\n").unwrap();
        let key = Key {
            path: dir.join("a"),
            size: 4,
            modified: None,
            hidden: false,
        };
        let mut pane = Pane {
            shown: true,
            ..Pane::default()
        };
        pane.reading.store(READERS, Ordering::Relaxed);
        let start = Instant::now();
        pane.tick(Some(key.clone()), start);
        assert!(pane.tick(Some(key.clone()), start + DWELL));
        assert!(pane.current().unwrap().lines[0].0.starts_with("Waiting"));
        // Nothing to do until a read ends: no busy loop.
        assert_eq!(pane.remaining(start + DWELL), None);
        assert!(!pane.tick(Some(key.clone()), start + DWELL));
        pane.reading.fetch_sub(1, Ordering::Relaxed);
        pane.tick(Some(key.clone()), start + DWELL);
        let deadline = Instant::now() + Duration::from_secs(5);
        while pane.current().unwrap().lines[0].0 != "one" && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
            pane.tick(Some(key.clone()), start + DWELL);
        }
        assert_eq!(pane.current().unwrap().lines[0].0, "one");
        fs::remove_dir_all(dir).unwrap();
    }
}
