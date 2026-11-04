//! The contents index: what each file searched holds, by its trigrams, so
//! that a search reads only the files that may match.
//!
//! Every file a contents search reads (a text file, or a document's text
//! from the cache) gets an entry, keyed by the file's identity and version:
//! device, inode, size, and modification and change times, so that an
//! edited file is another entry and nothing needs invalidating. An entry is
//! a Bloom filter of the text's trigrams, taken from runs of letters and
//! digits folded as the query matches them (case, Latin accents, a word
//! hyphenated at a line end joined), or a mark that the file is binary. A
//! file is read only when the filter holds every trigram of the query: a
//! filter answers "maybe" or "no", so no match is missed.
//!
//! On disk (`$XDG_CACHE_HOME/fff/index/`), `main` holds entries sorted by
//! key, its table read at once and its filters as needed; sessions add
//! segments (`new-*`), merged into `main` under a lock once they grow.
//! Merging also prunes: an entry goes after a few months unused, each after
//! its own number of days so that a search's entries do not all go at once,
//! and the least recently used go first while the index is over its size.
//! The text cache is pruned the same way, once a day.
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs,
    io::Write,
    os::unix::fs::{FileExt, MetadataExt},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime},
};

/// Entries unused this long go when the index is merged, and text cache
/// entries when it is pruned; each waits up to half as long again, by a
/// share of its own.
pub const KEEP_DAYS: u32 = 90;
/// The index's size, past which the least recently used entries go.
pub const INDEX_SIZE: u64 = 512 << 20;
/// The text cache's size, past which the least recently used texts go.
pub const TEXT_SIZE: u64 = 2 << 30;
/// An entry used again after this long is marked used today.
const REFRESH_DAYS: u32 = 7;
/// New entries kept in memory before they are written as a segment.
const FLUSH_BYTES: usize = 16 << 20;
/// Segments past this many, or this size, are merged into `main`.
const MERGE_SEGMENTS: usize = 16;
const MERGE_BYTES: u64 = 16 << 20;
/// A file changed this recently may change again within the same clock
/// tick, unseen in its times: it is searched but not indexed.
const SETTLE: Duration = Duration::from_secs(2);
/// Filter sizes, in bits: powers of two.
const MIN_BITS: usize = 512;
const MAX_BITS: usize = 1 << 20;
const MAIN_MAGIC: u64 = u64::from_le_bytes(*b"fffidx01");
const SEGMENT_MAGIC: u64 = u64::from_le_bytes(*b"fffseg01");
/// Words per entry in `main`'s table: the key, the day and kind, where its
/// filter starts and its length.
const ROW: usize = 9;

/// A file's identity and version.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Key([u64; 6]);
impl Key {
    pub fn of(meta: &fs::Metadata) -> Self {
        Self([
            meta.dev(),
            meta.ino(),
            meta.size(),
            meta.mtime() as u64,
            ((meta.mtime_nsec() as u64) << 32) | meta.ctime_nsec() as u64,
            meta.ctime() as u64,
        ])
    }
    /// A share of `KEEP_DAYS / 2` of its own, so that entries used on the
    /// same day are not all dropped on the same day.
    fn grace(&self) -> u32 {
        (mix(self.0.iter().fold(0, |h, w| mix(h ^ w))) % (KEEP_DAYS as u64 / 2 + 1)) as u32
    }
}
/// What the index knows of a file's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    /// Binary: it has no text.
    Binary,
    /// Its trigrams; empty when it has none.
    Filter(Arc<[u64]>),
}
/// How an entry is written: the kinds, and a touch that only marks an
/// entry used.
const BINARY: u64 = 0;
const FILTER: u64 = 1;
const TOUCH: u64 = 2;

fn today() -> u32 {
    days(SystemTime::now())
}
fn days(time: SystemTime) -> u32 {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| (d.as_secs() / 86_400) as u32)
}
/// Whether an entry last used on `used` is kept on `today`.
fn kept(used: u32, grace: u32, today: u32) -> bool {
    today.saturating_sub(used) <= KEEP_DAYS + grace
}

