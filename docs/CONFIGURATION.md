# Configuration, palettes, bookmarks, and caching

Fuzzy file finder reads an optional TOML file at startup. You can run it without a config;
the default palette is `midnight`, bookmarks start empty, and caching is enabled.

## Start here

Save this as `$XDG_CONFIG_HOME/fff/config.toml`. Following the XDG base
directory spec, `~/.config/fff/config.toml` is used when `XDG_CONFIG_HOME` is
unset, empty, or not absolute. `fff setup --check-config` prints the file it read:

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
cargo run -- setup --config docs/config.example.toml --check-config
cargo run -- --config docs/config.example.toml --palette ember
```

`fff setup --check-config` validates and exits without opening the TUI. Its status message
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
| `fff setup --check-config` | Validate the selected configuration and exit. |

The config directory is always `fff`, independently of the crate name
`fuzzy-file-finder` or the executable name.

A missing default file uses defaults. An explicitly requested missing file,
invalid TOML, unknown field, unknown palette, or invalid setting produces a clear
startup error naming the file. Explicit bookmark saves/deletions update the active
config; a missing default file is created on the first save. Startup alone never
writes it. With `--no-config`, bookmark edits last only for the session.
Palette edits reload live; external bookmark edits reload with r in the
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

## Result details

Each result row ends with details of the entry, right-aligned so they line up:

```toml
[ui]
columns = ["size", "modified"]   # any of "permissions", "size", "modified"; [] for none
```

- `permissions`: like `ls -l`, `drwxr-xr-x`.
- `size`: a file's size (`812B`, `4.2K`, `13M`); a directory's number of
  entries (`12`, `9999+` past ten thousand, `?` when unreadable).
- `modified`: time since the last change: `now`, `5m`, `3h`, `2d`, `3w`,
  `4mo`, `2y`.

Names come first: in a narrow pane, columns are dropped from the left until a
name has room. The footer shows the highlighted entry's path followed by a
link's target, its size (exact bytes) or entry count, modification date,
permissions and `user:group`, dropping the last ones first when space runs out.
Symbolic links show `→ target` after their name, in the `link` color; broken
links use the `error` color.

Only rows on screen are inspected, so discovery does no extra work:
one `lstat` per visible row, read again after each scan. Directory entry counts
come from a background thread, so a huge or slow directory never delays a frame;
the column stays blank until its count arrives.

## Mouse

With `[ui] mouse = true` (the default), the mouse works in the results, the
marked list and the choosers like in a graphical file manager. In the results:

- A click highlights a row and clears the marks.
- A double click acts like `l`: it enters a directory or opens a file in the
  background. In [`fff explorer`](DESKTOP.md#fff-explorer) it is Enter on the
  row instead: it enters a folder or chooses a file.
- Dragging with the button held clears the marks, then marks every row from
  where it was pressed to where it is; the highlight follows the drag. Held
  past the top or bottom edge of the results, the drag scrolls them a row at
  a time, marking as it goes, until it comes back or the button is released.
- The wheel scrolls the results and does nothing else; the highlight may leave
  the view, and the next key that moves it brings the view back.
- A click on a tab's number in the results' top-right corner switches to that
  tab, like Alt+↑/↓; the marks are kept, since tabs share them.

In the marked list:

- A click on a mark focuses the marked list there (like Alt+Space, then
  moving to it); a double click acts like `l` on it: a folder is entered and
  a file opens in the background (in `fff explorer`, Enter on it).
- The wheel over the marked list moves through the marks, focusing it.
- A click in the results returns to them, highlighting the row clicked.

In the choosers (bookmarks, bookmark icons, palettes, and the Alt+g
suggestions), a click selects a row, a double click is Enter on it (opening
the bookmark, choosing the icon, applying the palette, going to the folder),
and the wheel moves the selection. Clicks outside an open chooser do nothing.

Capturing the mouse takes it from the terminal's own text selection; most
terminals still select with Shift held. Set `mouse = false` to leave the mouse
to the terminal.

### Buttons

For mouse-only use, fff can show a bar of buttons above the footer. It is
opt-in:

```toml
[ui]
buttons = "mouse"   # "off" (default), "mouse" (once the mouse is used) or "always"
```

With `"mouse"`, keyboard-only sessions look exactly as without buttons; the bar
appears after the first click, drag or wheel step. Each button does what its
key does:

| Button | Key |
| --- | --- |
| ← Back | Ctrl+o: the tab's previous directory |
| ↑ Up | Alt+h: the parent directory |
| Open | `l`: enter the directory or open the file in the background |
| Mark / Unmark | Shift+Space, without moving: mark the highlighted row |
| Show / Hide hidden | Alt+.: hidden entries |
| Cancel | Ctrl+c: quit without output |
| Select & Exit | Ctrl+w with marks where they are returned (`fff pick`, `fff explorer --multiple`), else Normal-mode Enter on the highlighted row |

Buttons that cannot act (Back with no history, Up and Back in `fff seek`)
are dimmed. In the copy, move, link and permanent-deletion confirmations the bar shows
Cancel and Confirm (Esc and Enter). A narrow bar leaves out buttons from the
end of the left group. `buttons` other than `"off"` needs `mouse = true`.

### Dropping files

Terminals with drag and drop (kitty, foot, alacritty, WezTerm, Konsole, GNOME
Terminal, and st with the [drag-n-drop patch](https://st.suckless.org/patches/drag-n-drop/))
paste the paths of files dropped onto them. When such a paste arrives while
browsing (the query in Normal mode) and it consists only of existing absolute
paths (shell-quoted or backslash-escaped, or `file://` URIs), fff offers to copy
them into the current directory, with the usual confirmation and conflict
checks; the marks are left as they were. While typing, a paste is only text
for the query. Drops are ignored in `fff seek`.

