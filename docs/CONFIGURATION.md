# Configuration, palettes, bookmarks, and caching

Picker reads an optional TOML file at startup. You can run it without a config;
the default palette is `midnight`, bookmarks start empty, and caching is enabled.

## Start here

Save this as `~/.config/picker/config.toml` (or
`$XDG_CONFIG_HOME/picker/config.toml` when `XDG_CONFIG_HOME` is set):

```toml
[ui]
palette = "forest"

[[bookmarks]]
name = "Home"
path = "~"

[[bookmarks]]
name = "Notes"
path = "~/notes"
```

Then run `picker` and press **Alt+b** to open bookmarks. Type part of a bookmark's
name and press **Enter** to navigate. **Escape** returns to the file list.

The [commented example](config.example.toml) also shows a custom palette and all
cache settings. From the repository, you can try it without installing a config:

```sh
cargo run -- --config docs/config.example.toml --check-config
cargo run -- --config docs/config.example.toml --palette ember
```

`--check-config` validates and exits without opening the TUI. Its status message
uses stderr; stdout remains empty. It checks syntax, names, colors, and limits,
while unavailable bookmark directories are handled when you try to visit them.

## File selection and overrides

| Option | Effect |
| --- | --- |
| Default | Read `$XDG_CONFIG_HOME/picker/config.toml`, or `~/.config/picker/config.toml` when XDG is unset/empty/not absolute. |
| `--config PATH` | Read exactly this file, replacing default-file lookup. Relative `PATH` is relative to the invocation directory. |
| `--no-config` | Ignore the default file and use built-in settings. Cannot be combined with `--config`. |
| `--palette NAME` | Override `[ui].palette`; custom palettes still come from the selected config file. Shell completion offers built-in and custom names. |
| `--no-icons` | Hide filetype icons for this invocation, overriding `[ui] icons`. |
| `--check-config` | Validate the selected configuration and exit. |

A missing default file uses defaults. An explicitly requested missing file,
invalid TOML, unknown field, unknown palette, or invalid setting produces a clear
startup error naming the file. Picker does not create or rewrite your config.
Palette edits reload live; bookmark/cache/editor edits take effect on the next invocation.
Existing CLI discovery/output options
continue to work independently of this file.

## Color palettes

The original built-in choices are:

| Name | Appearance |
| --- | --- |
| `midnight` | Deep blue background with cyan directories and gold marks. |
| `light` | Light panels, dark text, and teal focus accents. |
| `forest` | Dark green panels with soft green accents and gold marks. |