/// A 64-bit mix (MurmurHash3's finalizer).
fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}
fn trigram(a: char, b: char, c: char) -> u64 {
    mix(((a as u64) << 42) | ((b as u64) << 21) | c as u64)
}
/// The forms a letter or digit takes as the query matches it, the same
/// for every character of its simple case folding (as the regex folds
/// case): the ASCII letter among them, lowercase, and the base letter of
/// each (as accents are folded), else the least of them. A query letter's
/// first form is among the forms of every character it matches. Mostly
/// one; two where nucleo's table gives a case's letters different bases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Forms {
    chars: [char; 4],
    len: u8,
}
impl Forms {
    fn one(c: char) -> Self {
        Self {
            chars: [c; 4],
            len: 1,
        }
    }
    fn first(&self) -> char {
        self.chars[0]
    }
    fn iter(&self) -> impl Iterator<Item = char> + '_ {
        self.chars[..self.len as usize].iter().copied()
    }
}
/// The forms of `c`; `None` for anything but letters and digits.
fn forms(c: char) -> Option<Forms> {
    if c.is_ascii() {
        return c
            .is_ascii_alphanumeric()
            .then(|| Forms::one(c.to_ascii_lowercase()));
    }
    thread_local! {
        static FOLDED: RefCell<HashMap<char, Option<Forms>>> = RefCell::new(HashMap::new());
    }
    FOLDED.with(|folded| {
        *folded.borrow_mut().entry(c).or_insert_with(|| {
            use regex_syntax::hir::{ClassUnicode, ClassUnicodeRange};
            let mut class = ClassUnicode::new([ClassUnicodeRange::new(c, c)]);
            let _ = class.try_case_fold_simple();
            let members = || class.ranges().iter().flat_map(|r| r.start()..=r.end());
            let found: Vec<char> = members()
                .filter(char::is_ascii)
                .map(|m| m.to_ascii_lowercase())
                .chain(members().filter_map(crate::grep::base_letter))
                .collect();
            let mut unique = Vec::new();
            for f in found {
                if !unique.contains(&f) {
                    unique.push(f);
                }
            }
            if unique.is_empty() {
                unique.push(members().next().unwrap_or(c));
            }
            if !unique.iter().all(|f| f.is_alphanumeric()) || unique.len() > 4 {
                // Not a letter or digit (more forms than that never occur).
                return None;
            }
            let mut forms = Forms::one(unique[0]);
            for (i, &f) in unique.iter().enumerate() {
                forms.chars[i] = f;
            }
            forms.len = unique.len() as u8;
            Some(forms)
        })
    })
}
/// The trigrams any match of `query` holds: those of each run of letters
/// and digits in it, folded. Sorted, each once.
pub fn trigrams(query: &str) -> Vec<u64> {
    let mut found = Vec::new();
    let mut run = Vec::new();
    for c in query.chars().chain([' ']) {
        match forms(c).filter(|_| c.is_alphanumeric()) {
            Some(forms) => run.push(forms.first()),
            None => {
                found.extend(run.windows(3).map(|w| trigram(w[0], w[1], w[2])));
                run.clear();
            }
        }
    }
    found.sort_unstable();
    found.dedup();
    found
}
/// The filter of `text`'s trigrams. A word hyphenated at a line end, or
/// with a soft hyphen, is one run, as the query finds it whole.
pub fn filter(text: &[u8]) -> Arc<[u64]> {
    let text = String::from_utf8_lossy(text);
    let bits = (text.len() * 2)
        .next_power_of_two()
        .clamp(MIN_BITS, MAX_BITS);
    let mut words = vec![0u64; bits / 64];
    let mask = bits as u64 - 1;
    let mut set = |h: u64| {
        for at in [h & mask, (h >> 32) & mask] {
            words[(at / 64) as usize] |= 1 << (at % 64);
        }
    };
    let mut any = false;
    let (mut a, mut b): (Option<Forms>, Option<Forms>) = (None, None);
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let c = if bytes[i] < 0x80 {
            bytes[i] as char
        } else {
            text[i..].chars().next().expect("a character starts here")
        };
        let next = i + c.len_utf8();
        if let Some(third) = forms(c) {
            if let (Some(a), Some(b)) = (a, b) {
                for x in a.iter() {
                    for y in b.iter() {
                        for z in third.iter() {
                            set(trigram(x, y, z));
                        }
                    }
                }
                any = true;
            }
            (a, b) = (b, Some(third));
            i = next;
            continue;
        }
        if b.is_some() && (c == '-' || c == '\u{ad}') {
            if let Some(after) = line_break(&bytes[next..]) {
                i = next + after;
                continue;
            }
            if c == '\u{ad}' {
                i = next;
                continue;
            }
        }
        (a, b) = (None, None);
        i = next;
    }
    if !any {
        return Arc::from([]);
    }
    // Halve it while it stays sparse: a filter folds onto its first half.
    let mut size = words.len();
    while size * 64 > MIN_BITS {
        let half = size / 2;
        let ones: usize = (0..half)
            .map(|i| (words[i] | words[i + half]).count_ones() as usize)
            .sum();
        if ones * 10 > half * 64 * 3 {
            break;
        }
        for i in 0..half {
            words[i] |= words[i + half];
        }
        size = half;
    }
    words.truncate(size);
    words.into()
}
/// The length of a line break after a hyphen, as the query's pattern
/// takes one: blanks, a newline, then blanks or a form feed.
fn line_break(rest: &[u8]) -> Option<usize> {
    let mut i = rest
        .iter()
        .take_while(|&&b| b == b' ' || b == b'\t')
        .count();
    if rest.get(i) == Some(&b'\r') {
        i += 1;
    }
    if rest.get(i) != Some(&b'\n') {
        return None;
    }
    i += 1;
    Some(
        i + rest[i..]
            .iter()
            .take_while(|&&b| matches!(b, b' ' | b'\t' | 0x0c))
            .count(),
    )
}
/// Whether a text with filter `words` may hold every one of `trigrams`.
pub fn may_match(words: &[u64], trigrams: &[u64]) -> bool {
    if trigrams.is_empty() {
        return true;
    }
    if words.is_empty() {
        return false;
    }
    let mask = words.len() as u64 * 64 - 1;
    trigrams.iter().all(|h| {
        [h & mask, (h >> 32) & mask]
            .iter()
            .all(|at| words[(at / 64) as usize] & (1 << (at % 64)) != 0)
    })
}
/// Whether a file with these times has settled: changed long enough ago
/// that a change now would show in them.
pub fn settled(meta: &fs::Metadata) -> bool {
    let changed = [
        (meta.mtime(), meta.mtime_nsec()),
        (meta.ctime(), meta.ctime_nsec()),
    ]
    .into_iter()
    .max()
    .map(|(s, ns)| SystemTime::UNIX_EPOCH + Duration::new(s.max(0) as u64, ns as u32))
    .unwrap_or(SystemTime::UNIX_EPOCH);
    SystemTime::now()
        .duration_since(changed)
        .is_ok_and(|age| age > SETTLE)
}

