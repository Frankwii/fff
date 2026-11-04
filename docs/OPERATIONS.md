# File operations

Operations act on results and the marked list. These keys act
only in Neovim Normal mode (the marked list always accepts them). Insert mode
keeps them as text.

| Key | Action |
| --- | --- |
| `v` | Move all marked paths into the current directory (asks before overwriting) |
| `p` | Copy all marked paths into the current directory (asks before overwriting) |
| `s` | Create absolute symlinks to all marked paths here |
| `x` | Move the highlighted path or the marked paths to the [trash](#trash-and-deletion) |
| `X` | Delete the highlighted path or the marked paths permanently, after confirmation |
| `u` | [Undo](#undo) the last file operation |
| `R` | In the [trash](#the-trash-folder), restore the current or marked entries |
| `r` | Open a batch rename buffer for every currently filtered result |
| `E` | Extract the current or marked archives into a prompted directory |
| `n` | Choose file (`f`) or directory (`d`), then enter a new name |
| `e` | Open the current or marked paths in `$EDITOR`, then come back |
| `gx` / `gX` | Run a program on the current or marked paths; come back / exit into it |

`v`, `p`, and `s` show **No files selected** when nothing is marked. They never
fall back to the highlighted path. With nothing to overwrite they act at once,
and the message says how to undo them (`Copied 2 entries; u undoes`). Target
names retain their source basenames. For symlinks, existing files, directories
and dangling symlinks are collisions: the link is refused and nothing is
overwritten. Two marked entries with the same name are refused.

When a move or copy collides, it asks first: an **Overwrite on move** /
**Overwrite on copy** window lists the marked entries with their sizes and the existing destinations with their sizes, like the deletion
confirmation. **Escape** cancels the whole operation; **Enter** moves or copies
everything and overwrites the listed entries. A directory is overwritten as a
whole, never merged. The listed entries are detached into a private staging
directory (for copies, only after every copy is
staged), and moved to the [trash](#trash-and-deletion) (or deleted, when it is
off or unavailable) only after every entry has been published; any failure
rolls the publications back and restores them. Entries changed since
confirmation, collisions that appear after confirmation, moving or copying a
path into its own directory, and overwriting an entry that contains a selected
path all abort the operation. To operate
on paths from another directory, mark them before navigating to the destination.
A move, copy, link, deletion or extraction takes the marks of the entries it
acts on as it starts; other marks stay, so the next selection can be marked
while it runs. Deletion also unmarks what was below the deleted paths, so
deleting an unmarked current path keeps the selection. An operation that
fails, rolls back, or is stopped with `u` marks its entries again (one
stopped with `x` in the [jobs list](#progress-and-stopping) does not), and so
does undoing it with `u`. Rename and restore keep the marks of the entries
they act on, which follow them to their new names.

Operations preserve original filesystem paths independently of abbreviated labels
and icons. Copy, move, symlink and delete support names containing non-UTF-8 bytes.
Overlapping selections (a directory and one of its descendants) are refused;
select each tree once. Moving/deleting the directory of any tab or its
ancestors, and copying a directory into
itself are refused. Moves across filesystems are refused; copy first instead.
Trees containing a mount point are refused for every operation, so a mounted
share is never copied or deleted through its parent. Mount points are detected
with the kernel's mount-root flag, which also catches bind mounts from the same
filesystem, and by device number for other filesystems and subvolumes. Recursive
copy and deletion check again at every directory, in case something is mounted
after validation. Sockets, devices and other
special files are refused anywhere in a tree, as are trees nesting more than
128 levels below the selected entry or with more than 100,000 entries. Symlinks are copied or
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
`success` color, such as `Moved 2 entries to the trash; u undoes` or `Renamed
notes.md`. An operation that took effect only partly (a deletion stopped
partway) reports a warning in the `error` color, as errors do.

## Progress and stopping

Operations run on a thread of their own, and so do the checks before them
(sizing and inventorying the chosen trees for a confirmation). Most finish at
once and look instant.

Checks still running after 150 ms show a sheet with their title (`Moving
tree to the trash`), `checking the entries` and the time, since a question may
follow them. **Ctrl+c** stops them; nothing has changed yet.

A confirmed operation still running after 150 ms goes on in the **jobs list**,
the lower half of the marks pane while anything runs, redrawn ten times a
second. Each job shows its title (`Copying 3 entries`), a bar and figures:

- `checking the entries`, while every chosen tree is inventoried and compared
  with what was confirmed; nothing has changed yet;
- bytes copied or extracted, with the total (and a percentage) when known, the
  speed and the time so far;
- entries deleted out of all of them, for a permanent deletion.

Meanwhile fff stays usable: browse, search, mark, preview, and start other
operations. A second operation runs alongside the first unless they collide,
that is, unless one changes what the other reads or changes (an entry, or a
folder holding it): copying `photos` while deleting `photos/2019`, or creating
`notes` in a folder a copy is about to place `notes` in. Then it is refused
with a message (`Copying photos uses photos; wait for it to finish`), and so
is `u` on an operation that touches the running one's entries. Reading the
same entries twice is no collision: two copies of one folder run together.
While an operation runs, the folders it moves away or deletes cannot be
entered (going back skips them), and the files it changes are not opened,
edited or given to a program (`gx`). A copy or extraction is written into a
hidden `.fff-stage-*` folder beside its destination and appears under its
name only once complete; with hidden entries shown, those folders and the
folders in them are drawn in the `staged_folder` color (pink in the built-in
palettes) and the files in them in `marked`, and content searches and live
updates leave them out. Live updates rescan folders at most once a second
while operations run.

`toggle_job_list` (**J**, Alt+J) focuses the jobs list and leaves it again.
There **j** and **k** (or the arrows) move, Esc goes back, the marked list's
key (**K**) goes to that list, and **x** stops the
highlighted operation and undoes what it had done: it stops as soon as
stopping changes nothing (during the checks, while a copy or an extraction is
still being staged, which is discarded, or between entries of a permanent
deletion, where what remains is put back as after a failure: `Stopped after
deleting 1 of 3; …`). A step that cannot stop (publishing staged copies,
moves, renames, trashing) is quick and finishes, and is then undone at once,
as `u` would. What was deleted permanently cannot come back. An undo cannot
itself be stopped. **u** there does the same and marks the operation's entries
again; in the results, `u` stops and undoes the latest operation in the same
way when it is still running.

Leaving fff while operations run (`q`, Ctrl+c, or any key that finishes) asks
first: **Enter** stops them as `x` would, without undoing what they finished,
and fff leaves once they have ended; **Esc** stays. Termination signals wait
for them, as described [below](#confirmations-and-interruptions).

What was deleted permanently cannot come back: undoing a selection where some
entries were deleted permanently brings back the others and says how many
cannot return, and a purely permanent deletion is not on the undo list, so
`u` after it undoes the operation before. The undo list lasts for the session.

Undoing changes nothing unless every entry is still the one the operation left
(the same file, by identity, even if edited since) and its old place is free:
an entry renamed, removed or replaced since, or a new file where the original
was, stops the undo with a message, and the operation leaves the list. Steps
are undone last to first; if one fails, those already undone are redone, so
an undo happens completely or not at all. Undoing that would move a tab's
directory or one of its ancestors, or that collides with a running operation,
is refused (and stays on the list): go up first, or wait. `u` undoes the
operation that started last, so with several running or ended out of order,
the order they were started in counts. Entries undone into the trash follow the trash's rules; with the trash
turned off, copies and new entries cannot be undone.

In the results' query, `u` therefore no longer undoes query edits in Normal
mode; `:u` (and Ctrl+r to redo) still does, or rebind `undo` to give `u` back
to Neovim.

## Confirmations and interruptions

A confirmation applies to the files that were shown, not merely their paths.
Delete, move, copy and link windows record each listed tree (identity and
metadata of every entry), and execution refuses any tree that differs, for
example after an edit or after a symlink in its path was repointed.

Operations run to completion or roll back: a terminal hangup, `SIGTERM` or
`SIGINT` arriving during operations is held until every one has finished,
then takes effect. Signals ignored when fff started (such as under `nohup`) stay ignored;
fff then ends once the operation has finished, as its terminal is gone. Copies
and extracted files are written in full and synced to disk before they appear
under their names, so no interruption leaves a truncated file at a destination.
A kill that cannot be held (`SIGKILL`, power loss) can leave hidden
`.fff-stage-*` directories holding data (an entry already moved to the trash
stays there, with its record); when fff enters a directory with such a
leftover from a process that no longer runs, the status line names it.
Rollbacks only move back entries this operation published, verified by identity.
The destination directory of a move, copy or link is bound to the confirmation
too. An overwritten entry that changes while detached is kept in its staging
directory (and named in the message) instead of being trashed or deleted.

## Guarantees and limits

Permanent deletions (`X`, the **d** answer, and overwrites when the trash is
off or unavailable) are as final as `rm`. Trashed entries can be undone while
the session lasts and recovered by hand until they are purged. Everything else
is designed so that only confirmed data is ever removed.

Parents are pinned by directory descriptors and nothing follows a symlink.
Destinations are published with no-replace renames; copies and extractions are
staged completely before anything is published. Every tree is inventoried
(identity and metadata of each entry) during validation and checked again right
before each step that changes it; a difference cancels the operation. A failure
partway through rolls earlier steps back, and anything that cannot be rolled
back is named with its path.

Copies preserve file contents, ordinary permission bits, symlinks and
access/modification times. Like `cp -r`, `rm -r` and overwriting, fff copies,
deletes and replaces directories without owner write permission: such a
directory gains it only for the instant it is renamed into or out of staging
(Linux requires it to change a directory's parent) and is left with its exact
mode. A plain move of such a directory to another folder fails, as with `mv`. They are not filesystem snapshots: ownership,
extended attributes, ACLs, hard-link relationships and sparse allocation are not
preserved. Concurrent writers can still race an operation; stop writers when a
stable copy matters. Operations, their checks and archive validation before
the extraction prompt run on other threads while the screen shows their
[progress](#progress-and-stopping).

## Archives and system requirements

The Linux operation engine uses `openat2`, mount-root detection through `statx`
(Linux 5.8+) and no-replace rename.
Building/running requires **libarchive**, including its development linker files
when building (for example `libarchive-dev` on Debian/Ubuntu). Browsing also links
to this library. No shell or external extractor is invoked by fff.

libarchive reads supported ZIP, RAR, 7z, tar and compressed-tar archives. Built-in
gzip, bzip2, xz and zstd filters are enabled. Before opening the destination buffer, `E` performs a read-only validation of
the highlighted file's contents, paths, payloads and extraction limits.
Directories, unsupported, encrypted, corrupt or unsafe archives report an
explanation and leave the main view open; no staging is created.
Detection uses contents, not filename extensions. Validation runs again during
extraction in case the archive changes after the prompt opens. Extraction stages all entries before publication and
refuses existing top-level destinations rather than merging or overwriting them.
It refuses absolute paths, parent traversal, backslash separators, control
characters in names (including C1 controls such as U+009B in UTF-8 names), names reserved for fff's staging directories, duplicate
entries, links, and special files. Colons are allowed. Extracted files keep the
archive's permission bits, limited by your umask; extracted directories get the
default mode (0777 limited by your umask). Each archive is limited to 10,000 entries, 64
path components, 2 GiB per file and 4 GiB extracted in all. Exceeding a limit discards
staging.

