# Workspaces

A workspace is a name for a place, or for a whole setup: a folder, plus
any settings you want to come back to, such as the depth, file filters or
pinned roots. Opening one applies what it keeps and leaves everything else
as it is.

Some uses:

- **Bookmarks.** A workspace that keeps only a folder is a place to jump
  to: `Downloads`, `Notes`, a project.
- **Saved searches.** "Every PDF and DjVu in my papers and books, ten
  levels deep" in one keypress, instead of a long command line.
- **Script shortcuts.** `fff pick --workspace papers` replaces a script that
  starts fff with the right roots and filters.

## Using workspaces

**`gw`** opens the workspace chooser. Each row shows the workspace's name,
its folder and what else it keeps (`pick · 2 roots · depth 10 · 2 filters`).

| Key | Does |
| --- | --- |
| `j` / `k` | Move |
| `i` | Type to filter by name |
| Enter | Open the highlighted workspace |
| Alt+`1`…`9` | Open that numbered workspace |
| Escape | Back to the results |

Opening a workspace keeps your marks and tabs, and whatever it doesn't
set, your query included. A workspace whose folder is missing is shown as
unavailable.

The go-to prompt (`/`) suggests your workspaces too, as you type their
names. From the command line, `--workspace NAME` starts in one:

```sh
fff --workspace notes
fff pick -0 --workspace papers | xargs -0 -r zathura
```

Whatever you give on the command line wins over the workspace: a command
over its program, folders over its folder and roots, and `--depth`,
`--glob`, `--exclude` and `--hidden` over the settings they set.

## Saving and editing

In the chooser, in Normal mode:

| Key | Does |
| --- | --- |
| `n` | Save a new workspace from the current session |
| `e` | Edit the highlighted workspace |
| `d` | Delete it, after asking |
| Up / Down | Move it up or down the list |
| `r` | Reload the workspaces from the configuration file |

While filtering (Insert mode), use them with Alt: Alt+n, Alt+e, Alt+d,
Alt+r.

`n` and `e` open a form:

- **Name**, **path** and **icon**. The path starts as the current folder.
  Leave it empty for a workspace that doesn't move you, only applies
  settings (a set of filters, say). **Space** on the icon row opens a list
  of icons to choose from.
- **Keeps**: a checklist of the settings it can keep, each shown with its
  current value. **Space** ticks one. A new workspace starts with nothing
  ticked; tick what you want it to remember, as it is right now.

Tab, or `j`/`k`, moves between fields. **Enter** saves, **Escape** cancels.

Workspaces are saved in your configuration file, which fff edits carefully:
your comments and other settings stay as they are.

## In the configuration file

Each workspace is a `[[workspaces]]` table:

```toml
[[workspaces]]
name = "Downloads"
path = "~/Downloads"

[[workspaces]]
name = "Notes"
icon = "󰠮"
path = "~/notes"

# Like `fff pick --depth 10 --glob '*.pdf' --glob '*.djvu' ~/phd ~/books`.
[[workspaces]]
name = "papers"
program = "pick"
roots = ["~/phd", "~/books"]
depth = 10
filters = ["*.pdf", "*.djvu"]
```

Every workspace needs a unique `name` and at least one other key:

| Key | Opening the workspace… |
| --- | --- |
| `path` | Goes to that folder. |
| `program` | Turns the session into `"roam"`, `"pick"` or `"seek"`, as `gm` does. |
| `roots` | Pins these folders as [roots](SEARCHING.md#pinned-roots), in place of the session's (`[]` unpins them). Without a `path`, goes to the first one. |
| `query`, `contents` | Sets the names query and the contents query. |
| `depth` | Sets the search depth. |
| `filters` | Replaces the [file filters](SEARCHING.md#file-filters), as `--glob` sets them. |
| `exclusions` | Replaces the whole list of [exclusions](SEARCHING.md#exclusions). |
| `sort`, `sort_descending` | Sorts the results, with the names `[ui] sort` uses. |
| `hidden` | Shows hidden entries (`true`) or hides them (`false`). |
| `kinds` | Lists `"all"`, only `"files"`, or only `"directories"`. |
| `preview` | Shows or hides the preview. |
| `exclusions_on`, `filters_on` | Turns the exclusions or the filters on or off. |
| `icon` | Not a setting: an icon shown before the name, one or two columns wide (a Nerd Font glyph or an emoji). |

**Paths** (in `path` and `roots`): `~` is your home, absolute paths are used
as written, and relative paths are relative to the configuration file's
folder, so a workspace goes to the same place wherever you start fff.
Environment variables aren't expanded.

After editing the file by hand, press `r` in the chooser to reload it, or
use `gc` to edit the configuration from inside fff, which applies it at
once.

In `fff window` and file dialogs, which can't change program, a
workspace's `program` is ignored and the rest applies.
