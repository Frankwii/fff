# fuzzy file finder

A Rust terminal file picker with fuzzy filename filtering, directory navigation,
live search depth, pinned roots and persistent marked paths. Version **0.24.0**.
The executable is **`fff`**; the crate package is **`fuzzy-file-finder`**.

## Build and run

Install from a local checkout:

```sh
cargo install --path . --bin fff
```

Or build and run directly:

```sh
cargo build --release
./target/release/fff
./target/release/fff ~/notes ~/papers --depth 2 --glob '*.tex' --glob '*.pdf'
./target/release/fff pick --relative --null > paths.bin
```

On Arch Linux, install a release as a package instead (the binary, manual
pages and completions, removed with `pacman -R`), built from its tag on
GitHub:

```sh
cd packaging/aur
makepkg -si
```

`fff setup --desktop` and `--browser` record the path of the `fff` that ran
them: after switching to the package, run them again as `/usr/bin/fff`.
Each release updates `packaging/aur`; run `makepkg -si` there again to update
(see [packaging](DEVELOPMENT.md#packaging)).

Run in a terminal with `/dev/tty`, on Linux or macOS (where the [desktop
integration](DESKTOP.md) and live updates are not available). The interface uses the controlling
terminal, never stdout. Put the binary on your PATH as `fff` and add the
[shell integration](#shell-integration) line so that leaving fff can change your
shell's directory. `-h` summarizes options; `--help` includes defaults, examples
and exit codes.

For a guided start, run `fff setup`. It lists the programs fff uses and
which of them are installed, with the command that installs each missing
one (pacman, apt, dnf or Homebrew), then asks whether your terminal shows
fff's icons and which installed programs open documents, images, media and
office files (`open` first on macOS, so the defaults are the apps macOS
uses). It shows what it would add to the [configuration
file](CONFIGURATION.md) and writes it once you confirm. A session
started without a configuration file suggests it.

## Commands

| Command | What it is for |
| --- | --- |
| `fff [ROOTS]`, or `fff roam [ROOTS]` | Browse; leaving changes your shell's directory. |
| `fff pick [ROOTS]` | Browse and print the chosen paths, for scripts and `$(...)`. |
| `fff seek [ROOTS]` | Choose files from below ROOTS without navigating ([details](#choosing-files-fff-seek)). |
| `COMMAND \| fff` | Choose among the paths another program writes to stdin, and print the choice ([details](#choosing-from-another-programs-paths-command--fff)). |
| `fff window [PATH]` | Browse in a terminal window of its own, as the desktop's file manager ([details](DESKTOP.md#opening-directories)). |
| `fff explorer open\|open-multiple\|folder\|save [PATH]` | Answer a file dialog ([details](DESKTOP.md#fff-explorer)). |
| `fff setup` | A guided start: checks the programs fff uses and writes a first configuration from a few questions ([details](CONFIGURATION.md#guided-start-fff-setup)). |
| `fff setup --OPTION` | Set fff up: `--init`, `--completions`, `--man`, `--desktop`, `--browser`, `--check-config`. |

The command only chooses how a session starts: `gm` turns a running session
into `roam`, `pick` or `seek` without losing your place (see
[switching programs](#switching-programs)). Each can also search what files
contain, PDFs included: Tab opens the [contents query](#searching-contents).

`fff` without a command is `fff roam`, unless paths come in on stdin. The
search, interface and configuration options (`--depth`, `--glob`,
`--palette`, `--config`, ...) work with every command, before or after its
name.

## Shell integration

`fff roam`, the default, changes directory on exit: `q` leaves you where you
started, `!` takes you to the directory you were browsing, `g!` into the
highlighted directory, and Enter on a file opens it and leaves you in its
folder. Ctrl+number opens the file in that row and leaves the same way.

`fff pick` prints the selection instead, for scripts and command substitution,
as described under [returning paths](#browsing-and-returning-paths):
`nvim "$(fff pick)"`, `fff pick -0 | xargs -0 ls -ld`. Scripts that already
list paths can [pipe them in](#choosing-from-another-programs-paths-command--fff)
instead: `nvim "$(rg -l TODO | fff)"`.

A program cannot change its parent shell's directory, so the shell does it, as
with nnn's quitcd scripts and zoxide. Add one line to your shell's startup file
(the [guided `fff setup`](CONFIGURATION.md#guided-start-fff-setup) offers to):

```sh
eval "$(fff setup --init zsh)"     # ~/.zshrc (or $ZDOTDIR/.zshrc)
eval "$(fff setup --init bash)"    # ~/.bashrc
fff setup --init fish | source     # ~/.config/fish/config.fish
```

This defines a shell function named `fff` that runs the real binary with
`--cd-file` (a private temporary file), then changes to the directory fff wrote
there, if any. Arguments pass through unchanged, so `fff pick` keeps printing
paths. Without the function, `fff roam` prints the chosen directory instead,
so `cd "$(command fff)"` also works. When that output goes to a terminal,
where nothing reads it, a note on stderr gives the line to add for `$SHELL`
(bash, zsh or fish, else all three), once: fff then leaves
`$XDG_STATE_HOME/fff/shell-hint` (`~/.local/state/fff/shell-hint`) behind,
and says it again only after that file is deleted. Leaving where the session
started prints nothing, so no note.

fff starts in the current directory, on the row last highlighted there (see
[remembered locations](CONFIGURATION.md#remembered-locations)).

fff can also be the desktop's file manager and file chooser: `fff setup
--desktop` opens directories from other programs in fff, and `fff setup
--browser` lets Firefox and other portal users pick files with it, through
`fff explorer`, a mode for answering file dialogs. See
[desktop integration](DESKTOP.md).

Every text field is a clean embedded Neovim (`nvim` must be on PATH). Fields start in **Normal mode**,
except prompts made for typing (new names, the go-to prompt, the gx/gX command
without zsh, the workspace form)
and the query of `fff pick`, `fff seek`, `fff explorer save` and a list read
from stdin, which start in **Insert mode**.
Use `j`/`k` to browse and `i` or `a` to type a filter. An empty query says
how to type: `type to filter names…` in Insert mode, `i to filter names…` in
Normal mode, where typing runs commands (likewise the contents bar, and a save
dialog's name). Directory navigation
preserves your current editing mode. See [editing](EDITING.md).

## Browsing and returning paths

This section describes what each confirmation returns in `fff pick`; in
`fff roam` the same keys choose a directory instead (see above).

Without root arguments, browsing starts in the invocation directory and follows navigation. Explicit roots stay pinned: results combine their descendants with
those of the current directory. Overlapping paths appear once at their minimum
depth. Default depth is **1** (**3** in `fff pick`, **6** in `fff seek`); immediate children are depth
1. Shallower matches come first, followed within each depth by directories
and fuzzy ranking; `fff pick` and `fff seek` rank by fuzzy score
first, then depth. The depth is shown on the results' bottom border, under the count.
Matching is a case-insensitive ordered subsequence of the path below the
result's root, folders included, scored as fzf does: the best alignment of
the query's letters, so runs and word starts (after `_`, `-`, `.`, `/`, a
space or a lowercase-to-uppercase step) beat letters scattered from the
start, however long the name. `pdestab` finds `pde/notes/stability.tex`, and
a `/` is a letter like any other, so `pde/stab` finds it too and `pde/` lists
what is inside folders matching `pde`. Skipping the rest of a folder's name
and the folders after it costs the same however long they are, when the
match goes on at a word start, so
`turinpdf` ranks `turing_patterns/figures/heat.pdf` (two runs) above
`the_C_programming_language_kernighan_ritchie.pdf` (letters scattered
through one name). Each letter matched in the basename counts a little
more, so `notes` lists the folder `notes` before the files inside it. Latin
accents are ignored unless the query has some: `evaluacion` finds
`Evaluación`. The matched letters are highlighted along the whole label.
Depth still weighs against the match, and directories come first among
matches as good (see [sorting](#sorting)).

Enter does the same in either editing mode:

- **`fff roam`:** on a file, fff **opens it and leaves** (`open_and_quit`):
  a terminal opener (an editor, a pager) takes over this terminal in fff's place,
  like `gX`, and any other opener starts detached; the shell then changes to
  the file's folder (also for deep results: `notes/more/a.pdf` leaves you in
  `notes/more`). On a directory Enter does nothing: `l` goes into it, and
  `g!` leaves into it (`quit_into_highlighted`).
- **`fff pick`:** Enter prints the marked paths in insertion order plus the
  highlighted one once (`print_highlighted_and_marks`); Shift+Enter prints
  only the highlighted one, Ctrl+Enter only the marks, and `!` the browsing
  directory. With nothing listed, Enter prints the browsing directory.
- **`fff seek`:** Enter leaves into the highlighted file's folder.

To move around, use `l` and `h` in Normal mode (Alt+l and Alt+h while
typing): `l` goes into a directory, and on a file into its folder with the
file highlighted. `L` **opens a file** with its
[opener](OPERATIONS.md#opening-files) in the background while fff keeps
the keyboard.

The keys that open or finish fit together:

| Key | Acts on | Then |
| --- | --- | --- |
| `l` | The highlighted entry: go into a directory, or a file's folder | fff stays |
| `L` | The highlighted file, opened in the background | fff stays |
| Enter | The highlighted file, opened (`fff roam`); the marks and the highlighted entry, printed (`fff pick`) | fff leaves |
| `g!` | The highlighted directory (`fff roam`) | fff leaves into it |
| `!` | The folder you are browsing | fff leaves into it |
| Ctrl+Enter | The marks, returned (`fff pick`) | fff leaves |
| `q` | Nothing | fff leaves where you started |

[`fff explorer`](DESKTOP.md#fff-explorer), the mode for answering file
dialogs, keeps these meanings: Enter chooses a file and enters a folder, `!`
takes the folder you are in, Ctrl+Enter the marks (in dialogs for several
files), and Ctrl+s saves. A folder
opened from the desktop (`fff window`) has no shell to leave into: Enter enters a
folder, and `!` starts a shell in the folder you are in
([details](DESKTOP.md#opening-directories)).

Space toggles a mark in Normal mode; marks survive navigation and
filtering. On a [contents match](#searching-contents) the mark is its file's,
and the highlight moves on to the next file. Ctrl+m clears every mark.
Ctrl+Enter finishes with the marks only, and works only where the answer can
be several paths: `fff pick` returns them (as do `fff explorer open-multiple`
and `folder --multiple`). Elsewhere (`fff roam`, `fff seek`, other dialogs)
it does not work and the status line says so; there the marks are for file
operations and `gx`. Enter while typing with a query matching nothing stays
open, and a short status message says nothing matches.
Missing marks block confirmation until unmarked.

### Tabs

Every session has three tabs, like nnn's contexts, numbered in the top-right
corner of the results, the active one in the `@` badge's colors. All start in
the session's first directory. Up / Down switch to the next / previous tab (Ctrl+Tab to the next too),
wrapping around; each tab keeps its own directory and Ctrl+o / Ctrl+i history,
while marks, the query, depth, hidden entries, exclusions and filters are
shared. Mark files in one tab, switch, browse somewhere far away, then move
or copy them there with `v` / `p`. Switching to a tab in another directory lists
it again with an empty query; a tab whose directory is moved follows it, and one whose directory vanished falls
back to the nearest existing parent. F2's session details list every tab's directory. `fff seek`, which cannot
change directory, has no tabs.

Rows are numbered on screen, and functions act on a numbered row: a
digit in Normal mode highlights it (`highlight_row`), Alt+number goes to
it as `l` does, into its folder or to its file's folder (`go_to_row`; in
`fff seek`, which does not navigate, it only highlights it, as
`highlight_row`), and Ctrl+number does what Enter does on it: it opens
its file and quits in `fff roam` and `fff window` (`open_row`; on a
directory it only highlights it), prints the marks and that row, a folder
too, in `fff pick` and on a list read from stdin (`print_row_and_marks`),
and leaves into its file's folder in `fff seek` (`quit_into_row`). Labels refer to the last fully rendered
page: 1–9 select that position, out-of-range digits clamp to the last row,
and 0 selects the last row (plain 0 stays Neovim's). With no visible rows,
they do nothing.

Output defaults to absolute, newline-terminated paths. `--relative` uses the
invocation directory as a stable base; `--null` (`-0`) writes NUL-delimited records.

Rows in `fff pick` show bare names: when listed entries share
a name, each gains the fewest trailing parent directories that tell it apart
(`folder1/main.pdf`, `folder2/main.pdf`), independently of the query, and the
labels follow the listing as you navigate. The footer shows the highlighted
entry's path. In `fff roam`, `fff seek` and `fff explorer`, labels are paths
relative to the browsing directory, with repeated leading parent steps
shortened to `.../`. `gl` in Normal mode (`toggle_labels`) switches between
the two for the session, and the choice stays when `gm` changes program.
Labels are presentation only: output paths are never shortened.

### Sorting

Rows are ranked by how well they match minus a penalty for their depth,
growing as its logarithm, so that the first levels down cost most: a much
better match deeper down comes before a poor one above it, and an empty
query lists by depth, directories first. `[ui] temperature` sets how much
depth costs (see [configuration](CONFIGURATION.md#sorting)). `fff
seek`, which chooses files from deep below, puts the best match first
whatever its depth. `gs` in Normal mode (`show_sorting`) opens the
sorting picker instead: type or move to a criterion and Enter sorts by it,
Tab switching between descending and ascending first. Descending (↓), as
the picker opens, is each criterion's own order, as listed below;
ascending (↑) reverses it. The query still
decides what is listed; the sort only how, with the ranking breaking its
ties:

- `name`: alphabetically from A, ignoring case, numbers by value (`file2`
  before `file10`);
- `size`: the largest first; a folder's is the files below it, as the size
  column shows it, once added up (until then it comes last). While the rows
  are sorted by size, every folder listed is added up in the background,
  after those on screen and near it, and the rows are ranked again by the
  totals found, at most every two seconds (`RESIZE_EVERY`), so that rows do
  not jump under the highlight, which stays on its entry;
- `modified`, `accessed`, and `created` where the filesystem records it:
  the newest first;
- `extension`, then name;
- `kind`: folders, then files, then links;
- `depth`, the shallowest first, then name;
- `shallowest`: depth, folders first, then the best match;
- `log`: the best match minus the penalty for depth, as the programs but
  `fff seek` rank;
- `program`: back to the program's own order, which has no direction.

The results' bottom border names the order (`sorted by size ↓`). `[ui] sort`
and `sort_descending` choose the one sessions start in (see
[configuration](CONFIGURATION.md#sorting)).

`--glob` is a file filter: only files whose basename matches it are listed, and
directories are hidden but still traversed. Repeat it to allow several patterns
(`--glob '*.pdf' --glob '*.epub'`); a file matching any of them is listed. `gF`
in Normal mode edits the filters during the session, one glob per line;
`gf` turns them off and on again, keeping them. Changing them re-lists
the files already found without rescanning.

Hidden entries are off by default. Exclusion rules, from `[exclusions] rules`
in the [configuration](CONFIGURATION.md#exclusions) and then each
`--exclude`, are enforced; a matching directory prunes its subtree. Rules match
basenames or scope-relative paths, with literal separators; one starting with
`/` or `~/` matches absolute paths (`~/backup`). No automatic `.gitignore`
integration is enabled.
The query box's bottom border shows `filters on` and `exclusions on` only
while filters or exclusion rules exist and are applied (exclusions not when they
are exactly the configuration's); F2's session details list their globs. It also shows discovery progress while a scan runs and for three
seconds after it completes (`◌ partial` stays while a scan stopped at
`max_paths`). `files only` or `folders only` appear beside the results' count.
Symlink directories can be selected or entered; recursive discovery does not
follow them.

### Choosing files: `fff seek`

`fff seek [ROOTS]` is a static file choice: it lists only the files below the
current directory (or ROOTS), disables location changes, and never returns
the current directory as an empty-results fallback. It takes the search
options (`--depth`, 6 by default, `--glob`, `--exclude`, `--hidden`), and ranks
the best fuzzy matches first, however deep. Choosing leaves into
the chosen file's folder; only `fff pick` prints paths. For a printed choice
of files, give `fff pick` a `--glob`, which lists only matching files:

```sh
fff pick ~/papers --depth 4 --glob '*.pdf' -0 | xargs -0 zathura
```

Discovery still traverses directories within the depth limit, and the query
starts in Insert mode, ready to type. Result rows show their path below the
folder searched; `gl` switches to bare file names, with only the parents
that tell same-named files apart, as in `fff pick`.

### Choosing from another program's paths: `COMMAND | fff`

With stdin a pipe or a redirected file, `fff` without a command chooses among
the paths written there, one per line, instead of browsing, and prints the
choice to stdout, so a script that already lists paths gains fff's choice of
them:

```sh
nvim "$(rg -l TODO | fff)"
git ls-files | fff | xargs -r nvim
zoxide query -l | fff
fd -0 -e pdf | fff -0 | xargs -0r zathura
```

- **The rows** are the paths as the list orders them, best matches first once
  you type. Relative paths are below the current directory and shown that
  way; others are shown whole. Lines naming no existing file or folder are
  skipped and counted on the status line, and a path named again is listed
  once. Paths appear as they are read, so a slow command can still be
  writing while you type. `-0` reads NUL-separated paths, for `fd -0`,
  `find -print0` and names with newlines.
- **Choosing** prints paths as the list spelled them (`./src/main.rs` stays
  `./src/main.rs`), NUL-terminated with `-0`: Enter prints the highlighted
  path and the marks, Shift+Enter the path alone, and Ctrl+Enter the marks. A double click is Enter. `q` prints
  nothing and Ctrl+c exits with 130, as in `fff pick`.
- **What works** is the choice and how it looks: the names and
  [contents](#searching-contents) queries, marks, the preview, sorting
  (`gs`), labels (`gl`), hiding folders or files, palettes, F1 and F2. Nothing
  navigates or lists anything else: no folders to go to, depth, hidden-entry,
  filter or exclusion keys, rescans or live updates; no file operations,
  undo, opening or running programs; no switching program (`gm`) or opening
  a workspace. Their keys say so.
- **The command line** takes only the interface and configuration options
  (`--palette`, `--no-icons`, `--config`, `--no-config`) besides `-0`: ROOTS,
  `--browse`, `--depth`, `--glob`, `--exclude`, `--hidden`,
  `--no-exclusions` and `--workspace` are usage errors (exit 2), since the
  list is listed as read; filter in the command that writes it. So is a
  command (`fff pick`, `fff seek`, ...) with paths on stdin, which would
  leave them unread; `fff setup` with an option reads nothing and ignores
  stdin, and without one asks its questions on a terminal. A terminal,
  `/dev/null` or a closed stdin is no list: `fff` browses as usual.
- The [shell function](#shell-integration) passes through unchanged: the
  session changes no directory.

### Searching contents

Every program, [file dialogs](DESKTOP.md#searching-contents-in-a-dialog)
included, searches what files contain as well as their names. Tab opens a second query, **Contents**, beside the
names query, which moves to the left as **Names**; Tab switches
between them, in either mode, each keeping its own. While the contents query
is set, the rows are the files listed that contain it, with **one row per
match**: the file, the match's page (`p. 12`), how far into an EPUB it is
(`42%`) or its line (`l. 40`), and the line it is on, the match
highlighted. Cleared, they are the entries again, and its bar stays while it
is set or typed into. The preview pane shows the lines around the
highlighted match, which PageDown and PageUp scroll from there, up to 500
lines either side, a text file's in its [syntax
colors](CONFIGURATION.md#syntax-colors) once the search has finished. **Names** filters the files by name as usual, so it
chooses which files are read, and `--depth`, `--glob`, `--exclude` and
`--hidden` apply as they do to names. For a printed choice of documents by
what they contain, use `fff pick`:

```sh
fff pick ~/papers --depth 6 --glob '*.pdf' --glob '*.djvu' --glob '*.epub' -0 | xargs -0 zathura
```

Escape clears the query being typed into (the other once it is empty).
Enter acts on the match's file as on any row: in `fff pick` it prints it
with the marks). Marking a match marks its file and moves
on to the next file. `L` opens it in the background and keeps fff open; an
[opener](CONFIGURATION.md#openers) with `at_grep_match` opens it at
the match, zathura on its page with the query highlighted.

The contents query is a literal, not a regular expression. It is case-smart
(an uppercase letter makes it case-sensitive), ignores Latin accents unless
it has some, as names do, lets whitespace match any whitespace, line breaks
included, and finds a word hyphenated at a line end. Text files are read as
they are; binary ones are skipped. PDF, DjVu and EPUB documents are read
through their text, extracted once with `pdftotext` (poppler) and `djvutxt`
(DjVuLibre), at low priority, into `~/.cache/fff/text` (`$XDG_CACHE_HOME`):
the first search through many documents takes a while, the next ones are
immediate. A scanned document without a text layer has nothing to find
(`ocrmypdf` adds one); a missing extractor is named in the query box.
A file shows its first 200 matches, and a search stops reading files once
20000 are found: the count beside the query then ends in `+` and says
which (`first 200 per file`, `stopped at 20000`). A longer query finds fewer.

Every file a search reads is also remembered in an index beside the cache
(`~/.cache/fff/index`): the trigrams of its text, folded as the query
matches them, keyed by the file's inode, size and times, so that an edited
file is read afresh. Later searches read only the files that may hold the
query (over 195,000 files, about 0.1 s for a word in a few of them, against
1 s reading them all), and binary files are not opened again. The index only
rules out files that cannot match, so it never loses a match; a query
without three letters or digits in a row reads every file.

Both stay small by themselves: once a day, a text or an index entry unused
for about three months goes (each after its own number of days, between 90
and 135, so that a search's entries do not all go at once), and the least
recently used go first while the text cache passes 2 GiB or the index
512 MiB.

The bar typed into takes three fifths of the width, or more when its query
needs it; the other gives way down to a third and cuts its query at the
end, an ellipsis saying it goes on (the one typed into scrolls instead, an
ellipsis at its start).

### Switching programs

`fff roam`, `fff pick` and `fff seek` are one program, and you can turn a
running session into any of them: `gm` in Normal mode (`switch_program`),
then the program's letter, `r`, `p` or `s`. From
then on fff behaves exactly as if it had been started as that program.
Start with `fff`, browse to `~/papers`, press `gm` then `s`, and you are in
`fff seek` there: only files, best match first, six levels deep. Press Tab
to search what they contain, and `gm` then `r` to go back to browsing.

What carries over:

- Everything you were looking at: the folders, tabs and their history, the
  query, the highlight, marks, exclusions, file filters and their toggles,
  hidden entries, the preview and labels chosen with `gl`. A highlighted folder that `fff seek`
  cannot list comes back once it is listed again, unless you move.
- The depth, if you set one. Otherwise it becomes the new program's default:
  1 for `roam`, 3 for `pick`, 6 for `seek`.
- The contents query and its matches, in every program.

How the session ends follows the program it ends in: `fff pick` prints the
chosen files, `fff roam` and `fff seek` change directory.

Dialogs (`fff explorer`) and `fff window` sessions do not switch: a dialog
answers the application that opened it, and a window has no shell to change
directory or print to.

### Workspaces

A workspace is a name for some of a session's settings: a folder, the
program, roots, queries, depth, filters, exclusions, sorting and toggles. `gw`
in Normal mode opens the chooser; Enter applies what the
highlighted workspace keeps and leaves the rest, the query included. One that
keeps only a folder is a place to go to. `n` there saves a new one: its name,
its folder (the current one; empty leaves the folder alone), and Space ticks
the other settings it keeps, as they are now.

`fff --workspace NAME` starts in one, the command line winning over what it
keeps. A workspace like this one replaces a script that starts
`fff pick` with its own roots and globs:

```toml
[[workspaces]]
name = "findnotes"
program = "pick"
roots = ["~/phd", "~/books"]
depth = 10
filters = ["*.pdf", "*.djvu"]
```

```sh
fff pick -0 --workspace findnotes | xargs -0 -r zathura
```

See [workspaces](CONFIGURATION.md#workspaces) for every key.

### Preview pane

The preview pane shows the highlighted entry in the bottom half of the
screen: a text file's first lines, a folder's entries, an archive's
contents, a PDF's facts and first pages' text, an image's size, a song's or a
video's duration, streams and tags. Text files show their syntax in the
palette's colors, through Neovim. It is text only, reads in the background
once the highlight rests, and never holds up the list. PDFs use poppler's
`pdfinfo` and `pdftotext`, audio and video FFmpeg's `ffprobe`, when installed.
`gpp` in Normal mode hides or shows it, PageDown and PageUp scroll it,
and `[ui] preview = false` starts with it hidden; see [preview
pane](CONFIGURATION.md#preview-pane).

### Details and live updates

Each result row ends with the entry's size (a directory's: every file below it,
added up in the background for the folders on screen and near it) and the time
since it was modified; the footer adds the highlighted entry's exact size (and a
folder's entry count), date, permissions and owner, and a symbolic link's target. Links show `→ target`
after their name, broken ones in the error color. When files appear, disappear
or change anywhere in the results, the results follow without F5. See
[result details](CONFIGURATION.md#result-details) for `[ui] columns`,
`[ui] mouse`, `[ui] buttons` (an opt-in button bar for mouse-only use) and
`[ui] watch`.

## Keys

F1 opens the **Keys screen**, a picker laid out like the results: every
function fff has, with its keys in Insert and Normal mode and where each
applies. Type to search names and descriptions; Tab switches to the Keys
view, a row per key with what it does and where (in Normal mode, `r` then a
key searches for it there); your own keys move through the list. Space goes a level deeper, Enter is done and Escape goes
back: Space on a function opens its keys, to add (`n`), replace the
highlighted one with the next key pressed (`R`), delete (`d`) or restore the
defaults (`D`, after asking), and Space on a key opens where it applies, as checklists of modes,
programs, lists and highlights; Enter then saves them to `keys.toml`.
Keys that would leave you no way to open the screen or to leave fff are
refused. F1 closes the screen. See [keys](CONFIGURATION.md#keys). F2
opens the Session tab: exact paths, scopes, depth, policies, palette, marks
and any errors in the configuration file. The footer's right end points to
F1 and says how to leave: `F1 keys  q quit` (`Ctrl+c quit` while typing,
where `q` is text).

The defaults are mostly Normal-mode keys, Vim's where it has one; while
typing, Alt+h/j/k/l move around, Up / Down switch tabs and Enter acts as in
Normal mode. Keys typed as text (letters, digits, `/`, `!`) apply in the
results and the marked list, and are Neovim's while the query has the
focus, but `j` / `k`, which go down into the results, and `gc`. Some functions have no default key: bind them on the Keys screen.

| Key | Action |
| --- | --- |
| i / a, then type | Edit a Neovim field |
| Escape | Leave transient editing modes, then give the results the focus back, then clear the query; keep the session |
| Tab | Type into the other query, contents or names, in either mode; from the results in Normal mode, give the query the focus |
| Shift+Tab | Give the query or the results the focus; from Insert mode, leave it for the results |
| Enter | Open the highlighted file and leave (`fff roam`), print the marks and the highlighted entry (`fff pick`), leave into the file's folder (`fff seek`); enter a folder in `fff window` and the dialogs |
| Shift+Enter / Ctrl+Enter | Print the highlighted entry alone / the marks alone (`fff pick`, dialogs choosing several) |
| j / k (Alt+j / Alt+k while typing) | Next / previous row |
| J / K | Next / previous page of results |
| gg / G | First / last row |
| j / k with the query in focus (Normal mode) | Go down into the results, to the first / last row |
| h (Alt+h while typing) | Parent directory; retain query and reset highlight/scroll |
| l (Alt+l while typing) | Enter the highlighted directory, or the highlighted file's folder |
| L | Open the highlighted file in the background |
| `/` | Go to a typed path, or words picking a workspace or one of zoxide's frequent folders |
| Ctrl+o / Ctrl+i | Older / newer folder in this tab's jump list (Ctrl+i needs a terminal that tells it from Tab) |
| Up / Down | Next / previous [tab](#tabs), in either mode |
| Ctrl+Tab | Next [tab](#tabs), in either mode (not in `fff seek`) |
| `gw` | [Workspace](#workspaces) chooser; in Normal mode n adds, e edits, d deletes, r reloads (Alt+ while typing); Up/Down reorder |
| `gP` | Palette chooser; highlight for 100 ms to preview, Enter applies, Escape reverts |
| `+` / `-` | Increase / decrease depth; minimum 1; from the query in focus too |
| Ctrl+Alt+1 … 9 | Search results to that depth |
| 1 … 9 | Highlight that visible row |
| Alt+1 … Alt+9 / Alt+0 | Highlight the row below that visible row / below the last |
| Ctrl+1 … Ctrl+9 / Ctrl+0 | Open the file in that visible row / the last row and leave (`fff roam`, `fff window`) |
| Space | Toggle mark; advance in results, past a file's other matches |
| m / M | Mark every listed row / invert the listed rows' marks (results or marked list) |
| Ctrl+m | Clear every mark |
| Ctrl+k | Focus / leave the marked list |
| Ctrl+j | Focus / leave the [jobs list](OPERATIONS.md#progress-and-stopping) of operations still running; x there stops one and undoes it |
| ZZ / ZQ | In multiline Neovim buffers (exclusions, filters, rename), save and close / discard and close; `:wq` / `:q!` also work |
| g. | Toggle hidden entries |
| gd | Hide directories, listing only files; again to show them |
| gD | Hide files, listing only folders; again to show them |
| gpp / gpw | Show / hide the [preview pane](CONFIGURATION.md#preview-pane) / wrap its lines |
| PageDown / PageUp | Scroll the preview pane down / up by half its height, keeping the highlight |
| gs | [Sort](#sorting) the matches by name, size, a date, extension, kind or depth; Tab there switches descending and ascending |
| gl | Label rows by name (with the parents that tell same-named entries apart) or by path below the folder searched |
| ge | Toggle exclusion enforcement |
| gE | Edit session-local exclusions, one glob per line; matching entries (and directory subtrees) are hidden |
| gF | Edit session-local file filters (`--glob`), one glob per line; only files whose name matches one are listed |
| gf | Turn the file filters off or on again, keeping them |
| v / p / s | Move / copy / symlink all marked paths into the current directory; a move or copy that would overwrite asks first |
| x | Move the current/selected entries to the [trash](OPERATIONS.md#trash-and-deletion) (asks only about very large entries) |
| X | Choose current/selected deletion, review sizes, then Enter deletes permanently |
| u | [Undo](OPERATIONS.md#undo) the last file operation (again for the one before); one still running is stopped and undone |
| gt | Open the [trash](OPERATIONS.md#the-trash-folder) like any folder; again, the next filesystem's trash |
| R | In the trash: restore the current or marked entries where they were deleted from |
| r | Batch rename the filtered results in a buffer |
| E | Extract the current or marked archives (asks when ambiguous) |
| n | Choose a new file or directory, then enter its name |
| F1 / F2 | Keys screen / Session details; the same key closes each |
| `:` | Pick a function that works here by name and run it (Enter), or go back (Esc) |
| F5 | Force fresh discovery, bypassing cache replay |
| gx / gX | Run a program on the current or marked paths and come back / exit fff into it |
| e | Open the current or marked paths in `$EDITOR`, then come back |
| gm | [Switch](#switching-programs) to `fff roam`, `pick` or `seek`, keeping the session |
| gc | [Edit the configuration](CONFIGURATION.md#editing-while-fff-runs) in `$EDITOR`, then apply it to the session; from the query in focus too |
| q / ! / g! | Leave without choosing (where you started) / leave into the current directory / leave into the highlighted one (`fff roam`); q from the query in focus too |
| Ctrl+c | Cancel without selected-path output |
| Click / double-click | Highlight a row (clearing the marks) / act like `l` on it (in `fff explorer`, like Enter) |
| Drag with the button held | Mark every row from where it was pressed to where it is; past the top or bottom edge, keep scrolling and marking |
| Mouse wheel | Scroll the results, leaving the highlight and marks alone (over the marked list or a chooser, move through it) |
| Click / double-click a mark or chooser row | Select it / act on it (`l` on a mark, Enter in a chooser) |
| Click a tab number | Switch to that [tab](#tabs), keeping the marks |
| Drop files on the terminal | Copy them into the current directory, after confirmation |

`q` and `!` exit with status 0. `q` chooses nothing: it prints nothing and
changes no directory, so `fff pick | xargs -r …` opens nothing. `!` prints
the current directory in `fff pick`, in the usual output format, and changes
to it in `fff roam`. In `fff seek`, which never changes directory, `!` does
nothing. Insert mode keeps these characters as
text. `gx` / `gX` are described under [running programs](OPERATIONS.md#running-programs).

Normal-mode list keys apply to results, marks, workspaces and palettes; the
location and exclusion fields keep Neovim motions. Plain digits are query text in
Insert mode; in Normal-mode results 1–9 highlight that row
(binding `highlight_row` to nothing makes them Vim counts again). Keys are case-sensitive.
Ctrl+digits, Shift+Enter, Ctrl+Enter and Ctrl+m require distinct terminal reporting; fff requests the Kitty
keyboard protocol and never guesses ambiguous input. Alt+digits also support
ESC+digit reporting. Every key is rebound on the [Keys screen](CONFIGURATION.md#keys)
(F1), which saves to `keys.toml` beside the configuration.

File operations that cannot be undone, or would overwrite, are confirmed
first (`x`, moves, copies and links without collisions act at once), refuse to
overwrite unless a move or copy confirmation says so, roll back on failure, and
can be undone with `u`; long
ones show their progress and stop safely with Ctrl+c. Name and destination buffers use Enter to apply and Escape to
cancel; the multiline rename buffer uses `ZZ` to apply or `ZQ` to discard. See [file operations](OPERATIONS.md) for confirmations, guarantees,
archive limits and current boundaries. Building
requires libarchive development files (macOS has them); the operation engine
requires Linux 5.8+ or macOS 13+.

## Configuration and responsiveness

Optional TOML lives at `$XDG_CONFIG_HOME/fff/config.toml`, falling back to
`~/.config/fff/config.toml` when XDG is unset, empty or not absolute.
`--config FILE` selects a file; `--no-config` uses built-ins. Validate with:

```sh
fff setup --config docs/config.example.toml --check-config
```

- [Configuration guide](CONFIGURATION.md): every setting, workspaces, icons,
  key mappings, cache limits and prefetch budgets.
- [Commented example](../config.example.toml): a complete starting configuration.
- [Palette catalog](PALETTES.md): built-ins, live switching and color sources.
- [Editing guide](EDITING.md): Normal/Insert modes and clean Neovim embedding.
- [Desktop integration](DESKTOP.md): fff as the file manager and Firefox's file chooser.

In the workspace editor, Enter saves and Escape cancels, Tab (or j/k outside
typing) switches fields and settings, the path is optional (a workspace can
be only settings, such as filters), Space ticks a setting to keep, and Space on the icon row opens a
searchable Nerd Font icon picker. In the chooser,
Up/Down reorder workspaces. Saves update the active TOML file; `--no-config`
keeps edits for this session only.

Discovery is asynchronous and breadth first. Ready results wake the UI immediately;
a folder whose scan finishes within 12 ms is drawn once, complete.
Recent complete snapshots skip redundant scans for 3 seconds by default. Depth-1
views refresh about every 3 seconds and can prefetch a child after a 500 ms
highlight dwell; recursive views reuse known descendants and refresh every
60 seconds by default. F5 forces freshness. All caches of listings are
session-local RAM (only contents searches keep a [cache on
disk](#searching-contents), and [folders'
sizes](CONFIGURATION.md#folder-sizes) a record of what was added up).

The interface shows the current directory on the `@` line and pinned roots by
name. Marks sit to the right on wide terminals and stack on narrow ones.
Nerd Font icons are on by default; `--no-icons` restores plain markers. `NO_COLOR`
is honored. Minimum working size is 36 × 12.

## Shell completions

With `fff` on PATH, add one line to your shell configuration:

```sh
source <(fff setup --completions bash)   # ~/.bashrc
source <(fff setup --completions zsh)    # ~/.zshrc, after compinit
fff setup --completions fish | source    # ~/.config/fish/config.fish
```

Or save the script once where the shell finds completions, and nothing goes in
its startup file:

```sh
fff setup --completions zsh > ~/.config/zsh/completions/_fff
fff setup --completions fish > ~/.config/fish/completions/fff.fish
```

zsh finds `_fff` only in a directory that is in `$fpath` **when `compinit`
runs**: compinit registers the completion files it sees then, and nothing added
to `$fpath` afterwards (by a plugin manager's `--completion`, say) is noticed
until the next shell that runs compinit with it. So in `~/.zshrc`:

```sh
fpath=(~/.config/zsh/completions $fpath)
autoload -Uz compinit && compinit
```

or save the file in a directory zsh always searches, such as
`/usr/local/share/zsh/site-functions` (as root). compinit keeps what it found in
its dump file (`~/.zcompdump`, or the file `compinit -d` names) and rebuilds it
when the number of completion files changes, so a new `_fff` is picked up by the
next shell; `print $_comps[fff]` in it says `_fff` once it is.

The script only calls `fff` back (the binary on PATH, as named when the script
was printed), so it needs no update when fff is upgraded.

Elvish and PowerShell are supported too. Completion calls the running binary and
covers options, root/config paths, editors, built-in/custom palettes and key
chords. `COMPLETE=SHELL fff` prints the same registration script.

## Manual pages

```sh
fff setup --man
```

writes `fff.1` and a page per command (`fff-pick.1`, `fff-explorer-save.1`,
...), from the same text as `--help`, into a directory `man` searches: the
first one `manpath` lists in your home (`~/.local/share/man`, which `man`
searches when `~/.local/bin` is on PATH), else the first you can write to. When
there is none, it says so and asks for the folder: `fff setup --man
~/.local/share/man/man1` writes there. Then `man fff` or `man fff-pick`. Run it
again after upgrading.

## Exit status

| Code | Meaning |
| --- | --- |
| 0 | Normal exit (directory chosen or paths printed), configuration valid, or help/completion printed |
| 1 | Startup, configuration or I/O error, including an unknown palette |
| 2 | Invalid CLI usage or malformed argument value |
| 130 | Cancelled with Ctrl+c; nothing printed and no directory change |

After `gX`, fff's process becomes the program, so the exit status is the
program's.

See [development](DEVELOPMENT.md) for the code map and verification commands.
Possible extensions are listed in the [roadmap](ROADMAP.md); what changed
in each release is in the [changelog](../../CHANGELOG.md).

## Install from crates.io

The crate package is `fuzzy-file-finder`; it installs the `fff` command:

```sh
cargo install fuzzy-file-finder
fff
```

Source code and issues: [Frankwii/fff](https://github.com/Frankwii/fff).
Version 1.0 is planned after the remaining [roadmap](ROADMAP.md) is implemented.

## License

Fuzzy file finder is licensed under the [GNU General Public License, version 3](../../LICENSE)
(`GPL-3.0-only`). Third-party palette notices are listed in
[THIRD_PARTY_NOTICES.md](../THIRD_PARTY_NOTICES.md).
