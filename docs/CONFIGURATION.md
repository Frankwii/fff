# Configuration

fff works without any configuration. When you want to change something, it
reads a TOML file, `~/.config/fff/config.toml` (or
`$XDG_CONFIG_HOME/fff/config.toml`). Keys live in a file of their own
beside it, `keys.toml`; see [Keys](KEYS.md).

## Getting started

- **`fff setup`** writes a first configuration from a few questions (your
  icons, and which programs open your documents, images and videos).
- **[`config.example.toml`](config.example.toml)** is a commented example
  of every setting.
- **`gc`** in fff opens the configuration in your `$EDITOR` and applies it
  as soon as you close the editor.
- **`fff setup --check-config`** checks the file and says what's wrong, and
  which file it read.

If the file has an error, fff doesn't start: it shows the line and what's
wrong with it. Editing with `gc` is safer: fff first keeps a copy of the
configuration that worked (`config.toml.bak`), and if your edit has an
error, it goes on with that copy, saying so, and F2 shows the error until
you fix the file.

From the command line, `--config FILE` reads another file, and
`--no-config` ignores it and uses the defaults.

## What applies when

Most settings apply as soon as you save them through `gc`. Palette colors
apply even when you edit the file outside fff. Two settings only say how a
session starts: `[ui] preview` and `[ui] sort`, since you change those
while browsing (`gpp`, `gs`).

## `[ui]`: the interface

```toml
[ui]
palette = "midnight"
icons = true
columns = ["size", "modified"]
preview = true
highlight = true
sort = "program"
sort_descending = true
temperature = 1.0
mouse = true
buttons = "off"
buttons_seconds = 5
watch = true
message_seconds = 4
```

These are the defaults. Each is described below.

### Palette

`palette` names the color palette, built-in or your own. See [Color
palettes](PALETTES.md).

### Icons

`icons = true` shows a [Nerd Font](https://www.nerdfonts.com) icon before
each name: a folder, or one for the file's type. Your terminal's font must
have the Nerd Font symbols, or you'll see boxes. `false` (or `--no-icons`
for one run) shows plain `▸` and `·` markers instead. `fff setup` asks
whether your terminal shows them, and how to install a Nerd Font if not.

### Rows and details

`columns` chooses the details at the end of each result row, from
`"permissions"` (`drwxr-xr-x`), `"size"` and `"modified"` (time since the
last change). `[]` shows none. In a narrow terminal, columns are dropped
before names are shortened.

### Preview

`preview = false` starts with the preview pane hidden (`gpp` shows it).
`highlight = false` shows text files without syntax colors.

### Sorting and ranking

`sort` sets the order sessions start in: `"program"` (each program's own,
the best match first), or one of `"name"`, `"size"`, `"modified"`,
`"accessed"`, `"created"`, `"extension"`, `"kind"`, `"depth"`,
`"shallowest"` or `"log"` (see [sorting](SEARCHING.md#sorting)).
`sort_descending = false` reverses the order.

`temperature`, from 0 to 1000, sets how much depth counts against a match
when ranking. At 0, depth only breaks ties, so the best match comes first
however deep. The default, 1, prefers shallower entries unless a deeper
one matches clearly better. Larger values list shallow entries first
almost regardless of the match; past 2 or so, they rarely help.

### Mouse and buttons

`mouse = false` leaves the mouse to the terminal, for selecting text.

`buttons` adds a bar of buttons above the footer, for using fff with the
mouse alone: Back, Up, Open, Mark, Show hidden, Cancel, and Select & Exit.

- `"off"`: no buttons (the default).
- `"mouse"`: shown once you use the mouse, hidden again after it rests for
  `buttons_seconds` seconds (0 keeps them).
- `"always"`: always shown.

### Live updates

`watch = true` updates the results when files change in the folders you're
looking at. `false` turns it off; F5 then rescans by hand.

### Messages

`message_seconds` is how long status messages stay, in seconds; `0` keeps
each until the next one.

## `[exclusions]`

```toml
[exclusions]
rules = ["node_modules", "~/.cache", "~/backup"]
```

Entries every session leaves out. See [exclusions](SEARCHING.md#exclusions).

## `[open]`

Which programs open which files, and the terminal fff opens windows in. See
[Opening files](OPENERS.md).

## `[[workspaces]]`

Saved places and setups. See [Workspaces](WORKSPACES.md); the `gw` chooser
writes them for you.

## `[palettes.NAME]`

Your own color palettes. See [Color palettes](PALETTES.md#making-your-own).

## `[trash]`

```toml
[trash]
enabled = true
days = 30
ask_above = "10 GiB"
```

- `enabled = false` turns the trash off: `x` then deletes permanently,
  after a review, like `X`.
- `days`: entries older than this are deleted from the trash when fff
  starts.
- `ask_above`: `x` asks before trashing anything larger, offering to delete
  it permanently instead. A number of bytes, or a size such as `"500 MiB"`
  or `"1.5G"`.

See [the trash](OPERATIONS.md#the-trash).

## `[cache]` and `[performance]`

fff keeps recent listings in memory, so that going back to a folder is
instant. The defaults suit almost everyone; these settings are for very
large trees or small machines.

```toml
[cache]
enabled = true          # reuse the listings of folders you visited
max_entries = 16        # how many listings to keep (1–128)
max_candidates = 4096   # paths kept per listing (1–100000)
ttl_seconds = 60        # how often a deep listing is refreshed (1–86400)
fresh_seconds = 3       # a listing this recent is shown without rescanning (0–60)
prefetch = true         # read a highlighted folder before you go into it

[performance]
refresh_ms = 50         # how often results are re-sorted during a search (0–1000)
cached_queries = 5      # shorter queries kept, so Backspace is instant (0–64)
max_paths = 1000000     # the most paths a search holds (1000–100000000)
```

`max_paths` bounds the memory a huge search can use: a search that finds
more stops there, and the query box says `◌ partial`. Lower the depth to
see the rest.

The [development notes](dev/CONFIGURATION.md#discovery-cache-and-performance)
describe exactly what each of these does.

## Files fff keeps

Besides your configuration, fff keeps a few files of its own. All of them
are safe to delete.

| Where | What |
| --- | --- |
| `~/.local/state/fff/state` | The row last highlighted in each folder |
| `~/.local/state/fff/open.log` | The output of programs opened in the background |
| `~/.local/share/fff/trash` | The trash (and `.fff-trash-UID` at the top of other filesystems) |
| `~/.cache/fff/text`, `~/.cache/fff/index` | Text extracted from documents, and an index of what files contain, for contents search |
| `~/.cache/fff/sizes` | Folder sizes, so they show at once next time |

Each follows its XDG variable when set (`$XDG_STATE_HOME`, `$XDG_DATA_HOME`,
`$XDG_CACHE_HOME`). The contents cache is readable only by you, since it
holds your documents' text; it cleans itself up after about three months
of not being used, and stays under 2.5 GiB.