/// Little-endian words from `bytes`.
fn words_of(bytes: &[u8]) -> Vec<u64> {
    let (words, _) = bytes.as_chunks::<8>();
    words.iter().map(|w| u64::from_le_bytes(*w)).collect()
}
/// `main`: its table in memory, its filters read when asked for.
#[derive(Default)]
struct Main {
    file: Option<fs::File>,
    table: Vec<u64>,
    /// Where the filters start, in bytes.
    data: u64,
}
impl Main {
    fn open(path: &Path) -> Self {
        Self::read(path).unwrap_or_default()
    }
    fn read(path: &Path) -> Option<Self> {
        let file = fs::File::open(path).ok()?;
        let mut header = [0u8; 16];
        file.read_exact_at(&mut header, 0).ok()?;
        let word = |i: usize| u64::from_le_bytes(header[i * 8..i * 8 + 8].try_into().unwrap());
        let count = word(1) as usize;
        if word(0) != MAIN_MAGIC {
            return None;
        }
        // A damaged file is no index, rather than a vast allocation.
        let length = file.metadata().ok()?.len();
        let table_bytes = count.checked_mul(ROW * 8)?;
        let data = 16u64.checked_add(table_bytes as u64)?;
        if data > length {
            return None;
        }
        let mut bytes = vec![0u8; table_bytes];
        file.read_exact_at(&mut bytes, 16).ok()?;
        let table = words_of(&bytes);
        let filters = length - data;
        let inside = |i: usize| {
            let (at, words) = (table[i * ROW + 7], table[i * ROW + 8]);
            at.checked_add(words)
                .and_then(|end| end.checked_mul(8))
                .is_some_and(|end| end <= filters)
        };
        if !(0..count).all(inside) {
            return None;
        }
        Some(Self {
            file: Some(file),
            table,
            data,
        })
    }
    fn len(&self) -> usize {
        self.table.len() / ROW
    }
    fn key(&self, i: usize) -> Key {
        Key(self.table[i * ROW..i * ROW + 6].try_into().unwrap())
    }
    fn used(&self, i: usize) -> u32 {
        self.table[i * ROW + 6] as u32
    }
    fn kind(&self, i: usize) -> u64 {
        self.table[i * ROW + 6] >> 32
    }
    fn find(&self, key: &Key) -> Option<usize> {
        let (mut low, mut high) = (0, self.len());
        while low < high {
            let middle = (low + high) / 2;
            match self.key(middle).cmp(key) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Some(middle),
            }
        }
        None
    }
    fn words(&self, i: usize) -> Option<Vec<u64>> {
        let (at, length) = (self.table[i * ROW + 7], self.table[i * ROW + 8] as usize);
        let mut bytes = vec![0u8; length * 8];
        self.file
            .as_ref()?
            .read_exact_at(&mut bytes, self.data + at * 8)
            .ok()?;
        Some(words_of(&bytes))
    }
    fn entry(&self, i: usize) -> Option<Entry> {
        match self.kind(i) {
            BINARY => Some(Entry::Binary),
            FILTER => Some(Entry::Filter(self.words(i)?.into())),
            _ => None,
        }
    }
}

