# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- Content search with alternating filtering (below).
- Path-aware matching (below).
- A text-only preview pane (below).
- Option completion in the gx/gX prompt for programs with neither a man page
  nor `--help` (Tab already completes options from those two).
- Resuming only when the last directory was the current one: a new session
  should restore its remembered location only when started from the
  directory that was being browsed last, instead of always jumping there.
- Progress for large copies and moves: bytes copied, speed, and Ctrl+c to
  cancel safely within the staged-operation guarantees. This should also move
  file operations off the UI thread, including their preparation: building a
  confirmation inventories the selected trees and extraction validates every
  archive payload first, so today even opening a confirmation can freeze the
  interface. A worker would prepare and run them with progress shown at once,
  keeping the confirmation binding, validation and rollback guarantees, and
  must define what stays usable meanwhile (navigation, marks, other
  operations), how live-update rescans interact, and what Ctrl+c does at each
  stage.
- No filesystem calls while drawing. Result rows already use cached details,
  but the marks pane stats each visible mark on every frame, the F1 guide
  checks every mark twice per frame, and confirmation lists stat their rows.
  Cheap on local disks, but a slow network or FUSE mount stalls the frame;
  these should read from the details cache, refreshed in the background.
- Mouse-only use beyond the opt-in buttons (below).
- With `--output-paths`, Normal-mode Enter on a file prints its folder, while
  everywhere else Enter finishes with the entry itself (the default mode opens
  it, `fff explorer` chooses it). Printing the file would complete that model,
  but changes what scripts receive.
- `--no-dir` as its own subcommand, e.g. `fff seek`, like `fff explorer`: a
  static file choice changes enough behavior (no navigation, Insert mode,
  bare names) to be a mode of its own rather than a flag.
- Desktop follow-ups to `--setup` (see [desktop integration](DESKTOP.md)):
  answering `org.freedesktop.FileManager1` over D-Bus, so "Show in folder"
  highlights the file instead of only opening its folder, and a hint in the
  file chooser saying which keys choose.
- Follow-ups to 0.7.0: live updates for deeper results and pinned roots
  (only the current directory is watched today); the mouse in the marked
  list and choosers; dragging past the pane's edge scrolling the
  results while it marks.
- Trash with undo, and an AUR package once the project nears 1.0.
- An optional query field (experimental, undecided): hide the query until `/`
  is pressed (moving the go-to prompt elsewhere), freeing a row and making
  more Normal-mode keys available; a flag or setting would start with the
  query visible.

There will be no image previews: see [design
choices](DEVELOPMENT.md#design-choices).

## Mouse-only use

The opt-in button bar (`[ui] buttons`, see [buttons](CONFIGURATION.md#buttons))
covers the results and the transfer and delete confirmations. Still to do
before it could become the default:

- Buttons for the other sheets: new file or directory, rename, extract, the
  scope choice, run prompts and the bookmark and palette choosers.
- Depth and filter controls.
- Turning it on automatically when fff is started as the desktop's file
  manager or a file picker.

## Path-aware matching

Today the query matches only basenames, so similarly named files in different
folders (`notes/stability.tex`, `pde/notes/stability.tex`) can only be told
apart by reading their labels. Path-aware matching would let `pde/stab` match
`pde/notes/stability.tex`.

- **Slash-triggered.** A query containing `/` matches the path relative to the
  result's root instead of the basename. No basename contains `/`, so such a
  query lists nothing today and no working query changes meaning; a query
  without `/` behaves exactly as now.
- **Segments in order.** Each `/`-separated part of the query is a
  case-insensitive subsequence of some path component, the components in
  order, and the last part must match the basename. Scoring favours whole
  components and the basename, and the highlighted letters span the whole label.
- **Ordering unchanged.** Depth stays the primary sort key, then directories
  before files, then the score, as today.
- **Same narrowing.** Appending letters still only narrows, so the incremental
  re-score keeps working.

## Text-only preview pane

An optional pane beside the results showing the highlighted entry. **Text
only**: no images, sixel, kitty graphics or other terminal image protocols, so
it works the same in st and any other plain terminal.

- **What it shows.** The first lines of a text file (tabs expanded, control
  characters made safe, long lines cut), PDF text through `pdftotext` when it
  is installed, a directory's first entries, and for anything else (binary
  files, images, archives) a short metadata summary: type, size, modification
  time, permissions, link target.
- **Never slows the list.** Reading happens in a worker, bounded by bytes and
  lines, and starts only once the highlight has rested briefly (like the
  palette preview's wait); moving again cancels it. The frame never waits for a
  preview, and stale previews are dropped.
- **Off by default.** A key toggles it and a setting can start with it shown;
  on narrow terminals it stacks below the results, as the marks do.
- **With content search.** A content match could preview the lines around it
  and E would open `$EDITOR` there.

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

Behavior, keybindings and persistence for these features remain to be designed
when the work is requested.

The current release is 0.7.0. Version 1.0 is planned once the remaining roadmap
above is implemented. Earlier versions will not be retroactively renumbered.
