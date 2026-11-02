# Changelog

User-visible changes in each release of fff (the `fuzzy-file-finder` crate).
Entries before 0.8.0 were written afterwards from the release notes and
history.

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
