# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- Content search with alternating filtering (below).
- Tab completion of program options in the gx/gX prompt for programs without a
  man page or `--help`.
- Becoming the desktop's file manager (below).
- Resuming only when the last directory was the current one: a new session
  should restore its remembered location only when started from the
  directory that was being browsed last, instead of always jumping there.
- Progress for large copies and moves: bytes copied, speed, and Ctrl+c to
  cancel safely within the staged-operation guarantees.
- Mouse-only use beyond the opt-in buttons (below).
- Follow-ups to 0.7.0: live updates for deeper results and pinned roots
  (only the current directory is watched today); the mouse in the marked
  list and choosers; dragging past the pane's edge scrolling the
  results while it marks.
- Trash with undo, and an AUR package once the project nears 1.0.
- An optional query field (experimental, undecided): hide the query until `/`
  is pressed (moving the go-to prompt elsewhere), freeing a row and making
  more Normal-mode keys available; a flag or setting would start with the
  query visible.

There is deliberately no preview pane: see [design
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

## Desktop file manager

The aim is for fff to be the machine's main file explorer, not only a launcher
from the shell.

- **A desktop entry.** Install an `fff.desktop` (`Terminal=true`,
  `MimeType=inode/directory`) so `xdg-mime default fff.desktop inode/directory`
  makes `xdg-open`, application launchers and "Show in folder" actions open
  fff in a terminal at that directory.
- **Firefox and other file pickers.** Browsers that use the
  xdg-desktop-portal FileChooser (Firefox with
  `widget.use-xdg-desktop-portal.file-picker = 1`) could use fff through
  [xdg-desktop-portal-termfilechooser](https://github.com/GermainZ/xdg-desktop-portal-termfilechooser),
  which runs a wrapper script in a terminal. fff would need a wrapper that maps
  the portal's open, open-multiple, directory and save requests onto
  `--output-paths` (marks for multiple files) and a save-name prompt, and
  writes the chosen paths where the portal reads them.

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

File operations shipped in 0.3.0; running programs (gx/gX, E), the shell
integration and remembered locations in 0.4.0; tabs and plain Normal-mode keys
in 0.5.0; openers, the Enter rules and Neovim-only editing in 0.6.0; result
details, the richer footer, visible links, live updates, the mouse, opt-in buttons and dropped files in 0.7.0.
See [the operation guide](OPERATIONS.md).

The current release is 0.7.0. Version 1.0 is planned once the remaining roadmap
above is implemented. Earlier versions will not be retroactively renumbered.
