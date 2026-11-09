# Changelog

User-visible changes in each release of fff (the `fuzzy-file-finder` crate).
Entries before 0.8.0 were written afterwards from the release notes and
history.

## Unreleased

### Added

- **Everyday tasks.** A new guide, [docs/TASKS.md](docs/TASKS.md), of
  short recipes with recordings: finding one file among thousands, finding
  which file says something, freeing disk space, tidying Downloads and
  renaming many files at once. The tutorial ends with what to try when
  something looks wrong.

### Changed

- **The crate is much smaller to download**: 0.7 MiB, where it was 5.8.
  It leaves out the guides' recordings, which crates.io shows from GitHub.
- **Sorted by size, folders take their place as they are added up.** Every
  folder listed is added up in the background, not only those on screen,
  and the list follows the totals as they arrive, at most every two
  seconds so that rows do not jump under the highlight. Choosing the order
  again is no longer needed.
- **Sorting by size, a date or kind no longer slows a scan's keys.** The
  values are read by the scan as it finds each path, rather than as the
  rows take it in: a scan of `/usr` sorted by date settles in half the
  time, and a stalled network mount no longer holds up the interface.
- **Keys stay quick while a large tree lists.** The largest merges of the
  rows found happen on another thread: listing `/usr` holds keys up for at
  most about 10 ms at a time, where it was up to 17 ms, and 15 ms sorted
  by size, where it was up to 37.

## 0.23.0 — 2026-11-08

Large trees list more than twice as fast, new documentation for users,
current keys in `fff --help`, and fixes from an audit: a contents match's
preview stays small, long names go into the trash on macOS, and saved
files are never written through a link.

### Changed

- **Large trees list more than twice as fast**: the 620,000 entries of
  `/usr` take about 0.4 seconds, where they took 0.9 in `fff roam` and 1.6
  in `fff pick` and `fff seek`. Storing each path found takes a fifth of
  the work, an empty query no longer reads the paths it lists, and short
  labels are worked out only for the rows on screen, the entries sharing a
  name grouped on a thread of their own. Keys and moving the highlight are
  as quick as before.
- **New documentation for users.** A short README with recordings of fff at
  work, a step-by-step [tutorial](docs/GETTING_STARTED.md), and guides to
  [browsing](docs/BROWSING.md), [finding files](docs/SEARCHING.md),
  [the programs](docs/PROGRAMS.md), [file operations](docs/OPERATIONS.md),
  [keys and contexts](docs/KEYS.md), [openers](docs/OPENERS.md) and their
  arguments, [workspaces](docs/WORKSPACES.md),
  [palettes](docs/PALETTES.md), [configuration](docs/CONFIGURATION.md) and
  [setup and integration](docs/SETUP.md) (the shell, completions, man
  pages, the desktop and Firefox). The complete reference moved to
  [docs/dev](docs/dev/DEVELOPMENT.md).

### Fixed

- **`fff --help` and the manual pages name the current keys.** Their key
  summary, and the help of `--depth`, `--glob`, `--exclude`, `--hidden`,
  `--no-exclusions`, `fff roam`, `fff pick`, `fff seek` and the dialogs
  for several files or folders, still gave the keys of before 0.22.0
  (Alt+w, Ctrl+w, Alt+←/→…).
- **`config.example.toml` names the current keys** in its comments.
- **A contents match's preview stays small on files of many short lines.**
  It kept each line of the match's file as a string of its own, so a few
  MiB of short lines (logs, generated data) took hundreds of MiB; it now
  keeps the file's text and finds the lines it shows. It reads nothing
  while the preview is hidden, and one file at a time while moving quickly
  over matches in large files.
- **Long names go into the trash on macOS.** A name too long for the trash
  was shortened by bytes, which could cut a character in two; macOS's
  filesystems refuse such a name, so trashing the entry failed. Names are
  now shortened between characters.
- **Files opened in the background on macOS start in the exact folder**
  (Ghostty, Terminal, iTerm2): a folder whose name ended in a newline lost
  it on the way.
- **Saved state and caches are never written through a symbolic link** left
  at the name of their temporary file; saving fails instead.

## 0.22.1 — 2026-11-08

Keys that feel more like Neovim's: j and k go down from the query into
the results, g. toggles hidden entries and gc works from the query.

### Changed

- **`j` and `k` in the query's Normal mode go down into the results**, to
  the first and the last row: `highlight_first` and `highlight_last` give
  the results the focus when run from the query in Normal mode.
- **`g.` toggles hidden entries** (`toggle_hidden`), as in oil.nvim; `.`
  is free, and in the query's Normal mode it is Neovim's repeat.
- **`gc` edits the configuration from the query in focus too**, instead of
  reaching Neovim's comment operator.

## 0.22.0 — 2026-11-08

The query can have the focus, where Normal mode edits it as Neovim does;
every function does one thing wherever it works, and the default keys are
new: mostly Normal-mode keys, Vim's where it has one.

### Added

- **The query can have the focus**, on the main screen and the Keys
  screen alike: typing (`i`, `a`, or any key no binding has) gives it the
  focus, Escape from Insert mode stays in it, and Escape again
  (`toggle_query_focus`) goes back to the results. In its Normal mode the
  query edits as in Neovim, with `h`, `l`, `x`, `p`, `u`, `v`, `:` and the
  other default keys typed as text left to Neovim, while Enter, Tab and the
  Alt and Ctrl keys still act on the results. Whichever of the query and
  the results lacks the focus is drawn dimmed.
- **`print_here`** prints the current directory and quits, in `fff pick`
  and folder dialogs (`!` there, as before), and **`clear_marks`** (Ctrl+m)
  clears every mark.
- **`explorer-folder-multiple`**: keys can apply to `fff explorer folder
  --multiple` apart from `folder`.

### Changed

- **Keys apply by focus instead of list**: the Keys screen's List column is
  now Focus (query, results, marked list), and `keys.toml` names it
  `focus = [...]`; `list = [...]` is an error. Insert-mode keys apply in
  the query focus. A key you bind comes before Neovim's in the query too
  unless its focus leaves the query out; the default keys typed as text do.
- **Escape from Insert mode stays in the query**, now in its Normal mode;
  a second Escape goes back to the results, and the one after that clears
  the query, as Escape in the results did.
- **The save dialog's hint names the keys that work where you are.**
- **Clearer function descriptions** on the Keys screen, written for someone
  new to fff.
- **Each function does one thing wherever it works, in both modes**, and
  works only where that means something: `quit_into_highlighted` in `fff
  roam` and `fff seek`, `quit_here` in `fff roam` and `fff window`, the
  `print_` functions where fff answers with paths (`print_marks` where
  several paths can be the answer), `open_in_background` outside the
  dialogs choosing files. Elsewhere their keys say they do not work there.
  With nothing highlighted, `quit_into_highlighted` leaves into the current
  directory in either mode.
- **The numbered-row functions highlight their row, then act there**:
  `open_row` opens a file and quits, as `open_and_quit` does, in `fff
  roam` and `fff window`; `highlight_row` (formerly `confirm_row`) only
  highlights, and `highlight_next_in_row`, new, highlights the row below it.
- **Escape no longer clears the marks** when pressed again with the query
  empty; Ctrl+m (`clear_marks`) does.