Also available: all four `catppuccin-*` flavors, `nord`, `gruvbox-dark` /
`gruvbox-light`, and all four `tokyonight-*` variants. See the
[palette catalog and official color sources](PALETTES.md). Saving color changes
live-reloads the running interface; invalid edits retain the last working colors.
While picker runs, **Alt+p** opens a live-preview palette chooser (Enter applies
for the session only; Escape reverts) and **Alt+P** re-reads this file's palette
immediately. The config file is never rewritten; see
[switching palettes while running](PALETTES.md#switching-palettes-while-running).

Choose one in TOML or for a single invocation:

```sh
picker --palette light
```

Define a named custom palette by inheriting a built-in and overriding only the
roles you want to change:

```toml
[ui]
palette = "ember"

[palettes.ember]
extends = "midnight"
accent = "#f0a76c"
marked = "#e8c979"
current_directory = "#623a28"
current_directory_text = "#fff3df"
```

`extends` defaults to `midnight` and must name a built-in palette. Custom names
must be nonempty and cannot replace built-in names. All color values accept
`"#RRGGBB"` or `"default"`, which uses the terminal's own foreground/background
color for that role. Unspecified roles are inherited.

| Color role | Used for |
| --- | --- |
| `background` | Whole-screen background and spacing. |
| `panel` | Result panes, marked pane, and dialogs. |
| `text` | Main foreground text. |
| `muted` | Metadata, paths, and keyboard hints. |
| `border` | Unfocused pane borders. |
| `accent` | Directories and active result/filter borders. |
| `marked` | Marks, matched query letters, and chooser accents. |
| `success` | Completed-discovery indicator. |
| `error` | Missing paths and error messages. |
| `selection` | Highlighted-row background. |
| `current_directory` | Background of the `@ <current directory>` segment below the filter. |
| `current_directory_text` | Foreground of that segment; `@` and the basename are bold, the basename underlined. |

`NO_COLOR` is honored. Labels, selection arrows, bold focus borders, and textual
progress/missing indicators remain useful without color.

## Filetype icons

Result and marked rows show a [Nerd Font](https://www.nerdfonts.com/) glyph for
each entry: a folder for directories and a filetype glyph chosen from well-known
names (`Cargo.toml`, `Makefile`, `.gitignore`, ...) or the extension (`.rs`,
`.py`, `.md`, `.tex`, `.pdf`, images, audio, video, archives, ...). Unknown files
get a generic file glyph. Icons are colored with palette roles (directories use
`accent`, code `success`, media and archives `marked`, documents `text`, others
`muted`), so custom palettes and `NO_COLOR` apply to them too.

Your terminal font must include Nerd Font glyphs; otherwise you will see boxes or
blanks. Turn icons off to restore the plain `▸` (directory) / `·` (file) markers:

```toml
[ui]
icons = false
```

or pass `--no-icons` for one invocation. Icons never affect matching, numbering,
or output paths, and the row layout has the same width either way.

## Bookmarks

Each `[[bookmarks]]` entry has a unique, nonempty `name` and a nonempty `path`.
Entries appear in config order:

```toml
[[bookmarks]]
name = "Research papers"
path = "~/Documents/Research papers"

[[bookmarks]]
name = "Shared workspace"
path = "/mnt/shared/work"

[[bookmarks]]
name = "Nearby notes"
path = "../../notes"
```

Path rules:

- `~` and `~/...` expand using `HOME`.
- Absolute paths are used as written.
- Other relative paths resolve from the **config file's directory**, so a bookmark
  has the same destination regardless of where you launch picker.
- Paths are literal TOML strings: spaces work directly; shell substitutions and
  environment-variable expansion are not performed.

In the chooser, typing fuzzy-filters bookmark **names**. Up/Down or Alt+k/Alt+j
moves the highlight; PageUp/PageDown moves farther. Enter opens a directory;
Escape or Alt+b returns to results. An unavailable directory or a bookmark that
points to a file is shown as unavailable; trying it leaves the chooser open with
an error. Bookmarks never select a file or finish the program.

Opening a bookmark preserves marks, discovery depth, policies, and explicitly
supplied fixed roots. It changes the single current directory and clears the
filename query. Bookmarks are navigation destinations, not additional roots or
preselected output paths.

Related navigation keys:

| Key | Action |
| --- | --- |
| Alt+l | Enter the highlighted directory, like Enter on a directory. On a file, an empty directory, or no results, do nothing; it never confirms output. |
| Alt+g | Open the location-entry prompt. Enter navigates, Escape cancels. |
| Alt+h | Parent directory, keeping the filename query. |
| Alt+b | Open/close the bookmark chooser. |
| Alt+p | Open/close the palette chooser (live preview; Enter applies for this session, Escape reverts). |
| Alt+P | Re-read the configured palette now, clearing an Alt+p session choice. |

Alt+l replaces the old location-prompt binding; that prompt is now on Alt+g.
These chords remain remappable with `--map-key`.

## Discovery cache

```toml
[cache]
enabled = true
max_entries = 16
max_candidates = 4096
ttl_seconds = 60
```

The cache lives **in memory for one picker session**. Visible prefixes are also
checkpointed before navigation, so an interrupted first visit can replay useful
results. Such prefixes remain provisional and cannot replace a complete snapshot.
When a scan completes
successfully, picker saves a bounded snapshot of its first matching results,
filling spare capacity with other known candidates. Revisiting that directory
with the same discovery options installs the snapshot immediately, then always
starts background discovery to refresh it. Large snapshots may contain only an
initial subset; the refresh fills the rest.

The cache key includes the current directory, ordered discovery scopes, depth,
hidden visibility, exclusion enforcement and rules, and positive file glob.
Filename query, highlight, viewport, and marks are not cached: the current query
filters restored candidates, and marks remain independent session state.

| Setting | Default | Allowed range |
| --- | --- | --- |
| `enabled` | `true` | `true` / `false` |
| `max_entries` | `16` | 1–128 completed-scope snapshots |
| `max_candidates` | `4096` | 1–100000 paths per snapshot |
| `ttl_seconds` | `60` | 1–86400 seconds from snapshot completion |

Least-recently-used entries are evicted at the entry limit. `ttl_seconds` marks
a snapshot stale; stale snapshots remain available for immediate provisional
replay until LRU eviction. A successful refresh replaces their age and contents.
Stale-generation and failed scans never become new snapshots. Cancelled visits
can retain an explicitly checkpointed prefix, always marked incomplete.

A replay gets a completed rendered frame before processing new scan batches;
the UI does not wait for the worker to refresh it. Snapshot sharing avoids an
extra full clone on lookup, and checkpoint work is bounded by the path limit.

While a snapshot is visible, the status says **cached · refreshing (incomplete)**.
The fresh scan reconciles additions, removals, file/directory changes, and sort
order, retaining a surviving highlighted path. Removed cached targets cannot be
confirmed during refresh. An empty cached listing cannot confirm an empty
directory until fresh discovery verifies that it is physically empty. Missing
marks still require unmarking before Enter/Ctrl+w confirmation.

Press **F5** to discard the current snapshot and force a fresh scan, preserving
your query, marks, roots, depth, and policies. Set `enabled = false` to disable
snapshot storage and replay entirely. No persistent index, watcher, or cache file
is created.

Cache tests verify synchronous replay without sleeps, option-key isolation,
query/mark independence, LRU eviction, deterministic expiry, snapshot bounds,
partial/error/cancelled-scan rejection, stale events, overlap attribution,
filesystem reconciliation, empty-directory safety, and forced refresh. Terminal
checks also revisit a directory after changing its files.


## Modal text editing

```toml
[editor]
backend = "basic" # Default: "nvim"; CLI override: --editor basic|nvim
```

By default a clean headless Neovim instance edits every field. When `backend`
is left unset and `nvim` is missing or fails to start, picker silently uses
basic append/Backspace editing; an explicit `"nvim"` reports the fallback. See the
[editing guide](EDITING.md) for Insert/Normal/Visual modes, Escape semantics,
reserved picker shortcuts, dependency/fallback behavior, and tested boundaries.

## Normal-mode list keys

While a field is in Neovim **NORMAL** mode, list views (results, bookmark
chooser, palette chooser, marked list) accept plain motion keys; see
[EDITING.md](EDITING.md#normal-mode-list-keys). Rebind them in `[keys.normal]`:

```toml
[keys.normal]
left = "h"           # Same as Alt+h in the view (results: parent directory)
down = "j"           # Same as Alt+j
up = "k"             # Same as Alt+k
right = "l"          # Same as Alt+l (results: enter highlighted directory)
first = "gg"         # Highlight the first row
last = ["G", "<space>e"] # A list gives alternatives
```

Each value is one key sequence or a list of sequences; `"none"`, `""`, or `[]`
disables the action. A sequence is typed literally, one printable key per
character (uppercase means Shift); write `<space>` for Space and `<lt>` for `<`.
Ctrl/Alt chords, Escape, Enter, and other named keys are rejected: Alt and Ctrl
chords remain picker shortcuts (remap those with `--map-key`).
Sequences must be unambiguous: two actions cannot share a sequence, and one
sequence cannot be a prefix of another, because a waiting prefix has no timeout.
Unbound keys and unfinished prefixes still reach Neovim. `--check-config` reports
unknown actions and invalid or conflicting sequences. Changes take effect on the
next invocation.
