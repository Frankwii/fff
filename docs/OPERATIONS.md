# File operations

Operations act on results and the marked list. These keys act
only in Neovim Normal mode (the marked list always accepts them). Insert mode
keeps them as text.

| Key | Action |
| --- | --- |
| `v` | Move all marked paths into the current directory, after confirmation |
| `p` | Copy all marked paths into the current directory, after confirmation |
| `s` | Create absolute symlinks to all marked paths here, after confirmation |
| `x` | Delete the highlighted path or the marked paths, with confirmation |
| `r` | Open a batch rename buffer for every currently filtered result |
| `e` | Extract the current or marked archives into a prompted directory |
| `n` | Choose file (`f`) or directory (`d`), then enter a new name |
| `E` | Open the current or marked paths in `$EDITOR`, then come back |
| `gx` / `gX` | Run a program on the current or marked paths; come back / exit into it |

`v`, `p`, and `s` show **No files selected** when nothing is marked. They never
fall back to the highlighted path. Because they are also common Vim keys, each
opens a **Move here** / **Copy here** / **Link here** window listing the marked
entries with their sizes; **Enter** proceeds and **Escape** cancels. Target names
retain their source basenames. For symlinks, existing files, directories and
dangling symlinks are collisions and never overwritten. Two marked entries with
the same name are refused before the window opens.

When a move or copy collides, the window becomes **Overwrite on move** /
**Overwrite on copy** and lists the existing destinations with their sizes, like the deletion
confirmation. **Escape** cancels the whole operation; **Enter** moves or copies
everything and overwrites the listed entries. A directory is overwritten as a
whole, never merged. The listed entries are detached into a private staging
directory (for copies, only after every copy is
staged), and deleted only after every entry has been published; any failure
rolls the publications back and restores them. Entries changed since
confirmation, collisions that appear after confirmation, moving or copying a
path into its own directory, and overwriting an entry that contains a selected
path all abort the operation. To operate
on paths from another directory, mark them before navigating to the destination.
Successful move, copy and symlink operations clear the entire marked list.
Deletion unmarks only the deleted paths and their descendants, so deleting an
unmarked current path keeps the selection. Failed or cancelled operations
preserve the selection. Rename updates marked paths without clearing the list.

Operations preserve original filesystem paths independently of abbreviated labels
and icons. Copy, move, symlink and delete support names containing non-UTF-8 bytes.
Overlapping selections (a directory and one of its descendants) are refused;
select each tree once. Moving/deleting the browsing directory or its ancestors,
and copying a directory into
itself are refused. Moves across filesystems are refused; copy first instead.
Trees containing a mount point are refused for every operation, so a mounted
share is never copied or deleted through its parent. Mount points are detected
with the kernel's mount-root flag, which also catches bind mounts from the same
filesystem, and by device number for other filesystems and subvolumes. Recursive
copy and deletion check again at every directory, in case something is mounted
after validation. Sockets, devices and other
special files are refused anywhere in a tree, as are trees deeper than 128
levels or with more than 100,000 entries. Symlinks are copied or
removed as links; recursive operations never follow them.

## Dialogs and messages

Every file-operation dialog is a sheet spanning the outer edges of the result
and mark panes and resting on their bottom border, as tall as its content (the
rename buffer fills the pane area). The text explains what each key does, and
the bottom border repeats the keys, for example
`─ c: Current ─── s: Selection ─── a: All ─── Esc: Cancel ─`. On narrow
terminals the hints compact (tighter separators, then without Esc, then keys
only) instead of clipping a key. The rename buffer lists `ZZ` / `ZQ`; name and destination buffers list
`Enter` / `Esc`. Destructive confirmations
(deletion, overwriting) use the `error` color; choices and buffers use
`marked`.

A successful operation reports what it did in the footer with a `✓` in the
`success` color, such as `Deleted 2 entries` or `Renamed notes.md`. An operation
that took effect only partly (a deletion stopped partway) reports a warning in
the `error` color, as errors do.

## Buffer interactions

New names and extraction destinations are single-line: **Enter applies** from
Normal or Insert mode and **Escape cancels** from Normal mode (Escape in Insert
mode returns to Normal first). Enter during command-line entry stays in Neovim,
and validation errors such as an empty or nested name keep the buffer open.
`ZZ` / `ZQ` also apply or cancel.

