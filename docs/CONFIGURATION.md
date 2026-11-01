# Configuration, palettes, bookmarks, and caching

Fuzzy file finder reads an optional TOML file at startup. You can run it without a config;
the default palette is `midnight`, bookmarks start empty, and caching is enabled.

## Start here

Save this as `$XDG_CONFIG_HOME/fff/config.toml`. Following the XDG base
directory spec, `~/.config/fff/config.toml` is used when `XDG_CONFIG_HOME` is
unset, empty, or not absolute. `fff --check-config` prints the file it read:

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

Run `fff` and press **Alt+b** to open bookmarks. Press **i** to filter names,
then **Enter** to navigate. From Normal mode, **Escape** returns to results.

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
| Default | Read `$XDG_CONFIG_HOME/fff/config.toml`, or `~/.config/fff/config.toml` when XDG is unset/empty/not absolute. |
| `--config PATH` | Read exactly this file, replacing default-file lookup. Relative `PATH` is relative to the invocation directory. |
| `--no-config` | Ignore the default file and use built-in settings. Cannot be combined with `--config`. |
| `--palette NAME` | Override `[ui].palette`; custom palettes still come from the selected config file. Shell completion offers built-in and custom names. |
| `--no-icons` | Hide filetype icons for this invocation, overriding `[ui] icons`. |
| `--check-config` | Validate the selected configuration and exit. |

The config directory is always `fff`, independently of the crate name
`fuzzy-file-finder` or the executable name. After renaming or rebuilding a local
installation, make sure your PATH entry or symlink points to `target/release/fff`
rather than an older `target/release/picker` binary.

A missing default file uses defaults. An explicitly requested missing file,
invalid TOML, unknown field, unknown palette, or invalid setting produces a clear
startup error naming the file. Explicit bookmark saves/deletions update the active
config; a missing default file is created on the first save. Startup alone never
writes it. With `--no-config`, bookmark edits last only for the session.
Palette edits reload live; external bookmark edits reload with Alt+r in the
bookmark chooser. Cache/editor edits take effect on the next invocation.
CLI discovery/output options are independent of this file.

## Color palettes

Browse built-ins and custom palettes with **Alt+p**; a highlight held for at least
100 ms previews colors. Moving again restarts the wait. Enter applies immediately
for the session, and Escape from Normal mode reverts. **Alt+P** re-reads the configured palette. The config file is never rewritten.

See the [palette catalog](PALETTES.md) for all names, live-reload behavior and
upstream sources.

Choose one in TOML or for a single invocation:

```sh
fff --palette light
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
| `link` | Result names of symbolic links, including links to directories. Built-ins use a vivid blue distinct from `accent`. |
| `executable` | Result names of regular files with any execute bit. Built-ins reuse `success`. |
| `empty` | Result names of empty (zero-byte) regular files. Built-ins reuse `error`. |

Result names use the first matching role: `link`, `accent` for directories,
`empty`, `executable`, then `text`. Only visible rows are inspected, so the
colors follow the file's current state without slowing discovery. A custom
palette that overrides `success` or `error` keeps the inherited `executable` and
`empty` colors unless it sets them too.

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
  has the same destination regardless of where you launch fff.
- Paths are literal TOML strings: spaces work directly; shell substitutions and
  environment-variable expansion are not performed.

In the chooser, press `i` to type and fuzzy-filter bookmark **names** (Neovim
fields initially start in Normal mode). Up/Down or Alt+k/Alt+j
moves the highlight; PageUp/PageDown moves farther. Alt+1 through Alt+9 open
the corresponding visible numbered bookmark, and Alt+0 opens the last visible
bookmark. Out-of-range digits clamp to the last visible entry, including after
filtering or scrolling. These shortcuts work in every editing mode.
Enter opens a directory;
Escape or Alt+b returns to results. An unavailable directory or a bookmark that
points to a file is shown as unavailable; trying it leaves the chooser open with
an error. Bookmarks never select a file or finish the program.

Opening a bookmark preserves marks, discovery depth, policies, and explicitly
supplied fixed roots. It changes the single current directory and clears the
filename query. Bookmarks are navigation destinations, not additional roots or
preselected output paths.

### Editing bookmarks

In the chooser, **Alt+n** adds the current directory, **Alt+e** edits the
highlighted bookmark, and **Alt+d** asks for confirmation before deleting it.
Deletion removes only the bookmark record. **Alt+r** reloads bookmarks from disk.
**Alt+Up** / **Alt+Down** move the highlighted bookmark one place and save the
new order immediately; while filtering, it moves past its visible neighbour.
Comments directly above a `[[bookmarks]]` table move with it.

The form opens at the bottom like the file-operation dialogs and edits name,
path and icon. **Enter** saves and **Escape** cancels, from any editing mode:
in Neovim, Escape first leaves Insert/Visual mode, and Enter during command-line
or pending input still goes to Neovim. Neovim's `ZZ` / `:wq` also save and
`ZQ` / `:q!` cancel. Tab/Shift+Tab or Up/Down switch fields. **Alt+i** (or
Space on the icon row) opens a searchable selection of 37 choices,
including common Nerd Font glyphs and **Automatic** to clear a custom icon.
Press `i` to filter in Neovim, then Escape and Enter to choose. Existing custom
icons remain intact unless changed. The form warns when the path is not an
existing directory; such bookmarks are still saved and shown as unavailable.

Paths entered in the form resolve relative to the current browsing directory;
`~` uses HOME. New or changed paths are stored as absolute paths. An unchanged
configured relative path retains its original spelling.

Saves preserve unrelated TOML settings, comments, existing file permissions and
valid config symlinks. If another process has changed the bookmark list since
it was loaded, saving refuses to overwrite it: cancel and use Alt+r to reload.
Save errors leave the form open and preserve the current bookmarks. Saving an
unchanged existing bookmark leaves the file alone. With configuration
disabled, edits are session-only. No directory or target file is modified.

For navigation and palette bindings, see the [key reference](../README.md#keys).

## Discovery cache and performance

```toml
[cache]
enabled = true          # reuse visited views and already-discovered descendants
max_entries = 16        # visited snapshots retained (1..128)
max_candidates = 4096   # paths retained per visited snapshot (1..100000)
ttl_seconds = 60        # stale age and recursive-view refresh interval (1..86400)
fresh_seconds = 3       # recent complete views skip rescans; 0 disables this (0..60)
prefetch = true         # pre-read a stable highlighted child at depth 1