Vanilla st has no drag and drop. With the drag-n-drop patch, st pastes dropped
paths backslash-escaped and separated by spaces through its usual bracketed
paste, which fff understands. Besides `config.def.h`, add the patch's
`xdndescchar` setting to your `config.h` (st builds from it), then rebuild and
reinstall st. The patch also puts the dropped paths in the PRIMARY selection.

## Live updates

With `[ui] watch = true` (the default), fff watches with inotify the current
directory, the pinned roots, and every listed folder whose entries are listed
too (those shallower than the depth), so deeper results follow changes as
well. Entries created, removed or renamed in any of them trigger a quiet
rescan after a short pause (bursts such as an extraction are coalesced, and a
change during a rescan waits for it to finish), keeping the rows and highlight
while it reconciles; entries written to update their details in place, and a
folder's entry count follows what is added to or removed from it. Folders that
appear are watched once the rescan lists them.

At most 4096 folders are watched, the roots first and then the shallowest
listed folders, leaving the system's inotify watches (often 8192 per user) to
other programs too; folders beyond that, or past the system's limit, follow the
[freshness](#discovery-cache-and-performance) rules and F5. Where inotify is
unavailable or a directory cannot be watched, fff works as with
`watch = false`.

## Preview pane

The preview pane, shown when fff starts, holds the highlighted entry, or the
highlighted mark in the marked list; Alt+v (`P` in Normal mode) hides or shows
it. It is text only: no images or terminal graphics protocols, so it looks the
same in st and any other terminal.

```toml
[ui]
preview = true   # false: start with the preview pane hidden
```

It takes the bottom half of the panes. In wide terminals it spans the full
width under the results and the marks, which keep their side-by-side split
and give up half their height: the marked list is empty most of the time, and
a full-width pane leaves long lines of code and PDF text whole. In compact
terminals (under 78 columns) it sits between the results and the marks; when
the marked list is focused there, it gives that list its room. Below 14 rows
of panes it is left out, since half of them would leave too few results.

What it shows depends on what the entry is, from its first bytes rather than
its name:

| Entry | Pane |
| --- | --- |
| Text (any name; no NUL bytes, mostly valid UTF-8) | The first 300 lines of the first 64 KiB; tabs expanded to every 4th column, control characters shown as their Unicode pictures (`␛`), long lines cut at the pane's edge. The title counts the lines when the whole file was read. |
| Directory | Its entries, folders first, then by name; dot entries only while hidden entries are shown. |
| PDF | Title, author, subject, pages, page size, creator, producer and encryption from `pdfinfo`, then the text of the first three pages from `pdftotext`, page breaks marked. |
| PNG, JPEG, GIF, WebP, BMP | Width × height (and, for PNG, color type and depth), read from the file's header. TIFF, AVIF, HEIF and icons ask `ffprobe`. |
| Audio and video (MP3, FLAC, Ogg, WAV, AIFF, MP4, MOV, Matroska, WebM, AVI, ...) | Duration, title, artist, album and date tags, each video stream's codec, size and frame rate, each audio stream's codec, sample rate and channels, subtitle tracks, cover art size and bit rate, from `ffprobe`. |
| Archives (zip, tar, gzip, xz, zstd, bzip2, 7-Zip, RAR, ar/deb, cpio, ISO 9660) and zip-based documents (EPUB, docx, odt, ...) | The first 300 entries through libarchive, without extracting anything, and the unpacked size when they were all listed. A compressed file that is not a tar only says what it is. |
| ELF binaries | Executable, shared library, object or core dump; 32 or 64 bit; machine. |
| SQLite databases | The page count. |
| Anything else | What it is when the first bytes say (fonts, Windows executables, WebAssembly, Parquet, ...), else `binary data`. Pipes, sockets and devices are named and never opened. |

Size, dates, permissions, owner and a link's target are not repeated: the
footer right below shows them for the same entry.

The helper programs are optional. Without poppler (`pdfinfo`, `pdftotext`) or
FFmpeg's `ffprobe`, the pane names the file's type and says which package
would show more. They run without a shell, with no terminal, on the file's own
path (`ffprobe` reads it as `file:PATH`, so a name cannot select another
protocol), and only the first 64 KiB of their output is kept.

The pane never slows the list. Reading starts once the highlight has rested
for 100 ms, on a thread of its own, bounded in bytes and lines; a helper
program gets 3 seconds. Moving on cancels it and kills the helper, and a
reply for an entry no longer highlighted is dropped. Until the preview
arrives the pane stays empty rather than showing the previous entry. The last
32 previews are kept, so going back to an entry shows it at once, until its
size or modification time changes.

## Status messages

Notices such as "Navigation is disabled in fff seek" or a failed palette reload
appear in red on the bottom line, or inside an open prompt. They clear on their
own after `message_seconds` (default 4). Triggering the same notice again restarts
the timer; `0` keeps each message until another replaces it.

```toml
[ui]
message_seconds = 4   # 0..3600; 0 = never hide automatically
```

## Trash

`x` moves entries to the trash, from where `u` brings them back; see
[trash and deletion](OPERATIONS.md#trash-and-deletion). The trash lives in
`$XDG_DATA_HOME/fff/trash` (`~/.local/share/fff/trash` when unset), with one
`.fff-trash-UID` folder at the top of each other filesystem used.

```toml
[trash]
enabled = true        # false: x deletes permanently, after confirmation, like X
days = 30             # entries older than this are deleted when fff starts; 1..36500
ask_above = "10 GiB"  # larger entries ask before going to the trash
```

`ask_above` is a number of bytes or a size with a unit (`B`, `KiB`, `MiB`,
`GiB`, `TiB`; `K`, `M`, `G`, `T` and `KB`… mean the same binary units), such as
`"500 MiB"` or `"1.5G"`. An entry counts its whole tree, like the deletion
window's sizes. The purge runs on a thread of its own at every start, so a
large trash never delays the first frame; with `enabled = false` nothing is
purged either.

## Remembered locations

Each directory remembers the entry last highlighted in it: going up highlights
the folder you came from (even several levels up), and entering a folder again
puts the cursor back where it was, as soon as that row is listed (moving first
cancels this). Every session starts in the current directory (or ROOTS), on
the row last highlighted there. Otherwise a newly opened location keeps the
highlight on the first row while its scan streams in, until you move it.

`fff --resume` continues the last `fff roam` session instead: its folder,
highlighted row, tabs with their back/forward history, marks, query, depth,
filters, exclusions and toggles (hidden entries, directories, preview pane).
Only the last session is kept, whichever directory it was started from; the
status line says when a session resumed, and `q` still exits to where the new
one started. `--resume` takes no ROOTS and none of the search options, since
the session brings its own. `fff pick`, `fff seek` and `fff explorer`
sessions are never saved.

Highlights are kept in `$XDG_STATE_HOME/fff/state` (`~/.local/state/fff/state`
when unset): up to 500 directories' highlights, least recently used dropped
first, as exact path bytes. The last session is in `fff/session` beside it.
Both files are readable only by you and replaced atomically when a session
ends; a missing or damaged file is ignored. Delete them to forget everything. `[ui] resume`, which
resumed automatically in 0.8, is still accepted and does nothing.

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
fields initially start in Normal mode). `j`/`k` in Normal mode or Alt+j/Alt+k
move the highlight; PageUp/PageDown moves farther. Alt+1 through Alt+9 open
the corresponding visible numbered bookmark, and Alt+0 opens the last visible
bookmark. Out-of-range digits clamp to the last visible entry, including after
filtering or scrolling. These shortcuts work in every editing mode.
Enter opens a directory;
Escape or Alt+b returns to results. An unavailable directory or a bookmark that
points to a file is shown as unavailable; trying it leaves the chooser open with
an error. Bookmarks never select a file or finish the program.

Opening a bookmark preserves marks, discovery depth, policies, and explicitly
supplied fixed roots. It changes the single current directory and clears the
filename query. The highlight then stays on the first row as the scan streams in
and as you type a search there, until you move it yourself. Bookmarks are navigation destinations, not additional roots or
preselected output paths.

### Editing bookmarks

In the chooser, **n** adds the current directory, **e** edits the
highlighted bookmark, and **d** asks for confirmation before deleting it.
Deletion removes only the bookmark record. **r** reloads bookmarks from disk.
These letters act while the filter is in Normal mode, so Vim's own `n`, `e`,
`d` and `r` are unavailable there; while typing a filter (Insert mode) use **Alt+n**, **Alt+e**, **Alt+d** and **Alt+r**.
**Up** / **Down** move the highlighted bookmark one place and save the
new order immediately; while filtering, it moves past its visible neighbour.
Comments directly above a `[[bookmarks]]` table move with it.

The form opens at the bottom like the file-operation dialogs and edits name,
path and icon. **Enter** saves and **Escape** cancels, from any editing mode:
in Neovim, Escape first leaves Insert/Visual mode, and Enter during command-line
or pending input still goes to Neovim. Neovim's `ZZ` / `:wq` also save and
`ZQ` / `:q!` cancel. Tab/Shift+Tab or Up/Down switch fields. **Space** on the
icon row opens a searchable selection of 37 choices,
including common Nerd Font glyphs and **Automatic** to clear a custom icon.
Press `i` to filter in Neovim, then Escape and Enter to choose. Existing custom
icons remain intact unless changed. The form warns when the path is not an
existing directory; such bookmarks are still saved and shown as unavailable.

Paths entered in the form resolve relative to the current browsing directory;
`~` uses HOME. New or changed paths are stored as absolute paths. An unchanged
configured relative path retains its original spelling.

Saves preserve unrelated TOML settings, comments, existing file permissions and
valid config symlinks. If another process has changed the bookmark list since
it was loaded, saving refuses to overwrite it: cancel and use r to reload.
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
cached_queries = 5      # shorter queries kept for deleting letters (0..64)
max_paths = 1000000     # paths the current view may hold (1000..100000000)
```

### Opening a directory

1. **Recent complete snapshot.** An exhaustive nonempty listing verified within
   `fresh_seconds` appears immediately without another scan. The age is measured
   from verification; revisiting, changing the query, and checkpointing do not
   renew it. Empty directories are still verified before Enter can confirm them.
2. **Older or partial snapshot.** Its rows are loaded at once while background
   discovery reconciles them: a scan that finishes within the first-frame wait
   (below) is drawn once, complete; a slower one shows the snapshot first, with
   `◌ cached, refreshing`. Snapshots truncated by the path limit never qualify
   for skipping a scan. If there is no snapshot, eligible
   already-known paths can seed the new view; depths and roots are recomputed.
   Such reused prefixes always remain provisional. Seeding examines at most
   16,384 old rows/index entries and copies at most `max_candidates` paths.
3. **Uncached first visit.** The first frame after a scan starts waits up to
   12 ms for it to finish, still reading keys meanwhile, so an ordinary small
   listing is drawn once, complete, rather than first empty and then again.
   Filesystem calls stay off the UI thread; a slow or large view streams
   normally after that bounded wait. Input and discovery share
   coalesced wake notifications, so ready results are drawn at once.
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
hidden visibility and exclusions. Query, file filters (`--glob`), highlight,
scroll position and marks are not part of it: filters only choose among
discovered files, so changing them reuses the snapshot. Query, highlight, scroll
position and marks are not restored. Nonexhaustive or interrupted snapshots cannot prove a
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
the tree being browsed, up to `[performance] max_paths` (about 450 bytes per
path, more in `fff pick` and `fff seek`, which keep labels: roughly 450–550 MB
at the default); lowering depth reduces that working set. Prefetch has its
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
| `refresh_ms` | `50` | 0–1000 | Minimum interval between merging new paths into the results during a scan. First results, completion and query changes update immediately, as does a backlog of 8,192 unmerged paths. Zero merges after every drained batch. |
| `cached_queries` | `5` | 0–64 | Results of this many shorter queries are kept while you type, so deleting letters restores them instead of ranking every path again; an edited query narrows from the longest kept query it still starts with. Each kept query costs about 8 bytes per matching row (rows share their paths). Zero keeps none. Discovery or a policy change discards them. |
| `max_paths` | `1000000` | 1000–100000000 | Most paths the current view holds. A scan that finds more stops there: the status line shows `◌ partial`, the listing is neither reconciled nor kept as complete, and a lower depth shows the rest. It bounds the memory browsing a huge tree can take. |

Workers batch up to 256 paths or 10 ms of discoveries, breadth first, so the
results fill in shallowest first. The UI reads waiting input before any
discovery work, draws the frame answering it before resuming, and stops taking
or merging discoveries as soon as a key arrives. Otherwise it takes
discoveries for up to 8 ms and merges them into the ranked results, oldest
first, for about 4 ms per iteration: new paths are ranked among themselves and
inserted, never re-sorting every known path. Once 8,192 paths wait to be
merged, the UI stops taking more and the worker waits, so a scan of a huge
tree takes longer instead of delaying keys; it completes only after its last
paths are merged. The index of known paths is sharded so it grows without
rehashing everything at once, a scan marks the paths it delivers instead of
keeping a separate reconciliation set, and large listings are freed on another
thread. `fff pick` and `fff seek` relabel only basenames that changed, with
the time left over, so a scan completes once its labels catch up.

Ranking every known path again (changing depth, hidden files, exclusions,
filters or directories-hidden, or a query that no kept result narrows) and
narrowing large results happen on another thread, in parallel pieces, once
16,384 or more paths are involved. The previous results stay on screen
meanwhile, with `◌ sorting` on the status line; a newer change cancels an
older ranking. Typing into the query never waits for one, but any other key
does, so moving, marking or choosing always acts on current results. ASCII
scoring avoids allocation, narrower queries re-score current matches, shorter
ones restore kept results (`cached_queries`), and unchanged refreshes skip
work. Non-ASCII paths and exclusions retain the general code paths. The UI
redraws after visible changes and at least once a second, with a single
bounded terminal-input reader waking it immediately alongside discovery.

Navigation performance can be measured with the manual PTY benchmark; see
[development verification](DEVELOPMENT.md#verification). Measurements depend on
storage, tree size, build mode and terminal scheduling.

## Openers

Which program opens a file, by extension or name, for `l` / Alt+l / Alt+number
(in the background) and `o` (in the terminal); see
[opening files](OPERATIONS.md#opening-files).

```toml
[open]
terminal = "st -e"   # how to run a terminal program in a new window (default)
fff_terminal = "st -c fff-picker -e"   # fff's own window from fff setup (default)

[[open.rules]]
command = "zathura"
extensions = ["pdf", "djvu", "epub"]

[[open.rules]]
command = "imv"
extensions = ["png", "jpg", "jpeg", "gif", "webp"]

[[open.rules]]
command = "mpv --fs"
extensions = ["mkv", "mp4", "webm"]

[[open.rules]]
command = "nvim"
patterns = ["Makefile", "*.tar.gz"]   # globs on the file name
terminal = true                       # needs a terminal: a new window in the background
```

Rules are tried in order and the first match wins. `extensions` end the file
name after its first character, ignoring case and a leading dot, so `"tar.gz"`
matches `notes.tar.gz` (as `"gz"` does) but not a file named `.tar.gz`;
`patterns` are globs matched against the file name. `command` is a program and its arguments, split like a typed command
(quotes and backslashes work, no shell); the files are added after it. A rule
needs a command and at least one extension or pattern. Files no rule matches
open with `$EDITOR`, as a terminal program.

`terminal` must start a terminal window running the rest of the line: `st -e`,
`alacritty -e`, `foot`, `kitty`. `fff_terminal` does the same for fff itself
when the desktop entry or the file chooser installed by `fff setup` starts
it; its default gives the window the class `fff-picker`, so a window manager
rule can float it (see [desktop integration](DESKTOP.md#floating-windows)).
`fff setup --check-config` reports the opener and terminal programs the
configuration names that are not installed (the built-in `st` defaults are
not checked).

## Modal text editing

A clean headless Neovim instance edits every field, initially in Normal mode
(prompts made for typing start in Insert); press `i` or `a` to type. Neovim is
required: fff exits with an error when `nvim` is missing or cannot start.
`[editor] backend = "nvim"` remains accepted and is the only choice. See the
[editing guide](EDITING.md) for Insert/Normal/Visual modes, Escape semantics and
reserved fff shortcuts.

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
mark = "<space>"     # Mark/unmark like Shift+Space (results and marks only)
mark_all = "m"       # Mark every listed row (results and marks only)
invert_marks = "M"   # Toggle every listed row's mark (results and marks only)
quit = "q"           # Exit choosing nothing (no output, no directory change)
quit_current = "!"   # Exit to the current browsing directory
run = "gx"           # Run a program on the current/marked paths, come back
run_exit = "gX"      # Run a program on the current/marked paths in place of fff
move = "v"           # Move marked paths here (with a review)
copy = "p"           # Copy marked paths here (with a review)
link = "s"           # Symlink marked paths here (with a review)
delete = "x"         # Move current/marked paths to the trash
delete_permanently = "X" # Delete current/marked paths permanently (with a review)
rename = "r"         # Batch rename the results in a buffer
extract = "e"        # Extract current/marked archives
edit = "E"           # Open current/marked paths in $EDITOR
new = "n"            # Create a file or directory
open = "o"           # Open current/marked files with their openers, here
undo = "u"           # Undo the last file operation
shallower = "<left>" # Search one level shallower, like Alt+Left (results)
deeper = "<right>"   # Search one level deeper, like Alt+Right (results)
bookmarks = "b"      # Bookmark chooser, like Alt+b (results)
hidden = "."         # Show/hide hidden entries, like Alt+. (results)
directories = "d"    # Hide/show directories, like Alt+d (results)
files = "D"          # Hide/show files, like Alt+D (results)
preview = "P"        # Show/hide the preview pane, like Alt+v (results)
go_to = "/"          # Go-to prompt, like Alt+g (results)
next_tab = "<up>"     # Next tab, like Alt+Up (results)
prev_tab = "<down>"   # Previous tab, like Alt+Down (results)
depth_digits = true  # 1-9 set the search depth (results)
```

`shallower`, `deeper`, `bookmarks`, `hidden`, `directories`, `files`, `preview`, `go_to`, the tab
actions and the depth digits work only in Normal-mode results; the Alt chords keep working in every mode. With
`depth_digits = true` (the default), no sequence may start with 1–9; set it to
`false` to give digits back to Neovim as counts or to use them in sequences.

File operations (`move` … `undo`) work in Normal-mode results and in the
marked list. `undo` takes `u` from Neovim's undo in the query (`:u` still
undoes query edits); set `undo = "U"` or `"none"` to give `u` back. To give one of their letters to another action, disable or rebind the
operation first; for example `mark = "x"` needs `delete = "none"` or another
sequence such as `delete = "D"`, because two actions cannot share a sequence.