- **New default keys**, mostly in Normal mode and Vim's where it has one;
  many functions have no default key any more, and the Keys screen binds
  them. Among them:
  - Enter does the same in both modes: in `fff roam` it opens a file and
    leaves (on a directory it does nothing; `g!` leaves into it), in `fff
    pick` it prints the marks and the highlighted entry, Shift+Enter only the
    highlighted one and Ctrl+Enter only the marks (formerly Ctrl+w).
  - `l` goes into a directory or a file's folder, `L` opens a file in the
    background; `/` goes to a path (formerly `g/`).
  - Digits highlight their row, Alt+number the row below it and
    Ctrl+number opens its file and leaves; Ctrl+Alt+number sets the depth.
  - `J` and `K` page through the results; Ctrl+j and Ctrl+k open the jobs
    and marked lists, and PageDown and PageUp scroll the preview.
  - `gd` and `gD` hide folders and files, `gl` switches labels, `gpp` and
    `gpw` the preview and its wrapping, `gP` opens palettes.
  - Up and Down switch tabs in either mode; Space marks, in Normal mode.
  - The file operations' keys (`v`, `p`, `s`, `x`, `X`, `E`, `u`, `R`) work
    in the results, not in the marked list.
  - `open_in_terminal`, `reload_palette` and most Alt keys for features
    have no default key.
- **The jump list's functions are named as Neovim's**: `go_to_older_jump`
  (Ctrl+o) and `go_to_newer_jump` (Ctrl+i), formerly `go_back` and
  `go_forward`.

## 0.21.0 — 2026-11-08

`fff setup` is a guided start: it checks the programs fff uses and writes
a first configuration from a few questions. Terminal programs open on macOS
without configuration, and renames that change only the case work there.

### Added

- **`fff setup` without an option is a guided start**: it lists the
  programs fff uses and which are installed (with the pacman, apt, dnf or
  Homebrew command for each missing one), asks whether your terminal shows
  fff's icons (turning them off, and saying how to get a Nerd Font, when it
  does not) and which of the installed programs open documents, images,
  media and office files (`open` first on macOS), then shows what it would
  add to the configuration file and writes it once you confirm. Settings the
  file already has are left alone: it asks only about the files your own
  `[[open.rules]]` do not open, and adds the new rules after them.
  `fff setup --check-config` reports the same programs, and a session
  started without a configuration file suggests `fff setup`.
- **Opening a terminal program works on macOS without configuration**:
  with `[open] terminal` unset, fff finds the terminal it runs in there too
  (among its parent processes, else by `$TERM_PROGRAM`, which covers iTerm2
  and Ghostty), and falls back to Terminal.app. Terminal.app and iTerm2 get
  the program typed into a new window through `osascript`, Ghostty a new
  instance through `open`; kitty, Alacritty and WezTerm run from their app
  bundles when not on the PATH.

### Fixed

- **A rename may change only a name's case on macOS's case-insensitive
  disks** (`notes` to `Notes`) instead of being refused as a collision.
- **The shell reads keys normally after fff exits** in terminals with the
  kitty keyboard protocol that keep the main and alternate screens' keyboard
  modes apart (kitty, Ghostty, WezTerm): typing no longer shows escape codes.
- **Operations on many marked entries start sooner on macOS**: each folder
  they are in is opened once, not once per entry.

## 0.20.2 — 2026-11-08

fff runs on macOS. Queries match the path below each result, folders
included, as fzf does.

### Added

- **fff runs on macOS** (13 or newer), from the same code as on Linux:
  browsing, the contents query, previews, file operations with their
  guarantees, the trash and undo, archives and the shell integration all
  work. Live updates are not available there yet (F5 reads a folder again),
  and the desktop integration, which needs a freedesktop.org desktop, is
  Linux's: `fff setup --desktop`, `--browser` and `fff dbus-file-manager`
  say so instead of installing anything.

### Changed

- **Queries match the path below each result's root, folders included**,
  as fzf does: `pdestab` finds `pde/notes/stability.tex` without typing a
  slash. A `/` in the query is a letter like any other rather than a
  separator between parts matched component by component, so `pde/stab`
  and `pde/` still find what they did. Skipping folders costs the same
  however long their names are when the match goes on at a word start, so letters in runs across folders beat
  letters scattered through one name, and letters matched in the basename
  count a little more, so a folder comes before what is inside it. A long
  gap costs in full, however good the letters after it, so letters
  scattered through a long name rank low even near the top of the tree.
- **Rankings of large listings use up to 16 threads** rather than 8, fewer
  on machines with fewer.

### Fixed

- **Archives open with a libarchive that lacks one of its compression
  filters** (zstd is missing from some builds): that filter is left off, and
  its archives are reported unsupported, instead of every archive failing to
  open.
- **File operations work without `/proc` mounted**, as in some containers
  and chroots: entries are read relative to their folders' descriptors.

## 0.20.1 — 2026-11-08

fff no longer needs st: terminal windows open in the terminal fff runs in.
The query box shows the Neovim mode in color.

### Changed

- **`[open] terminal` and `fff_terminal` default to the terminal fff runs
  in** rather than st: fff finds it among its parent processes, else uses
  `$TERMINAL`, `x-terminal-emulator` or the first known terminal installed
  (st, Alacritty, kitty, WezTerm, Ghostty, foot, Konsole, GNOME Terminal,
  Xfce Terminal, urxvt, xterm), each with its own way to run a command and,
  for fff's windows, the `fff-picker` class where it has one. st is no
  longer needed; setting either key works as before.

### Added

- **The query box shows the Neovim mode in color**: a filled label after
  its title and the mode's color on its border, title and prompt (Normal
  in `accent`, Insert in `success`, Visual and Select in `staged_folder`,
  Replace in `error`, Command, Operator and Pending in `marked`), in every
  palette. Other boxes still name the mode in their title.

## 0.20.0 — 2026-11-07

Depth weighs against the match: every program but `fff seek` ranks the best
match minus a penalty for depth, so a much better match deeper down comes
before a poor one near the top. Sorts start descending, in each criterion's
own order.

### Changed

- **`fff roam`, `fff pick`, `fff window` and the file dialogs rank by the
  match minus a penalty for depth**, growing as its logarithm so that the
  first levels down cost most, rather than by depth first (`fff roam`, the
  window and the dialogs) or by the match first (`fff pick`). An empty
  query still lists by depth, folders first, and `[ui] temperature` sets
  how much depth costs (0 ranks by the match first, as `fff pick` did; a
  large one by depth first). `fff seek` and lists read from stdin rank as
  before; `shallowest` in `gs` still sorts depth first.
- **Sorts are descending to start, and descending is each criterion's own
  order:** names from A, the largest, the newest, folders first, the
  shallowest and the best match first; ascending (`sort_descending =
  false`, Tab in `gs`) reverses it. `gs` opens descending, and
  `sort_descending` is `true` unless set: `name`, `extension`, `kind` and
  `depth` with `sort_descending = false` now list in reverse.
- **The program's own order is called `program`** in `gs`, `[ui] sort` and
  workspaces, rather than `ranking`.

### Removed

- **The `linear`, `sqrt` and `quadratic` sorts.** `log` stays, and is how
  the programs rank.

## 0.19.4 — 2026-11-07

Depth can weigh against the match: four sorts rank the best match first
minus a penalty for its depth, set by `[ui] temperature`.

### Added

- **Depth can weigh against the match instead of coming before it.** The
  sorting picker (`gs`) and `[ui] sort` have `linear`, `sqrt`, `log` and
  `quadratic`: the best match first, minus a penalty for depth growing in
  proportion to it, as its square root, its logarithm or its square, so a
  much better match deeper down rises above a poor one near the top.
  `[ui] temperature` (1 by default) sets how much depth costs; 0 ranks as
  `fff pick`, a large one as depth first. `shallowest` sorts as `fff roam`
  ranks: depth first, then the best match.

## 0.19.3 — 2026-11-07

Scans from `/` no longer report folders you may not read, a contents query
over a huge tree no longer lags, and large trees list faster.

### Changed

- **Large trees list faster**, most in `fff pick`: `/usr` (620,000
  entries) is listed in about 1.7 seconds in a terminal where it took 1.9,
  and in about 1 second in `fff roam` where it took 1.1. While a scan runs,
  what it finds is drawn at most every 33 ms (keys still draw at once), and
  the labels of entries sharing a name are kept up with less work.

