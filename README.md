# picker

An interactive Rust terminal file picker with fuzzy filename filtering, live depth
changes, fixed search roots, and persistent marked paths.

```sh
cargo build --release
./target/release/picker
./target/release/picker ~/notes ~/papers --depth 2 --glob '*.tex'
./target/release/picker --exclude build --exclude '*.log' --relative --null > paths.bin
```

Run in a terminal on Linux/Unix with `/dev/tty`. The interface uses the controlling
terminal; stdout contains only selected paths, so redirection and command
substitution work. With no root arguments, browsing starts in the invocation
directory and follows navigation; the initial directory is not pinned as a fixed
search root. Defaults: depth 3,
absolute paths, newline records, hidden entries off, explicit exclusions enforced.
Use `--null` (`-0`) for exact record boundaries and `--relative` for paths relative
to the invocation directory. `--help` lists options. Success exits 0, Ctrl+c exits
130 with no selected paths, and startup/I/O errors exit 1.

Results combine descendants of the one current directory and every fixed startup
root explicitly passed on the command line. Navigation changes only the current
directory. The current directory itself is not listed as a selectable result.
Overlapping results appear
once, at their minimum depth. Ties use startup root order, with the additional
current-directory scope ordered after startup roots. Depth comes first, followed
by directories, fuzzy score, root order, and original absolute pathname bytes.
Result labels, marked paths, and the selected-path footer are relative to the
current browsing directory. A single `../` is kept; two or more leading parent
steps collapse to `.../` (for example, `../../papers/report.tex` becomes
`.../papers/report.tex`). This affects display only. The header identifies the
current directory, and F1 retains exact absolute paths and scope-relative labels.
Each result shows its depth and scope number; F1 displays full scope paths and
session details. Discovery runs in a cancellable background worker and incomplete
results are labeled. Query edits filter known results without restarting discovery.

| Key | Action |
| --- | --- |
| Type / Backspace | Edit the filename query |
| Escape | Clear query; leave marks/details; cancel a prompt |
| Enter | Enter directory; return marks plus highlighted file; return current directory plus marks when physically empty and no candidate is highlighted |
| Up / Alt+k | Move highlight up (results and marked list) |
| Down / Alt+j | Move highlight down (results and marked list) |
| Alt+h | Parent directory, retaining query and resetting highlight/scroll |
| Alt+l | Enter the highlighted directory; never confirm files or empty directories |
| Alt+g | Location prompt; Enter navigates, Escape cancels |
| Alt+b | Bookmark chooser; filter names, Enter to visit, Escape to return |
| Alt+Right / Alt+Left | Change depth (minimum 1) |
| Tab | Toggle mark and advance; in marks, toggle without removing the row |
| Alt+Space | Enter/leave marked list; arrows move within it |
| Ctrl+w | Return marks only; with none, stay open |
| Ctrl+1 … Ctrl+9 | Immediately return that visible row, clamping to last row |
| Ctrl+0 | Immediately return last visible row |
| Alt+1 … Alt+9 / Alt+0 | Activate that visible row like Enter: enter directories; confirm files with marks |
| PageUp / PageDown | Change visible page and numbered targets |
| Alt+. | Toggle hidden entries |
| Alt+H | Toggle exclusion enforcement (uppercase H) |
| Alt+e | Built-in exclusion editor; Enter adds a line, Backspace deletes, Ctrl+w applies |
| F1 | Inspect full paths, roots, policies, rules and marks; arrows/page keys scroll |
| F5 | Bypass the current cached snapshot and refresh discovery |
| Ctrl+c | Cancel without selected paths |

Ctrl+number choices ignore marks, return directories immediately, and fall back to
the current directory with no visible rows. They use the last fully rendered
labels. Plain digits always edit the query. Missing marks are labeled and block
Enter/Ctrl+w confirmation until unmarked; numbered choices still ignore marks.
Enter with a no-match query in a nonempty directory leaves the picker open.
Alt+number uses the same visible labels and clamping, but follows Enter behavior:
directories open, files confirm marks plus that file, and missing marks block
confirmation. With no visible rows, Alt+number follows Enter's empty-directory
rules; it does not force a return for a no-match query. Plain Alt+digits work via
standard ESC+digit reporting as well as CSI-u.

Physical Ctrl+digit requires a terminal that reports distinct chords. Picker
requests the Kitty keyboard protocol via Crossterm. CSI-u decoding is tested;
legacy ambiguous bytes (such as Ctrl+2 encoded as NUL) are never guessed. A real
terminal/hardware compatibility matrix has not yet been verified. You can remap
these actions to other distinct chords:

```sh
picker --map-key ctrl+n=alt+j --map-key alt+j=none
picker --map-key ctrl+o=alt+2
```

`--map-key SOURCE=TARGET` translates a chord to a documented default chord in every
view; `TARGET=none` disables it. Modifiers are `ctrl+`, `alt+`, `shift+`; key names
include `enter`, `esc`, `tab`, `space`, `backspace`, arrows, `pageup`, `pagedown`,
`f1`, and `f5`. Letter case matters. Printable source characters are reserved for text.
Mappings are applied once; later mappings for the same source take precedence.

Matching uses a case-insensitive ordered subsequence of the **basename**, scored
by adjacency, word starts, and early positions. Empty queries have equal score.
`--glob` matches only file basenames and never prevents parent traversal.
Exclusions match a basename or a path relative to a discovery scope, including
ancestor prefixes; a matching directory prunes its subtree. Rules use globset
syntax (`*`, `?`, classes, `**`), with literal separators. No `.gitignore` or other
automatic ignore-file integration is enabled. Editor changes are session-local.
Symlink directories can be selected/navigated, but traversal does not follow them.
The built-in editors currently support append/backspace editing rather than a
full editor buffer. No content search or file opener is included.

The Ratatui interface supports configurable palettes, a distinctive current-directory
banner, rounded pane borders, colored focus
and selection, and highlighted fuzzy matches. At 80 columns or wider, marked
paths appear in a pane to the right of the working list. Narrower terminals stack
the panes; Alt+Space expands the marked pane for inspection. Long marked labels
wrap, and F1 shows complete session details. The minimum working size is 36 × 12;
a smaller terminal shows a resize hint with no invisible numbered targets. Frames
use diff rendering and synchronized updates to reduce flicker. `NO_COLOR` is
honored; shortcut text, selection arrows, and missing/progress labels remain visible.

Configuration is optional TOML at `~/.config/picker/config.toml` (or
`$XDG_CONFIG_HOME/picker/config.toml`). It supports built-in/custom color palettes,
ordered bookmarks, and a bounded session discovery cache. Try:

```sh
cargo run -- --config docs/config.example.toml --check-config
cargo run -- --config docs/config.example.toml --palette forest
```

See [the configuration guide](docs/CONFIGURATION.md) and the
[commented config example](docs/config.example.toml) for every setting, path
resolution rules, bookmark controls, and cache behavior. Completed discovery
snapshots appear immediately on matching revisits and refresh in the background.
Cache size and expiry are configurable; F5 bypasses the current snapshot.

Acceptance sources are in [the specification](docs/SPECIFICATION.md) and active
[tui examples](docs/tui_functionality_examples/README.md). First-version decisions and
remaining questions are recorded in the specification. Curated examples remain
unchanged.

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo build --offline
python3 tests/terminal_smoke.py
```

The Rust tests inject discovery events and use temporary filesystem fixtures. The
Linux PTY suite tests Crossterm input decoding, marks, paging, empty states,
policy editing, key remapping, cancellation, and exact separate stdout bytes.
It does not replace physical keyboard checks in your terminal emulator.
