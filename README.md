# fuzzy file finder

A Rust terminal file picker with fuzzy filename filtering, directory navigation,
live search depth, pinned roots and persistent marked paths. Version **0.14.0**.
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

Run in a Linux/Unix terminal with `/dev/tty`. The interface uses the controlling
terminal, never stdout. Put the binary on your PATH as `fff` and add the
[shell integration](#shell-integration) line so that leaving fff can change your
shell's directory. `-h` summarizes options; `--help` includes defaults, examples
and exit codes.

## Commands

| Command | What it is for |
| --- | --- |
| `fff [ROOTS]`, or `fff roam [ROOTS]` | Browse; leaving changes your shell's directory. `--resume` continues the last session. |
| `fff pick [ROOTS]` | Browse and print the chosen paths, for scripts and `$(...)`. |
| `fff seek [ROOTS]` | Choose files from below ROOTS without navigating ([details](#choosing-files-fff-seek)). |
| `fff grep [ROOTS]` | Choose files by what they contain, PDFs included, and print them ([details](#searching-contents-fff-grep)). |
| `fff explorer [PATH]` | Answer a file dialog ([details](docs/DESKTOP.md#fff-explorer)). |
| `fff setup --OPTION` | Set fff up: `--init`, `--completions`, `--desktop`, `--file-chooser`, `--check-config`. |

`fff` without a command is `fff roam`. The search, interface and
configuration options (`--depth`, `--glob`, `--palette`, `--config`, ...) work
with every command, before or after its name.

## Shell integration

`fff roam`, the default, changes directory on exit: `q` leaves you where you
started, `!` takes you to the directory you were browsing, Normal-mode Enter
on a directory takes you into it, and on a file opens it and leaves you in its
folder. Ctrl+number is Enter on that row. To open several files at once, `o`
opens the marked ones in this terminal and comes back.

`fff pick` prints the selection instead, for scripts and command substitution,
as described under [returning paths](#browsing-and-returning-paths):
`nvim "$(fff pick)"`, `fff pick -0 | xargs -0 ls -ld`.

A program cannot change its parent shell's directory, so the shell does it, as
with nnn's quitcd scripts and zoxide. Add one line to your shell's startup file:

```sh
eval "$(fff setup --init zsh)"     # ~/.zshrc (or $ZDOTDIR/.zshrc)
eval "$(fff setup --init bash)"    # ~/.bashrc
fff setup --init fish | source     # ~/.config/fish/config.fish
```

This defines a shell function named `fff` that runs the real binary with
`--cd-file` (a private temporary file), then changes to the directory fff wrote
there, if any. Arguments pass through unchanged, so `fff pick` keeps printing
paths. Without the function, `fff roam` prints the chosen directory instead,
so `cd "$(command fff)"` also works.

### Resuming a session

`fff --resume` continues the last `fff roam` session as it ended: the folder
and highlighted row, the other tabs and their back/forward history, the
marks, the query, the depth, filters, exclusions, and the hidden-entry,
directory and preview toggles. Only the last session is kept, wherever it was
started, and `q` still returns to where you start the new one. Folders that
are gone fall back to their nearest existing parent. `fff pick`, `fff seek`
and `fff explorer` sessions are never saved. Without `--resume`, fff starts in
the current directory, on the row last highlighted there (see [remembered
locations](docs/CONFIGURATION.md#remembered-locations)).

fff can also be the desktop's file manager and file chooser: `fff setup
--desktop` opens directories from other programs in fff, and `fff setup
--file-chooser` lets Firefox and other portal users pick files with it, through
`fff explorer`, a mode for answering file dialogs. See
[desktop integration](docs/DESKTOP.md).

Every text field is a clean embedded Neovim (`nvim` must be on PATH). Fields start in **Normal mode**,
except prompts made for typing (new names, the go-to prompt, the gx/gX command,
the bookmark form)
and the query of `fff pick`, `fff seek` and `fff explorer --save`, which start
in **Insert mode**.
Use `j`/`k` to browse and `i` or `a` to type a filter. Directory navigation
preserves your current editing mode. See [editing](docs/EDITING.md).

## Browsing and returning paths

This section describes what each confirmation returns in `fff pick`; in
`fff roam` the same keys choose a directory instead (see above).

Without root arguments, browsing starts in the invocation directory (or, with
`--resume`, where the last session ended) and follows navigation. Explicit roots stay pinned: results combine their descendants with
those of the current directory. Overlapping paths appear once at their minimum
depth. Default depth is **1** (**3** in `fff pick`, **6** in `fff seek` and `fff grep`); immediate children are depth
1. Shallower matches come first, followed within each depth by directories
and fuzzy ranking; `fff pick` and `fff seek` rank by fuzzy score first, then
depth. The depth is shown on the results' bottom border, under the count.
Matching is a case-insensitive ordered subsequence of the basename, scored as
fzf does: the best alignment of the query's letters, so runs and word starts
(after `_`, `-`, `.`, a space or a lowercase-to-uppercase step) beat letters
scattered from the start, however long the name. Latin accents are ignored
unless the query has some: `evaluacion` finds `Evaluación`.

A query containing `/` matches the path below the result's root instead, so
`pde/stab` lists `pde/notes/stability.tex` but not `notes/stability.tex`.
Each `/`-separated part is a subsequence of a component, the components in
order, and the last part matches the basename: `pde/` lists everything inside
folders matching `pde`, and a leading `/` (`/stab`) only switches to path
ranking. Parts naming whole components, and parts on consecutive components,
rank higher; the matched letters are highlighted along the whole label. Depth
and directories still sort first, except in `fff pick` and `fff seek`.

Enter depends on the editing mode:

- **Normal mode:** Enter *finishes with* the highlighted entry. On a directory
  fff exits into it. On a file, in `fff roam`, fff **opens it and leaves**:
  a terminal opener (an editor, a pager) takes over this terminal in fff's place,
  like `gX`, and any other opener starts detached, like `l`; the shell then
  changes to the file's folder (also for deep results: `notes/more/a.pdf` leaves
  you in `notes/more`). In `fff pick`, Enter on a file prints the file;
  in `fff seek` it exits into its folder. With
  nothing listed, Enter exits into the browsing directory. Marks are ignored.
  In the marked list Enter does the same for the highlighted mark.
- **While typing (Insert mode):** Enter opens a directory. On a file, in
  `fff pick` it returns the marked paths in insertion order plus that file
  once; in `fff roam` it *reveals* the file: fff moves to its folder,
  clears the query and highlights it. `fff seek`, which never moves, exits
  into the file's folder, as in Normal mode.

To move around, use `l` (or Alt+l, or Alt+number for a labelled row): it enters
a directory, and **opens a file** with its [opener](docs/OPERATIONS.md#opening-files)
in the background while fff keeps the keyboard. `o` opens the current or marked
files in this terminal instead, and fff comes back when the program ends.

The keys that open or finish fit together:

| Key | Acts on | Then |
| --- | --- | --- |
| `l` | The highlighted entry: go into a directory, open a file in the background | fff stays |
| `o` | The current or marked files, opened in this terminal | fff comes back |
| Enter (Normal) | The highlighted entry: leave into a directory, open a file | fff leaves |
| `!` | The folder you are browsing | fff leaves into it |
| Ctrl+w | The marks, returned (`fff pick` only) | fff leaves |
| `q` | Nothing | fff leaves where you started |

[`fff explorer`](docs/DESKTOP.md#fff-explorer), the mode for answering file
dialogs, keeps these meanings: Enter chooses a file and enters a folder, `!`
takes the folder you are in, Ctrl+w the marks (in dialogs for several files),
and Ctrl+s saves. A folder
opened from the desktop (`--window`) has no shell to leave into: Enter enters a
folder, and `!` starts a shell in the folder you are in
([details](docs/DESKTOP.md#opening-directories)).

Shift+Space toggles a mark in every mode; Space does in Normal mode and Tab
while typing (for terminals that cannot tell Shift+Space from Space);
marks survive navigation and filtering. Escape twice in Normal mode, with the
query empty, clears every mark. Ctrl+w finishes
with the marks only, and only where the answer can be several paths: `fff
pick` returns them (as does `fff explorer --multiple`). Elsewhere (`fff roam`,
`fff seek`, other dialogs) it does nothing but say so; there the marks are for
file operations, `o`, `e` and `gx`. Insert-mode Enter in a verified physically empty directory returns marks
plus that directory; a no-match query in a nonempty directory stays open, and
a short status message says nothing matches (Ctrl+number likewise).
Missing marks block confirmation until unmarked.

### Tabs

Every session has three tabs, like nnn's contexts, numbered in the top-right
corner of the results, the active one in the `@` badge's colors. All start in
the session's first directory. Alt+Up / Alt+Down (or Up / Down in Normal mode)
switch to the next / previous tab, wrapping around; each tab keeps its own directory and Ctrl+o / Ctrl+i history,
while marks, the query, depth, hidden entries, exclusions and filters are
shared. Mark files in one tab, switch, browse somewhere far away, then move
or copy them there with `v` / `p`. Switching to a tab in another directory lists
it again with an empty query; a tab whose directory vanished falls back to the
nearest existing parent. F2's session details list every tab's directory. `fff seek`, which cannot
change directory, has no tabs: Up / Down in Normal mode move the highlight there.

Ctrl+number is Enter on a visible row, exactly as if that row were highlighted
and Enter pressed, in the current editing mode: in Normal mode it finishes with
the row (opens a file and leaves, prints it in `fff pick`, leaves into
a directory); while typing it enters a directory, and on a file returns the
marks plus that file (`fff pick`) or reveals it. Alt+number acts like `l`
on the same row: it enters a directory or opens a file in the background.
Labels refer to the last fully rendered page: 1–9 select that position,
out-of-range digits clamp to the last row, and 0 selects the last row. With no
visible rows, Ctrl+number is Enter with nothing highlighted (in Normal mode,
the current directory; in `fff seek`, nothing) and Alt+number does nothing.

Output defaults to absolute, newline-terminated paths. `--relative` uses the
invocation directory as a stable base; `--null` (`-0`) writes NUL-delimited records.

Rows in `fff pick` (and `fff seek`) show bare names: when listed entries share
a name, each gains the fewest trailing parent directories that tell it apart
(`folder1/main.pdf`, `folder2/main.pdf`), independently of the query, and the
labels follow the listing as you navigate. The footer shows the highlighted
entry's path. In `fff roam` and `fff explorer`, labels are relative to the
browsing directory, with repeated leading parent steps shortened to `.../`.
Labels are presentation only: output paths are never shortened.

`--glob` is a file filter: only files whose basename matches it are listed, and
directories are hidden but still traversed. Repeat it to allow several patterns
(`--glob '*.pdf' --glob '*.epub'`); a file matching any of them is listed. Alt+f
(`gF` in Normal mode) edits the filters during the session, one glob per line;
Alt+F (`gf`) turns them off and on again, keeping them. Changing them re-lists
the files already found without rescanning.

Hidden entries are off by default. Explicit `--exclude` glob rules are enforced;
a matching directory prunes its subtree. Rules match basenames or scope-relative
paths, with literal separators. No automatic `.gitignore` integration is enabled.
The query box's bottom border shows `filters on` and `exclusions on` only
while filters or exclusion rules exist and are applied; F2's session details list
their globs. It also shows discovery progress while a scan runs and for three
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
starts in Insert mode, ready to type. Result rows show bare file names, with
only the parents that tell same-named files apart, as in `fff pick`.

### Searching contents: `fff grep`

`fff grep [ROOTS]` chooses files by what they contain. It is `fff pick`
with a contents query: it browses the same way (tabs, back and forward,
bookmarks, Enter into a folder), looks from every folder down to `--depth`
(6 by default; `--glob`, `--exclude` and `--hidden` apply), and prints the
chosen files (`--relative`, `-0`):

```sh
fff grep ~/papers --glob '*.pdf' --glob '*.djvu' --glob '*.epub' -0 | xargs -0 zathura
```

It has two queries side by side. **Contents**, typed into first, keeps the
files that contain it, with **one row per match**: the file, the match's page
(`p. 12`), how far into an EPUB it is (`42%`) or its line (`l. 40`), and
the line it is on, the match highlighted. The
preview pane shows the lines around the highlighted match. **Names** filters
the files by name as in `fff seek`, so it chooses which files are read.
Tab switches between them, in either mode (Shift+Space marks there);
Escape clears the one being typed into (the
other once it is empty). Enter prints the file (while typing, the marks and
it); marking a match marks its file. `l` opens it in the background and
keeps fff open; an [opener](docs/CONFIGURATION.md#openers) with
`at_grep_match` opens it at the match, zathura on its page with the query
highlighted.

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

### Switching programs

`gm` in Normal mode (`switch_program`) goes on as another of `fff roam`,
`fff pick`, `fff seek` and `fff grep`, chosen by its letter (r, p, s, g).
The session stays: folders, tabs and their history, the query, the
highlight (one `fff seek` cannot list, a folder, comes back once it is
listed again, unless you move), marks, exclusions, file filters and their
toggles, hidden entries and the preview. The depth follows the new
program's default (1, 3, 6 and 6) unless you set another, which stays.
Leaving `fff grep` keeps its contents query for when you come back to it.
The session then leaves as the program it ended in: `fff pick` and
`fff grep` print the chosen files, `fff roam` and `fff seek` change
directory. Dialogs (`fff explorer`) and `--window` sessions do not switch:
a dialog answers the application that opened it, and a window has no shell
to change directory or print to.

### Preview pane

The preview pane shows the highlighted entry in the bottom half of the
screen: a text file's first lines, a folder's entries, an archive's
contents, a PDF's facts and first pages' text, an image's size, a song's or a
video's duration, streams and tags. Text files show their syntax in the
palette's colors, through Neovim. It is text only, reads in the background
once the highlight rests, and never holds up the list. PDFs use poppler's
`pdfinfo` and `pdftotext`, audio and video FFmpeg's `ffprobe`, when installed.
Alt+v (`P` in Normal mode) hides or shows it, Ctrl+j and Ctrl+k scroll it,
and `[ui] preview = false` starts with it hidden; see [preview
pane](docs/CONFIGURATION.md#preview-pane).

### Details and live updates

Each result row ends with the entry's size (a directory's entry count) and the
time since it was modified; the footer adds the highlighted entry's exact size,
date, permissions and owner, and a symbolic link's target. Links show `→ target`
after their name, broken ones in the error color. When files appear, disappear
or change anywhere in the results, the results follow without F5. See
[result details](docs/CONFIGURATION.md#result-details) for `[ui] columns`,
`[ui] mouse`, `[ui] buttons` (an opt-in button bar for mouse-only use) and
`[ui] watch`.

## Keys

F1 opens the **Keys screen**, a picker laid out like the results: every
function fff has, with its keys in Insert and Normal mode and where each
applies. Type to search names and descriptions, or a key's name to see what
it does (in Normal mode, `r` then a key writes its name); your own keys move
through the list. Space goes a level deeper, Enter is done and Escape goes
back: Space on a function opens its keys, to add (`n`), delete or reset
them, and Space on a key opens where it applies, as checklists of modes,
programs, lists and highlights; Enter then saves them to `keys.toml`.
Keys that would leave you no way to open the screen or to leave fff are
refused. F1 closes the screen. See [keys](docs/CONFIGURATION.md#keys). F2
opens the Session tab: exact paths, scopes, depth, policies, palette, marks
and any errors in the configuration file. The footer's right end points to
F1 and says how to leave: `F1 keys  q quit` (`Ctrl+c quit` while typing,
where `q` is text).

The defaults: Alt keys that move around (Alt+h/j/k/l, Alt+arrows, Alt+Space,
Alt+digits) work in both modes. Alt keys for features (Alt+b, Alt+g, Alt+t,
Alt+., Alt+d, Alt+D, Alt+v, Alt+e, Alt+H, Alt+f, Alt+F, Alt+p, Alt+P) work
while typing; in Normal mode their letters do the work instead, and the Alt
key says which one.

| Key | Action |
| --- | --- |
| i / a, then type | Edit a Neovim field |
| Escape | Leave transient editing modes, then clear the query or close a chooser/location prompt; keep the session |
| Escape twice in Normal | With the query empty: clear every mark |
| Tab | `fff grep`: type into the other query, contents or names (elsewhere, while typing, Tab marks) |
| Enter in Normal | Exit into a highlighted directory; on a highlighted file, open it and leave (`fff roam`), print it (`fff pick`, `fff grep`) or exit into its folder (`fff seek`) |
| Enter while typing | Enter a directory; on a file, return marks + file (`fff pick`, `fff grep`), exit into its folder (`fff seek`) or reveal it in its folder |
| Up / Alt+k | Previous row (in Normal-mode results Up switches [tabs](#tabs), except in `fff seek`; use k) |
| Down / Alt+j | Next row (in Normal-mode results Down switches [tabs](#tabs), except in `fff seek`; use j) |
| h / j / k / l | Normal-mode list actions equivalent to Alt+h/j/k/l |
| gg / G | First / last row in Normal-mode lists |
| Alt+h | Parent directory; retain query and reset highlight/scroll |
| Alt+l / l in Normal | Enter the highlighted directory, or open the highlighted file in the background |
| o in Normal | Open the current or marked files with their openers in this terminal, then come back |
| Alt+g / `g/` in Normal | Go to a typed path, or words picking a bookmark or one of zoxide's frequent folders |
| Ctrl+o / Ctrl+i | Back / forward through the folders this tab visited (Ctrl+i needs a terminal that tells it from Tab) |
| Alt+Up / Alt+Down | Next / previous [tab](#tabs) (Up / Down in Normal-mode results) |
| Alt+b / b in Normal | Bookmark chooser; in Normal mode n adds, e edits, d deletes, r reloads (Alt+ while typing); Up/Down reorder |
| Alt+p | Palette chooser; highlight for 100 ms to preview, Enter applies, Escape reverts |
| Alt+P | Reload the configured palette, clearing a session choice |
| Alt+Right / Alt+Left | Increase / decrease depth; minimum 1 (`+` / `-` in Normal-mode results) |
| 1 … 9 in Normal | Search results to that depth |
| Shift+Space / Space in Normal / Tab while typing | Toggle mark; advance in results (Shift+Space needs a terminal with the kitty keyboard protocol) |
| m / M in Normal | Mark every listed row / invert the listed rows' marks (results or marked list) |
| Alt+Space | Focus / leave the marked list |
| Ctrl+w | Finish with the marks, from results or the marked list: return them (`fff pick`, `fff explorer --multiple`; nothing elsewhere) |
| ZZ / ZQ | In multiline Neovim buffers (exclusions, filters, rename), save and close / discard and close; `:wq` / `:q!` also work |
| Ctrl+1 … Ctrl+9 / Ctrl+0 | Enter on that visible row / the last row |
| Alt+1 … Alt+9 / Alt+0 | Like `l` on a visible target / last target |
| PageUp / PageDown | Page through results |
| Alt+. / . in Normal | Toggle hidden entries |
| Alt+d / d in Normal | Hide directories, listing only files; again to show them (`fff seek` lists only files: d and D reach Neovim there) |
| Alt+D / D in Normal | Hide files, listing only folders; again to show them |
| Alt+v / P in Normal | Show / hide the [preview pane](docs/CONFIGURATION.md#preview-pane) |
| Ctrl+j / Ctrl+k | Scroll the preview pane down / up by half its height, keeping the highlight |
| Alt+H / ge in Normal | Toggle exclusion enforcement |
| Alt+e / gE in Normal | Edit session-local exclusions, one glob per line; matching entries (and directory subtrees) are hidden |
| Alt+f / gF in Normal | Edit session-local file filters (`--glob`), one glob per line; only files whose name matches one are listed |
| Alt+F / gf in Normal | Turn the file filters off or on again, keeping them |
| v / p / s in Normal | Move / copy / symlink all marked paths into the current directory; a move or copy that would overwrite asks first |
| x in Normal | Move the current/selected entries to the [trash](docs/OPERATIONS.md#trash-and-deletion) (asks only about very large entries) |
| X in Normal | Choose current/selected deletion, review sizes, then Enter deletes permanently |
| u in Normal | [Undo](docs/OPERATIONS.md#undo) the last file operation (again for the one before) |
| Alt+t / gt in Normal | Open the [trash](docs/OPERATIONS.md#the-trash-folder) like any folder; again, the next filesystem's trash |
| R in Normal | In the trash: restore the current or marked entries where they were deleted from |
| r in Normal | Batch rename the filtered results in a buffer |
| E in Normal | Extract the current or marked archives (asks when ambiguous) |
| n in Normal | Choose a new file or directory, then enter its name |
| F1 / F2 | Keys screen / Session details; the same key closes each |
| F5 | Force fresh discovery, bypassing cache replay |
| gx / gX in Normal | Run a program on the current or marked paths and come back / exit fff into it |
| e in Normal | Open the current or marked paths in `$EDITOR`, then come back |
| gm in Normal | [Switch](#switching-programs) to `fff roam`, `pick`, `seek` or `grep`, keeping the session |
| gc in Normal | [Edit the configuration](docs/CONFIGURATION.md#editing-while-fff-runs) in `$EDITOR`, then apply it to the session |
| q / ! in Normal | Leave without choosing (where you started) / leave into the current directory, ignoring marks |
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
to it in `fff roam`. `fff seek` makes either exit successfully without
output. Insert mode keeps these characters as
text. `gx` / `gX` are described under [running programs](docs/OPERATIONS.md#running-programs).

Normal-mode list keys apply to results, marks, bookmarks and palettes; the
location and exclusion fields keep Neovim motions. Plain digits are query text in
Insert mode; in Normal-mode results 1–9 set the search depth
(binding `search_to_depth` to nothing makes them Vim counts again). Keys are case-sensitive.
Ctrl+digits require distinct terminal reporting; fff requests the Kitty
keyboard protocol and never guesses ambiguous input. Alt+digits also support
ESC+digit reporting. Every key is rebound on the [Keys screen](docs/CONFIGURATION.md#keys)
(F1), which saves to `keys.toml` beside the configuration.

File operations that cannot be undone, or would overwrite, are confirmed
first (`x`, moves, copies and links without collisions act at once), refuse to
overwrite unless a move or copy confirmation says so, roll back on failure, and
can be undone with `u`; long
ones show their progress and stop safely with Ctrl+c. Name and destination buffers use Enter to apply and Escape to
cancel; the multiline rename buffer uses `ZZ` to apply or `ZQ` to discard. See [file operations](docs/OPERATIONS.md) for confirmations, guarantees,
archive limits and current boundaries. Building
requires libarchive development files; the operation engine requires Linux 5.8+.

## Configuration and responsiveness

Optional TOML lives at `$XDG_CONFIG_HOME/fff/config.toml`, falling back to
`~/.config/fff/config.toml` when XDG is unset, empty or not absolute.
`--config FILE` selects a file; `--no-config` uses built-ins. Validate with:

```sh
fff setup --config docs/config.example.toml --check-config
```

- [Configuration guide](docs/CONFIGURATION.md): every setting, bookmarks, icons,
  key mappings, cache limits and prefetch budgets.
- [Commented example](docs/config.example.toml): a complete starting configuration.
- [Palette catalog](docs/PALETTES.md): built-ins, live switching and color sources.
- [Editing guide](docs/EDITING.md): Normal/Insert modes and clean Neovim embedding.
- [Desktop integration](docs/DESKTOP.md): fff as the file manager and Firefox's file chooser.

In the bookmark editor, Enter saves and Escape cancels, Tab switches fields and
Space on the icon row opens a searchable Nerd Font icon picker. In the chooser,
Up/Down reorder bookmarks. Saves update the active TOML file; `--no-config`
keeps edits for this session only.

Discovery is asynchronous and breadth first. Ready results wake the UI immediately;
a folder whose scan finishes within 12 ms is drawn once, complete.
Recent complete snapshots skip redundant scans for 3 seconds by default. Depth-1
views refresh about every 3 seconds and can prefetch a child after a 500 ms
highlight dwell; recursive views reuse known descendants and refresh every
60 seconds by default. F5 forces freshness. All caches are session-local RAM.

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

Elvish and PowerShell are supported too. Completion calls the running binary and
covers options, root/config paths, editors, built-in/custom palettes and key
chords. `COMPLETE=SHELL fff` prints the same registration script.

## Exit status

| Code | Meaning |
| --- | --- |
| 0 | Normal exit (directory chosen or paths printed), configuration valid, or help/completion printed |
| 1 | Startup, configuration or I/O error, including an unknown palette |
| 2 | Invalid CLI usage or malformed argument value |
| 130 | Cancelled with Ctrl+c; nothing printed and no directory change |

After `gX`, fff's process becomes the program, so the exit status is the
program's.

See [development](docs/DEVELOPMENT.md) for the code map and verification commands.
Possible extensions are listed in the [roadmap](docs/ROADMAP.md); what changed
in each release is in the [changelog](CHANGELOG.md).

## Install from crates.io

The crate package is `fuzzy-file-finder`; it installs the `fff` command:

```sh
cargo install fuzzy-file-finder
fff
```

Source code and issues: [Frankwii/fff](https://github.com/Frankwii/fff).
Version 1.0 is planned after the remaining [roadmap](docs/ROADMAP.md) is implemented.

## License

Fuzzy file finder is licensed under the [GNU General Public License, version 3](LICENSE)
(`GPL-3.0-only`). Third-party palette notices are listed in
[THIRD_PARTY_NOTICES.md](docs/THIRD_PARTY_NOTICES.md).
