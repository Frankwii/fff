# fuzzy file finder

A Rust terminal file picker with fuzzy filename filtering, directory navigation,
live search depth, pinned roots and persistent marked paths. Version **0.6.0**.
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
./target/release/fff --output-paths --no-dir --relative --null > paths.bin
```

Run in a Linux/Unix terminal with `/dev/tty`. The interface uses the controlling
terminal, never stdout. Put the binary on your PATH as `fff` and add the
[shell integration](#shell-integration) line so that leaving fff can change your
shell's directory. `-h` summarizes options; `--help` includes defaults, examples
and exit codes.

## Shell integration

fff has two modes:

- **Default: change directory on exit.** `q` leaves you where you started, `!`
  takes you to the directory you were browsing, and Enter on a file takes you to
  that file's folder (Ctrl+w: the last mark's folder; Ctrl+number: its target's
  folder, or the target itself for a directory).
- **`--output-paths`: print the selection.** The picker behavior for scripts and
  command substitution, described under [returning paths](#browsing-and-returning-paths):
  `nvim "$(fff --output-paths)"`, `fff --output-paths -0 | xargs -0 ls -ld`.

A program cannot change its parent shell's directory, so the shell does it, as
with nnn's quitcd scripts and zoxide. Add one line to your shell's startup file:

```sh
eval "$(fff --init zsh)"     # ~/.zshrc (or $ZDOTDIR/.zshrc)
eval "$(fff --init bash)"    # ~/.bashrc
fff --init fish | source     # ~/.config/fish/config.fish
```

This defines a shell function named `fff` that runs the real binary with
`--cd-file` (a private temporary file), then changes to the directory fff wrote
there, if any. Arguments pass through unchanged, so `fff --output-paths` keeps
printing paths. Without the function, the default mode prints the chosen
directory instead, so `cd "$(command fff)"` also works.

Every text field is a clean embedded Neovim (`nvim` must be on PATH). Fields start in **Normal mode**,
except prompts made for typing (new names, the go-to prompt, the gx/gX command)
and the query of a `--no-dir` session, which start in **Insert mode**.
Use `j`/`k` to browse and `i` or `a` to type a filter. Directory navigation
preserves your current editing mode. See [editing](docs/EDITING.md).

## Browsing and returning paths

This section describes what each confirmation returns with `--output-paths`; in
the default mode the same keys choose a directory instead (see above).

Without root arguments, browsing starts in the invocation directory (or, in the
default mode, where your last session ended; see [remembered
locations](docs/CONFIGURATION.md#remembered-locations)) and follows navigation. Explicit roots stay pinned: results combine their descendants with
those of the current directory. Overlapping paths appear once at their minimum
depth. Default depth is **2**; immediate children are depth 1. Shallower matches
always come first, followed within each depth by directories and fuzzy ranking.
Matching is a case-insensitive ordered subsequence of the basename.

Enter depends on the editing mode:

- **Normal mode:** Enter exits *into the folder* of the highlighted entry: a
  directory itself, or the folder a file is in, also for deep results
  (`notes/more/a.pdf` exits into `notes/more`); with nothing listed, the browsing
  directory. In the default mode the shell changes there; with `--output-paths`
  the folder is printed. Marks are ignored. In the marked list Enter does the
  same for the highlighted mark.
- **While typing (Insert mode):** Enter opens a directory. On a file, with
  `--output-paths` it returns the marked paths in insertion order plus that file
  once; in the default mode it *reveals* the file: fff moves to its folder,
  clears the query and highlights it.

To move around, use `l` (or Alt+l, or Alt+number for a labelled row): it enters
a directory, and **opens a file** with its [opener](docs/OPERATIONS.md#opening-files)
in the background while fff keeps the keyboard. `o` opens the current or marked
files in this terminal instead, and fff comes back when the program ends.

Tab toggles a mark; marks survive navigation and filtering. Ctrl+w returns marks
only. Insert-mode Enter in a verified physically empty directory returns marks
plus that directory; a no-match query in a nonempty directory stays open.
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
are unavailable with `--no-dir`.

Ctrl+number immediately returns one visible target, ignoring marks. Alt+number
acts like `l` on the same target: it enters a directory or opens a file in the
background. Labels refer to the last fully rendered page: 1–9 select that
position, out-of-range digits clamp to the last row, and 0 selects the last row.
With no visible results, Ctrl+number returns the current directory and
Alt+number does nothing.

`--no-dir` provides static file selection: it hides directories, disables location
changes, and never returns the current directory as an empty-results fallback.
Discovery still traverses directories within the depth limit, and the query
starts in Insert mode, ready to type. Result rows show
bare file names; when listed files share a name, each gains the fewest trailing
parent directories that tell it apart (`folder1/main.pdf`, `folder2/main.pdf`),
independently of the query. The footer still shows the highlighted file's path.

Output defaults to absolute, newline-terminated paths. `--relative` uses the
invocation directory as a stable base; `--null` (`-0`) writes NUL-delimited records.
Other display labels are relative to the browsing directory, with repeated leading
parent steps shortened to `.../`; output paths never use that abbreviation.

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

## Keys

F1 opens the complete guide, including configured Normal-mode bindings. Its
Session tab shows exact paths, scopes, depth, policies, palette and marks.

| Key | Action |
| --- | --- |
| i / a, then type | Edit a Neovim field |
| Escape | Leave transient editing modes, then clear the query or close a chooser/location prompt; keep the session |
| Enter in Normal | Exit into the highlighted entry's folder (printed with `--output-paths`) |
| Enter while typing | Enter a directory; on a file, return marks + file (`--output-paths`) or reveal it in its folder |
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
| Ctrl+w | Return marks from results or the marked list |
| ZZ / ZQ | In multiline Neovim buffers (exclusions, filters, rename), save and close / discard and close; `:wq` / `:q!` also work |
| Ctrl+1 … Ctrl+9 / Ctrl+0 | Immediately return one visible target / last target |
| Alt+1 … Alt+9 / Alt+0 | Like `l` on a visible target / last target |
| PageUp / PageDown | Page through results |
| Alt+. / . in Normal | Toggle hidden entries |
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

`q` and `!` exit with status 0. With `--output-paths` they print the launch or
current directory in the usual output format; in the default mode `!` changes
to the current directory and `q` changes nothing. `--no-dir` makes either exit
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
fff --config docs/config.example.toml --check-config
```

- [Configuration guide](docs/CONFIGURATION.md): every setting, bookmarks, icons,
  key mappings, cache limits and prefetch budgets.
- [Commented example](docs/config.example.toml): a complete starting configuration.
- [Palette catalog](docs/PALETTES.md): built-ins, live switching and color sources.
- [Editing guide](docs/EDITING.md): Normal/Insert modes and clean Neovim embedding.

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
source <(fff --completions bash)        # ~/.bashrc
source <(fff --completions zsh)         # ~/.zshrc, after compinit
fff --completions fish | source         # ~/.config/fish/config.fish
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
Possible extensions are listed in the [roadmap](docs/ROADMAP.md).

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
