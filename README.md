# fuzzy file finder

A Rust terminal file picker with fuzzy filename filtering, directory navigation,
live search depth, pinned roots and persistent marked paths. Version **0.9.1**.
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
| `fff explorer [PATH]` | Answer a file dialog ([details](docs/DESKTOP.md#fff-explorer)). |
| `fff setup --OPTION` | Set fff up: `--init`, `--completions`, `--desktop`, `--file-chooser`, `--check-config`. |

`fff` without a command is `fff roam`. The search, interface and
configuration options (`--depth`, `--glob`, `--palette`, `--config`, ...) work
with every command, before or after its name.

## Shell integration

`fff roam`, the default, changes directory on exit: `q` leaves you where you
started, `!` takes you to the directory you were browsing, Normal-mode Enter
on a directory takes you into it, and on a file opens it and leaves you in its
folder; Ctrl+w does the same with the marked files, leaving you in the last
one's folder. Ctrl+number is Enter on that row.

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
except prompts made for typing (new names, the go-to prompt, the gx/gX command)
and the query of `fff seek`, which start in **Insert mode**.
Use `j`/`k` to browse and `i` or `a` to type a filter. Directory navigation
preserves your current editing mode. See [editing](docs/EDITING.md).

## Browsing and returning paths

This section describes what each confirmation returns in `fff pick`; in
`fff roam` the same keys choose a directory instead (see above).

Without root arguments, browsing starts in the invocation directory (or, with
`--resume`, where the last session ended) and follows navigation. Explicit roots stay pinned: results combine their descendants with
those of the current directory. Overlapping paths appear once at their minimum
depth. Default depth is **2**; immediate children are depth 1. Shallower matches
always come first, followed within each depth by directories and fuzzy ranking.
Matching is a case-insensitive ordered subsequence of the basename.

A query containing `/` matches the path below the result's root instead, so
`pde/stab` lists `pde/notes/stability.tex` but not `notes/stability.tex`.
Each `/`-separated part is a subsequence of a component, the components in
order, and the last part matches the basename: `pde/` lists everything inside
folders matching `pde`, and a leading `/` (`/stab`) only switches to path
ranking. Parts naming whole components, and parts on consecutive components,
rank higher; the matched letters are highlighted along the whole label. Depth
and directories still sort first.

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
  clears the query and highlights it.

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
| Ctrl+w | The marks: opens the marked files (`fff roam`) or returns them (`fff pick`) | fff leaves |
| `q` | Nothing | fff leaves where you started |

[`fff explorer`](docs/DESKTOP.md#fff-explorer), the mode for answering file
dialogs, keeps these meanings: Enter chooses a file and enters a folder, `!`
takes the folder you are in, Ctrl+w the marks, and Ctrl+s saves. A folder
opened from the desktop (`--window`) has no shell to leave into: Enter enters a
folder, and `!` starts a shell in the folder you are in
([details](docs/DESKTOP.md#opening-directories)).

Tab toggles a mark; marks survive navigation and filtering. Ctrl+w finishes
with the marks only. In `fff pick` it returns them, in `fff seek` it leaves
into the last one's folder; in
`fff roam` it **opens them and leaves**, like Normal-mode Enter on a
file: files sharing an opener go to one invocation (three PDFs, one zathura),
GUI openers start detached, and one terminal opener may take over this
terminal. Marked directories, or files needing two different terminal programs,
are refused with a message and fff stays; `o` opens those one after another. Insert-mode Enter in a verified physically empty directory returns marks
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
nearest existing parent. F1's session details list every tab's directory; tabs
are unavailable in `fff seek`.

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
edits the filters during the session, one glob per line. Changing them re-lists
the files already found without rescanning.

Hidden entries are off by default. Explicit `--exclude` glob rules are enforced;
a matching directory prunes its subtree. Rules match basenames or scope-relative
paths, with literal separators. No automatic `.gitignore` integration is enabled.
The status line shows `filters on` and `exclusions on` only while filters or
enforced exclusion rules exist; F1's session details list their globs.
Symlink directories can be selected or entered; recursive discovery does not
follow them.

### Choosing files: `fff seek`

`fff seek [ROOTS]` is a static file choice: it lists only the files below the
current directory (or ROOTS), disables location changes, and never returns
the current directory as an empty-results fallback. It takes the search
options (`--depth`, `--glob`, `--exclude`, `--hidden`). Choosing leaves into
the chosen file's folder; only `fff pick` prints paths. For a printed choice
of files, give `fff pick` a `--glob`, which lists only matching files:

```sh
fff pick ~/papers --depth 4 --glob '*.pdf' -0 | xargs -0 zathura
```

Discovery still traverses directories within the depth limit, and the query
starts in Insert mode, ready to type. Result rows show bare file names, with
only the parents that tell same-named files apart, as in `fff pick`.

### Preview pane

Alt+v (`zp` in Normal mode) shows the highlighted entry in the bottom half of
the screen: a text file's first lines, a folder's entries, an archive's
contents, a PDF's facts and first pages' text, an image's size, a song's or a
video's duration, streams and tags. It is text only, reads in the background
once the highlight rests, and never holds up the list. PDFs use poppler's
`pdfinfo` and `pdftotext`, audio and video FFmpeg's `ffprobe`, when installed.
`[ui] preview = true` starts with it shown; see [preview
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

F1 opens the complete guide, including configured Normal-mode bindings. Its
Session tab shows exact paths, scopes, depth, policies, palette and marks. The
footer's right end points to it and says how to leave: `F1 keys  q quit`
(`Ctrl+c quit` while typing, where `q` is text).

| Key | Action |
| --- | --- |
| i / a, then type | Edit a Neovim field |
| Escape | Leave transient editing modes, then clear the query or close a chooser/location prompt; keep the session |
| Enter in Normal | Exit into a highlighted directory; on a highlighted file, open it and leave (`fff roam`), print it (`fff pick`) or exit into its folder (`fff seek`) |
| Enter while typing | Enter a directory; on a file, return marks + file (`fff pick`) or reveal it in its folder |
| Up / Alt+k | Previous row (in Normal-mode results Up switches [tabs](#tabs); use k) |
| Down / Alt+j | Next row (in Normal-mode results Down switches [tabs](#tabs); use j) |
| h / j / k / l | Normal-mode list actions equivalent to Alt+h/j/k/l |
| gg / G | First / last row in Normal-mode lists |
| Alt+h | Parent directory; retain query and reset highlight/scroll |
| Alt+l / l in Normal | Enter the highlighted directory, or open the highlighted file in the background |
| o in Normal | Open the current or marked files with their openers in this terminal, then come back |
| Alt+g / `/` in Normal | Go to a typed path, or words picking a bookmark or one of zoxide's frequent folders |
| Ctrl+o / Ctrl+i | Back / forward through the folders this tab visited (Ctrl+i needs a terminal that tells it from Tab) |
| Alt+Up / Alt+Down | Next / previous [tab](#tabs) (Up / Down in Normal-mode results) |
| Alt+b / b in Normal | Bookmark chooser; in Normal mode n adds, e edits, d deletes, r reloads (Alt+ while typing); Up/Down reorder |
| Alt+p | Palette chooser; highlight for 100 ms to preview, Enter applies, Escape reverts |
| Alt+P | Reload the configured palette, clearing a session choice |
| Alt+Right / Alt+Left | Increase / decrease depth; minimum 1 (Right / Left in Normal-mode results) |
| 1 … 9 in Normal | Search results to that depth |
| Tab / Space in Normal | Toggle mark; advance in results |
| m / M in Normal | Mark every listed row / invert the listed rows' marks (results or marked list) |
| Alt+Space | Focus / leave the marked list |
| Ctrl+w | Finish with the marks, from results or the marked list: open the marked files and leave (`fff roam`) or return them (`fff pick`) |
| ZZ / ZQ | In multiline Neovim buffers (exclusions, filters, rename), save and close / discard and close; `:wq` / `:q!` also work |
| Ctrl+1 … Ctrl+9 / Ctrl+0 | Enter on that visible row / the last row |
| Alt+1 … Alt+9 / Alt+0 | Like `l` on a visible target / last target |
| PageUp / PageDown | Page through results |
| Alt+. / . in Normal | Toggle hidden entries |
| Alt+d / zd in Normal | Hide directories, listing only files; again to show them |
| Alt+v / zp in Normal | Show / hide the [preview pane](docs/CONFIGURATION.md#preview-pane) |
| Alt+H | Toggle exclusion enforcement |
| Alt+e | Edit session-local exclusions, one glob per line; matching entries (and directory subtrees) are hidden |
| Alt+f | Edit session-local file filters (`--glob`), one glob per line; only files whose name matches one are listed |
| v / p / s in Normal | Move / copy / symlink all marked paths into the current directory, after confirmation |
| x in Normal | Choose current/selected deletion, review sizes, then Enter confirms |
| r in Normal | Batch rename the filtered results in a buffer |
| e in Normal | Extract the current or marked archives (asks when ambiguous) |
| n in Normal | Choose a new file or directory, then enter its name |
| F1 | Key guide; Tab switches to Session details |
| F5 | Force fresh discovery, bypassing cache replay |
| gx / gX in Normal | Run a program on the current or marked paths and come back / exit fff into it |
| E in Normal | Open the current or marked paths in `$EDITOR`, then come back |
| q / ! in Normal | Exit to the launch / current directory, ignoring marks |
| Ctrl+c | Cancel without selected-path output |
| Click / double-click | Highlight a row (clearing the marks) / act like `l` on it (in `fff explorer`, like Enter) |
| Drag with the button held | Mark every row from where it was pressed to where it is; past the top or bottom edge, keep scrolling and marking |
| Mouse wheel | Scroll the results, leaving the highlight and marks alone (over the marked list or a chooser, move through it) |
| Click / double-click a mark or chooser row | Select it / act on it (`l` on a mark, Enter in a chooser) |
| Click a tab number | Switch to that [tab](#tabs), keeping the marks |
| Drop files on the terminal | Copy them into the current directory, after confirmation |

`q` and `!` exit with status 0. In `fff pick` they print the launch or
current directory in the usual output format; in `fff roam` `!` changes
to the current directory and `q` changes nothing. `fff seek` makes either exit
successfully without output. Insert mode keeps these characters as
text. `gx` / `gX` are described under [running programs](docs/OPERATIONS.md#running-programs).

Normal-mode list keys apply to results, marks, bookmarks and palettes; the
location and exclusion fields keep Neovim motions. Plain digits are query text in
Insert mode; in Normal-mode results 1–9 set the search depth
(`depth_digits = false` in `[keys.normal]` makes them Vim counts again). Chords are case-sensitive.
Ctrl+digits require distinct terminal reporting; fff requests the Kitty
keyboard protocol and never guesses ambiguous input. Alt+digits also support
ESC+digit reporting. Remap chords with `--map-key`, or permanently in the
[`[keys.map]`](docs/CONFIGURATION.md#chord-remapping) configuration table:

```sh
fff --map-key ctrl+n=down --map-key ctrl+p=up
fff --map-key alt+j=none
```

Mappings translate once to default chords; later mappings for a source win.
Printable source characters are reserved for editing. Normal-mode sequences are
configured separately under `[keys.normal]`; see [configuration](docs/CONFIGURATION.md).

File operations are confirmed before they change anything, refuse to overwrite
unless a move or copy confirmation says so, and roll back on failure; there is
no undo. Name and destination buffers use Enter to apply and Escape to
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
small uncached directories get up to 4 ms to complete before their first frame.
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
