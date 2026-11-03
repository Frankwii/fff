# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- Content search with alternating filtering (below).
- Using fff while a long file operation runs. Operations and their checks
  already run on another thread with their progress shown and Ctrl+c to stop
  them; meanwhile only Ctrl+c is heard. Keeping navigation, marks and other
  operations usable would need rules for what a second operation may touch and
  how live-update rescans interact. Archive validation before the extraction
  prompt still runs on the interface thread.
- Preview pane follow-up: previewing the marked list's rows from the
  results.
- Follow-ups to the Keys screen (below): programs as commands, the
  `keys.toml` layout, and keys inside choosers.
- Switching modes without restarting (below): from `fff roam` to `fff seek`,
  for example.
- Resizing the font from fff (below).
- Opening folders from other applications. `--window` exists so that fff,
  set up as the desktop's file manager, can be opened from programs such as
  Telegram (opening a download's folder), but Telegram does not open fff yet.
  What remains is to find how it asks (`xdg-open`, the D-Bus FileManager1
  service or the portal's OpenURI) and make that path reach fff.
- An AUR package once the project nears 1.0.

The preview pane stays text only; there will be no image previews: see
[design choices](DEVELOPMENT.md#design-choices).

## The Keys screen, continued

The Keys screen (F1) and `keys.toml` came in 0.14.0. What remains:

- **Programs as commands.** Keys apply per program, and two programs are
  still options: `fff roam --window` and `fff explorer --multiple /
  --directory / --save`. They would become `fff window` and `fff explorer
  open | open-multiple | folder | save PATH`, named as on the Keys screen,
  with the launchers and portal wrapper `fff setup` writes calling them.
- **`keys.toml` itself.** Its layout (one table per mode, scopes as inline
  tables) is a first version, to be settled.
- **Keys inside choosers, prompts and sheets** are fixed for now; they could
  become functions with places of their own.

## Content search with alternating filtering

Today the query filters names. Content search would add a second query that
filters by what files contain, and a key (for example Alt+/) alternates which
of the two queries you are typing into. Both stay active at once: the name
query narrows which files are read, the content query keeps those whose
contents match, so `name: .rs` plus `content: TODO` finds Rust files mentioning
TODO without leaving the browser.

- **One results list.** Matching files appear in the usual list, in the usual
  order, each with its first matching line (number and a trimmed snippet) in
  the row's right-hand column and the match highlighted. Marks, Tab, Enter,
  Ctrl+w, gx/gX and E keep working; E opens `$EDITOR` at the matched line when
  the editor accepts `+N`.
- **Inside documents, not only text.** Searching PDFs matters as much as
  plain text. Text files are searched as they are; PDFs, DjVu and EPUB
  through text extracted once into fff's own cache: `pdftotext` (poppler)
  given the file's path, `djvutxt`, and EPUB unzipped by fff with its HTML
  stripped. The formats [rga](https://github.com/phiresky/ripgrep-all) adds
  (Office documents, archives) can fill the same cache through
  `rga-preproc`, one run per file. A missing tool skips those files, and the
  query box says so.
- **fff's own text cache.** One file per document under
  `$XDG_CACHE_HOME/fff/text/`, named from its device, inode, size and
  modification time in nanoseconds: an edited or replaced file is a new
  entry, so nothing needs invalidating, and old entries are pruned by age and
  total size. `pdftotext` and `djvutxt` separate pages with a form feed, so a
  match's page is one more than the form feeds before it.
- **Searched inside fff, not by running rga.** rga starts a preprocessor for
  every file on every search, cached or not: over 2,400 PDFs a warm rga
  search took 1.85 s and about 25 CPU-seconds, against 0.02 s for the same
  cached text searched in one process. Both extract with `pdftotext`, so the
  first extraction costs the same (about a minute of every core for 4 GB of
  PDFs), and rga's pipe through `pdftotext`'s stdin costs a third more CPU
  than passing the path. ripgrep's engine (its `grep-searcher` and
  `grep-regex` crates, or `regex-automata` and `memchr`) searches each cached
  text on fff's threads, in display order, stopping as soon as the query
  changes.
- **Extraction in the background.** It starts the first time a content query
  meets files not yet extracted, never at startup: top rows first, at low CPU
  and I/O priority on about half the cores, with progress in the query box
  and matches appearing as files finish.
- **Streaming and cancellable.** Matches stream in as they are found; typing
  restarts the search without blocking, and an obsolete search is stopped.
  Excluded paths and hidden entries (unless shown) are skipped, so it respects
  the same policy as name filtering.
- **Literal first, across lines.** The content query is a case-smart literal
  (the common case, and fast). Whitespace in it matches any whitespace,
  line breaks included, and a word hyphenated at a line end still matches:
  extracted text breaks lines where the page does, and a line-by-line search
  missed 7 of 87 occurrences of a two-word phrase in the PDFs measured. Latin
  accents are ignored unless the query has some, as in name matching. Adding
  letters only narrows, so a longer query searches only the files that
  matched. A `/regex/` form could follow. Fuzzy matching makes little sense
  for contents. A match reports its page in documents and its line in text
  files.
- **No OCR.** A scanned PDF without a text layer cannot match; about a
  quarter of the PDFs measured had none, scanned books among them. OCR takes
  seconds a page, too slow for fff; `ocrmypdf` adds the text layer to the
  file once, for fff and every other program.
- **Visible mode.** The query box title and a pill in its border say which
  query is being edited and whether the content filter is active; Escape
  clears the content query before the name query.
- **In the preview pane.** For a content match, the [preview
  pane](CONFIGURATION.md#preview-pane) would show the lines around the match
  (or the page it is on, from the cached text) instead of the file's first
  lines, with the match highlighted, and E would open `$EDITOR` there.

## Switching modes

`fff roam`, `pick`, `seek` and `explorer` are chosen when fff starts. A key
could switch the running session to another mode, say from browsing with
`fff roam` to `fff seek`'s flat list of files below here, keeping the query,
marks and directory where they still make sense. A small mode picker, a
chooser like the palette's, would list the modes with what each one does and
returns. What a switch does to output matters: `fff pick` prints paths and
`fff roam` changes the shell's directory, so a session started from a script
(`$(fff pick)`) or a file dialog (`fff explorer`) must not become one that
answers differently from what its caller expects; those would offer only the
modes that answer the same way.

## Resizing the font

Shift with `+` and `-` would make the terminal's font larger or smaller, for
reading from a distance or on a large screen. A terminal program cannot set
the font itself; it asks the terminal, and only some terminals listen: kitty
through its remote control (`kitty @ set-font-size`, when allowed), xterm and
a few others through the OSC 50 font sequence, others not at all. fff would
use what the terminal supports and otherwise say that it cannot. `+` and `-`
change the search depth in Normal mode, and on many layouts Shift+`-` is `_`,
so the chords need checking against what terminals report (the kitty keyboard
protocol tells them apart).

Behavior, keybindings and persistence for these features remain to be designed
when the work is requested.

## Post-launch

Ideas for after 1.0, once the features above are done.

### A tree view

A key would show the results as a tree, drawn with branch lines the way
`pass` and `tree` do: folders with their contents indented below them, so
the shape of a folder is visible at a glance. Rows would keep working as
rows (marks, operations, the preview), and the query would show the matching
entries with the folders leading to them.

### Sorting

Results are ranked by depth and match quality today (match quality first in
`fff pick` and `fff seek`). A key would open a
sorting picker, a chooser like the others: type or move to choose the
criterion, Tab switches between ascending and descending. Criteria:

- name, alphabetically (and naturally, so `file2` comes before `file10`);
- size (a folder by its entry count, as the size column shows it);
- last modification, last access, and creation where the filesystem records
  it;
- extension, then name;
- kind: folders first, then files, then links;
- depth, then name.

The order applies among the fuzzy matches of the current query: the query
still decides what is listed, the sort only how. The chosen order would show
on the results' border and could be the default in the configuration.

### Permissions

A key would show each entry's permissions, owner and group, the way `nnn`
does, beside its name (`-rw-r--r--  frank  users`), toggled on and off. The
`permissions` [column](CONFIGURATION.md#result-details) exists and the footer
shows the highlighted entry's `user:group`; this would make them a quick
toggle for every row rather than a configuration change, with owner and
group as columns of their own, and could show octal modes too.

The current release is 0.14.0. Version 1.0 is planned once the roadmap before
[Post-launch](#post-launch) is implemented. Earlier versions will not be retroactively renumbered.