### Fixed

- **Scanning from `/` no longer reports folders you may not read.** A
  folder below the scanned one that is closed to you (`/etc/audit/plugins.d`,
  say) is listed but not entered, without "Permission denied" in the status
  line; the scan also counts as complete again, so `/` is cached and not
  scanned afresh at each visit.
- **A contents query over a huge tree no longer makes fff lag.** Working out
  which files to search and in what order the matches go read every row on
  each frame (hundreds of milliseconds per frame on a million paths); it now
  happens on a thread of its own, the search's threads run below the
  interface's priority, and each frame takes found matches for at most a few
  milliseconds. Matches may appear a moment later; keys and scrolling stay
  immediate.

## 0.19.2 — 2026-11-06

The contents cache is private, and file operations on many marks or on
folders open in other tabs go ahead.

### Changed

- **The contents cache is private**: extracted document text and the
  contents index are written readable only by you (folders 0700, files
  0600) whatever the umask, and an existing cache folder others could read
  is closed to them.
- **Starting an operation on many marks is quick**: moving, copying or
  deleting tens of thousands of marked entries no longer spends seconds to
  minutes comparing every selected path with every other before it begins.
- **A rollback or undo that cannot finish names what it left**: the message
  lists the entries left at their destinations, or where an undo left them,
  instead of only saying that it stopped partway.

### Fixed

- **A folder open in another tab can be moved or deleted**: every tab starts
  where fff started, so after going up from there, deleting that folder was
  refused with "Cannot change a tab's directory" although no visible tab was
  in it. Only the current tab's directory is protected now; another tab
  follows its moved directory, or falls back to the nearest existing parent.
- **A new extraction folder follows your umask**: it was always mode 0755,
  even under a stricter umask such as 077.
- **An edited document is searched again** after an edit that kept its size
  and restored its modification time: the extracted text was the old one.
- **Content searches never read more than 64 MiB of a file**: a text file
  growing while it was read, an oversized cache entry, or an extractor
  printing more than that could make one search hold any amount of memory. A
  document whose text is longer is searched in its first 64 MiB.
- **The operations guide has its sections back** on buffers, opening files,
  running programs, the trash and undo, which links pointed to, and the help
  and guides no longer contradict the program on the default depth, the key
  opening `$EDITOR`, Tab in file dialogs, moving read-only folders and what
  survives a power loss.

## 0.19.1 — 2026-11-06

Every chooser and picker numbers its rows for Alt+number.

### Changed

- **Every chooser and picker numbers its rows** as the results and the
  workspace chooser do: in the `:` and sorting pickers, the palette chooser
  and the workspace icon chooser, Alt+1 through Alt+9 is Enter on that row
  and Alt+0 on the last, in every mode. The pickers also take the mouse now:
  a click highlights a row, the wheel moves, a double click is Enter.

## 0.19.0 — 2026-11-06

The results can be sorted, workspaces (what bookmarks were) keep settings as
well as a folder, and fff chooses among paths piped to it.

### Added

