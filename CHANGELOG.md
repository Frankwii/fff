# Changelog

User-visible changes in each release of fff (the `fuzzy-file-finder` crate).
Entries before 0.8.0 were written afterwards from the release notes and
history.

## Unreleased

### Added

- **D hides files**, listing only folders (Alt+D while typing); again to show
  them. `d` and `D` switch between files only and folders only. Configurable
  as `files` in `[keys.normal]`.

### Changed

- **`q` in `fff pick` prints nothing**, like Ctrl+c, instead of the launch
  directory: a script such as `fff pick | xargs -r zathura` no longer opens
  the folder you started in. `!` still prints the browsing directory.
- **Shorter Normal-mode keys:** `d` hides directories (was `zd`) and `P`
  shows the preview pane (was `zp`); `p` still copies. In the results, `d`,
  `D` and `P` no longer reach Neovim as delete and put.
- A key set in `[keys.normal]` takes over a default binding of the same
  key instead of refusing to start, so a configuration with `delete = "D"`
  keeps working and `files` is then left unbound.
- **`fff seek` has no tabs at all:** Up / Down in Normal mode move the
  highlight there instead of reporting that navigation is disabled. Like
  the other keys of modes that navigate, `d`, `D` and `P` are not bound
  there either and reach Neovim.

### Fixed

- **The first "Show in folder" of a session works.** When the desktop started
  fff to answer it, that call was lost and the asking program waited 25
  seconds for nothing; only later calls opened fff.
- A leading `~` in `[open]` commands (`terminal`, `fff_terminal`, a rule's
  `command`) no longer fails the configuration with "must name a program".
- Tab after a bare `~` in the gx/gX prompt completes inside the home folder
  instead of turning it into a relative path.
- A `file://` URI with `%+5`-style escapes is no longer decoded as a byte.
- **Renaming many entries no longer quits fff.** Saving a rename list larger
  than about 64 KiB (thousands of rows, as in a deep `fff pick` or
  `fff seek`) ended the session with "Neovim stopped: Neovim RPC
  disconnected"; such lists now rename like short ones. Any large multiline
  field closed with ZZ or `:wq` had the same problem.
- **Small folders open in one frame, complete.** Entering a folder, going up
  or starting fff no longer draws the location first empty or from the cache
  (with `◌ cached · refreshing`) and then again a moment later: a scan that
  finishes within about a frame is shown only once it has, so the label and
  the flicker are left to listings that really take a while. Quiet rescans
  after live updates no longer flash the label either.
- Tab in the Alt+g prompt no longer crashes on a path ending in `/.` after
  a name with accented letters.
- Long result names shortened to fit a narrow window no longer crash the
  match highlighter.

## 0.9.1 — 2026-11-02

Path-aware matching, "Show in folder" with the file highlighted, live updates
across the whole listing, more of the mouse, and huge trees that stay
responsive while they load and re-rank.

### Changed

- **Live updates reach deeper results and pinned roots.** With `[ui] watch`,
  files created, removed or renamed in any listed folder (not only the
  current directory) or in a pinned root appear and disappear without F5, and
  a folder's entry count follows. Up to 4096 folders are watched, shallowest
  first.
- **Browsing stays responsive while huge trees load.** Moving the highlight
  during a scan of hundreds of thousands of paths (say `$HOME` with hidden
  files at depth 9) no longer stalls for a second at a time: keys are handled
  before discovery work, and new paths are merged into the results a little
  at a time instead of re-sorting everything, shallowest first. Such scans
  also finish several times faster and take less memory.
- **The highlight stays on the first row while a location loads**, instead of
  following whichever row was first when the scan began, until you move it
  (or a remembered row, such as the folder you came up from, is listed).
- **Re-ranking a large listing no longer freezes the screen.** Changing
  depth, hidden files, exclusions, filters or Alt+d, and typing a query, rank
  big listings on other threads in parallel; the previous results stay up,
  with `◌ sorting` on the status line, until the new ones are ready. Typing
  never waits for them; keys that act on rows do.
