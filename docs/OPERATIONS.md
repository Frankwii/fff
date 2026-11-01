# File operations

Version 0.3.0 adds operations from results and the marked list. These keys act
only in Neovim Normal mode (the marked list always accepts them). Insert mode
keeps them as text. The basic editor can invoke operations from the marked list;
its prompt buffers use Ctrl+s to apply and Escape to discard.

| Key | Action |
| --- | --- |
| `v` | Move all marked paths into the current directory, after confirmation |
| `p` | Copy all marked paths into the current directory, after confirmation |
| `s` | Create absolute symlinks to all marked paths here, after confirmation |
| `x` | Delete the highlighted path or the marked paths, with confirmation |
| `r` | Open a batch rename buffer for every currently filtered result |
| `e` | Extract the current or marked archives into a prompted directory |
| `n` | Choose file (`f`) or directory (`d`), then enter a new name |

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
only) instead of clipping a key. Buffers list `ZZ` / `ZQ`
with Neovim and `Ctrl+s` / `Esc` with basic editing. Destructive confirmations
(deletion, overwriting) use the `error` color; choices and buffers use
`marked`.

A successful operation reports what it did in the footer with a `✓` in the
`success` color, such as `Deleted 2 entries` or `Renamed notes.md`. An operation
that took effect only partly (a deletion stopped partway) reports a warning in
the `error` color, as errors do.

## Buffer interactions

Rename, extraction destinations, and new names follow the same lifecycle as
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

Executing commands on chosen files remains unimplemented.
