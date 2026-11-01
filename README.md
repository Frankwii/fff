# fuzzy file finder

A Rust terminal file picker with fuzzy filename filtering, directory navigation,
live search depth, pinned roots and persistent marked paths. Version **0.3.0**.
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
./target/release/fff ~/notes ~/papers --depth 2 --glob '*.tex'
./target/release/fff --no-dir --relative --null > paths.bin
```

Run in a Linux/Unix terminal with `/dev/tty`. The interface uses the controlling
terminal; stdout contains only selected paths, so redirection and command
substitution work. Use the built binary directly or put it on your PATH as `fff`.
`-h` summarizes options; `--help` includes defaults, examples and exit codes.

The default editor is a clean embedded Neovim. Fields start in **Normal mode**,
except new file/directory naming buffers, which start in **Insert mode**.
Use `j`/`k` to browse and `i` or `a` to type a filter. Directory navigation
preserves your current editing mode. Neovim is optional;
`--editor basic` selects direct typing/Backspace, and missing Neovim causes a quiet
fallback unless explicitly requested. See [editing](docs/EDITING.md).

## Browsing and returning paths

Without root arguments, browsing starts in the invocation directory and follows
navigation. Explicit roots stay pinned: results combine their descendants with
those of the current directory. Overlapping paths appear once at their minimum
depth. Default depth is **3**; immediate children are depth 1. Shallower matches
always come first, followed within each depth by directories and fuzzy ranking.
Matching is a case-insensitive ordered subsequence of the basename.

Enter opens directories. On a file, it returns marked paths in insertion order
plus that file once. Tab toggles a mark; marks survive navigation and filtering.
Ctrl+w returns marks only. Enter in a verified physically empty directory returns
marks plus that directory; a no-match query in a nonempty directory stays open.
Missing marks block confirmation until unmarked.

Ctrl+number immediately returns one visible target, ignoring marks. Alt+number
activates the same target like Enter. Labels refer to the last fully rendered
page: 1–9 select that position, out-of-range digits clamp to the last row, and 0
selects the last row. With no visible results, Ctrl+number returns the current
directory; Alt+number follows Enter's empty-state rules.

`--no-dir` provides static file selection: it hides directories, disables location
changes, and never returns the current directory as an empty-results fallback.
Discovery still traverses directories within the depth limit. `--glob '*.tex'`
likewise lists matching files while traversing their parents.

Output defaults to absolute, newline-terminated paths. `--relative` uses the
invocation directory as a stable base; `--null` (`-0`) writes NUL-delimited records.
Display labels are relative to the browsing directory, with repeated leading
parent steps shortened to `.../`; output paths never use that abbreviation.

Hidden entries are off by default. Explicit `--exclude` glob rules are enforced;
a matching directory prunes its subtree. Rules match basenames or scope-relative
paths, with literal separators. No automatic `.gitignore` integration is enabled.
Symlink directories can be selected or entered; recursive discovery does not
follow them.

## Keys

F1 opens the complete guide, including configured Normal-mode bindings. Its
Session tab shows exact paths, scopes, depth, policies, palette and marks.

| Key | Action |
| --- | --- |
| i / a, then type | Edit a Neovim field; basic editing accepts text directly |
| Escape | Leave transient editing modes, then clear the query or close a chooser/location prompt; keep the session |
| Enter | Enter a directory or confirm files/marks |
| Up / Alt+k | Previous row |
| Down / Alt+j | Next row |
| h / j / k / l | Normal-mode list actions equivalent to Alt+h/j/k/l |
| gg / G | First / last row in Normal-mode lists |
| Alt+h | Parent directory; retain query and reset highlight/scroll |
| Alt+l | Enter highlighted directory only |
| Alt+g | Location prompt |
| Alt+b | Bookmark chooser; Alt+n adds, Alt+e edits, Alt+d deletes, Alt+Up/Down reorders, Alt+r reloads |
| Alt+p | Palette chooser; highlight for 100 ms to preview, Enter applies, Escape reverts |
| Alt+P | Reload the configured palette, clearing a session choice |
| Alt+Right / Alt+Left | Increase / decrease depth; minimum 1 |
| Tab / Space in Normal | Toggle mark; advance in results |
| Alt+Space | Focus / leave the marked list |
| Ctrl+w | Return marks from results or the marked list |
| ZZ / ZQ | In Neovim buffers (exclusions, operations), save and close / discard and close; `:wq` / `:q!` also work |
| Ctrl+1 … Ctrl+9 / Ctrl+0 | Immediately return one visible target / last target |
| Alt+1 … Alt+9 / Alt+0 | Activate a visible target / last target like Enter |
| PageUp / PageDown | Page through results |
| Alt+. | Toggle hidden entries |
| Alt+H | Toggle exclusion enforcement |
| Alt+e | Edit session-local exclusions |
| v / p / s in Normal | Move / copy / symlink all marked paths into the current directory, after confirmation |
| x in Normal | Choose current/selected deletion, review sizes, then Enter confirms |
| r in Normal | Batch rename the filtered results in a buffer |
| e in Normal | Extract the highlighted archive |
| n in Normal | Choose a new file or directory, then enter its name |
| F1 | Key guide; Tab switches to Session details |
| F5 | Force fresh discovery, bypassing cache replay |
| q / ! in Normal | Exit and return the launch / current directory, ignoring marks |
| Ctrl+c | Cancel without selected-path output |

`q` and `!` return one directory path with status 0, using the usual output
format. `--no-dir` makes either exit successfully without output. Insert/basic
editing keeps these characters as text. A shell wrapper can use the returned
path to change directories; fff itself cannot change its parent shell's directory.

Normal-mode list keys apply to results, marks, bookmarks and palettes; the
location and exclusion fields keep Neovim motions. Plain digits are query text in
Insert/basic mode and Vim counts in Normal mode. Chords are case-sensitive.
Ctrl+digits require distinct terminal reporting; fff requests the Kitty
keyboard protocol and never guesses ambiguous input. Alt+digits also support
ESC+digit reporting. Remap chords with `--map-key`:

```sh
fff --map-key ctrl+n=down --map-key ctrl+p=up
fff --map-key alt+j=none
```

Mappings translate once to default chords; later mappings for a source win.
Printable source characters are reserved for editing. Normal-mode sequences are
configured separately under `[keys.normal]`; see [configuration](docs/CONFIGURATION.md).

File operations are confirmed before they change anything, refuse to overwrite
unless a move or copy confirmation says so, and roll back on failure; there is
no undo. Rename and destination/name buffers use `ZZ` to apply or `ZQ` to
discard. See [file operations](docs/OPERATIONS.md) for confirmations, guarantees,
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
Alt+i opens a searchable Nerd Font icon picker. In the chooser, Alt+Up/Alt+Down
reorder bookmarks. Saves update the active TOML file; `--no-config`
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
| 0 | Paths selected, configuration valid, or help/completion printed |
| 1 | Startup, configuration or I/O error, including an unknown palette |
| 2 | Invalid CLI usage or malformed argument value |
| 130 | Cancelled with Ctrl+c; no selected paths |

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