- `fff pick` and `fff seek` update their labels for shared names only where
  they changed, so a large tree no longer slows them down as it fills.

### Added

- **"Show in folder" highlights the file.** `fff setup --desktop` also
  installs a D-Bus service for `org.freedesktop.FileManager1`, so Firefox's
  "Show in folder" (and other programs' "Open containing folder") opens fff in
  the file's folder with the file highlighted, instead of only the folder.
  Run `fff setup --desktop` again to add it.
- **The mouse in the marked list and the choosers.** A click on a mark
  focuses the marked list there and a double click acts like `l` on it; in
  the bookmark, icon and palette choosers and the Alt+g suggestions, a click
  selects a row and a double click chooses it. The wheel moves through them.
- **Dragging past the results' edge scrolls them**, marking rows as it goes,
  for as long as the button is held there.
- **Path-aware matching.** A query containing `/` matches the path below
  each result's root instead of its name: `pde/stab` finds
  `pde/notes/stability.tex` but not `notes/stability.tex`, and `pde/` lists
  what is inside folders matching `pde`. The parts match components in
  order, the last one the name; whole and consecutive components rank
  higher, and the matched letters are highlighted along the whole path.
  Queries without `/` match as before.
- **`[performance] max_paths`** (default 1,000,000) bounds the paths a view
  holds, so browsing a huge tree cannot exhaust memory. A scan that finds
  more stops there and the status line shows `◌ partial`; a lower depth shows
  the rest.

## 0.9.0 — 2026-11-02

A text-only preview pane, `fff --resume`, and a command for each way of using
fff: `fff pick`, `fff seek` and `fff setup` join `fff explorer`, and plain `fff`
is `fff roam`.

### Added

- **A text-only preview pane.** Alt+v (`zp` in Normal mode) shows the
  highlighted entry in the bottom half of the screen: a text file's first
  lines, a folder's entries, an archive's or EPUB's contents, a PDF's title,
  author, pages and first pages' text (with poppler), an image's dimensions,
  and audio and video duration, streams and tags (with `ffprobe`). Reading
  happens in the background after the highlight rests and never holds up the
  list. `[ui] preview = true` starts with it shown; rebind it as `preview` in
  `[keys.normal]`.
- **`fff --resume`** continues the last `fff roam` session as it ended: its
  folder and highlighted row, tabs and their history, marks, query, depth,
  filters, exclusions and toggles. Only the last session is kept; `fff pick`,
  `fff seek` and `fff explorer` sessions are never saved.
- **Alt+d (`zd` in Normal mode) hides directories**, listing only the files
  found down to the current depth; press it again to show them. Navigation
  keeps working, and the status line shows `files only` meanwhile. Rebind it
  as `directories` in `[keys.normal]`.
- **`fff roam`** names what fff does without a command: browse, and change the
  shell's directory on leaving. `fff` alone stays the same as `fff roam`.

### Changed

- **`--output-paths` is now `fff pick`**, the only command that prints paths:
  `nvim "$(fff pick)"`, `fff pick --relative -0`. It takes ROOTS and
  `--browse` like `fff roam`. Its rows show bare names, with only the parent
  directories that tell same-named entries apart, as `--no-dir` did. Scripts
  that combined `--output-paths` with `--no-dir` and `--glob` use
  `fff pick --glob ...`, which lists only the matching files.
- **`--no-dir` is now `fff seek`**: `fff seek ~/papers --glob '*.pdf'`. It
  behaves as `--no-dir` did without `--output-paths`, leaving into the chosen
  file's folder, and rejects `--browse`, `--window` and `--resume`.
- **Setting up is `fff setup`**: `--init SHELL`, `--completions SHELL`,
  `--desktop`, `--file-chooser` (or `--browser`) and `--check-config` replace
  `fff --init`, `--completions`, `--setup desktop`, `--setup file-chooser`
  and `--check-config`. Update the shell lines in your startup files, e.g.
  `eval "$(fff setup --init zsh)"` and `source <(fff setup --completions
  zsh)`. Each moved option now fails with a pointer to its replacement.
  Launchers and wrappers written by `fff --setup` keep working and are
  replaced when setup runs again.
- `--glob` and `--exclude` also work after a command: `fff explorer --glob
  '*.pdf'` lists only PDFs in a file dialog.
- **Every session's footer says how to leave**: `q quit` (`Ctrl+c quit` while
  typing) beside `F1 keys`, not only in `fff explorer`.