/// An entry as a segment holds it.
struct Record {
    key: Key,
    used: u32,
    kind: u64,
    words: Arc<[u64]>,
}
impl Record {
    fn bytes(&self) -> usize {
        (ROW + self.words.len()) * 8
    }
}
fn read_segment(path: &Path) -> Vec<Record> {
    let Ok(bytes) = fs::read(path) else {
        return Vec::new();
    };
    let words: Vec<u64> = words_of(&bytes);
    if words.first() != Some(&SEGMENT_MAGIC) {
        return Vec::new();
    }
    let mut records = Vec::new();
    let mut at = 1;
    while at + 8 <= words.len() {
        let length = words[at + 7] as usize;
        let Some(filter) = (at + 8)
            .checked_add(length)
            .and_then(|end| words.get(at + 8..end))
        else {
            break;
        };
        records.push(Record {
            key: Key(words[at..at + 6].try_into().unwrap()),
            used: words[at + 6] as u32,
            kind: words[at + 6] >> 32,
            words: filter.into(),
        });
        at += 8 + length;
    }
    records
}
/// Write `bytes` to `path` whole or not at all.
fn write_whole(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temporary = dir.join(format!(
        ".{}.{}",
        path.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let written = fs::File::create(&temporary).and_then(|mut f| f.write_all(bytes));
    match written.and_then(|_| fs::rename(&temporary, path)) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = fs::remove_file(&temporary);
            Err(e)
        }
    }
}
fn segments(dir: &Path) -> Vec<(PathBuf, u64)> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("new-"))
        .filter_map(|e| Some((e.path(), e.metadata().ok()?.len())))
        .collect()
}

