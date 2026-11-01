# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- Content search with alternating filtering (below).
- Tab completion of program options in the gx/gX prompt for programs without a
  man page or `--help`.
- Launching a program in the background while browsing continues, for GUI
  programs that should not pause fff.
- A preview pane, opening by file type, trash with undo, and an AUR package
  once the project nears 1.0.

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
- **Streaming and cancellable.** Files are read by the existing breadth-first
  scan, in background workers, as rows arrive; typing restarts the search
  without blocking. Binary files, files over a size limit, excluded paths and
  hidden entries (unless shown) are skipped, so it respects the same policy as
  name filtering.
- **Literal first.** The content query is a case-smart literal substring (the
  common case, and fast); a `/regex/` form could follow. Fuzzy matching makes
  little sense for contents.
- **Visible mode.** The query box title and a pill in its border say which
  query is being edited and whether the content filter is active; Escape
  clears the content query before the name query.
- **No external tools.** Reading happens in-process (no grep or shell), so
  results arrive through the same generation and cancellation checks as
  discovery, and nothing is executed.

Behavior, keybindings and persistence for these features remain to be designed
when the work is requested.

File operations shipped in 0.3.0; running programs (gx/gX, E), the shell
integration and remembered locations in 0.4.0. See [the operation
guide](OPERATIONS.md).

The current release is 0.4.0. Version 1.0 is planned once the remaining roadmap
above is implemented. Earlier versions will not be retroactively renumbered.