### Removed

- **Automatic resuming.** A new session starts in the current directory, on
  the row last highlighted there; use `fff --resume` to continue the last
  session. `[ui] resume` is still accepted and does nothing, and `--here` is
  accepted for scripts but no longer needed.

## 0.8.1 — 2026-11-02

Folders opened from the desktop now behave like a file manager window.

### Changed

- **Folders opened from the desktop are browsed, not left.** fff started by
  `xdg-open`, a launcher or "Show in folder" has no shell to change into, so
  Normal-mode Enter on a folder (or the Select button) now enters it instead
  of closing the window, and `!` turns the window into a shell (`$SHELL`) in
  the folder being browsed. Enter on a file still opens it and closes the
  window. Run `fff --setup desktop` again to update the launcher.

### Added

- **`--window`**, for fff running in a terminal window of its own: Enter
  enters folders, `!` starts a shell in the current folder, and nothing is
  printed. The desktop launcher passes it.

## 0.8.0 — 2026-11-02

fff can now be the desktop's file manager and Firefox's file chooser, and the
keys that open or finish something were reworked into one consistent model.

### Added

- **`fff --setup desktop`** makes fff the program that opens folders: it
  installs a desktop entry and a small launcher script, makes it the default
  for `inode/directory` with `xdg-mime` (checking that it took effect and
  saying how to restore the previous default), so `xdg-open`, application
  launchers and "Show in folder" open fff in a terminal window.
- **`fff --setup file-chooser`** lets Firefox and other applications that use
  the desktop portal pick and save files with fff, through
  xdg-desktop-portal-termfilechooser: it writes the portal wrapper and selects
  it in the portal configuration, editing existing files one key at a time.
- **`fff explorer [--multiple] [--directory] [--save] [PATH]`**, a mode made
  for answering a file dialog. It starts beside the suggested file, prints the
  answer one path per line, and keeps the usual keys: Enter chooses a file
  and enters a folder (in either editing mode), `!` takes the current folder,
  Ctrl+w the marks; `q` cancels. The empty query line says how to choose, and
  the footer shows `q quit` beside `F1 keys`.
- **Saving from a dialog.** The query box is the file name, and its title
  shows `Save as NAME`. `!` or Ctrl+s (also while typing) saves under the
  typed or suggested name in the current folder, and Enter does while typing.
  The name highlights similar files without hiding anything. Saving over a
  file always asks: Enter replaces, `k` keeps both (`paper (1).pdf`), Escape
  goes back.
- `--browse`: start browsing at a path instead of pinning it as a search root;
  a file starts in its folder, highlighted.
- `[open] fff_terminal` (default `st -c fff-picker -e`): the terminal for
  fff's own windows from `--setup`, with a window class that a window manager
  rule can float.
- Enter or Ctrl+number with nothing to act on now says why (nothing matches,
  still searching, nothing listed) instead of doing nothing.
- In open and save dialogs, `l` on a file explains how to choose it instead
  of trying to open it; a double click is Enter on the row.
- New guide: [desktop integration](docs/DESKTOP.md), with a full
  `fff explorer` reference.

### Changed

