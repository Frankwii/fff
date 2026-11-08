# Configuration, palettes, workspaces, and caching

Fuzzy file finder reads an optional TOML file at startup. You can run it without a config;
the default palette is `midnight`, workspaces start empty, and caching is enabled.

## Start here

`fff setup` writes a first configuration for you (see [guided
start](#guided-start-fff-setup)); a session started without a configuration
file says so. To write one by hand, save this as `$XDG_CONFIG_HOME/fff/config.toml`. Following the XDG base
directory spec, `~/.config/fff/config.toml` is used when `XDG_CONFIG_HOME` is
unset, empty, or not absolute. `fff setup --check-config` prints the file it read:

```toml
[ui]
palette = "forest"

[[workspaces]]
name = "Home"
path = "~"

[[workspaces]]
name = "Notes"
path = "~/notes"
```

Run `fff` and press **gw** in Normal mode to open workspaces. Press **i** to filter names,
then **Enter** to open one. From Normal mode, **Escape** returns to results.

The [commented example](../config.example.toml) also shows a custom palette and all
cache settings. From the repository, you can try it without installing a config:

```sh
cargo run -- setup --config docs/config.example.toml --check-config
cargo run -- --config docs/config.example.toml --palette ember
```

`fff setup --check-config` validates and exits without opening the TUI. Its report
(what was checked, and how it went) uses stderr; stdout remains empty. It checks syntax, names, colors, and limits,
while unavailable workspace directories are handled when you try to visit them.
It also warns, without failing, about the [desktop integration](DESKTOP.md)
when it was set up: files `fff setup --desktop` or `--browser` wrote that
are missing, out of date or run another fff binary, folders that open in
another application than fff, and an environment in which Qt applications
would not ask the portal for file dialogs. Its **Programs** section is the
one the [guided start](#guided-start-fff-setup) begins with.

## Guided start: `fff setup`

`fff setup` without an option asks a few questions on the terminal and adds
the answers to the configuration file (the default one, or `--config PATH`,
which it creates when missing). Questions and report go to stderr; stdout
stays empty. Without a terminal to ask on (stdin or stderr redirected) it is
a usage error (exit 2). In order:

1. **Programs.** Which of the programs fff runs are installed: Neovim
   (required), poppler (`pdftotext`, `pdfinfo`: PDF previews and contents
   search), DjVuLibre (`djvutxt`), FFmpeg (`ffprobe`) and zoxide (the
   location prompt's suggestions), each missing one with the command that
   installs it with the package manager found (pacman, apt, dnf; Homebrew on
   macOS); then the terminal [terminal programs](#openers) open in, and
   whether `$EDITOR` is set.
2. **Icons.** It draws a few of fff's [icons](#filetype-icons) and asks
   whether they show. Answered no, it sets `[ui] icons = false` and, under
   **Next**, says how to get them: a Nerd Font is found installed by the
   names of the font files (then the terminal's font needs changing), or how
   to install one (`ttf-nerd-fonts-symbols` with pacman, the cask
   `font-symbols-only-nerd-font` with Homebrew, else nerdfonts.com).
3. **Opening files.** For documents (PDF, DjVu, EPUB), images, audio and
   video, and office files, the installed programs that open them, the
   first one the default: on macOS `open` (the application macOS uses),
   then zathura or mpv if installed; on Linux zathura, okular, evince or
   mupdf, imv, nsxiv, sxiv, feh, eog, gwenview or ristretto, `mpv
   --force-window` (a window even for audio) or VLC, LibreOffice, and
   `xdg-open` (the desktop's default) last. zathura, okular and mupdf get
   their [`at_grep_match`](#opening-at-a-contents-match). `0` leaves a kind
   to `$EDITOR`. Kinds opened by the same program share one rule.

   Extensions the file's own `[[open.rules]]` already open (by extension or
   pattern) are not asked about; a kind they all open is skipped.

It then shows the TOML it would add and asks before writing. Nothing the file
sets is asked about or changed: `[ui] icons` once set, and its rules, after
which the new ones are added, so that its own still match first; everything
else in the file, comments included, stays as it was. The end of input (Ctrl+d) stops it with nothing written. Its last
part, **Next**, lists what is left: the shell integration and completions
for `$SHELL`, the desktop integration on Linux, and where every setting is
described.

## Editing while fff runs

`gc` in Normal mode (`edit_config` on the [Keys screen](#keys)) opens the
configuration file in `$EDITOR`, as `e` would, and applies it as soon as the
editor ends, reading `keys.toml` again too: workspaces, exclusions,
openers, icons, columns, the mouse and the button bar, live updates, the
trash, cache and performance limits, message timing and syntax colors.
`--no-icons` and `--palette` still win over the file. A palette
chosen with `gP` stays for the session; otherwise the file's palette takes
over. `[ui] preview` and `[ui] sort` say how a session starts, so the pane
and the order stay as they are.
A file that does not exist yet is created by the editor; with `--no-config`
there is nothing to edit.

Before the editor starts, fff copies a configuration that loads to
`config.toml.bak` beside it (`NAME.bak` for `--config NAME`). When the edited
file loads, the copy is removed. When it has errors, the copy stays and fff
reads it instead: a message says so and that F2 shows the errors, the query
box's border says `config errors`, and F2's Session tab gives the message
and TOML's excerpt of the file, carets under each error, until the file
loads again. With no key for `show_session`, the message gives the first error
itself. Run `gc` again to fix it; the copy is kept as
it was, since it is the last configuration that loaded. Later sessions read
the copy too while the file has errors, saying so when they start, and
`fff setup --check-config` still checks the file itself. `reload_palette` and the
palette's own reloads read the copy as well.

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
startup error naming the file. Explicit workspace saves/deletions update the active
config; a missing default file is created on the first save. Startup alone never
writes it. With `--no-config`, workspace edits last only for the session.
Palette edits reload live; external workspace edits reload with r in the
workspace chooser. Other edits made outside fff take effect at the next start, or
at once through [`gc`](#editing-while-fff-runs).
CLI discovery/output options are independent of this file, except that
`--exclude` adds to its [exclusions](#exclusions).

## Exclusions

`[exclusions] rules` are exclusion rules every session starts with, in
every program (roam, pick, seek and the file dialogs):

```toml
[exclusions]
rules = [
  "~/backup",   # this folder only
  "~/.cache",
  "node_modules", # any folder or file of this name
]
```

A rule starting with `/` or `~/` (the home folder) is a glob of absolute
paths and excludes that place and everything below it; any other rule
matches basenames or paths below the searched roots, as `--exclude` does.
Globs keep `/` literal (`*` stays within one component; `**` crosses them).
A folder an absolute rule excludes is still listed from inside, once you go
there.

They are the first lines of the session's exclusion list, so `gE` edits and
removes them for the session (the file is not written) and `ge` turns them
all off; each `--exclude` adds a line after them. `--no-config` leaves them
out. The query box's
`exclusions on` stays away while the list is exactly these rules. When `gc`
changes them, the file's new rules replace the old ones at the top of the
list and the session's own lines stay below them.

## Color palettes

Browse built-ins and custom palettes with **gP**; a highlight held for at least
100 ms previews colors. Moving again restarts the wait. Enter applies immediately
for the session, and Escape from Normal mode reverts. `reload_palette` (no default key) re-reads the configured palette. The config file is never rewritten.

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
| `accent` | Directories, active result/filter borders, and the query box in Normal mode. |
| `marked` | Marks, matched query letters, chooser accents, and the query box in Command, Operator and Pending modes. |
| `success` | Completed-discovery indicator and the query box in Insert mode. |
| `error` | Missing paths, error messages, and the query box in Replace mode. |
| `selection` | Highlighted-row background. |
| `current_directory` | Background of the `@ <current directory>` segment below the filter. |
| `current_directory_text` | Foreground of that segment; `@` and the basename are bold, the basename underlined. |
| `link` | Result names of symbolic links, including links to directories. Built-ins use a vivid blue distinct from `accent`. |
| `executable` | Result names of regular files with any execute bit. Built-ins reuse `success`. |
| `empty` | Result names of empty (zero-byte) regular files. Built-ins reuse `error`. |
| `function` | Function names on the Keys screen (F1), in the `:` picker and where a key clashes. Built-ins reuse `error`. |
| `staged_folder` | Names of folders a copy or extraction is still writing (`.fff-stage-*` and the folders in it, listed with hidden entries); the files in them use `marked`. Built-ins use a pink no other result role uses. It also colors the query box in Visual and Select modes. |

Result names use the first matching role: `staged_folder` or `marked` for
what an operation is still writing, `link`, `accent` for directories,
`empty`, `executable`, then `text`. Only visible rows are inspected, so the
colors follow the file's current state without slowing discovery. A custom
palette that overrides `success` or `error` keeps the inherited `executable`,
`empty` and `function` colors unless it sets them too.

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

## Sorting

```toml
[ui]
sort = "program"          # or "name", "size", "modified", "accessed", "created", "extension", "kind", "depth",
                          # "shallowest", "log"
sort_descending = true    # with a criterion: its own order; false reverses it
temperature = 1.0         # what depth costs a match in the programs' ranking and with "log": 0..1000
```

The order sessions start in; `"program"` is each program's own (the best
match minus a penalty for depth, as `"log"` sorts; the best match first in
`fff seek`).
Descending is each criterion's own order: names from A, the largest, the
newest, folders first, the shallowest, the best match first; `false`
reverses it. `gs`
changes it for the session (see [sorting](USAGE.md#sorting)); like
`[ui] preview`, it says how a session starts, so `gc` leaves the session's
order as it is. Sizes, dates and kinds are read for every match before it
is ranked, which takes a moment in a large listing.

The programs but `fff seek` rank each match, and `"log"` sorts it, by its
score per letter of the query less `temperature × p(depth)`, the highest
first, where p(depth) = 32·ln(1 + depth) / ln 5:

| depth    | 1  | 2  | 3  | 4  | 6  | 8  |
|----------|----|----|----|----|----|----|
| p(depth) | 14 | 22 | 28 | 32 | 39 | 44 |

The first levels cost most; four levels down, a level has cost 8 points
per letter on average at temperature 1, nucleo's bonus for a letter
starting a word: per letter,
matches score from about 3 to 31 (over a home folder, half of them above
13), and a good match scores some 6 to 14 more than a typical one. The
score is taken per letter so that depth weighs the same as the query
grows. At `temperature = 0` depth only breaks ties, as in `fff seek`; a
large one ranks as `shallowest`. Ties go to depth, folders first. A
temperature from 0.25 to 1 is where it weighs most; from 2 on, a long name
near the top that matches almost any query poorly comes before deep good
matches.

## Result details

Each result row ends with details of the entry, right-aligned so they line up:

```toml
[ui]
columns = ["size", "modified"]   # any of "permissions", "size", "modified"; [] for none
```

- `permissions`: like `ls -l`, `drwxr-xr-x`.
- `size`: a file's size (`812B`, `4.2K`, `13M`); a directory's, the sizes
  of every file below it added up (`1.3G`; `2.1G…` while still adding up, `?`
  when unreadable). See [folder sizes](#folder-sizes).
- `modified`: time since the last change: `now`, `5m`, `3h`, `2d`, `3w`,
  `4mo`, `2y`.

Names come first: in a narrow pane, columns are dropped from the left until a
name has room. The footer shows the highlighted entry's path followed by a
link's target, its size (exact bytes; a folder's, then its number of
entries, `9999+` past ten thousand), modification date,
permissions and `user:group`, dropping the last ones first when space runs out.
It colors them by kind as `eza -l` does, in the palette's roles: the name as
its row, the size or count in `success`, the date in `accent`, the owner in
`marked`, and the permissions letter by letter (the kind in `accent`, `r` in
`marked`, `w` in `error`, `x` in `success`, unset bits in `muted`).
Symbolic links show `→ target` after their name, in the `link` color; broken
links use the `error` color.

Only rows on screen are inspected, so discovery does no extra work:
one `lstat` per visible row, read again after each scan. Directory entry counts
and sizes come from background threads, so a huge or slow directory never
delays a frame; the column stays blank until they arrive.

### Folder sizes

A folder's size is the sum of the sizes of the files below it, at any depth,
as `du --apparent-size` adds them: files with several links count once,
symbolic links are not followed, and folders on another filesystem (a mount
point below it) are left out, so an empty folder is `0B`. When some folders
below cannot be read, the footer says `(some unreadable)` after what could be.

Sizes are added up by a background thread per mount, so a stuck mount holds up
only its own folders, and only for the folders that matter now: those on
screen first, from the top, then those a page below and above the view.
A folder that scrolls away is given up, but the totals of the folders inside
it that were finished are kept, so coming back to it, or opening one of
them, is quick. A large folder shows its running total with `…` (and `so far`
in the footer) until it is done.

What was found is kept for the next session in `$XDG_CACHE_HOME/fff/sizes`
(`~/.cache/fff/sizes`), so sizes show as soon as fff starts and are then
checked in the background. Checking reads again only the folders that
changed: a folder whose modification and change times are as they were
keeps the bytes of its files, and only its subfolders are looked at, one
`stat` each, so checking a home folder of 270,000 folders takes a fraction of
a second where adding it up took several. Several threads read each folder
being added up.

A file that grows in place (a log, a database, a virtual machine's disk)
leaves its folder's times alone, so its new size counts once something says
it changed: a change in a watched folder (see [live updates](#live-updates))
reads that folder again and adds up the folders above it, and F5 reads every
folder again. A total is checked again after ten minutes. Until the new total
arrives, the old one is shown. Deleting the file only makes the next session
add everything up again.

### Folders you cannot read

A folder below the one scanned that you may not read (much of `/etc`,
`/var` and `/root` from `/`) is listed but not entered, as `fd` does: no
error, and the scan still counts as complete, so its results are cached as
any other. A scanned folder itself that cannot be read is an error.

### Kernel filesystems

`/proc`, `/sys`, `/dev` and the kernel's other filesystems hold no files of
their own: their sizes are made up (`/proc/kcore` claims the whole address
space, every `/sys` attribute 4 KiB), their contents change as they are read,
their entries come and go with the processes they describe, and some reads
wait forever or act on hardware. fff knows them by the type the mount table
gives them, wherever they are mounted, not by their paths: `proc`, `sysfs`,
`devtmpfs`, `devpts`, `cgroup`, `cgroup2`, `debugfs`, `tracefs`,
`securityfs`, `pstore`, `bpf`, `configfs`, `efivarfs`, `fusectl`, `mqueue`,
`hugetlbfs`, `binfmt_misc`, `nsfs` and `rpc_pipefs`. There:

- **Browsing works as anywhere**: `/sys/class` or `/proc/1` list their
  entries.
- **No sizes.** The size column shows `—` for their folders and files, and the
  footer leaves the size out; nothing there is added up, and the folders
  above them leave them out, as any other mount.
- **Scans do not enter them from outside.** At a depth past 1, `fff pick` and
  `fff seek` from `/` list `proc/` and `sys/` but not what is below; started
  inside one (`fff pick /sys/class`), the scan goes on as anywhere.
- **The contents query does not read their files.**
- **The preview reads at most 4 KiB**, without waiting: `/proc/cpuinfo` shows,
  and a file with nothing to read now (`/proc/kmsg`) says so. A file on
  `devtmpfs` is not read at all.
- **No live updates**: inotify hears nothing from them. F5 and the usual
  [freshness](#discovery-cache-and-performance) rules still apply.
- **An entry gone while it was listed is no error**: a process that ended
  between listing `/proc` and reading its folder is simply left out.

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
  tab, like Up/Down; the marks are kept, since tabs share them.

In the marked list:

- A click on a mark focuses the marked list there (like Ctrl+k, then
  moving to it); a double click acts like `l` on it: a folder is entered and
  a file opens in the background (in `fff explorer`, Enter on it).
- The wheel over the marked list moves through the marks, focusing it.
- A click in the results returns to them, highlighting the row clicked.

In the choosers (workspaces, workspace icons, palettes, and the go-to
suggestions), a click selects a row, a double click is Enter on it (opening
the workspace, choosing the icon, applying the palette, going to the folder),
and the wheel moves the selection. Clicks outside an open chooser do nothing.

Capturing the mouse takes it from the terminal's own text selection; most
terminals still select with Shift held. Set `mouse = false` to leave the mouse
to the terminal.

### Buttons

For mouse-only use, fff can show a bar of buttons above the footer. It is
opt-in:

```toml
[ui]
buttons = "mouse"   # "off" (default), "mouse" (while the mouse is used) or "always"
buttons_seconds = 5 # how long a resting mouse keeps the "mouse" bar; 0 keeps it
```

With `"mouse"`, keyboard-only sessions look exactly as without buttons; the bar
appears when the mouse moves, clicks, drags or scrolls, and goes again once the
mouse has rested for `buttons_seconds` (5 by default, at most 3600; 0 keeps it
for the rest of the session). Each button does what its
key does:

| Button | Key |
| --- | --- |
| ← Back | Ctrl+o: the tab's previous directory |
| ↑ Up | `go_to_parent`: the parent directory |
| Open | `l`: enter the directory or open the file in the background |
| Mark / Unmark | `toggle_mark`, without moving: mark the highlighted row |
| Show / Hide hidden | `toggle_hidden`: hidden entries |
| Cancel | Ctrl+c: quit without output |
| Select & Exit | `print_marks` with marks where they are returned (`fff pick`, `fff explorer open-multiple`), else Enter on the highlighted row |

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
while it reconciles; entries written to update their details in place (and,
with a [contents query](USAGE.md#searching-contents), are searched again,
their matches and preview following), and a folder's entry count and size follow what is added to or removed from it. Folders that
appear are watched once the rescan lists them.

At most 4096 folders are watched, the roots first and then the shallowest
listed folders, leaving the system's inotify watches (often 8192 per user) to
other programs too; folders beyond that, or past the system's limit, follow the
[freshness](#discovery-cache-and-performance) rules and F5. Where inotify is
unavailable (macOS has no inotify) or a directory cannot be watched, fff works as with
`watch = false`. Folders on [kernel filesystems](#kernel-filesystems) are not
watched.

## Preview pane

The preview pane, shown when fff starts, holds the highlighted entry, or the
highlighted mark in the marked list; `gpp` in Normal mode hides or shows
it. PageDown and PageUp scroll its content down and up by half its height
without moving the highlight (facts such as a PDF's pages stay on top); it
starts at the top again for each entry. Pressed while the pane is hidden, they
say which key opens it. Long lines wrap onto the next rows, at a space
where there is one; `toggle_preview_wrap` (`gpw`) cuts them at the pane's
edge instead. It is text only: no images or terminal graphics protocols, so it looks the
same in st and any other terminal.

```toml
[ui]
preview = true   # false: start with the preview pane hidden
highlight = true # false: text files in plain text
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
footer right below shows them for the same entry. Facts, such as a PDF's
pages or where a trashed entry came from, are set apart from the content
below them by a rule.

### Syntax colors

Text files show their syntax in the palette's colors: comments muted,
strings in its success color, keywords in its accent, types in its marked
color, functions in its link color, numbers and constants in its error
color. Neovim, already fff's text engine, does the work in a private
headless instance of its own, started with the first text preview: it finds
the file type from the name and first lines, then colors with a tree-sitter
parser and highlight queries where it has them (those built in, and those
installed for your Neovim under `~/.local/share/nvim/site`, such as by
nvim-treesitter), else with its classic `:syntax` files, which cover
hundreds of file types. Your Neovim configuration and plugins are not
loaded.

Only what the pane holds is colored, the first 300 lines of the first
64 KiB, so a large file costs no more than a small one: a few milliseconds
for most files, more for the first file of a language while Neovim loads its
parser. The text never waits for its colors: when they take longer than
50 ms, it is shown plain and they follow. Moving on drops colors not yet
worked out. `[ui] highlight = false` turns colors off; without `nvim`, text
stays plain.

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
purged either, and `gt` and R say the trash is off.

`gt` in Normal mode opens the trash as a folder, and `R` there
restores entries; see [the trash folder](OPERATIONS.md#the-trash-folder). A
workspace to the trash is not needed.

## Remembered locations

Each directory remembers the entry last highlighted in it: going up highlights
the folder you came from (even several levels up), and entering a folder again
puts the cursor back where it was, as soon as that row is listed (moving first
cancels this). Every session starts in the current directory (or ROOTS), on
the row last highlighted there. Otherwise a newly opened location keeps the
highlight on the first row while its scan streams in, until you move it.
Typing or deleting a letter of the query always highlights the best match;
rows a scan adds under the same query keep the highlighted entry.

Highlights are kept in `$XDG_STATE_HOME/fff/state` (`~/.local/state/fff/state`
when unset): up to 500 directories' highlights, least recently used dropped
first, as exact path bytes. The file is readable only by you and replaced
atomically when a session ends; a missing or damaged file is ignored. Delete
it to forget everything.

## Workspaces

A workspace is a name for some of a session's settings. Opening one applies
the settings it keeps and leaves every other one as it is; one that keeps
only a folder is a place to go to. Each `[[workspaces]]` entry has a unique,
nonempty `name`, an optional `icon`, and at least one of these keys:

| Key | What opening it does |
| --- | --- |
| `path` | Browses that folder. |
| `program` | Turns the session into `"roam"`, `"pick"` or `"seek"`, as `gm` does (the depth then follows the program's default unless the workspace keeps one). |
| `roots` | Pins these roots in place of the session's (`[]` unpins them). Without a `path`, it browses the first one. |
| `query`, `contents` | Sets the names query and the contents query. |
| `depth` | Sets the search depth (1 or more). |
| `filters` | Replaces the file filters (globs, as `--glob`). |
| `exclusions` | Replaces the whole exclusion list, `[exclusions] rules` included. |
| `sort`, `sort_descending` | Sorts the results, as `[ui] sort` names the orders (`"program"` for the program's own). |
| `hidden`, `kinds`, `preview`, `exclusions_on`, `filters_on` | Hidden entries shown, the kinds listed (`"all"`, `"files"` or `"directories"`), the preview pane shown, exclusions enforced, filters applied. |

```toml
[[workspaces]]
name = "Downloads"
icon = "󱃩"
path = "~/Downloads/"

[[workspaces]]
name = "Shared workspace"
path = "/mnt/shared/work"

[[workspaces]]
name = "Nearby notes"
path = "../../notes"

# Like `fff pick --depth 10 --glob '*.pdf' --glob '*.djvu' ~/phd ~/books`.
[[workspaces]]
name = "findnotes"
program = "pick"
roots = ["~/phd", "~/books"]
depth = 10
filters = ["*.pdf", "*.djvu"]
```

`icon` is any text one or two terminal columns wide (surrounding spaces are
trimmed), typically a Nerd Font glyph or an emoji. It is drawn before the name in
the chooser. Workspaces without one show the folder icon when `[ui] icons` is on;
a configured `icon` always shows, even with file icons off.

Path rules, for `path` and `roots`:

- `~` and `~/...` expand using `HOME`.
- Absolute paths are used as written.
- Other relative paths resolve from the **config file's directory**, so a workspace
  has the same destination regardless of where you launch fff.
- Paths are literal TOML strings: spaces work directly; shell substitutions and
  environment-variable expansion are not performed.

Press **gw** in Normal mode to open the chooser. It
works in every program; each workspace shows its folder and what else it
keeps (`pick · 2 roots · depth 10 · 2 filters`). In the chooser, press `i`
to type and fuzzy-filter workspace **names** (Neovim fields initially start
in Normal mode). `j`/`k` in Normal mode or Alt+j/Alt+k move the highlight;
PageUp/PageDown moves farther. Alt+1 through Alt+9 open the corresponding
visible numbered workspace, and Alt+0 opens the last visible workspace.
Out-of-range digits clamp to the last visible entry, including after
filtering or scrolling. These shortcuts work in every editing mode. Enter
opens a workspace; Escape returns to results. A `path` that is not a
directory is shown as unavailable; trying it leaves the chooser open with an
error. Workspaces never select a file or finish the program.

Opening a workspace keeps the marks, the tabs and whatever it does not set,
the query included. Roots that are gone are left out, and the status line
names them; when none of a workspace's roots is there and it has no `path`,
nothing changes. `fff window` and `fff explorer` cannot change program: they
apply the rest of a workspace and say the program stayed, and leave out of
the chooser the workspaces that would only change it. In `fff seek`,
whose folder does not change otherwise, a workspace's folder still applies.
The highlight then stays on the first row as the scan streams in, until you
move it yourself; typing a search there highlights the best match. The
go-to prompt (`/`) offers the workspaces that have a `path` and only goes to it.

`fff --workspace NAME` (any command takes it; shell completion offers the
names) starts in a workspace. What the command line gives wins over it: a
command over its `program`, ROOTS (or the PATH of `fff explorer`) over its
`path` and `roots`, and `--depth`, `--glob`, `--exclude`, `--hidden` and
`--no-exclusions` over the settings they set. A name that is not in the
configuration, or a workspace that cannot be opened, is a startup error. To
pipe its choice, name the program: `fff pick -0 --workspace findnotes |
xargs -0 zathura`.

### Editing workspaces

The chooser is drawn like the other pickers: the filter (it matches names)
under the title, one row per workspace with its number for Alt+digit, its
icon, its name and what it keeps (an unavailable folder in the error
color), the count on the top border and its keys on the bottom one. In the chooser, **n** saves a new workspace, **e** edits the
highlighted workspace, and **d** asks for confirmation before deleting it.
Deletion removes only the workspace record. **r** reloads workspaces from disk.
These letters act while the filter is in Normal mode, so Vim's own `n`, `e`,
`d` and `r` are unavailable there; while typing a filter (Insert mode) use **Alt+n**, **Alt+e**, **Alt+d** and **Alt+r**.
**Up** / **Down** move the highlighted workspace one place and save the
new order immediately; while filtering, it moves past its visible neighbour.
Comments directly above a `[[workspaces]]` table move with it.

The form opens at the bottom like the file-operation dialogs and edits name,
path and icon, then lists the other settings under **Keeps** as a checklist,
like the Keys screen's: ticked `●`, not ticked `○`, or crossed out `×` where
there is nothing to keep (the program in `fff window` or a file dialog).
Each shows the value it keeps: the session's as it is now, or, editing a
workspace, the one it already keeps. A new workspace starts with the current
directory as its path and nothing ticked. The path is optional, as the form
says under it: left empty, the workspace stays in whatever folder you are in
and applies only its ticked settings (a set of filters, say); a workspace
needs a path or a ticked setting. **Space** ticks the
highlighted setting. **Enter** saves and **Escape** cancels, from any editing mode:
in Neovim, Escape first leaves Insert/Visual mode, and Enter during command-line
or pending input still goes to Neovim. Neovim's `ZZ` / `:wq` also save and
`ZQ` / `:q!` cancel. Tab/Shift+Tab or Up/Down switch fields and settings; so do **j** / **k** on
the icon and settings rows, and in the name or path in Normal mode. **Space** on the
icon row opens a searchable selection of 37 choices,
including common Nerd Font glyphs and **Automatic** to clear a custom icon.
Press `i` to filter in Neovim, then Escape and Enter to choose. Existing custom
icons remain intact unless changed. The form warns, under the path, when it is not an
existing directory; such workspaces are still saved and shown as unavailable.

Paths entered in the form resolve relative to the current browsing directory;
`~` uses HOME. New or changed paths are stored as absolute paths. An unchanged
configured relative path retains its original spelling, and an unchanged
setting its formatting; an unticked setting's keys are removed.

Saves preserve unrelated TOML settings, comments, existing file permissions and
valid config symlinks. If another process has changed the workspace list since
it was loaded, saving refuses to overwrite it: cancel and use r to reload.
Save errors leave the form open and preserve the current workspaces. Saving an
unchanged existing workspace leaves the file alone. With configuration
disabled, edits are session-only. No directory or target file is modified.

For navigation and palette bindings, see the [key reference](USAGE.md#keys).

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
path, more while rows are labelled by name, as in `fff pick`
(see `gl`), which keeps labels: roughly 450–550 MB
at the default); lowering depth reduces that working set. Prefetch has its
own small allocation budgets and cannot evict visited snapshots.

### Bounded child prefetch

At depth 1, a directory highlighted in main results for at least **500 ms** can
have its immediate children read before you enter it. There must be no foreground
scan pending. Only a direct child is eligible; file, marks, workspace, palette and
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
- Mount metadata admits only known local filesystem types (APFS on macOS) and
  rejects separate mounts (including bind mounts), remote/unknown filesystems and
  ambiguous escaped mount paths. Symlink targets are skipped.
- A prefetched listing can skip the foreground scan only if it is exhaustive,
  recent, nonempty, and covers the whole new depth-1 scope. With pinned scopes,
  truncation, symlink entries or a greater requested depth, it is only a prefix.

Budgets are checked between filesystem calls. A blocking call cannot be interrupted
at 100 ms; it never blocks input or creates a growing chain of speculative threads.
No user file contents are read, and no cache/index files are written. Mount
classification reads `/proc/self/mounts` on Linux and `getmntinfo` on macOS.

### `[performance]` settings

| Setting | Default | Range | Effect |
| --- | --- | --- | --- |
| `refresh_ms` | `50` | 0–1000 | Minimum interval between merging new paths into the results during a scan. First results, completion and query changes update immediately, as does a backlog of 8,192 unmerged paths. Zero merges after every drained batch. |
| `cached_queries` | `5` | 0–64 | Results of this many shorter queries are kept while you type, so deleting letters restores them instead of ranking every path again; an edited query narrows from the longest kept query it still starts with. Each kept query costs about 24 bytes per matching row, a pointer and its ranking key (rows share their paths). Zero keeps none. Discovery or a policy change discards them. |
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
thread. With rows labelled by name (`fff pick`, or after `gl`), the paths
are grouped by name on a thread of their own, and only the rows drawn are
labelled.

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

### Contents cache

The contents query keeps documents' extracted text in `~/.cache/fff/text`
and an index of what files contain in `~/.cache/fff/index` (both under
`$XDG_CACHE_HOME` when it is set). Neither has settings: once a day each
prunes the entries unused for 90 to 135 days, then the least recently used
while the text cache passes 2 GiB or the index 512 MiB. Deleting either is
safe; it is rebuilt as searches read the files again. See [searching
contents](USAGE.md#searching-contents).

Both are readable only by you (folders 0700, files 0600, whatever the
documents' own modes), since the text cache holds documents' plain text: it
stays after a document is deleted or made unreadable, until pruned or
deleted. Text files over 64 MiB are not searched; a document's text (PDF,
DjVu or EPUB) is read up to 64 MiB, and searches find only what comes
before.

Folders' sizes are kept in `~/.cache/fff/sizes` (see [folder
sizes](#folder-sizes)): a record per folder added up, about 100 bytes each,
written when a session ends. Past a million folders it starts again from
the folders on screen. Deleting it is safe.

## Openers

Which program opens a file, by extension or name, for `L`
(in the background) and `open_in_terminal` (in the terminal); see
[opening files](OPERATIONS.md#opening-files).

```toml
[open]
terminal = "st -e"   # how to run a terminal program in a new window (unset: detected)
fff_terminal = "st -c fff-picker -e"   # fff's own window from fff setup (unset: detected)

[[open.rules]]
command = "zathura"
extensions = ["pdf", "djvu", "epub"]
at_grep_match = "--page {page} --find {query}"   # from a contents match: open there

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

### Opening at a contents match

`at_grep_match` is optional. When `L` opens a file from
a [contents query](USAGE.md#searching-contents) match row, its words
are added to the command, with placeholders filled in from the match; opened
any other way (a file row, `open_in_terminal`, Enter), the rule's `command` runs as it is.
The command line is the rule's `command`, then `at_grep_match`, then the file:

```toml
[[open.rules]]
command = "zathura"
extensions = ["pdf", "djvu"]
at_grep_match = "--page {page} --find {query}"
# runs: zathura --page 233 --find 'Sobolev embedding' /home/you/Gilbarg-Trudinger.pdf
```

| Placeholder | Becomes | Example |
| --- | --- | --- |
| `{page}` | The match's page in a PDF or DjVu (from 1). In a text file or an EPUB, which have none, its line. | `233` |
| `{line}` | Its line, from 1: in a text file its real line, in a document its line in the extracted text (which a viewer does not know). | `14873` |
| `{percent}` | How far into the file the match starts, 0 to 100, by its place in the text. The useful position in an EPUB. | `42` |
| `{query}` | The contents query as you typed it, spaces at its ends dropped. | `Sobolev embedding` |
| `{match}` | The text that matched, as the file has it: its case and accents, and for a match broken over lines, its parts joined by one space. | `Sobolev Embedding` |
| `{nth}` | Which of the file's matches it is, from 1, in the file's order. | `3` |
| `{count}` | How many matches the file has (at most 200 are kept). | `7` |
| `{path}` | The file's full path. Named anywhere in `at_grep_match`, the file goes there instead of at the end. | `/home/you/notes/Brezis.pdf` |

How the words are made:

- `at_grep_match` is split into words like `command`: spaces separate,
  quotes and backslashes work, `~` is your home, and no shell runs.
- Placeholders are filled in after splitting, so a value with spaces (a
  query, a path) stays one argument, and a placeholder can sit inside a word:
  `--page={page}`, `+{line}`, `"{nth}/{count}"`.
- Each word is filled in once: a value that happens to read like a
  placeholder (a query `{page}`) stays as it is. Unknown braces, such as
  `{x}`, are left alone.
- With `terminal = true` the whole line runs in `[open] terminal`, as for
  any terminal opener.

Some uses:

```toml
# zathura: on the match's page, the query found and highlighted.
at_grep_match = "--page {page} --find {query}"

# mupdf takes the page after the file.
at_grep_match = "{path} {page}"

# Neovim (terminal = true): on the line, the cursor on the match.
at_grep_match = "+{line} -c /{query}"

# Your own script: everything fff knows, as separate arguments.
command = "my-opener"
at_grep_match = "--path {path} --page {page} --line {line} --percent {percent} --query {query} --match {match} --nth {nth} --count {count}"
```

A shell is the way to do more, such as a notification or a log, but a
placeholder pasted into the script's text is pasted unquoted. Pass the
values as the shell's own arguments instead, and quote them there:

```toml
command = "sh -c"
extensions = ["pdf"]
at_grep_match = '''
'notify-send "Match $3 of $4" "$2"; exec zathura --page "$1" --find "$2" "$5"'
fff {page} {query} {nth} {count} {path}
'''
```

`terminal` must start a terminal window running the rest of the line: `st -e`,
`alacritty -e`, `foot`, `kitty`. `fff_terminal` does the same for fff itself
when the desktop entry or the file chooser installed by `fff setup` starts
it; give the window a class, such as `fff-picker`, so that a window manager
rule can float it (see [desktop integration](DESKTOP.md#floating-windows)).
The file chooser waits for the terminal to exit, so it must not hand the
window to a running server and return at once.

Unset, both are detected, in this order:

1. the terminal fff runs in, found among its parent processes, else on
   macOS by the `$TERM_PROGRAM` it sets (iTerm2, WezTerm and Ghostty do;
   inside tmux it names tmux);
2. `$TERMINAL`, a known terminal in its own syntax, any other with `-e`;
3. `x-terminal-emulator -e` (Debian's choice of terminal);
4. the first of the known terminals installed: st, Alacritty, kitty,
   WezTerm, Ghostty, foot (under Wayland), Konsole, GNOME Terminal, Xfce
   Terminal, urxvt and xterm;
5. on macOS, Terminal.app.

A known terminal runs a command in the foreground (`gnome-terminal --wait
--`, `konsole --nofork -e`, `xfce4-terminal --disable-server -x`, `wezterm
start --always-new-process --`), and for `fff_terminal` gives the window
the class `fff-picker` where it can: `st -c`, `alacritty --class`, `kitty
--class`, `wezterm start --class`, `xterm -class`, and the app id `foot
--app-id`. Ghostty (whose class must be a dotted application id), Konsole,
GNOME Terminal, Xfce Terminal and urxvt get no class: set `fff_terminal` for
one.

On macOS, Terminal.app and iTerm2 take no command on their command line:
fff runs `osascript` with a script that types the program, in the folder fff
runs it in, into a new window's shell (the first time, macOS asks whether
the terminal fff runs in may control Terminal.app or iTerm2). Ghostty's
command opens no window there either, so a new instance of the app is
started with `open -na Ghostty`, quitting when its window closes. Opening
returns at once, as it does with any terminal. An app's own binary (such as
`/Applications/kitty.app/Contents/MacOS/kitty`) runs where its command is not
on the PATH.

When nothing is found, opening a terminal program and `fff setup` say
so and ask for the setting. `fff setup --check-config` reports the opener
and terminal programs the configuration names that are not installed.

## Modal text editing

A clean headless Neovim instance edits every field, initially in Normal mode
(prompts made for typing start in Insert); press `i` or `a` to type. Neovim is
required: fff exits with an error when `nvim` is missing or cannot start. See the
[editing guide](EDITING.md) for Insert/Normal/Visual modes, Escape semantics and
reserved fff shortcuts.

## Keys

Everything a key can do is a **function** with a name (`go_to_parent`,
`quit_into_highlighted`, `toggle_filters`), a description, and a fixed set of places
where it works. **F1 opens the Keys screen**, which lists them with their
keys and edits them; edits apply at once and are saved to **`keys.toml`**
beside the configuration file (`~/.config/fff/keys.toml`, or beside
`--config FILE`). With `--no-config` they last for the session.

### Names

A function's name is a verb and what it acts on, and its description starts
with that verb: `highlight_` functions move the highlight, `go_to_` ones
change the current directory, `toggle_` ones turn something on or off,
`show_` ones open a screen or chooser, `edit_` ones open something in an
editor, `print_` ones answer with paths and leave, and `quit_` ones leave.
File operations are their verb alone (`trash`, `delete`, `rename`), acting
on the current or marked entries.

### Where keys apply

A place is a combination of three things, fixed by design:

| Dimension | Values |
| --- | --- |
| program | `roam`, `window`, `pick`, `seek`, `explorer-open`, `explorer-open-multiple`, `explorer-folder`, `explorer-folder-multiple`, `explorer-save` (`fff explorer open`, `open-multiple`, `folder`, `folder --multiple` and `save`), `piped` (`fff` with [paths on stdin](USAGE.md#choosing-from-another-programs-paths-command--fff)) |
| focus | `query`, `results`, `marks` (the marked list) |
| highlight | `dir`, `file`, `none` |

Each function does one thing, the same in both modes, and works only in
the places where that thing means something, which may be several:
`quit_into_highlighted` works in `roam` and `seek`, whose shell follows
fff; `print_highlighted` where fff answers with a path (`pick`, `piped`, a
dialog), on a file or a folder as the program asks; `print_marks` where the
answer can be several paths; `go_to_parent` everywhere but `seek` and
`piped`; `restore` in the results and the marked list. In `piped` only
moving the highlight, the marks, the queries, the view, printing and
quitting work. Whatever works on the results works from their query too.

Keys are bound per mode, Insert or Normal, and each key applies in one place
you choose: some programs, some focuses, some highlights, all of them unless
narrowed. Two keys may be the same only where their places, within where
their functions work, do not meet, so a key does exactly one thing wherever
it is pressed. That is how Enter does different things in different places:
in Normal mode it is `quit_into_highlighted` on a directory in `fff roam`,
`go_to_highlighted` on one in a window of its own, `open_and_quit` on a file
in `fff roam`, `print_highlighted` in `fff pick`, `print_here` with nothing
highlighted there, and so on. Some functions have no key by default
(`clear_marks`); bind them to use them.

### The query in focus

The focus is what keys act on: the results, the marked list, or the query
above the results; the one without it has a dimmed border. Typing gives
the query the focus: Insert mode always types into it, so Insert-mode keys
apply in the `query` focus only, and a key pressed in the results that no
binding has goes to Neovim and gives it the focus too (`i`, `a`, `cw`, `0`).
Escape from Insert mode stays in the query, whose Normal mode edits it as
Neovim does: by default every key typed as text (`h`, `l`, `x`, `v`, `p`,
`u`, `.`, digits, `g` sequences, `:`) is Neovim's there but `j` and `k`,
which give the results the focus on their first or last row
(`highlight_first` and `highlight_last` do that from the query in Normal
mode), and `gc` (`edit_config`), while Enter,
Tab, the arrows and the Alt and Ctrl keys still act on the results below. Escape
there (`toggle_query_focus`) gives the results the focus back, and a click
on a row does too. The Keys screen's query and list take the focus the
same way.

A key you bind always comes before Neovim's: bound without a `focus`, it
applies in the query too (`trash = "x"` trashes from the query as well).
To keep a key for Neovim there, leave `query` out of its focus, as the
default keys typed as text do (`focus = ["results", "marks"]`, or untick
**query** on the Keys screen). Keys no binding has go to Neovim, in either
focus.

### The Keys screen

F1 replaces the results with a picker over the functions, laid out like
them: the query box on top, the functions with their keys in Insert and
Normal mode, the highlighted one's details beside them (what it does, each
key and where it applies, and where the function works), and a line saying
what the screen's own keys do. It lists the functions that work in the
program and list you opened it from (`showing what works in fff roam ·
results`); Ctrl+a shows every function, and the fixed keys too.

The query is a Neovim field like the results' own, ready to type. Tab
switches between two views of the same keys: **Functions**, a row per
function with its keys in Insert and Normal mode, searched by name,
description and key names; and **Keys**, a row per key and place, with
columns for the key, the function, the mode, the programs, the focuses and
the highlights, searched by key name (an exact name first, as typed or in
Vim's notation such as `<CR>`) or function name. In Normal mode, `r` and
then a key writes that key's name into the query and shows the Keys view,
for keys you cannot type there.

Your own keys work here as they do in the results: `highlight_previous` and
`highlight_next` (Alt+k/Alt+j in Insert mode, `k`/`j` in Normal mode), the
other `highlight_` functions, `scroll_preview_down` and `scroll_preview_up`
(PageDown/PageUp) for the details, `clear_query` (Escape in Normal mode),
`show_session`, `cancel` and `quit`. Escape never closes the screen; F1
(`show_keys`) does, from anywhere in it.

Where a key applies is shown the same way everywhere: as **checklists**, a
column each for the mode, the program, the focus and what is highlighted,
with a value ticked `●` or not `○`. The key applies wherever a tick of each
column meets. A value crossed out `×` cannot apply with the other columns'
ticks: the function does not work there, or the key cannot be bound in that
mode (a key typed as text in Insert mode, `i` or `a` in Normal mode, Insert
mode anywhere but the query). Ticking
a program can cross out a highlight, and so on. Where columns are shown in
a few words, they say `all`, a list, or `all but` a list, whichever is
shorter.

The same three keys work at every level: **Space goes a level deeper**
(Enter does too in the picker, where Space is typed in Insert mode),
**Enter is done** and **Escape goes back**.

- **The picker:** Space opens the highlighted function's keys in a sheet.
  Changes there are staged: Enter saves them (to the
  session and `keys.toml`), Escape drops them, and F1 closes the screen
  dropping them too.
- **A function's keys:** one row per place: its key, mode, programs, focuses
  and highlights, with the highlighted row's checklists below (left out when
  the sheet would not fit otherwise). Moving past the last row goes round to
  the first, and back. Space edits where the highlighted key applies. `n`
  adds a key: press it (the first key may be any key, more may follow for a
  sequence; Backspace takes one back), then Enter goes on to where it
  applies. `R` replaces the highlighted key: press the new one, and Enter
  puts it where the old one applied, at once when nothing else has it there,
  else through where it applies, with the clash shown. A digit for
  `search_to_depth`, `open_row`, `highlight_row` or `highlight_next_in_row`
  stands for every digit. `d`
  deletes the highlighted row, and `D` puts the function's default keys
  back, after asking.
- **Where a key applies:** the checklists, to change. Tab and Shift+Tab go
  to the next and previous column, arrows (or `h`/`j`/`k`/`l`) move, Space
  ticks, Enter is done, Escape goes back
  unchanged. When the key clashes with another function's, the sheet lists
  the keys it clashes with, their functions and where they meet, and marks
  the ticked values they share: untick until nothing clashes, or `u` keeps
  it only where nothing else has it, or `t` takes it from the others there
  (they keep it elsewhere).

A sheet that does not fit the terminal is not cut: fff says how large the
terminal must be until it is, or until the sheet is left.

One row is one set of ticks. For places one set cannot say, such as in
`fff pick` anywhere, or in `fff roam` on a directory, give the function the
same key again with `n`: its rows read **or**.

Saving keys that would lock you in is refused, with the reason, and the
sheet stays open: something must open the Keys screen (`show_keys`) and
something must leave fff (`cancel`, or `quit` in Normal mode) everywhere in
the results, something must leave the marked list (`toggle_marked_list`)
and, in Normal mode, the query in focus (`toggle_query_focus`),
Insert-mode keys cannot start with a key typed as text, nor be Escape, and
Normal-mode keys cannot start with `i`, `a`, `I` or `A`, which reach
Insert mode, or `:`, which opens the command picker. Space, Enter, Tab, `r` and Ctrl+a on
the screen, and the sheets' own keys, are fixed.

### `keys.toml`

```toml
[insert]
highlight_next = ["<C-n>", "<Down>"]
highlight_previous = ["<C-p>", "<Up>"]

[normal]
quit = "Q"
go_to_highlighted = [
  { keys = "l", highlight = ["dir"] },
  { keys = "<CR>", program = ["roam", "window"], highlight = ["dir"] },
  { keys = "<CR>", program = ["pick"], focus = ["query", "results"], highlight = ["dir"] },
]
```

Each entry replaces one function's keys in one mode; functions not named
keep their defaults. A value is keys, `"none"` (or `""`, `[]`) for no keys,
or a list of keys and tables with `keys` and any of `program`, `focus` and
`highlight`, each a list of the values [above](#where-keys-apply), named as the
Keys screen's columns name them. Keys use Vim's notation: printable keys stand for themselves
(`gg`, `g!`, `G` for Shift+g), others are named in `<...>` with `C-` (Ctrl),
`M-` (Alt) and `S-` (Shift) before them: `<CR>`, `<Esc>`, `<Tab>`, `<BS>`,
`<Del>`, `<Insert>`, `<Space>`, `<lt>` (`<`), `<Up>`, `<Down>`, `<Left>`,
`<Right>`, `<PageUp>`, `<PageDown>`, `<Home>`, `<End>`, `<F1>` and up, or
one character (`<M-h>`, `<C-w>`, `<M-P>`, `<M->>`; with Ctrl, a capital
needs `S-` too: `<C-S-P>`). `<#>` is the digits 1–9, and `<M-#>` and
`<C-#>` Alt or Ctrl with any digit. A sequence is several keys (`gx`,
`<M-g>u`); one cannot be the start of another where both apply, since a
waiting prefix has no timeout. Unbound keys and unfinished prefixes reach
Neovim.

A key you bind takes over a default one where they clash: `quit = "Q"` frees
`q`, and `toggle_hidden = "q"` alone would take `q` from `quit` in the results,
leaving it in the marked list. Two keys you bind may not clash. With errors,
fff uses the default keys and says so; `fff setup --check-config` reports
them. [`docs/keys.example.toml`](../keys.example.toml) is a commented example.

### Keys on screen

What the screen says is what works: the Keys screen, footers, empty-pane
hints, query-line hints and status messages name each function's key as
bound, preferring one that applies where you are and in the mode you are
in. A key that works in the other mode says so when pressed (`Alt+l works
in Insert mode; in Normal mode, l`), and one that does nothing where it is
pressed says which function it is and that it does not work there. Keys
inside choosers, prompts and confirmation sheets (Enter, Escape, Tab,
`j`/`k`/`gg`/`G` and `q`/`!` in Normal mode) are fixed.