The rename list follows the same lifecycle as
exclusions: **`ZZ` writes and closes to apply**, or
**`ZQ` closes without writing to discard** (`:wq` / `:q!` work too). `:w` records a snapshot but keeps editing;
a later close applies that snapshot, discarding subsequent unwritten edits.
Escape and native Neovim commands stay inside the buffer. The command line
appears on the buffer's last row. `ZZ` is mapped to `:wq`, so it applies even an
unchanged buffer, such as an empty extraction destination.

The rename buffer freezes the filtered result list in its displayed order,
including rows below the current page. It contains scope-relative paths, without
abbreviations, and exactly one original path per row. Rows map to original
paths by position, so keep the row count and order; edit only final names.
Directory rows may be renamed on their own line. Descendant rows may spell
their parent with either the original or the renamed directory names, so with
rows `docs` and `docs/myfile`, both `documents` + `docs/my_file` and
`documents` + `documents/my_file` rename the directory and the file. A
descendant row cannot rename a directory: `docs` + `documents/myfile`, or
`documents` + `Documents/myfile`, abort. Parents are renamed first; each entry
stays pinned to its parent directory, and a later failure rolls earlier renames
back. Any other parent change, `..`, empty row, duplicate destination, existing
destination, unsafe new name, or changed row count aborts the entire plan
before any rename. Swaps/cycles are refused because they target existing
entries. Unchanged rows take no part. An unchanged
buffer does nothing. Names containing newlines or non-UTF-8 bytes
cannot be represented safely here, so opening that rename buffer is refused.

New names from rename, creation and a new extraction destination must be safe
to see and type. Control characters (tab, carriage return, escape, NUL...),
leading or trailing whitespace, a leading `-` or `~`, and invalid UTF-8 (which
Neovim lines show as U+FFFD) abort the operation. Copy, move and symlink keep
existing source names unchanged. Extraction destinations starting with `~` are
refused because fff does not expand them.

Single-line extraction and creation buffers show the query box's `❯` prompt.
Normal-mode Enter
is ignored, and `o` / `O` enter Insert mode without adding a row. Insert-mode
Enter is also ignored; command-line Enter still executes `:wq` / `:q!`.

`e` extracts the current entry when nothing is marked, and the marks when the
current entry is one of them. When the current entry is unmarked while others
are marked, it asks: **current (`c`)**, **selection (`s`)** or **all (`a`)**, the
selection plus the current entry. Every chosen file is validated before the
destination prompt opens; if any is not an extractable archive, nothing happens
and each refused file is named with the reason. Several archives extract
together or not at all: they are staged completely, a file present in two
archives (or already in the destination) aborts everything, and only then is
anything published, with rollback on failure. Directories common to several
archives combine. An empty extraction destination extracts each archive beside
itself (in its parent directory), even if it comes from a deeper search result. Other relative destinations start at the
browsing directory. The destination must either exist as a real directory or
have an existing parent; extraction does not follow a destination symlink.
A new destination directory gets mode 0755. Extraction into an existing
directory stages inside it, so its parent need not be writable.
New file/directory naming buffers start in Insert mode, ready for typing.
Names must be a single nonempty basename in the current directory; existing
entries are refused. New files get ordinary permissions under your umask. Rename and extraction buffers start in Normal mode.

## Opening files

