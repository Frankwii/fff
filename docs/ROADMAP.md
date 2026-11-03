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
- An interactive key guide (below), building on the live-reloaded
  configuration (`gc`) and keys shown from the configuration.
- Switching modes without restarting (below): from `fff roam` to `fff seek`,
  for example.
- Resizing the font from fff (below).
- An AUR package once the project nears 1.0.

The preview pane stays text only; there will be no image previews: see
[design choices](DEVELOPMENT.md#design-choices).

## An interactive guide

Keys are [shown from the configuration](CONFIGURATION.md#keys-on-screen)
(0.13.0), and `gc` [applies an edited configuration at
once](CONFIGURATION.md#editing-while-fff-runs). What remains:

- **One way to name actions.** `[keys.normal]` names actions, `[keys.map]`
  names default chords, with a [preliminary set of action
  names](CONFIGURATION.md#chord-remapping) for its targets. Both should name
  the same actions the same way, chords and sequences alike.
- **An interactive F1 guide.** The key list becomes searchable: a fuzzy
   query over both the descriptions and the mappings. Choosing an entry and
   pressing a key rebinds it, saved to the configuration file the way
   bookmarks are (preserving comments and unrelated settings), and applied
   through the live reload.

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
  plain text, so the likely backend is [rga](https://github.com/phiresky/ripgrep-all)
  (ripgrep-all), which also reads EPUB, Office documents, archives and more,
  with its own extraction cache. It would run without a shell, on the files
  the name query and the scope allow, with its output parsed into rows; when
  rga is missing, plain-text search (ripgrep, or a built-in reader) still
  works and the query box says which backend is active.
- **Streaming and cancellable.** Matches stream in as they are found; typing
  restarts the search without blocking, and an obsolete search is stopped.
  Excluded paths and hidden entries (unless shown) are skipped, so it respects
  the same policy as name filtering.
- **Literal first.** The content query is a case-smart literal substring (the
  common case, and fast); a `/regex/` form could follow, since rga and ripgrep
  accept regular expressions. Fuzzy matching makes little sense for contents.
  For PDFs a match reports its page where the backend provides one.
- **Visible mode.** The query box title and a pill in its border say which
  query is being edited and whether the content filter is active; Escape
  clears the content query before the name query.
- **In the preview pane.** For a content match, the [preview
  pane](CONFIGURATION.md#preview-pane) would show the lines around the match
  (or the PDF page it is on) instead of the file's first lines, with the
  match highlighted, and E would open `$EDITOR` there.

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

Results are ranked by depth and match quality today. A key would open a
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

The current release is 0.13.0. Version 1.0 is planned once the roadmap before
[Post-launch](#post-launch) is implemented. Earlier versions will not be retroactively renumbered.