/// An entry being merged: its key, day, kind and where its filter is.
type Row<F> = (Key, u32, u64, F);
/// Write `main` from `rows`, sorted by key, whole or not at all: a filter
/// that cannot be read stops it, rather than be written as empty.
fn write_main<F>(
    path: &Path,
    rows: &[Row<F>],
    length: impl Fn(&F) -> usize,
    words: impl Fn(&F) -> Option<Arc<[u64]>>,
) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let temporary = dir.join(format!(".main.{}", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut out = std::io::BufWriter::new(fs::File::create(&temporary)?);
        out.write_all(&MAIN_MAGIC.to_le_bytes())?;
        out.write_all(&(rows.len() as u64).to_le_bytes())?;
        let mut at = 0u64;
        for (key, used, kind, from) in rows {
            for word in key.0 {
                out.write_all(&word.to_le_bytes())?;
            }
            let length = length(from) as u64;
            for word in [*used as u64 | kind << 32, at, length] {
                out.write_all(&word.to_le_bytes())?;
            }
            at += length;
        }
        for (_, _, _, from) in rows {
            let filter = words(from)
                .filter(|f| f.len() == length(from))
                .ok_or(std::io::ErrorKind::InvalidData)?;
            for word in filter.iter() {
                out.write_all(&word.to_le_bytes())?;
            }
        }
        out.into_inner().map_err(|e| e.into_error())?;
        fs::rename(&temporary, path)
    };
    write().inspect_err(|_| {
        let _ = fs::remove_file(&temporary);
    })
}
/// The contents index of a text cache, shared by a session's searches.
pub struct Index {
    dir: PathBuf,
    texts: PathBuf,
    main: RwLock<Arc<Main>>,
    /// Entries not in `main`: the segments found on opening, and those
    /// found since, with the day each was last used.
    fresh: RwLock<HashMap<Key, (Entry, u32)>>,
    /// What is to be written in the next segment.
    pending: Mutex<Vec<Record>>,
    pending_bytes: AtomicUsize,
    /// Writing or merging runs on a thread of its own, one at a time.
    busy: AtomicBool,
}
impl Index {
    /// The index of the text cache in `texts`, beside it (`index` next to
    /// `text`). Merging and pruning start in the background when due.
    pub fn open(texts: &Path) -> Arc<Self> {
        let dir = texts.with_file_name("index");
        let main = Main::open(&dir.join("main"));
        let mut fresh = HashMap::new();
        let found = segments(&dir);
        for (path, _) in &found {
            for record in read_segment(path) {
                let entry = match record.kind {
                    BINARY => Entry::Binary,
                    FILTER => Entry::Filter(record.words),
                    _ => continue,
                };
                fresh.insert(record.key, (entry, record.used));
            }
        }
        let index = Arc::new(Self {
            dir,
            texts: texts.to_owned(),
            main: RwLock::new(Arc::new(main)),
            fresh: RwLock::new(fresh),
            pending: Mutex::default(),
            pending_bytes: AtomicUsize::new(0),
            busy: AtomicBool::new(false),
        });
        if index.merge_due() || index.prune_due() {
            index.maintain();
        }
        index
    }
    /// What is known of the file with `key`, marking it used.
    pub fn lookup(self: &Arc<Self>, key: &Key) -> Option<Entry> {
        if let Some((entry, _)) = self.fresh.read().ok()?.get(key) {
            return Some(entry.clone());
        }
        let main = Arc::clone(&*self.main.read().ok()?);
        let i = main.find(key)?;
        let entry = main.entry(i)?;
        let today = today();
        if main.used(i) + REFRESH_DAYS < today {
            self.add(*key, entry.clone(), TOUCH);
        }
        Some(entry)
    }
    /// Remember what the file with `key` holds.
    pub fn record(self: &Arc<Self>, key: Key, entry: Entry) {
        let kind = match entry {
            Entry::Binary => BINARY,
            Entry::Filter(_) => FILTER,
        };
        self.add(key, entry, kind);
    }
    fn add(self: &Arc<Self>, key: Key, entry: Entry, kind: u64) {
        let used = today();
        let words = match (&entry, kind) {
            (Entry::Filter(words), FILTER) => Arc::clone(words),
            _ => Arc::from([]),
        };
        if let Ok(mut fresh) = self.fresh.write() {
            fresh.insert(key, (entry, used));
        }
        let record = Record {
            key,
            used,
            kind,
            words,
        };
        let bytes = record.bytes();
        if let Ok(mut pending) = self.pending.lock() {
            pending.push(record);
        }
        if self.pending_bytes.fetch_add(bytes, Ordering::Relaxed) + bytes > FLUSH_BYTES {
            self.save();
        }
    }
    /// Write what is pending in the background, then merge if due.
    pub fn save(self: &Arc<Self>) {
        if self.pending_bytes.load(Ordering::Relaxed) > 0 {
            self.maintain();
        }
    }
    fn maintain(self: &Arc<Self>) {
        if self.busy.swap(true, Ordering::AcqRel) {
            return;
        }
        let index = Arc::clone(self);
        let spawned = std::thread::Builder::new()
            .name("fff-index".into())
            .spawn(move || {
                index.flush();
                if index.merge_due() || index.prune_due() {
                    index.merge();
                }
                index.busy.store(false, Ordering::Release);
            });
        if spawned.is_err() {
            self.busy.store(false, Ordering::Release);
        }
    }
    /// Write what is pending as a segment.
    fn flush(&self) {
        let records = match self.pending.lock() {
            Ok(mut pending) => std::mem::take(&mut *pending),
            Err(_) => return,
        };
        self.pending_bytes.store(0, Ordering::Relaxed);
        if records.is_empty() {
            return;
        }
        let mut bytes = Vec::with_capacity(8 + records.iter().map(Record::bytes).sum::<usize>());
        bytes.extend(SEGMENT_MAGIC.to_le_bytes());
        for r in &records {
            for word in r.key.0 {
                bytes.extend(word.to_le_bytes());
            }
            bytes.extend((r.used as u64 | r.kind << 32).to_le_bytes());
            bytes.extend((r.words.len() as u64).to_le_bytes());
            for word in r.words.iter() {
                bytes.extend(word.to_le_bytes());
            }
        }
        static COUNT: AtomicUsize = AtomicUsize::new(0);
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            "new-{}-{nanos}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        );
        let _ = fs::create_dir_all(&self.dir);
        let _ = write_whole(&self.dir.join(name), &bytes);
    }
    fn merge_due(&self) -> bool {
        let found = segments(&self.dir);
        found.len() > MERGE_SEGMENTS || found.iter().map(|(_, l)| l).sum::<u64>() > MERGE_BYTES
    }
    /// Pruning is due once a day: `main`'s time says when it last ran.
    fn prune_due(&self) -> bool {
        let merged = fs::metadata(self.dir.join("main")).and_then(|m| m.modified());
        match merged {
            Ok(time) => days(time) < today(),
            Err(_) => !segments(&self.dir).is_empty(),
        }
    }
    /// Merge the segments into `main`, dropping what has gone unused, and
    /// prune the text cache; another session's merge running, skip it.
    fn merge(&self) {
        let _ = fs::create_dir_all(&self.dir);
        let Ok(lock) = fs::File::create(self.dir.join("lock")) else {
            return;
        };
        if rustix::fs::flock(&lock, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err() {
            return;
        }
        let today = today();
        let main = Main::open(&self.dir.join("main"));
        let found = segments(&self.dir);
        // Each key's latest: from `main` at a row, or from a segment.
        enum From {
            Main(usize),
            Segment(Arc<[u64]>),
        }
        let mut merged: HashMap<Key, (u32, u64, From)> = HashMap::with_capacity(main.len());
        for i in 0..main.len() {
            merged.insert(main.key(i), (main.used(i), main.kind(i), From::Main(i)));
        }
        for (path, _) in &found {
            for r in read_segment(path) {
                match (merged.get_mut(&r.key), r.kind) {
                    (Some(entry), TOUCH) => entry.0 = entry.0.max(r.used),
                    (None, TOUCH) => {}
                    (_, kind) => {
                        merged.insert(r.key, (r.used, kind, From::Segment(r.words)));
                    }
                }
            }
        }
        let length = |from: &From| match from {
            From::Main(i) => main.table[i * ROW + 8] as usize,
            From::Segment(words) => words.len(),
        };
        let mut rows: Vec<Row<From>> = merged
            .into_iter()
            .filter(|(key, (used, _, _))| kept(*used, key.grace(), today))
            .map(|(key, (used, kind, from))| (key, used, kind, from))
            .collect();
        // Over its size, the least recently used go first.
        let size = |rows: &[Row<From>]| {
            rows.iter()
                .map(|r| ((ROW + length(&r.3)) * 8) as u64)
                .sum::<u64>()
        };
        if size(&rows) > INDEX_SIZE {
            rows.sort_by_key(|r| std::cmp::Reverse((r.1, r.0.grace())));
            let mut total = 0;
            rows.retain(|r| {
                total += ((ROW + length(&r.3)) * 8) as u64;
                total <= INDEX_SIZE / 5 * 4
            });
        }
        let path = self.dir.join("main");
        if found.is_empty() && rows.len() == main.len() {
            // Nothing to change: only say when pruning ran.
            if let Ok(file) = fs::File::options().append(true).open(&path) {
                let _ = file.set_modified(SystemTime::now());
            }
        } else {
            rows.sort_unstable_by_key(|r| r.0);
            if write_main(&path, &rows, length, |from| match from {
                From::Main(i) => main.words(*i).map(Arc::from),
                From::Segment(words) => Some(Arc::clone(words)),
            })
            .is_err()
            {
                return;
            }
            for (path, _) in &found {
                let _ = fs::remove_file(path);
            }
        }
        // This session reads the new `main`; what it holds beyond it is
        // what is still to be written.
        if let Ok(mut current) = self.main.write() {
            *current = Arc::new(Main::open(&self.dir.join("main")));
        }
        let waiting: HashSet<Key> = self
            .pending
            .lock()
            .map(|p| p.iter().map(|r| r.key).collect())
            .unwrap_or_default();
        if let Ok(mut fresh) = self.fresh.write() {
            fresh.retain(|key, _| waiting.contains(key));
        }
        prune_texts(&self.texts, today);
        // Writes a session left unfinished.
        for entry in fs::read_dir(&self.dir).into_iter().flatten().flatten() {
            let old = entry
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|m| days(m) < today);
            if old && entry.file_name().to_string_lossy().starts_with('.') {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
}
impl Drop for Index {
    fn drop(&mut self) {
        self.flush();
    }
}

/// A text cache entry was read: mark it used today, by its modification
/// time, unless it already was.
pub fn used(file: &fs::File) {
    let Ok(modified) = file.metadata().and_then(|m| m.modified()) else {
        return;
    };
    if days(modified) < today() {
        let _ = file.set_modified(SystemTime::now());
    }
}
/// Drop the texts unused for `KEEP_DAYS` (and each its share of grace),
/// then the least recently used while the cache is over `TEXT_SIZE`, and
/// writes left unfinished.
fn prune_texts(dir: &Path, today: u32) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut texts = Vec::new();
    for entry in entries.flatten() {
        let (Ok(meta), name) = (entry.metadata(), entry.file_name()) else {
            continue;
        };
        let used = meta.modified().map_or(0, days);
        let name = name.to_string_lossy();
        let grace = (mix(name.bytes().fold(0, |h, b| mix(h ^ b as u64)))
            % (KEEP_DAYS as u64 / 2 + 1)) as u32;
        let unfinished = name.starts_with('.') && used < today;
        if unfinished || !kept(used, grace, today) {
            let _ = fs::remove_file(entry.path());
        } else if meta.is_file() {
            texts.push((used, grace, meta.len(), entry.path()));
        }
    }
    let mut total: u64 = texts.iter().map(|t| t.2).sum();
    if total <= TEXT_SIZE {
        return;
    }
    texts.sort();
    for (_, _, length, path) in texts {
        if total <= TEXT_SIZE / 5 * 4 {
            break;
        }
        if fs::remove_file(&path).is_ok() {
            total -= length;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_holds_what_any_match_holds() {
        for (query, text) in [
            ("canard", "A CANARD here"),
            ("evaluacion", "la Evaluación final"),
            ("munoz", "Muñoz"),
            ("explosion", "the explo-\n  sion of"),
            ("explosion", "the explo\u{ad}sion"),
            ("σοφία", "ΣΟΦΊΑ and σοφίας"),
            ("kelvin", "\u{212a}elvin"),
            ("long", "ſ long"),
            ("ab", "ab"),
        ] {
            let found = trigrams(query);
            assert!(
                may_match(&filter(text.as_bytes()), &found),
                "{query} in {text}"
            );
            assert!(
                crate::grep::Pattern::new(query)
                    .unwrap()
                    .matches(text.as_bytes()),
                "{query} matches {text}"
            );
        }
        // A filter says no for most of what is not there.
        let text = "the quick brown fox jumps over the lazy dog ".repeat(50);
        let words = filter(text.as_bytes());
        assert!(!may_match(&words, &trigrams("canard")));
        assert!(!may_match(&filter(b""), &trigrams("canard")));
        // A query without three letters in a row reads every file.
        assert!(may_match(&filter(b""), &trigrams("a b")));
        // Filters stay small for small texts.
        assert_eq!(filter(b"hello world").len(), MIN_BITS / 64);
    }
    /// Every character a query letter matches folds as that letter does,
    /// so that its trigrams are the query's.
    #[test]
    fn letters_fold_as_the_query_matches_them() {
        let queries: Vec<char> = ('a'..='z')
            .chain('A'..='Z')
            .chain("0123456789éÉñÑüßσςΣåÅçøœæıİǅṡẛ".chars())
            .collect();
        let candidates: Vec<char> = (0..0x3000u32)
            .chain(0xfb00..0xfb50)
            .chain(0xff00..0xfff0)
            .filter_map(char::from_u32)
            .collect();
        let letters = |c: &char| c.is_alphanumeric();
        let accented: Vec<char> = ('\u{c0}'..='\u{24f}')
            .chain('\u{370}'..='\u{3ff}')
            .chain('\u{1e00}'..='\u{1eff}')
            .filter(letters)
            .collect();
        let near: Vec<char> = ('\0'..='\u{7f}').chain(accented.iter().copied()).collect();
        let all = queries.iter().map(|&q| (q, &candidates));
        for (q, candidates) in all.chain(accented.iter().map(|&q| (q, &near))) {
            let pattern = crate::grep::Pattern::new(&q.to_string()).unwrap();
            for &t in candidates {
                if pattern.matches(t.to_string().as_bytes()) {
                    let first = forms(q).unwrap().first();
                    let of_t = forms(t).unwrap_or_else(|| panic!("{t:?} matches {q:?}"));
                    assert!(of_t.iter().any(|f| f == first), "{t:?} matches {q:?}");
                }
            }
        }
    }
    #[test]
    fn entries_are_written_merged_and_found_again() {
        let root = std::env::temp_dir().join(format!("fff-index-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let texts = root.join("text");
        fs::create_dir_all(&texts).unwrap();
        let key = |n: u64| Key([1, n, 3, 4, 5, 6]);
        let index = Index::open(&texts);
        index.record(key(1), Entry::Filter(filter(b"canard explosion")));
        index.record(key(2), Entry::Binary);
        assert_eq!(index.lookup(&key(2)), Some(Entry::Binary));
        index.flush();
        drop(index);
        // Another session finds them in the segment, then merged.
        let index = Index::open(&texts);
        while index.busy.load(Ordering::Acquire) {
            std::thread::sleep(Duration::from_millis(5));
        }
        index.merge();
        assert!(segments(&root.join("index")).is_empty());
        assert!(index.fresh.read().unwrap().is_empty());
        let Some(Entry::Filter(words)) = index.lookup(&key(1)) else {
            panic!("the filter was kept");
        };
        assert!(may_match(&words, &trigrams("canard")));
        assert_eq!(index.lookup(&key(2)), Some(Entry::Binary));
        assert_eq!(index.lookup(&key(3)), None);
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn a_damaged_main_is_no_index() {
        let root = std::env::temp_dir().join(format!("fff-index-damaged-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = root.join("main");
        let main = |words: &[u64]| {
            let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
            fs::write(&path, bytes).unwrap();
            Main::open(&path).len()
        };
        // A table that cannot fit, or a filter past the end of the file.
        assert_eq!(main(&[MAIN_MAGIC, u64::MAX / 2]), 0);
        let row = [1, 2, 3, 4, 5, 6, FILTER << 32, 0];
        let mut words = vec![MAIN_MAGIC, 1];
        words.extend(row);
        words.push(u64::MAX / 4);
        assert_eq!(main(&words), 0);
        // The same entry with its one word of filter loads.
        *words.last_mut().unwrap() = 1;
        words.push(0);
        assert_eq!(main(&words), 1);
        fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn unused_entries_go_each_on_its_own_day() {
        let graces: HashSet<u32> = (0..200).map(|n| Key([n, 0, 0, 0, 0, 0]).grace()).collect();
        assert!(graces.len() > 20 && graces.iter().all(|&g| g <= KEEP_DAYS / 2));
        assert!(kept(100, 0, 100 + KEEP_DAYS));
        assert!(!kept(100, 0, 101 + KEEP_DAYS));
        assert!(kept(100, 10, 105 + KEEP_DAYS));
        // The text cache: old entries and unfinished writes go.
        let root = std::env::temp_dir().join(format!("fff-prune-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let old = SystemTime::now() - Duration::from_secs(86_400 * (KEEP_DAYS as u64 * 2));
        for name in ["old.txt", ".unfinished.txt.1", "new.txt"] {
            fs::write(root.join(name), "text").unwrap();
        }
        for name in ["old.txt", ".unfinished.txt.1"] {
            let file = fs::File::options()
                .write(true)
                .open(root.join(name))
                .unwrap();
            file.set_modified(old).unwrap();
        }
        prune_texts(&root, today());
        let left: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|e| e.unwrap().file_name().into_string().unwrap())
            .collect();
        assert_eq!(left, ["new.txt"]);
        fs::remove_dir_all(&root).unwrap();
    }
}