- **Normal-mode Enter on a file opens it and leaves** (default mode): a
  terminal opener such as an editor replaces fff in the terminal, like `gX`; a
  GUI opener starts detached, like `l`; the shell then lands in the file's
  folder. Together, `l` stays, `o` comes back, Enter leaves.
- **Ctrl+w in the default mode opens the marked files and leaves**, grouped
  by opener like `o`, instead of only changing to the last mark's folder.
- **Ctrl+number is Enter on that row**, in the current editing mode, instead
  of returning the row's path directly.
- With `--output-paths`, Normal-mode Enter on a file (and Enter in the marked
  list) prints the file instead of its folder.
- The Select & Exit button without marks acts like Normal-mode Enter.
- General options (`--depth`, `--hidden`, `--config`, ...) may follow a
  subcommand.
- fff asks the terminal what it supports and sends the kitty keyboard
  protocol and synchronized output only where confirmed; frames are written
  in one piece. Terminals such as st no longer log a complaint on every frame.

## 0.7.0 — 2026-11-02

### Added

- `[ui] columns`: result rows end with details, by default the size (a
  directory's entry count) and age; permissions are available too. Columns
  drop from the left on narrow terminals. Only visible rows are read, and
  entry counts come from a background worker.
- A richer footer: the highlighted entry's link target, exact size or entry
  count, date, permissions and owner, dropping the least important when
  narrow.
- Links show `→ target` after their name; broken links use the error color,
  and a link's size is its target's.
- `[ui] watch`: the current directory follows changes as they happen, with
  coalesced rescans and details refreshed in place.
- `[ui] mouse`: a click highlights a row and clears the marks, a double click
  acts like `l`, dragging marks the rows it spans, the wheel scrolls, and
  clicking a tab number switches tabs.
- `[ui] buttons` (`off`, `mouse` or `always`): an opt-in button bar for
  mouse-only use, in the results and in the transfer and delete
  confirmations.
- Files dropped onto the terminal (for example st with the drag-n-drop patch)
  are offered for copying into the current directory.

### Changed

- Much faster with many rows or marks: constant-time marks, a windowed marks
  pane, kept results for shorter queries (`[performance] cached_queries`) and
  a remembered highlight index.

## 0.6.0 — 2026-11-01

### Added

- **Openers.** `[open]` rules choose a program per extension or name pattern
  (first match wins, otherwise `$EDITOR`). `l`, Alt+l and Alt+number open a
  file in the background: GUI programs detached from the terminal, terminal
  programs in a new window through `[open] terminal`. `o` opens the current
  or marked files in this terminal, grouped by opener. `--check-config`
  reports missing opener programs.
- Pop-up notices when a background program fails.
- Normal-mode `m` marks every listed row and `M` inverts their marks.

### Changed

- Normal-mode Enter exits into the highlighted entry's folder (also for deep
  results; printed with `--output-paths`). Enter while typing enters
  directories and returns or reveals files. Alt+number acts like `l`.
- Tabs: Up goes to the next tab and Down to the previous one; the tab numbers
  sit in the results' top-right corner.

### Removed

- The basic built-in editor and `--editor`: Neovim is now required.

## 0.5.0 — 2026-11-01

### Added

- **Three tabs**, each with its own directory and Ctrl+o / Ctrl+i history,
  sharing the marks: Alt+Up / Alt+Down, or Up / Down in Normal mode.
- Plain Normal-mode keys in the results: Left / Right and 1–9 for the depth,
  `.` for hidden entries, `/` for the go-to prompt and `b` for bookmarks, all
  rebindable in `[keys.normal]`.
- `[keys.map]`: remap or disable any chord from the configuration, like
  `--map-key` (which still wins).
- The go-to prompt (Alt+g) suggests bookmarks and zoxide's frequent folders.
- Ctrl+o / Ctrl+i walk back and forward through the folders visited.
- File operations are rebindable in `[keys.normal]`.
- `gx` / `gX` complete options from the program's man page or `--help`.
- The F1 guide explains exclusions and file filters.
