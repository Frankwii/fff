# File operations

fff copies, moves, links, renames, creates, extracts and deletes files. Every
change can be undone with `u` (except a permanent deletion, which always asks
first), and nothing is ever overwritten without asking.

## Where these keys work

The file operation keys are **Normal-mode keys in the results**. They don't
work:

- **while typing** (Insert mode), where they're just letters;
- **while the query has the focus** (after Escape from Insert mode), where
  `x`, `p`, `u` and the rest edit the query as in Neovim;
- **in the marked list** (Ctrl+k), which is for looking at and unmarking
  your marks.

If a key edits your query instead of acting, press Escape to give the
results the focus. You can bind the operations elsewhere on the [Keys
screen](KEYS.md).

## Marks

Marks collect the files an operation acts on. They stay as you browse, so
you can mark files in several folders, or in one [tab](BROWSING.md#tabs),
and act on them from another.

| Key | Does |
| --- | --- |
| Space | Mark or unmark the highlighted row, and move on to the next |
| `m` | Mark everything listed (all matches, not only what's on screen) |
| `M` | Invert the marks of everything listed |
| Ctrl+m | Clear every mark |
| Ctrl+k | Go into the marked list, and back |

With the mouse, drag over rows to mark them. In the marked list, `j`/`k`
move, Space unmarks, and `l` goes to a mark's folder.

## Which files an operation acts on

There are two kinds of operation:

- **Copy, move and link** (`p`, `v`, `s`) bring the **marked** files into
  the folder you're in. With nothing marked, they do nothing.
- **The others** (`x`, `X`, `E`, `e`, `gx`) act on the **highlighted**
  entry, or on the marks when the highlighted entry is one of them. When
  you've marked some files but highlighted another, fff asks: the current
  one (`c`), the selection (`s`), or all of them (`a`).

## Copy, move and link

1. Mark the files.
2. Go to the destination folder.
3. Press `p` to copy, `v` to move, or `s` to make symbolic links to them.

When nothing would be overwritten, it happens at once, and the footer says
how to undo it (`Copied 2 entries; u undoes`). When a file of the same name
is already there, a window lists what would be replaced, with sizes:
Enter replaces them (the old ones go to the trash), Escape cancels
everything. A folder is replaced as a whole, never merged.

Moving across filesystems isn't supported: copy, then delete.

## Trash and delete

| Key | Does |
| --- | --- |
| `x` | Move to the trash, at once |
| `X` | Delete permanently, after listing what goes with its size |
| `u` | Undo; after `x`, bring the files back |

`x` asks only about entries that are very large (over 10 GiB by default),
which would take up the space for weeks, and those that can't go to the
trash: for them, `d` deletes permanently, `t` trashes anyway, and Escape
cancels everything.

`X` shows the entries with their sizes, largest first. Enter deletes them,
Escape cancels. A permanent deletion can't be undone.

### The trash

fff keeps its own trash in `~/.local/share/fff/trash`, and a
`.fff-trash-UID` folder at the top of each other filesystem you delete
from (moving into the trash is always a quick rename, never a copy). It's
laid out like the desktop trash, so you can find things there by hand too.

- **`gt`** opens the trash like any folder. Press it again to go to the
  next filesystem's trash.
- **`R`**, in the trash, restores the highlighted or marked entries to
  where they came from. If something is in their place now, it asks:
  Enter replaces it, `k` keeps both (`name (1).ext`), Escape cancels.
- Entries are deleted from the trash after 30 days, when fff starts.

The `[trash]` section of the [configuration](CONFIGURATION.md#trash) sets
the number of days and the size that asks first, or turns the trash off
(then `x` deletes permanently, after the same review as `X`).

## Undo

`u` undoes the last file operation of the session; press it again to undo
the one before, up to 100. It works on everything but permanent deletions:
trashed files come back, moved and renamed files go back, and copies, new
files, links and extracted files go to the trash.

With the trash turned off, copies and new files can't be undone, since
undoing them would delete them for good.

An undo only happens if nothing has changed since: if a file was edited,
renamed or replaced in the meantime, fff refuses and says why. It's all or
nothing: an undo never leaves things half done.

## Rename

`r` opens a Neovim buffer with the paths of everything listed, one per
line. Edit the names, then `ZZ` to rename them all (or `ZQ` to cancel).
Filter first to rename just some files: `r` lists only what the query
matches.

- Keep the number and order of lines: each line is one file.
- Change only names, not which folder a file is in.
- Renaming a folder and the files in it at once works.

If anything would go wrong (a name already taken, two files with the same
new name), nothing is renamed.

## Create

`n` asks whether to make a file (`f`) or a folder (`d`), then its name.
Enter creates it, Escape cancels.

## Extract archives

`E` extracts the highlighted archive (or the marked ones): zip, tar and
compressed tars, 7-Zip, RAR and more. A prompt asks for the destination
folder; leave it empty to extract each archive next to itself. fff checks
every archive first and refuses unsafe ones, such as entries that would
land outside the destination.

## Long operations

Most operations finish at once. One that takes longer moves to the **jobs
list** below the marks, with a progress bar, its speed and the time so
far. You can keep browsing and start other operations meanwhile; one that
would touch the same files is refused until the first one finishes.

| Key | Does |
| --- | --- |
| Ctrl+j | Go into the jobs list, and back |
| `x` (in the jobs list) | Stop the highlighted operation and undo what it did |
| `u` | Undo the latest operation, stopping it first if it's still running |

A copy or extraction is written to a hidden `.fff-stage-…` folder first and
appears under its name only when complete, so you never see a half-copied
file. Leaving fff while operations run asks first.

## How fff keeps your files safe

- **A confirmation is for the files you saw.** If a file changes between
  the confirmation and the operation, the operation stops.
- **All or nothing.** If something fails halfway, what was done is undone.
- **No following links.** Operations act on symbolic links themselves,
  never on what they point to.
- **No surprises across mounts.** A folder with something mounted inside
  it is never copied or deleted through it.
- **Interruptions wait.** Closing the terminal during an operation lets it
  finish (or roll back) first, and leaving fff with `q` or Ctrl+c asks.

Copies keep contents, permissions, links and timestamps, but not
ownership, extended attributes or ACLs.

## Opening files and running programs

Opening files (Enter, `L`), editing them (`e`) and running a program on
them (`gx`, `gX`) are described in [Opening files](OPENERS.md).
