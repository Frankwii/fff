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
- Bringing back trashed entries from earlier sessions inside fff (a trash
  view with restore), since undo lasts for the session.
- No filesystem calls while drawing. Result rows already use cached details,
  but the marks pane stats each visible mark on every frame, the F1 guide
  checks every mark twice per frame, and confirmation lists stat their rows.
  Cheap on local disks, but a slow network or FUSE mount stalls the frame;
  these should read from the details cache, refreshed in the background.
- Preview pane follow-ups: scrolling it without moving the highlight, and
  previewing the marked list's rows from the results.
- An AUR package once the project nears 1.0.
- An optional query field (experimental, undecided): hide the query until `/`
  is pressed (moving the go-to prompt elsewhere), freeing a row and making
  more Normal-mode keys available; a flag or setting would start with the
  query visible.

The preview pane stays text only; there will be no image previews: see
[design choices](DEVELOPMENT.md#design-choices).

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

Behavior, keybindings and persistence for these features remain to be designed
when the work is requested.

The current release is 0.10.0. Version 1.0 is planned once the remaining roadmap
above is implemented. Earlier versions will not be retroactively renumbered.
