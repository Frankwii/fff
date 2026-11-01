# Configuration, palettes, bookmarks, and caching

Picker reads an optional TOML file at startup. You can run it without a config;
the default palette is `midnight`, bookmarks start empty, and caching is enabled.

## Start here

Save this as `$XDG_CONFIG_HOME/picker/config.toml`. Following the XDG base
directory spec, `~/.config/picker/config.toml` is used when `XDG_CONFIG_HOME` is
unset, empty, or not absolute. `picker --check-config` prints the file it read:

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

## Status messages

Notices such as "Navigation is disabled with --no-dir" or a failed palette reload
appear in red on the bottom line, or inside an open prompt. They clear on their
own after `message_seconds` (default 4). Triggering the same notice again restarts
the timer; `0` keeps each message until another replaces it.

```toml
[ui]
message_seconds = 4   # 0..3600; 0 = never hide automatically
```

## Bookmarks

Each `[[bookmarks]]` entry has a unique, nonempty `name`, a nonempty `path`, and
an optional `icon`. Entries appear in config order:

```toml
[[bookmarks]]
name = "Downloads"
icon = "󱃩"
path = "~/Downloads/"

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

`icon` is any text one or two terminal columns wide (surrounding spaces are
trimmed), typically a Nerd Font glyph or an emoji. It is drawn before the name in
the chooser. Bookmarks without one show the folder icon when `[ui] icons` is on;
a configured `icon` always shows, even with file icons off.

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

## Discovery cache and performance

```toml
[cache]
enabled = true          # keep snapshots of visited directories in memory
max_entries = 16        # how many directory snapshots to keep (1..128)
max_candidates = 4096   # paths stored per snapshot (1..100000)
ttl_seconds = 60        # age after which a snapshot counts as stale (1..86400)

[performance]
refresh_ms = 50         # minimum time between re-sorts during a scan (0..1000)
```

Everything here is optional. The defaults keep memory modest (the cache tops out
around 8 MB) while making revisits instant; none of these settings changes which
paths are found, their order, or what is returned.

### What happens when you open a directory

1. **Snapshot replay.** If the cache holds a snapshot for exactly this view, its
   paths appear at once (the status pill says `◌ cached · refreshing`) and the
   frame is drawn before any new scan results are processed.
2. **Background scan.** A worker thread always walks the directory again, down to
   the current depth, and sends what it finds in batches. Input stays live: you
   can type, move, or leave while it runs, and leaving cancels it.
3. **Live ordering.** While results stream in, the list is re-sorted at most every
   `refresh_ms`, plus immediately for the first batch and when the scan ends, so
   shallower results still move above deeper ones as they arrive.
4. **Reconciliation.** When the scan completes, paths that no longer exist and
   changed file/directory kinds are corrected, the status becomes `● complete`,
   and a fresh snapshot is stored.

What is cached is a list of discovered paths per **view**, where a view is the
combination of current directory, pinned roots, depth, hidden-file visibility,
exclusion enforcement and rules, and `--glob`. Changing any of these (e.g.
Alt+→ for more depth) is a different view with its own snapshot. Your query,
highlight, scroll position, and marks are never cached: the current query filters
whatever the snapshot restores, and marks are separate session state.

### `[cache]` settings

| Setting | Default | Range | What it does |
| --- | --- | --- | --- |
| `enabled` | `true` | `true`/`false` | Turns snapshots on or off. Off means every visit starts from an empty list and waits for the scan; nothing else changes. |
| `max_entries` | `16` | 1–128 | How many views are remembered. When full, the view used least recently is dropped. Raise it if you hop between many directories (or depths) and want all of them instant; each extra entry costs at most `max_candidates` paths. |
| `max_candidates` | `4096` | 1–100000 | How many paths one snapshot keeps. Picker stores the best-ranked matches of the view first, then fills the remaining room with other found paths. Directories larger than this replay a partial list instantly and the scan fills in the rest, so the limit trades instant completeness of big directories against memory. |
| `ttl_seconds` | `60` | 1–86400 | Age after which a snapshot is *stale*. Stale snapshots are still shown instantly (they are only dropped by `max_entries`), but they are always re-verified by the scan; this setting mostly tells you how old the instant view may be. Every successful scan resets the age. |

**Memory.** A cached path costs roughly 100–150 bytes (the path text plus a little
bookkeeping), so the worst case is about `max_entries × max_candidates × 125 B`:
around 8 MB with the defaults, 50 MB at `max_entries = 32` and
`max_candidates = 12000`. The cache only fills up to what you actually visit.

Separately from the cache, picker holds an index of the **current** view while you
browse it, sized by what that view contains (depth matters most: depth 3 over a
home directory can be hundreds of thousands of paths). Lower the depth with
Alt+← or `--depth` if a view is too large; the cache settings do not bound it.

**Safety.** A cached path that has disappeared cannot be returned while the
refresh runs, and an empty cached listing cannot confirm an empty directory until
the scan verifies it. Failed or obsolete scans never become snapshots. A visit you
leave early can store the matches you already saw, marked incomplete; it never
replaces a complete snapshot.

**F5** discards the current view's snapshot and rescans, keeping your query,
marks, roots, depth, and policies. Nothing is written to disk: there is no
persistent index, cache file, or filesystem watcher.

### `[performance]` settings

| Setting | Default | Range | What it does |
| --- | --- | --- | --- |
| `refresh_ms` | `50` | 0–1000 | Minimum milliseconds between re-sorting the result list while a scan is still streaming in. Each re-sort ranks every path found so far, so on large trees re-sorting after every batch dominated scan time. Lower values make the list settle more continuously during a scan; higher values finish big scans sooner. `0` re-sorts after every batch (the old behavior). Typing, navigation and the end of a scan always update immediately. |

### Built-in optimizations

These need no configuration and do not change results; tests compare each fast
path with the original definition.

- **Batched discovery.** The worker sends up to 256 paths per message (or whatever
  it has after 10 ms), and the interface processes messages for at most 8 ms per
  frame, so input is never stuck behind a burst of results.
- **Throttled re-sorting** during scans, as described under `refresh_ms`.
- **Incremental filtering.** Typing more letters can only remove matches, so
  picker re-scores just the current matches instead of every known path. Deleting
  letters (or any change of directory, depth, or policy) uses a full re-rank.
- **Allocation-free scoring** for ASCII names, and a byte-level hidden/root check
  when no exclusion rules are active. Non-ASCII names and active exclusion rules
  use the original, fully general code.
- **Idle frames.** The screen is redrawn after input or a visible change, and once
  a second otherwise (so a deleted mark still shows as missing). An idle picker
  waits 250 ms between checks instead of 30 ms.
- **Memory.** The scan's bookkeeping set is freed when the scan completes.

Measured on a synthetic tree of 102 000 entries (release build, depth 3):

| | Before | After |
| --- | --- | --- |
| Cold scan until `● complete` | 18.6 s | 1.2–1.4 s |
| Typing a letter (average / worst) | 96 / 146 ms | 17–25 / 34–42 ms |
| Deleting a letter (average / worst) | 78 / 93 ms | 65 / 95 ms |
| CPU while idle | 30 % | 1 % |
| Memory after the scan (RSS) | 118 MB | 84–100 MB |

On ordinary directories (a few thousand entries) every interaction is well under
a millisecond of picker work.

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