- **The results can be sorted**: `gs` in Normal mode (`show_sorting`) opens
  a sorting picker, a chooser like `:`, to sort the matches by name
  (naturally: `file2` before `file10`), size (a folder's once added up), last
  modification, last access, creation, extension, kind (folders, files,
  links) or depth, or to go back to the program's ranking. Tab there
  switches between ascending and descending. The query still decides what is
  listed, the results' border names the order (`sorted by size ↓`), and
  `[ui] sort` and `sort_descending` choose the one sessions start in.
- **Workspaces keep more than a folder**: a workspace (what bookmarks were)
  can also keep the program, the pinned roots, both queries, the depth,
  filters, exclusions, sorting and toggles, as `program`, `roots`, `query`,
  `contents`, `depth`, `filters`, `exclusions`, `sort`, `hidden`, `kinds`,
  `preview`, `exclusions_on` and `filters_on` in its `[[workspaces]]` table,
  with or without a `path`. Opening one applies what it keeps and leaves the
  rest; roots that are gone are left out and named. The form saving one ticks
  the settings to keep, as they are now, in a checklist like the Keys
  screen's, and the chooser shows what each keeps. `fff --workspace NAME`
  starts in one, the command line winning over it, so a script that starts
  `fff pick` with its own roots and globs can be a workspace instead.
  `fff window` and `fff explorer` apply all but the program.

- **Paths from other programs**: with paths on stdin, `fff` without a
  command chooses among them and prints the choice, as the lines spelled it,
  for scripts that already list paths: `nvim "$(rg -l TODO | fff)"`,
  `git ls-files | fff | xargs -r nvim`, `fd -0 | fff -0 | xargs -0r ls -ld`.
  The rows keep the list's order (best matches first once you type), lines
  naming nothing are skipped and counted, and the names and contents queries,
  marks, the preview and sorting work as in `fff pick`. Nothing navigates,
  rescans, changes or opens files there, and the options choosing what is
  listed (`--depth`, `--glob`, ROOTS, ...) are usage errors, as is a command
  with paths on stdin. `-0` reads and prints NUL-separated paths.

### Changed

- **Bookmarks are workspaces**: `[[bookmarks]]` tables are now
  `[[workspaces]]` (an old table is an error: rename it), the chooser opens
  with `gw` in Normal mode and Alt+w while typing instead of `b` and Alt+b,
  and the function is `show_workspaces`. The chooser works in every program,
  `fff seek` included.
- **Opening a workspace keeps the query**; opening a bookmark cleared it.
- **Tab keeps the editing mode**: switching between the names and contents
  queries types into the other one in the mode Tab was pressed in, so Tab
  while typing goes on typing; before, each query came back in its own mode.
- **The workspace chooser looks like the other pickers**: a `❯` filter
  line, one row per workspace with what it keeps beside its name, the count
  on the top border and the keys on the bottom one.
- **The workspace form says the path is optional** right under it: left
  empty, the workspace applies only its ticked settings, wherever you are.
  The warning about a missing folder moved there too. j and k move through
  its rows outside typing, as Tab does.
- **The workspace form is drawn like the chooser** it opens from: a dialog
  in the middle of the screen with where it saves on the top border, every
  row (the name, path and icon as well as the settings) marked `›` and
  highlighted under the cursor, and the keys on the bottom border in the
  order Tab, Enter, Space, Esc. Space is there only where it does
  something, saying what (`Choose icon`, `Keep`, `Don't keep`), instead of
  grey text in the form.
- **The `:` and sorting pickers have a Normal mode**, as the choosers do:
  Escape while typing goes to Normal mode, where `j`, `k`, `gg` and `G` move
  through the choices, and Escape there closes the picker (it used to close
  it at once). The top border counts the commands or orders.
- **Enter in a save dialog saves under the typed name** (or the suggested
  one), in Normal mode too and whatever is highlighted: it no longer enters
  the highlighted folder or offers to replace the highlighted file (`l`
  enters a folder; Ctrl+number or a double click on a file's row, or typing
  its name, replaces it). A name whose extension differs from the suggested
  one's, or that drops it, is warned about on the status line first, and
  saving it again takes it.

### Removed

- **`--resume`** (and `fff window --resume`) is gone, and sessions are no
  longer saved when they end. `~/.local/state/fff/session` is left from
  earlier versions and can be deleted; each folder's remembered highlight
  stays.

## 0.18.1 — 2026-11-04

Listing a large tree is a little faster.

### Changed

- **Listing a large tree is a little faster**: paths arriving from a scan no
  longer move the results already ranked. `/usr`'s 620,000 entries are
  listed in about 1.1 s instead of 1.2 in `fff roam`, and 2.8 s instead of
  3.1 in `fff pick`. Nothing listed, its order or what keys do with it
  changes.

## 0.18.0 — 2026-11-04

fff stays usable while file operations run, `fff setup` reports read more
easily, and folders still being written stand out.

### Added

- **fff stays usable while file operations run.** A copy, move, deletion or
  extraction taking a while goes on in a jobs list below the marks, with its
  progress, while you browse, search, mark and start other operations; one
  that would collide with a running operation (changing what it reads or
  changes) waits instead. `J` (Alt+J) focuses the list, where `x` stops an
  operation and undoes what it had done, and `u` in the results does the same
  to the latest one while it runs. Quitting asks before stopping them.
  Folders being moved or deleted cannot be entered meanwhile, and files being
  changed are not opened or given to programs.
- **Copies and extractions being written stand out**: with hidden entries
  shown, their `.fff-stage-*` folders and the folders in them use the new
  `staged_folder` palette color (pink in the built-in palettes), and the
  files in them `marked`. Content searches and live updates leave them out.
- **K (Alt+K) focuses and leaves the marked list**, beside J for the jobs
  list; Alt+Space still does too.

### Changed

- **Operations take their marks as they start.** A move, copy, link,
  deletion or extraction unmarks the entries it acts on at once, so others
  can be marked for the next one; the rest stay marked, where a move, copy or
  link used to clear every mark when it ended. Undoing it with `u`, or a
  failure, marks them again.
- **Archives are checked for extraction in the background**, with the
  progress sheet when that takes a while.
- **Live updates rescan at most once a second while operations run.**

- **`fff setup` reports read more easily.** Each part has a heading, every
  file a line saying whether it was written, updated or left unchanged
  (paths in your home start with `~`), and what is left to do by hand comes
  last under "Next", with the commands to copy; colored on a terminal unless
  `NO_COLOR` is set. `--man` says how many pages it wrote, and
  `--check-config` lists what it checked, not only what is wrong.
- **`fff setup --check-config` warns when folders no longer open in fff**:
  after `fff setup --desktop`, when `xdg-mime query default inode/directory`
  names another application, or none, with the command that makes fff the
  default again.

## 0.17.0 — 2026-11-04

File dialogs search contents, manual pages, `/proc`, `/sys` and `/dev` no
longer flood fff with work, and what earlier versions kept from older ones is
gone.

### Changed

- **`fff setup --file-chooser` is `fff setup --browser`**, the name it had
  beside it; files it wrote say so, and `--check-config` asks to run it
  again.
- **`fff setup --browser` says what Qt applications need.** Telegram and
  other Qt applications use the portal's file dialog, and so fff, only with
  `QT_QPA_PLATFORMTHEME=xdgdesktopportal` in their environment; outside a
  desktop environment they draw Qt's own dialog instead. Setup and the
  [desktop guide](docs/dev/DESKTOP.md#qt-applications-telegram-) say so.
- **The docs say where zsh looks for `_fff`**: in a directory that is in
  `$fpath` when `compinit` runs, not one added later.
- **`fff setup --check-config` checks the desktop integration.** When `fff
  setup --desktop` or `--browser` was run, it warns about files they
  wrote that are missing, out of date or run another fff binary, and about an
  environment in which Qt applications draw their own file dialog.

### Added

- **File dialogs search contents.** `fff explorer` has the contents query
  too (Tab or Alt+/), as every other program does. In a save dialog the name
  saved under still comes only from the names query, whose bar keeps its
  `Save as NAME` title. See [searching contents in a
  dialog](docs/dev/DESKTOP.md#searching-contents-in-a-dialog).
- **Manual pages.** `fff setup --man` writes `fff.1` and a page per command
  (`man fff-pick`, `man fff-explorer-save`) into a directory `man` searches,
  in your home if it can, or into the folder given (`--man DIR`).

### Removed

- **What earlier versions accepted only to say where it went, or to ignore
  it.** Options that became commands (`--no-dir`, `--output-paths`,
  `--init`, `--completions`, `--setup`, `--check-config` before a command,
  `--window`, `fff explorer --multiple`, `--directory`, `--save`) and `--here`
  are unknown options now. In the configuration, `[editor]`, `[keys]` and
  `[ui] resume` are errors; in `keys.toml`, `place` and the program
  `explorer-multiple`. Highlights remembered by versions before 0.9 are
  forgotten, and setup no longer replaces files written by `fff --setup`
  (0.8).

### Fixed

- **`/proc`, `/sys` and `/dev` no longer flood fff with work.** Their sizes
  were made up (`/proc` showed 128T, `/sys` 900M), and as root, adding them
  up or scanning into them read every process's folders while the
  processes came and went. The kernel's own filesystems, known by their type
  in the mount table, now show `—` for sizes, are not entered by scans from
  outside them, are not searched by the contents query nor watched, and the
  preview reads only their first 4 KiB, without waiting. Browsing them works
  as before. See [kernel
  filesystems](docs/dev/CONFIGURATION.md#kernel-filesystems).
- **Emptying old trash never deletes the wrong entry.** When an entry renamed
  in the trash left its old name to another entry put there by hand, the
  purge deleted the newcomer once the renamed entry's time was up, and kept
  the old one. It now deletes only the entry its record names.
- **Contents matches follow files changed while the query is set.** A file
  edited, or replaced, in a watched folder kept the matches found before the
  change until the query changed; it is now searched again, and the preview
  reads its lines again.
- **A contents search says when it shows only some matches.** A file shows
  its first 200 matches and a search stops at 20000, which the count beside
  the query did not tell: it now reads `200+ matches`, followed by
  `first 200 per file` or `stopped at 20000`.

## 0.16.4 — 2026-11-04

Folder sizes are kept between sessions and added up faster, a window of its
own and each file dialog are commands, and zsh completion can be a file.

### Changed

- **Folder sizes are kept between sessions and added up faster.** fff
  remembers what it added up, so sizes show as soon as it starts, and checks
  them in the background by reading again only the folders that changed.
  Adding a folder up the first time reads its folders on several threads,
  about five times faster. A file that grows in place without its folder
  changing counts once a watched folder sees it, or after F5.
- **A window of its own and each file dialog are commands.** `fff roam
  --window` is now `fff window [PATH]`, starting at PATH (a file's folder,
  with the file highlighted), and `fff explorer` takes the kind of dialog:
  `fff explorer open`, `open-multiple`, `folder` (`--multiple` for several)
  or `save`, named as the Keys screen names these programs. The old options
  say what replaced them. Run `fff setup --desktop` and `fff setup
  --file-chooser` again so that the launchers and the portal wrapper call
  the new commands.
- **`keys.toml` names things as the Keys screen does.** Where a key applies
  is `list = [...]` (was `place`), and the program choosing several files is
  `explorer-open-multiple` (was `explorer-multiple`). Files with the old
  names are still read, and rewritten with the new ones when the Keys
  screen saves.
- **zsh completion can be a file.** `fff setup --completions zsh >
  DIR/_fff`, for a DIR in `$fpath`, completes from the first Tab, with no
  line in `.zshrc`; sourcing it works as before.

## 0.16.3 — 2026-11-04

Folders show their size, added up in the background where you look, and
opening files works while typing the query (Alt+o).

### Changed

- **Folders show their size.** The size column gives a folder the sizes of
  every file below it added up, rather than its number of entries, which
  the footer keeps beside the exact size. They are added up in the
  background for the folders on screen first, then those a page away; a
  large folder shows its running total with `…` until it is done.
- **Opening and editing files work while typing the query.** Alt+o opens
  the current or marked files in this terminal from Insert mode, as `o`
  does in Normal mode; `edit_files` and `open_and_quit` work in Insert mode
  too, for keys you bind on the Keys screen.

## 0.16.2 — 2026-11-04

The query box names exclusions only when they differ from the
configuration's.

### Changed

- **`exclusions on` is not shown for the configuration's own exclusions**:
  only once the list differs from `[exclusions] rules` (Alt+e or
  `--exclude`).

## 0.16.1 — 2026-11-04

Exclusions in the configuration and for one place, long names that keep
their matched part in view, and fixes to contents matches, keys.toml and
read-only folders.

### Added

- **Exclusions in the configuration.** `[exclusions] rules` lists
  exclusion rules every session starts with, in every program; they are
  the first lines of the list Alt+e edits, `--exclude` adds lines after
  them, and `gc` applies changes to them at once.
- **Exclusion rules for one place**: a rule starting with `/` or `~/`
  (`~/backup`) matches absolute paths, so it excludes that folder and not
  every folder of the same name.

### Changed

- **Names too long for their row keep the part that matters in view**:
  the letters the query matches, with an ellipsis on each side where the
  name is cut, and otherwise the start of the name rather than its end.

### Fixed

- **A match no longer loses its highlight in the rows.** With a contents
  query, a match a little past the start of a long line was shown without
  its highlight (and a debug build stopped there); in the preview too, with
  wrapping off.
- **Alt+number opens a file at the match of that row** (`at_grep_match`),
  not at the file's first match or the one highlighted before.
- **The highlighted match stays put while matches arrive.** Matches found
  later in files above it no longer move the highlight to another match.
- **Read-only folders can be moved and restored from the trash**, as they
  could already be trashed, deleted and copied: `v` and `R` no longer fail
  with "Permission denied".
- **Keys bound on the Keys screen always load again.** Alt+`>`, Insert,
  F13 and up and Ctrl+Shift+letter were saved in a form keys.toml could
  not read, which dropped every key of yours at the next start. keys.toml
  now reads `<M->>`, `<C-S-A>`, `<Insert>` and `<F13>` up as Vim does; a
  key it cannot hold (Ctrl or Alt with `#`, which reads as any digit) is
  refused when you bind it, and keys.toml is written whole or not at all.
- **A marked link to a missing target can be the answer** of `fff pick`
  and file dialogs, as Enter in the marked list already allowed, rather
  than blocking it as a missing path.
- **A longer contents query finds every match** when it narrows one that
  had just reached its match limit; a file could be left out before.
- **Contents searches read EPUBs whose package names documents with `../`**,
  and stop reading a book that inflates without end.
- **UTF-16 text files are no longer previewed as MP3.**
- **Confirmation sheets never wait for the filesystem** to color the paths
  they list, and a contents match whose file has gone no longer keeps fff
  busy re-reading it.
- **A damaged contents index is ignored** rather than stopping searches.
- Messages name the keys you bound, not Space and Esc, for unmarking a
  missing mark and clearing the query; `fff --help` no longer mentions a
  `--map-key` option.

## 0.16.0 — 2026-11-04

The contents query moves into `fff roam`, `pick` and `seek`, on Tab, with
an index that reads only the files that may match; `fff grep` goes, `gd`
switches labels, and large trees list faster.

### Added

- **A contents query in every program.** `fff roam`, `fff pick` and
  `fff seek` search what files contain, PDFs included: Tab (or Alt+/), in
  either mode, opens the contents query beside the names query, and while
  it is set the rows are its matches, one per match. It stays when `gm`
  changes program.
- **An index of what files contain** makes contents searches over large
  trees read only the files that may match: every file a search reads is
  remembered by its trigrams in `~/.cache/fff/index`, keyed by its inode,
  size and times. Over 195,000 files a word in a few of them takes about
  0.1 s instead of 1 s. No match is lost to it.
- **The text cache and the index prune themselves**, once a day: entries
  unused for 90 to 135 days go, each on a day of its own, and the least
  recently used while the text cache passes 2 GiB or the index 512 MiB.
- **`gd` switches how rows are labelled** (`toggle_labels`): by name, with
  only the parent folders that tell same-named entries apart
  (`folder1/main.pdf`), or by their path below the folder searched. The
  choice stays when `gm` changes program.
- **`R` replaces a key on the Keys screen**: in a function's keys, press the
  new key and it takes the highlighted one's place, at once when nothing
  else has it there; where it clashes, where it applies opens with the clash.
- **A `function` palette color** for function names on the Keys screen, in
  the `:` picker and where a key clashes; each built-in palette's red.

### Changed

- **Listing a large tree is faster**: each depth's folders are read on
  several threads, still shallowest first, so that `max_paths` keeps the
  shallowest entries. `/usr`'s 620,000 entries are listed in about 0.85 s
  instead of 1.0 s; folders of their own (depth 1) are read as before.
- **Contents searches with many matches stay quick**: the rows are rebuilt
  at most every 50 ms while matches stream in, and files past the 20,000th
  match are no longer read, only counted.
- **Tab switches between the names and contents queries**, in either mode,
  and no longer marks while typing: Shift+Space marks, and Space in Normal
  mode. `m` marks every listed row and `M` inverts their marks, as before.
- **The names query is on the left** and the contents query on the right
  while both show. The bar typed into widens for a long
  query and the other gives way; a query cut at the end shows an ellipsis.
- **The footer is colored like `eza -l`**: the highlighted entry's name in
  its row's color, its size, date, owner and each permission letter in the
  palette's green, blue, yellow and red, so it follows the colorscheme.
- **`fff seek` labels rows by their path** below the folder searched, as
  `fff roam` does; `fff pick` keeps names. `gd` switches.
- **Putting a function's default keys back is `D`** (was `R`), and asks
  first.
- **Clashes read as a table** where a key applies: the keys it clashes with,
  their functions and where they meet, and the ticked values they share are
  marked in the checklists.
- **A function's keys go round**: moving past the last row comes back to the
  first, and before the first to the last.
- **The Keys screen's `r` hint shows in Normal mode only**, where `r` works,
  and its hints take a second line rather than being cut.
- **`--resume` follows the program a session ends in**: one switched to
  `fff roam` with `gm` is saved for it, and one switched away is not.
- **Enter on a cached row that turned from a file into a folder** (or back)
  waits for the rescan instead of acting on what it was.
- **A sheet too tall for the terminal asks for a larger one** instead of
  being cut, as fff does below 36 × 12; a function's keys first leave out
  the checklists that repeat the highlighted row.

### Removed

- **`fff grep`**: the contents query it had is in `fff roam`, `fff pick`
  and `fff seek` now (Tab). For a printed choice of files by what they
  contain, use `fff pick` and Tab; `gm` offers roam, pick and seek. A
  `keys.toml` key that names `grep` among its programs must drop it.

### Fixed

- **Space on a contents match no longer unmarks its file the next time**:
  marking moves on past the file's other matches, to the next file.
- **`gd` with a contents query no longer flickers**: rows labelled by path
  were cut to the width of their names, and in large listings rows showed
  bare names for a moment before their labels came back.
- **Descriptions that had fallen behind**: `extract` on the Keys screen
  says it asks where to extract (it always does), and `fff seek --help`
  says rows show their paths.
- **A longer contents query no longer misses matches** in files that the
  shorter one, having reached its 20,000 matches, had only counted.

## 0.15.0 — 2026-11-03

`fff grep` searches what files contain, `:` runs a function by name, and
the Keys screen's tables fit what they hold.

### Added

- **`:` in Normal mode picks a function by name**: a small fuzzy picker
  of the functions that work where you are, each with its key there; Enter
  runs the highlighted one as its key would, Escape goes back. In the
  results' query it replaces Neovim's command line.
- **`fff grep` chooses files by what they contain.** A contents query beside
  the names query (Tab switches) lists one row per match, with its page or
  line and the line it is on; the preview shows the lines around it. PDF,
  DjVu and EPUB documents are searched through their text, extracted once
  into `~/.cache/fff/text`; later searches are immediate. The query is a
  case-smart literal that ignores accents unless typed and matches across
  line breaks and hyphenation. Enter prints the chosen files, like
  `fff pick`, which it browses like.
- **Opening at a match.** An `[[open.rules]]` entry's `at_grep_match` adds
  arguments when a file opens from a `fff grep` match (`l`, Alt+l), with
  placeholders for everything fff knows about it: `{page}`, `{line}`,
  `{percent}`, `{query}`, `{match}`, `{nth}`, `{count}` and `{path}`.
  `--page {page} --find {query}` opens zathura on the match's page with the
  query highlighted.
- **`toggle_preview_wrap`** cuts the preview's long lines instead of
  wrapping them; it has no key by default.
- **`gm` switches program**: go on as `fff roam`, `pick`, `seek` or `grep`
  (`switch_program`). The folders, query, highlight, marks, exclusions and
  filters stay; the depth follows the new program's default unless you set
  another; `fff grep`'s contents query waits for coming back to it. The
  session leaves as the program it ends in. Dialogs and `--window` sessions
  do not switch.

### Changed

- **The Keys screen's tables fit their contents**: columns are as wide as
  what they hold, so long program lists are no longer cut while other
  columns sit empty; the details pane lists a function's keys one row each,
  and a key's further places read `or Enter` instead of a bare `or`. Hints,
  prose and checklists line up with the rows they explain.

- **The preview wraps long lines** onto the next rows instead of cutting
  them at the pane's edge.

- **Matching scores like fzf.** A name's score is the best alignment of the
  query's letters, so a whole word or a run deep in a long name beats letters
  scattered from its start: `canard` finds
  `krupa_szmolyan_relaxation_oscillation_canard_explosion.pdf` before
  `minicozzi_colding_a_course_in_minimal_surfaces.djvu`. The highlighted
  letters are the ones that scored. Latin accents are ignored unless the
  query has some: `evaluacion` finds `Evaluación`.
- **`fff pick` ranks the best match first**, as `fff seek` does, whatever its
  depth; depth only breaks ties.
- **Typing highlights the best match.** Each letter typed or deleted moves
  the highlight to the first row, instead of following the entry that was
  highlighted before however far it fell. Rows a scan adds under the same
  query still keep the highlighted entry.

### Fixed

- **Typing in a large listing no longer waits for every keystroke's
  ranking.** A letter typed before the previous one was ranked used to rank
  and relabel every known path again: over 600,000 paths, the match took 1.5
  seconds to show, and now 0.15.

- **`fff grep`'s match preview scrolls**: Ctrl+j and Ctrl+k
  (`scroll_preview_down`, `scroll_preview_up`) move it from the match, past
  the lines kept around it, up to 500 lines either side; another match
  starts at its own match again.

## 0.14.0 — 2026-11-03

F1 opens the Keys screen: search every function and key, rebind them, and
choose where each key applies.

### Added

- **The Keys screen (F1).** A picker laid out like the results: every
  function fff has, by name, with its keys in Insert and Normal mode, and the
  highlighted one's details beside (Ctrl+j/Ctrl+k scroll them). The query
  searches names, descriptions and keys, and your own keys move through it
  (Alt+j/Alt+k, `j`/`k`, Escape clears the query); Tab switches to the Keys
  view, a row per key with its function, mode, programs, lists and
  highlights, and `r` then a key searches for that key there.
  It shows what works where you opened it; Ctrl+a shows everything. Space
  goes a level deeper at every level, Enter is done and Escape goes back:
  Space opens a function's keys in a sheet like the bookmark form (`n` adds,
  `d` deletes, `R` resets), Space on a key opens where it applies, and
  Enter saves to `keys.toml` while Escape drops the changes. F1 closes the
  screen.
- **Where a key applies is four checklists**: the mode, the program, the
  list and what is highlighted, ticked `●` or not `○`, and crossed out `×`
  where the key cannot apply with the other ticks. Columns say `all`, a list
  or `all but` a list, whichever is shorter. A key that applies in places
  one set of ticks cannot say is given again, and its rows read "or".
  Tab and Shift+Tab move between the columns.
- **Functions are named a verb and what it acts on** (`highlight_next`,
  `go_to_parent`, `go_to_highlighted`, `toggle_hidden`, `show_bookmarks`,
  `print_highlighted`, `quit_into_highlighted`...), and each description
  starts with that verb.
- **`go_to_highlighted`**: goes into the highlighted directory, or to the
  highlighted file's directory with the file highlighted. `l` and Alt+l do it
  on a directory, Enter in Insert mode in `fff roam` on a file; give it a key
  for both on the Keys screen.
- **Keys apply where you choose.** A key can be limited to programs (roam,
  window, pick, seek, each kind of explorer dialog), to the results or the
  marked list, and to a highlighted directory, file or nothing. One key does
  one thing wherever it is pressed: when keys clash, the Keys screen offers
  to narrow the new one or take it from the others there.
- **No way to lock yourself in.** Saving keys that would leave nothing to open
  the Keys screen or to leave fff, that would turn typing into commands, or
  that would take `i`, `a` or `:` in Normal mode, is refused with the reason.
  A `keys.toml` with errors gives way to the default keys, and fff says why.
- **`gc` edits the configuration while fff runs** and applies it as soon as
  the editor ends: keys, bookmarks, openers, columns, the mouse, the trash
  and the rest take effect without restarting. A copy of the configuration
  is kept as `config.toml.bak` while you edit; if the edit has errors, fff
  goes on reading the copy, F2 shows what is wrong, and the query box says
  `config errors` until the file loads again (also in later sessions).
- **F2 opens the Session tab** directly, F1 the Keys screen; each key closes
  its own tab.
- **File filters can be turned off and on again** without losing them:
  `gf` in Normal mode, Alt+F while typing.
- **Normal-mode keys for exclusions, filters and palettes**: `gE` edits the
  exclusions and `ge` turns their enforcement on or off, `gF` edits the file
  filters, `gp` opens the palette chooser and `gP` goes back to the
  configuration's palette.
- **The button bar hides once the mouse rests**: with `buttons = "mouse"`,
  it shows while the mouse is moving or clicking and goes after
  `[ui] buttons_seconds` (5 by default; 0 keeps it).
- **Syntax colors in the preview pane**: text files show comments, strings,
  keywords, types, functions and numbers in the palette's colors, worked out
  by a private Neovim with its tree-sitter parsers or `:syntax` files. The
  text never waits for them. `[ui] highlight = false` turns them off.

### Changed

- **Keys live in `keys.toml`** beside the configuration, written in Vim's
  notation (`<M-h>`, `<CR>`, `gg`) by function, in an `[insert]` and a
  `[normal]` table. Each Enter, `l` and `o` case is a function of its own
  (`quit_into_highlighted`, `open_and_quit`, `print_highlighted`, `go_to_highlighted`,
  `open_in_background`, `open_in_terminal`...), so each can be bound apart. A
  configuration with `[keys]` says where they went.
- **Keys inside choosers, prompts and sheets are fixed**: Enter, Escape, Tab,
  and `j`, `k`, `gg`, `G`, `q` and `!` in Normal mode.
- **Moves, copies and links act at once** when nothing would be overwritten,
  and the message says `u` undoes them. Overwriting still asks first, and so
  do files dropped onto the terminal.
- **Default depth: 1 in `fff roam` and `fff explorer`, 3 in `fff pick`**
  (was 2); `fff seek` keeps 6.
- **`+` and `-` change the depth in Normal mode**, instead of Right and Left,
  and **`g/` opens the go-to prompt** instead of `/`.
- **Alt keys for features work only while typing**: in Normal-mode results,
  Alt+b, Alt+g, Alt+t, Alt+., Alt+d, Alt+D, Alt+v, Alt+e, Alt+H, Alt+f,
  Alt+F, Alt+p and Alt+P say which Normal-mode key to use instead (`b`, `g/`,
  `gt`, `.`, `d`, `D`, `P`, `gE`, `ge`, `gF`, `gf`, `gp`, `gP`). Alt keys
  that move around work in both modes.
- **`e` opens in `$EDITOR` and `E` extracts archives**, swapped from before.
- **Pinned roots that do not fit point to F2**, whose Session tab lists them.
- **A rule in the preview pane sets an entry's facts** (a PDF's pages,
  where a trashed entry came from) apart from its content.

### Removed

- **`[keys.normal]`, `[keys.map]` and `--map-key`**, replaced by `keys.toml`
  and the Keys screen; `depth_digits = false` is now `depth = "none"` in
  `[normal]`.
- **The printed key guide**, replaced by the Keys screen.

## 0.13.0 — 2026-11-03

The trash opens like a folder and R restores from it, the preview pane
scrolls, and the screen names keys as you configured them.

### Added

- **The trash opens like any folder: Alt+t** (`gt` in Normal mode). Browse,
  filter, preview, mark and rename there as anywhere; pressed again, it goes
  on to the trash of another filesystem. The location line says you are in
  the trash, and the preview pane says where an entry came from and when.
- **R restores entries from the trash** to where they were deleted from,
  only while in the trash. It finds the way even after an entry was renamed
  there, by fff or by hand (records now carry the entry's inode). When
  something has taken the place, a sheet asks to overwrite it (into the
  trash), keep both as `name (1)`, or cancel. `u` puts them back into the
  trash. Configurable as `restore` and `trash` in `[keys.normal]`.
- **Ctrl+j / Ctrl+k scroll the preview pane** by half its height without
  moving the highlight. With the pane hidden, they say how to open it.

### Changed

- **The screen names keys as you configured them.** The F1 guide, sheet
  borders, footers, hints and messages show a chord remapped in `[keys.map]`
  or `--map-key` by the key that now does its work, and `[keys.normal]`
  actions by their sequences; keys that no longer do anything are left out.

### Fixed

- **The purge follows entries renamed in the trash** instead of keeping them
  forever without a record.

## 0.12.1 — 2026-11-03

Large trees list faster, and a stuck mount holds up only its own entries.

### Fixed

- **Large trees list faster.** New paths merge into the results without
  copying every row each time, so a scan of over a million entries fills the
  list up to about three times sooner, with shorter pauses between keys.
- **A stuck network or FUSE mount no longer holds up details elsewhere.**
  Entries on other mounts keep their sizes, dates and counts, owners show by
  number until their names arrive, and stuck previews are capped: past 16, the
  preview pane says it waits for one to end.
- **Ctrl+c leaves at once** while a large listing is still being sorted.

## 0.12.0 — 2026-11-03

fff never waits for a slow disk to draw.

### Fixed

- **A slow network or FUSE mount no longer freezes the screen.** Drawing used
  to check entries on disk: result rows the first time they showed, every
  visible mark on every frame (and every mark twice in F1), the rows of
  confirmations and bookmarks' folders. These details are now read in the
  background, and a frame shows the last ones known until fresh ones arrive.

## 0.11.1 — 2026-11-03

Choosing files starts typing, `fff seek` looks deeper and ranks by score,
marking moves to Shift+Space, and the query box's border carries less.

### Changed

- **`fff pick`, `fff seek` and `fff explorer --save` start in Insert mode**,
  ready to type the query or the name to save under. So do the bookmark
  form's name and path.
- **`fff seek` searches 6 levels deep by default** (2 elsewhere), and ranks
  the best fuzzy matches first, however deep, instead of shallower first.
- **Shift+Space marks**, in Normal and Insert mode alike (it needs a terminal
  with the kitty keyboard protocol); Space still marks in Normal mode, and Tab
  only while typing. The `mark` key in `[keys.normal]` stands for Shift+Space,
  and `[keys.map]` chords that pointed at `tab` to mark now name `shift+space`.
- **The depth is shown on the results' bottom border**, under the count, and
  `files only` / `folders only` beside the count, instead of on the query
  box's border.
- **The progress marker (`✓ complete`, `◌ scanning`) shows only while a scan
  runs and for three seconds after.** Quiet background refreshes no longer
  show it.

### Added

- **Escape twice clears the marks**: in Normal mode with the query empty.

## 0.11.0 — 2026-11-03

The preview pane is shown from the start.

### Changed

- **The preview pane is shown when fff starts.** `[ui] preview = false`
  starts with it hidden, as before.

## 0.10.0 — 2026-11-03

A trash with undo, progress and Ctrl+c for long operations, a folders-only
listing, and many fixes: operations finish safely when the terminal closes,
Enter never leaves with an entry that is gone, and the status on the query
box reads like its title.

### Added

- **A trash.** `x` moves entries to the trash at once, without a
  confirmation: `$XDG_DATA_HOME/fff/trash` (`~/.local/share/fff/trash`), or a
  `.fff-trash-UID` folder at the top of another filesystem, so trashing is
  always a rename. Entries larger than `[trash] ask_above` (10 GiB) ask first,
  in one sheet for the whole selection: delete them permanently or trash them
  anyway. Entries that cannot go to the trash are offered permanent deletion.
  When fff starts, a background thread deletes what has been in the trash for
  more than `[trash] days` (30). `[trash] enabled = false` keeps permanent
  deletion.
- **`X` deletes permanently**, after the size review `x` used to show.
- **`u` undoes file operations**, last first: trashing, moves, renames,
  copies, links, new entries and extractions (created entries go to the
  trash), and overwrites (the overwritten entries come back from the trash).
  An undo checks that every entry is still the one the operation left and
  happens completely or not at all. Configurable as `undo` in `[keys.normal]`.
- **Progress for long operations.** Copies, extractions, deletions, and the
  checks before every operation run on their own thread; one taking more than
  a moment shows a sheet with a bar, the bytes or entries done, the speed and
  the time. Ctrl+c stops it while stopping changes nothing (and between the
  entries of a permanent deletion, putting the rest back); confirmations of
  large selections no longer freeze the screen while they are sized.
- **D hides files**, listing only folders (Alt+D while typing); again to show
  them. `d` and `D` switch between files only and folders only. Configurable
  as `files` in `[keys.normal]`.

### Changed

- `x` moves to the trash instead of deleting permanently; overwritten
  entries of a move or copy also go to the trash when they can.
- **`u` in Normal-mode results undoes file operations**, no longer query
  edits; `:u` still undoes those, or set `undo` to another key or `"none"`.
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
  highlight there instead of reporting that navigation is disabled. Since
  it lists only files, `d` and `D` are not bound there either and reach
  Neovim; `P` shows the preview pane as elsewhere.
- **Small polish across the screen.** The F1 guide shortens a long
  description at its end, keeping the words that say what the key does, and
  its session details say "no query" and spell out whether hidden entries
  are shown and exclusions enforced. Notices and session details wrap
  between words. A compact marked list shows marks without blank lines
  between them, and long mark names keep their extension in view. The
  current / selection / all question names the current entry once, in a
  shorter sentence. `x` or `X` with nothing to act on says "Nothing to
  delete", and refusals to change or undo around the current directory say
  to go up first.
- **The status on the query box's border reads like its title**,
  `✓ complete · depth 2 · hidden shown`, instead of shaded boxes that cut
  through the border; the finished scan shows a check mark rather than a dot
  that small fonts drew as `·`.

### Removed

- **Ctrl+w only returns marks where several paths can be the answer**:
  `fff pick` and `fff explorer --multiple`. In `fff roam` it no longer opens
  the marked files and leaves (`o` opens them here and comes back), in `fff
  seek` it no longer leaves into the last mark's folder, and in other dialogs
  it no longer answers with a mark: in a save dialog that replaced a marked
  file without asking. There it now only says so. The Select & Exit button
  follows suit.

### Fixed

- **fff ends when its terminal closes, also under `nohup`.** With the hangup
  signal ignored it kept running forever, one thread spinning a CPU core on
  the closed terminal; during an operation that thread spun until the
  operation finished.
- **Quitting fff while it empties old entries from the trash** could leave a
  half-deleted entry with its record, looking whole and restorable. Entries
  now leave `files/` and lose their record before they are deleted, and the
  next start finishes an interrupted purge.
- **A `gX` program that cannot start leaves your shell where it was.** The
  shell integration still changed to the browsing directory.
- **`$` and backticks inside double quotes are refused** in `gx` / `gX`
  lines, `[[open.rules]]` commands and `$EDITOR`, as they already were outside
  quotes: a shell would expand them, and fff passed them on literally. Write
  `\$` or `'$'` for a literal one.
- **Copies and moves into a folder below one you can enter but not list**
  (such as another user's home of mode 0711) work; they failed with
  "Permission denied" while checking the destination was not inside a
  source.
- **Tab completes the options of a program typed as a path** (`./script`)
  from its `--help`; it asked `man`, which formatted the file itself as a
  manual page.
- **`[[open.rules]]` extensions with several parts match**: `"tar.gz"` never
  matched anything, since only the part after the last dot was compared.
  An empty extension is now refused when the configuration loads.
- **Enter while typing in `fff seek` chooses the file**, leaving into its
  folder as in Normal mode. It tried to reveal the file in its folder
  instead, which seek forbids, so a file below the starting folder only got
  "Navigation is disabled in fff seek" until Esc left Insert mode.
- **Ctrl+number and double clicks act on their row while typing a name in
  `fff explorer --save`.** They saved under the typed name instead, so Ctrl+1
  on a folder saved in the current one and Ctrl+number on a file did not
  offer to replace it.
- **Operations on large folders no longer freeze fff afterwards.** With live
  updates on, the thousands of changes from deleting, copying or extracting a
  large tree took seconds to process, during which the screen stood still.
- **Capitals and shifted symbols type correctly in kitty, foot, ghostty and
  WezTerm.** With the kitty keyboard protocol, Shift+a reached every text
  field as `a` (and `:` as `;`), so names, commands and `ZZ` could not be
  typed.
- **Pasting into the gx/gX command prompt works**; the text was dropped.
- **Escape leaves Insert mode in the gx/gX command prompt**, and Escape again
  cancels it. Escape was swallowed there, so it could not cancel the prompt.
- Escape leaving Insert mode in the bookmark chooser keeps the highlighted
  bookmark instead of jumping to the first one.
- `fff setup --check-config` no longer fails on a machine without `st` when
  the configuration does not name it: the built-in terminal defaults are not
  checked.
- The Alt+g prompt suggests the folder fff was started in from zoxide's
  list; zoxide left out the directory it ran in, which was always that one.
- Bookmark names and paths with line breaks or other control characters (as
  `dd` then `P` in the form leaves) are refused instead of being written to
  the configuration.
- `--resume` drops relative paths from a damaged session file instead of
  resolving them against wherever the new session runs; a relative tab
  folder makes the file count as damaged.
- An unknown palette's error lists every built-in palette, not three.
- **Normal-mode Enter on a cached row whose entry was deleted stays** and
  says so, until the refresh drops the row. It left into the entry's folder,
  which `fff pick` printed although nobody chose it.
- **Insert-mode Enter that reveals a file in the current folder keeps
  Insert mode**, as revealing one elsewhere does. The query dropped to
  Normal mode, so the next letters acted as commands (`x` trashed the file).
- Enter on a marked-list row whose path was deleted says so instead of
  printing (`fff pick`) or opening the missing path.
- In `fff seek`, Normal-mode Enter with nothing listed stays instead of
  leaving into the starting folder, and Ctrl+o / Ctrl+i say navigation is
  disabled instead of reaching Neovim, where Ctrl+o made the next letter a
  command.
- Ctrl+number or a double click on a row that a just-finished re-sort
  removed does nothing, instead of acting as Enter with nothing highlighted.
- fff no longer keeps a core busy while a large listing is re-sorted on
  another thread, or during a scan after a change on disk.
- The preview pane no longer fails on a file holding only a SQLite header,
  and shows the line count of a text file of exactly 64 KiB.
- PageDown at the end of the results keeps a full page in view.
- Ctrl+s in the new-name and extraction prompts keeps them open on a
  validation error, as Enter does.
- **Stopping a permanent deletion right after an entry went no longer
  reports that entry as only partly deleted**, keeps its mark or leaves an
  empty `.fff-stage-*` folder behind. Ctrl+c is heard before an entry is
  deleted, never after, and a stop before anything of an entry was deleted
  says it was not deleted.
- `e` with an empty destination extracts beside the archive in a folder
  browsed through a symlink; it failed with "Not a directory".
- `u` refuses to undo a trashing while you browse inside the trashed folder
  in the trash, as it does for other undos that would move the current
  directory away.
- The trash purge keeps entries whose recorded date is impossible (a 13th
  month, a 31st of April) instead of reading them as a later real date.
- **The Select & Exit button in `fff explorer` is Enter on the highlighted
  row**, as documented: saving over a file asks first, and a folder is
  entered. It answered with the row as it was, so in a save dialog it
  overwrote a file without asking.
- Enter while typing in an empty folder no longer closes a `--window`
  session, or cancels a dialog choosing files in `fff explorer`; only where
  a folder is an answer (`fff pick`, `fff roam`, folder dialogs) does it
  answer with the folder.
- Live updates no longer watch the targets of symlinked folders listed
  below the browsed one, which fff never lists the insides of: changes in
  `/tmp` behind a `tmp` link no longer rescan the results, and a link into
  an unreachable mount cannot hang the screen.
- Closing the terminal while a file operation runs lets the operation
  finish before fff ends. fff ended at once, stopping it partway and leaving
  entries in hidden `.fff-stage-*` folders.
- The remembered-locations and session files are readable only by you; they
  hold your folders, marks and queries.
- **Lowering the depth after `max_paths` stopped a scan lists the shallower
  results completely**, instead of stopping again at once and showing
  deleted files until F5.
- A change on disk noticed just as you left a folder no longer lets fff show
  the old listing as current when you come back.
- The chosen Alt+g suggestion stays visible on short terminals, and editing
  the typed path from Neovim's command line clears the choice, so Enter
  never goes to a row you cannot see.
- Greek queries with Σ, σ or ς match names ending in a capital Σ.
- **The first "Show in folder" of a session works.** When the desktop started
  fff to answer it, that call was lost and the asking program waited 25
  seconds for nothing; only later calls opened fff.
- A leading `~` in `[open]` commands (`terminal`, `fff_terminal`, a rule's
  `command`) no longer fails the configuration with "must name a program".
- Tab after a bare `~` in the gx/gX prompt completes inside the home folder
  instead of turning it into a relative path.
- A `file://` URI with `%+5`-style escapes is no longer decoded as a byte,
  and the short `file:/path` form is understood. Files dropped on the
  terminal decode their URIs the same way; a `%` without two hex digits
  after it no longer turns the whole drop into query text.
- Tab in the gx/gX prompt no longer treats a backslash inside single quotes
  as an escape, which made it complete the wrong word.
- `fff explorer` never answers a save dialog with a path containing a
  newline, which the portal would read as two paths.
- **Read-only folders can be copied, deleted and overwritten.** A folder
  without write permission for its owner (mode 555, say) failed with
  "Permission denied" where `cp -r` and `rm -r` succeed; the copy keeps the
  folder's mode.
- Extraction refuses archive entry names containing C1 control characters
  such as U+009B, which a terminal reads like Escape, as it already did for
  other control characters.
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
  a name with accented letters, and completes after the text as typed:
  `~` and `..` complete inside those folders, and `only/.` offers hidden
  folders, instead of gluing a name onto the text.
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
- New guide: [desktop integration](docs/dev/DESKTOP.md), with a full
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