Each file type can have an **opener**, configured under `[open]` (see the
[configuration guide](CONFIGURATION.md#openers)); files no rule matches use
`$EDITOR`. There are three ways to open:

- **In the background:** `l`, Alt+l, or Alt+number on a file. fff keeps the
  keyboard. A GUI opener (zathura, imv, mpv) starts detached from the terminal:
  in its own session, so closing the terminal or quitting fff leaves it open,
  with no terminal input and its output appended to `open.log` next to the
  remembered state (`~/.local/state/fff/open.log`). A terminal opener (one with
  `terminal = true`, and `$EDITOR`) cannot share fff's terminal, so it opens in
  a new terminal window, using `[open] terminal` (`st -e` by default). The status
  line says what was opened; if the program later fails, a **Notice** pop-up
  appears at the top right with its exit status and the log's location, and
  fades after a few seconds.
- **In this terminal:** `o` (configurable as `open` in `[keys.normal]`) opens the
  current or marked files with the same current / selection / all choice as `E`.
  The opener takes over the terminal, GUI or not, and fff comes back when it
  ends. Files sharing an opener go to one invocation (three PDFs, one zathura);
  different openers run one after another. `o` refuses directories (`l` enters
  them).
- **And leave:** Normal-mode Enter on a file, or Ctrl+w on the marked files,
  in the default mode (not with `--output-paths` or in `fff seek`, where they
  return paths). Files sharing an opener go to one invocation, as with `o`. GUI
  openers start detached, as with `l`; a terminal opener replaces fff in this
  terminal, like `gX`, and fff's exit status becomes the program's. Only one
  terminal program can take over, so marks needing two (say `nvim` and
  `less`) are refused, as are marked directories. Every program is found
  before any starts. The shell integration then leaves you in the (last)
  file's folder. If something cannot open, the status line says why and fff
  stays.

Openers are resolved like `gx` commands: no shell, the program found on PATH
before anything runs, and the file paths added as separate arguments.

## Running programs

`gx` runs a program on the current or marked paths and comes back to fff; `gX`
runs it in place of fff. Both use the same current / selection / all choice as
extraction and deletion; with nothing current or marked, the program runs on no
paths. Both are Normal-mode sequences, configurable as `run` and `run_exit` in
`[keys.normal]`.

A one-line prompt (starting in Insert mode) takes the command. **Tab** completes
the command name from the programs on your PATH, and later words as paths
relative to the browsing directory; when several candidates match, Tab inserts
their common prefix, lists them, and pressing Tab again cycles through them. A
line under the input shows what the command resolves to (`✓ /usr/bin/nvim`) or
that it does not exist. **Enter** reviews; **Esc** cancels. Nothing is recorded:
there is no command history.

Tab on a word starting with `-` completes the program's options, such as
`dragon --and` → `dragon --and-exit`. They come from its man page when it has
one (reading it runs only `man`), otherwise from `program --help`. That run is
detached from the terminal (it cannot open `/dev/tty`), gets no input, and is
stopped together with anything it started after one second, so a program that
ignores `--help` cannot take over the screen or hang fff. Options are read once
per prompt; only names at the start of a help line count, so options mentioned
inside descriptions are not offered.

There is no shell. The line is split into words like a shell would split it
(spaces separate; `'…'` is literal; `"…"` allows `\"` and `\\`; a backslash
escapes one character; a leading `~` is your home), but nothing else is
interpreted. Unquoted ``| & ; < > $ ` * ? ( )`` are refused with a message, since
without a shell they could not mean what they mean in one. The first word must
be a program on PATH (empty PATH entries, meaning the current directory, are
ignored) or a path to an executable file, such as `./script`; an unknown command
aborts with an error, suggesting the closest command name when there is one.
The chosen paths are appended after the typed words, as separate absolute
arguments, so no file name is ever read as syntax or as an option.

A **Run here** / **Run and exit** window then shows the exact command, the
program it resolved to, and the paths in argument order. **Enter** runs it;
**Esc** cancels.

- `gx`: fff leaves its screen and gives the program the terminal, then returns,
  redraws and re-scans; marks are kept. The status line says the program
  finished, or shows its exit status. If the program fails, fff first waits for
  Enter so its error messages stay readable. While the program runs, Ctrl+c and
  Ctrl+\\ (quit) reach only the program.
- `gX`: fff restores the terminal and replaces itself with the program, which
  then owns the terminal; the exit status is the program's. In the default mode
  the shell integration then changes to the directory you were browsing, as `!`
  would.

`E` opens the current or marked paths in `$EDITOR`, then comes back, like
`gx` but with no prompt and no review (it only edits). It uses the same current
/ selection / all choice. `$EDITOR` is split into words and resolved like a
typed command, so `nvim -p` or `code --wait` work. When it is unset, empty,
badly quoted or not a known command, nothing runs and the status line says
which, suggesting the closest command name when there is one.

Programs always read and write the terminal (`/dev/tty`), never fff's stdout,
so both work while fff's output is redirected. They start in the browsing
directory with fff's environment.

## Deletion confirmation

`x` deletes the current entry when nothing is marked, and the marks when the
current entry is one of them. When the current entry is unmarked while others
are marked, it asks: **current (`c`)**, **selection (`s`)** or **all (`a`)**, the
selection plus the current entry, as extraction does. The **Delete permanently** window
lists the paths and their cumulative logical sizes. Directory sizes include
hidden descendants; symlinks count their own link bytes, never target contents.
Hard-linked entries count separately. Sizes use B, KiB, MiB, GiB and TiB.

Rows sort by decreasing size, sizes right-aligned and names styled like result
rows. If they exceed the available height, the final row reports how many more
entries remain and their summed size. The top border shows the entry count and
total size. **Enter** confirms and **Escape** cancels.
A failed size calculation prevents deletion. After confirmation, all roots are
detached into private staging directories before recursive
deletion starts. If deleting one root fails, deletion stops: that root (with
whatever remained of it, directory modes unchanged) and every root not yet
deleted are moved back, and a warning says how many were deleted and which root
was only partly deleted. Marks of the deleted roots are removed. Anything that
cannot be moved back, here or in any other rollback, is named with its staging
path.

## Confirmations and interruptions

A confirmation applies to the files that were shown, not merely their paths.
Delete, move, copy and link windows record each listed tree (identity and
metadata of every entry), and execution refuses any tree that differs, for
example after an edit or after a symlink in its path was repointed.

Operations run to completion or roll back: a terminal hangup, `SIGTERM` or
`SIGINT` arriving during an operation is held until it finishes, then takes
effect. Signals ignored when fff started (such as under `nohup`) stay ignored.
A kill that cannot be held (`SIGKILL`, power loss) can leave hidden
`.fff-stage-*` directories holding data; when fff enters a directory with such a
leftover from a process that no longer runs, the status line names it.
Rollbacks only move back entries this operation published, verified by identity.
The destination directory of a move, copy or link is bound to the confirmation
too. An overwritten entry that changes while detached is kept in its staging
directory (and named in the message) instead of being deleted.

## Guarantees and limits

There is no undo: confirmed deletions and overwrites are as final as `rm`.
Everything else is designed so that only confirmed data is ever removed.

Parents are pinned by directory descriptors and nothing follows a symlink.
Destinations are published with no-replace renames; copies and extractions are
staged completely before anything is published. Every tree is inventoried
(identity and metadata of each entry) during validation and checked again right
before each step that changes it; a difference cancels the operation. A failure
partway through rolls earlier steps back, and anything that cannot be rolled
back is named with its path.

Copies preserve file contents, ordinary permission bits, symlinks and
access/modification times. They are not filesystem snapshots: ownership,
extended attributes, ACLs, hard-link relationships and sparse allocation are not
preserved. Concurrent writers can still race an operation; stop writers when a
stable copy matters. Operations run synchronously, including size calculation.

## Archives and system requirements

The Linux operation engine uses `openat2`, mount-root detection through `statx`
(Linux 5.8+) and no-replace rename.
Building/running requires **libarchive**, including its development linker files
when building (for example `libarchive-dev` on Debian/Ubuntu). Browsing also links
to this library. No shell or external extractor is invoked by fff.

libarchive reads supported ZIP, RAR, 7z, tar and compressed-tar archives. Built-in
gzip, bzip2, xz and zstd filters are enabled. Before opening the destination buffer, `e` performs a read-only validation of
the highlighted file's contents, paths, payloads and extraction limits.
Directories, unsupported, encrypted, corrupt or unsafe archives report an
explanation and leave the main view open; no staging is created.
Detection uses contents, not filename extensions. Validation runs again during
extraction in case the archive changes after the prompt opens. Extraction stages all entries before publication and
refuses existing top-level destinations rather than merging or overwriting them.
It refuses absolute paths, parent traversal, backslash separators, control
characters in names, names reserved for fff's staging directories, duplicate
entries, links, and special files. Colons are allowed. Extracted files keep the
archive's permission bits, limited by your umask. Limits are 10,000 entries, 64 path components,
2 GiB per file and 4 GiB total extracted bytes. Exceeding a limit discards staging.