`run` and `run_exit` work in results and the marked list; see
[running programs](OPERATIONS.md#running-programs). Quit actions work in
Normal-mode fields and in the marked list, ignoring marks;
`fff seek` exits without returning a directory. Insert mode keeps literal
`q` and `!`. Disable `quit` before reusing `q` as a prefix for another action.

Marking is active only in results and the marked list. Buffer editors use
Neovim writes and closes; the legacy `apply` setting is accepted but ignored.

Each value is one key sequence or a list of sequences; `"none"`, `""`, or `[]`
disables the action. A sequence is typed literally, one printable key per
character (uppercase means Shift); write `<space>` for Space, `<lt>` for `<`,
and `<left>`, `<right>`, `<up>`, `<down>` for the arrow keys. Ctrl/Alt chords,
Escape, Enter, and other named keys are rejected: Alt and Ctrl chords remain fff
shortcuts (remap those in [`[keys.map]`](#chord-remapping)).
Sequences must be unambiguous: two actions cannot share a sequence, and one
sequence cannot be a prefix of another, because a waiting prefix has no timeout.
Only sequences you set are checked against each other: one that takes a
default action's key (say `delete = "D"`) leaves that action without it.
Unbound keys and unfinished prefixes still reach Neovim. `fff setup --check-config` reports
unknown actions and invalid or conflicting sequences. Changes take effect on the
next invocation.

To use Space as a sequence prefix instead of marking, disable its default first:

```toml
[keys.normal]
mark = "none"
last = ["G", "<space>e"]
```

## Chord remapping

Every other binding (Alt and Ctrl chords, Shift+Space, arrows, Enter, Escape, F1, F5,
PageUp/PageDown) is remapped in `[keys.map]`, the configuration form of
`--map-key`. Each entry maps a source chord to the default chord whose action
it should perform, or to `"none"` to disable the source:

```toml
[keys.map]
"ctrl+n" = "down"     # Emacs-style movement
"ctrl+p" = "up"
"ctrl+g" = "alt+g"    # go-to prompt on Ctrl+g ...
"alt+g" = "none"      # ... and no longer on Alt+g
"ctrl+t" = "alt+p"    # palette chooser
```

Chords are written as `ctrl+`, `alt+` and `shift+` prefixes followed by one
character or a key name: `f1`, `f5`, `enter`, `esc`, `tab`, `backspace`, `up`,
`down`, `left`, `right`, `pageup`, `pagedown` or `space`. Targets name the
default chords in the [key reference](../README.md#keys) and the F1 guide.
Each key is translated once, so chords can be swapped (`"alt+h" = "alt+l"` with
`"alt+l" = "alt+h"`). Plain printable characters cannot be sources, because
they are typed into fields; rebind Normal-mode letters in `[keys.normal]`.
`--map-key` rules apply after `[keys.map]` and win for the same source.
Translation applies everywhere, including choosers and prompts. Letters inside
confirmation sheets (such as `c`/`s`/`a` in a scope choice) are fixed.
`fff setup --check-config` reports invalid chords.