[performance]
refresh_ms = 50         # minimum time between re-sorts during scans (0..1000)
```

### Opening a directory

1. **Recent complete snapshot.** An exhaustive nonempty listing verified within
   `fresh_seconds` appears immediately without another scan. The age is measured
   from verification; revisiting, changing the query, and checkpointing do not
   renew it. Empty directories are still verified before Enter can confirm them.
2. **Older or partial snapshot.** It appears immediately and receives its own
   frame before background discovery reconciles it. Snapshots truncated by the
   path limit never qualify for skipping a scan. If there is no snapshot, eligible
   already-known paths can seed the new view; depths and roots are recomputed.
   Such reused prefixes always remain provisional. Seeding examines at most
   16,384 old rows/index entries and copies at most `max_candidates` paths.
3. **Uncached first visit.** The first frame gives the worker up to 4 ms to finish
   an ordinary small listing. Filesystem calls stay off the UI thread; a slow or
   large view streams normally after that bounded wait. Input and discovery share
   coalesced wake notifications, so ready results no longer wait for a 30 ms
   keyboard-poll timeout.
4. **Quiet updates.** While staying in a view, depth-1 listings refresh every
   `fresh_seconds` (3 seconds by default). Recursive views refresh every
   `ttl_seconds` (60 seconds by default) to avoid repeatedly walking a large tree.
   With `fresh_seconds = 0`, both use `ttl_seconds`. The previous rows stay visible
   during refresh; completion reconciles additions, deletions and kind changes.
   Scheduling is serviced by the UI's idle timer, so refresh may start up to about
   250 ms after its deadline. Slow scans extend the interval between completions.

Discovery is breadth first, within the requested depth. All rendered results
retain the exact depth/kind/score/root/path ordering and path-based highlight.
A scan never changes the last rendered numbered targets; only a completed frame
publishes new ones. Query edits filter known paths without starting a scan.

A cached **view** is the combination of current directory, ordered scopes, depth,
hidden visibility, exclusions and file glob. Query, highlight, scroll position
and marks are not restored. Nonexhaustive or interrupted snapshots cannot prove a
view complete. Missing marked paths block confirmation; cached targets are checked
for existence and file/directory kind when activated. F5 bypasses all replay and
rescans immediately, preserving query, marks and settings.

### `[cache]` settings

| Setting | Default | Range | Effect |
| --- | --- | --- | --- |
| `enabled` | `true` | boolean | Enables visited snapshots, reuse of known descendants, and speculative prefetch. False disables these; scans still wake the UI immediately and the current view can refresh quietly. |
| `max_entries` | `16` | 1–128 | Maximum visited views retained in a session LRU. Evicts the least recently used view. |
| `max_candidates` | `4096` | 1–100000 | Maximum paths per visited snapshot. Best-ranked rows are retained first, then other known paths. Larger listings replay partially and always scan to recover the rest. |
| `ttl_seconds` | `60` | 1–86400 | Age that marks replayed data stale, and the refresh interval for recursive views (also depth 1 when `fresh_seconds = 0`). Stale data remains available for provisional replay until LRU eviction. |
| `fresh_seconds` | `3` | 0–60 | Maximum verification age for skipping a revisit scan; also the active depth-1 refresh interval. Freshness never exceeds `ttl_seconds`. Zero restores immediate revalidation on every revisit. |
| `prefetch` | `true` | boolean | Enables the bounded depth-1 child prefetch described below. False disables speculative filesystem reads while retaining normal caching and reuse. |

**Memory.** Visited snapshots retain at most `max_entries × max_candidates` paths.
A rough estimate of 100–150 bytes per path gives about 8 MiB at defaults; long
paths can cost considerably more. This is a count limit, not an exact byte cap.
The current view's index, result rows and BFS queue are separate and scale with
the tree being browsed; lowering depth reduces that working set. Prefetch has its
own small allocation budgets and cannot evict visited snapshots.

### Bounded child prefetch

At depth 1, a directory highlighted in main results for at least **500 ms** can
have its immediate children read before you enter it. There must be no foreground
scan pending. Only a direct child is eligible; file, marks, bookmark, palette and
help highlights do not start scans. A recent exhaustive visited snapshot also
suppresses redundant prefetch. At greater depths, known descendants are reused
on navigation instead of speculatively reading them again.

- One speculative worker at a time; at most **2,048 entries inspected**, plus a
  lookahead entry to detect truncation, with a cooperative **100 ms** budget.
- At most **1 MiB** of candidate-vector/path allocations per job and **3 MiB**
  retained across up to four speculative snapshots. Allocator metadata, thread
  stack, policy/key objects, and transient conversion overhead are additional;
  these budgets do not promise a particular process RSS.
- A bounded 32-target history suppresses another attempt for the same target and
  settings for **60 seconds**. Speculative entries have a separate eviction queue
  and never displace visited snapshots unless the directory is actually visited.
- Highlight/query changes, leaving results focus, foreground work and navigation
  cancel an outstanding job. No successor starts until that worker has exited,
  even if its filesystem call is slow. Results from cancelled jobs are discarded.
- On Linux, mount metadata admits only known local filesystem types and rejects
  separate mounts (including bind mounts), remote/unknown filesystems and ambiguous
  escaped mount paths. Symlink targets are skipped. Other platforms currently
  disable speculative reads while retaining normal discovery and caching.
- A prefetched listing can skip the foreground scan only if it is exhaustive,
  recent, nonempty, and covers the whole new depth-1 scope. With pinned scopes,
  truncation, symlink entries or a greater requested depth, it is only a prefix.

Budgets are checked between filesystem calls. A blocking call cannot be interrupted
at 100 ms; it never blocks input or creates a growing chain of speculative threads.
No user file contents are read, and no cache/index files are written. Mount
classification reads `/proc/self/mountinfo`.

### `[performance]` settings

| Setting | Default | Range | Effect |
| --- | --- | --- | --- |
| `refresh_ms` | `50` | 0–1000 | Minimum interval between result re-sorts during a scan. First results, completion and query changes update immediately. Zero re-sorts after every drained batch; larger intervals reduce repeated ranking on large trees. |

Workers batch up to 256 paths or 10 ms of discoveries; the UI drains for up to
8 ms per iteration. ASCII scoring avoids allocation, narrower queries re-score
current matches, unchanged refreshes skip work, and completed scans release their
reconciliation set. Non-ASCII paths and exclusions retain the general code paths.
The UI redraws after visible changes and at least once a second, with a single
bounded terminal-input reader waking it immediately alongside discovery.

Navigation performance can be measured with the manual PTY benchmark; see
[development verification](DEVELOPMENT.md#verification). Measurements depend on
storage, tree size, build mode and terminal scheduling.

## Modal text editing

```toml
[editor]
backend = "basic" # Default: "nvim"; CLI override: --editor basic|nvim
```

By default a clean headless Neovim instance edits every field, initially in
Normal mode; press `i` or `a` to type. When `backend`
is left unset and `nvim` is missing or fails to start, fff silently uses
basic append/Backspace editing; an explicit `"nvim"` reports the fallback. See the
[editing guide](EDITING.md) for Insert/Normal/Visual modes, Escape semantics,
reserved fff shortcuts, dependency/fallback behavior, and tested boundaries.

## Normal-mode keys

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
last = "G"           # Highlight the last row
mark = "<space>"     # Mark/unmark like Tab (results and marks only)
quit = "q"           # Exit and return the launch directory
quit_current = "!"   # Exit and return the current browsing directory
```

Quit actions work in Normal-mode fields and in the marked list, ignoring marks;
`--no-dir` exits without returning a directory. Basic text fields keep literal
`q` and `!`. Disable `quit` before reusing `q` as a prefix for another action.

Marking is active only in results and the marked list. Buffer editors use
Neovim writes and closes; the legacy `apply` setting is accepted but ignored.

Each value is one key sequence or a list of sequences; `"none"`, `""`, or `[]`
disables the action. A sequence is typed literally, one printable key per
character (uppercase means Shift); write `<space>` for Space and `<lt>` for `<`.
Ctrl/Alt chords, Escape, Enter, and other named keys are rejected: Alt and Ctrl
chords remain fff shortcuts (remap those with `--map-key`).
Sequences must be unambiguous: two actions cannot share a sequence, and one
sequence cannot be a prefix of another, because a waiting prefix has no timeout.
Unbound keys and unfinished prefixes still reach Neovim. `--check-config` reports
unknown actions and invalid or conflicting sequences. Changes take effect on the
next invocation.

To use Space as a sequence prefix instead of marking, disable its default first:

```toml
[keys.normal]
mark = "none"
last = ["G", "<space>e"]
```
