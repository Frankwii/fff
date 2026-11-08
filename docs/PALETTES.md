# Color palettes

fff comes with 37 color palettes, from well-known families such as
Catppuccin, Gruvbox and Tokyo Night, and you can make your own.

## Choosing a palette

Press **`gP`** to open the palette chooser. Move through it with `j` and
`k`, or press `i` and type to filter: each palette is tried on the whole
screen after a moment. **Enter** keeps it for this session; **Escape** goes
back to the colors you had.

The chooser never changes your configuration. To keep a palette, set it
there:

```toml
[ui]
palette = "catppuccin-mocha"
```

or choose one for a single run with `fff --palette nord`. Shell completion
offers every name.

## Built-in palettes

| Family | Names |
| --- | --- |
| fff's own | `midnight` (the default), `light`, `forest` |
| Catppuccin | `catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`, `catppuccin-mocha` |
| Nord | `nord` |
| Gruvbox | `gruvbox-dark`, `gruvbox-light` |
| Tokyo Night | `tokyonight-night`, `tokyonight-storm`, `tokyonight-moon`, `tokyonight-day` |
| Dracula | `dracula` |
| Everforest | `everforest-dark`, `everforest-light` |
| Kanagawa | `kanagawa-wave`, `kanagawa-dragon` |
| One Dark | `onedark` |
| Rosé Pine | `rose-pine`, `rose-pine-moon`, `rose-pine-dawn` |
| Solarized | `solarized-dark`, `solarized-light` |
| Ayu | `ayu-dark`, `ayu-light`, `ayu-mirage` |
| Monokai | `monokai` |
| Material | `material-darker` |
| Zenburn | `zenburn` |
| Base16 classics | `ocean`, `eighties`, `ashes`, `atelier-dune`, `black-metal`, `tomorrow-night` |

`gruvbox`, `tokyonight`, `everforest`, `kanagawa` and `solarized` also work,
as their dark variant.

## Making your own

A custom palette starts from a built-in one and changes only the colors you
name:

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

- `extends` names the built-in palette to start from (`midnight` if left
  out).
- Colors are `"#RRGGBB"`, or `"default"` for your terminal's own color.
  `background = "default"` keeps a transparent or themed terminal
  background, for example.
- A custom palette needs a name of its own, not a built-in's.

Custom palettes appear in the `gP` chooser after the built-ins.

These are the colors you can set:

| Color | Used for |
| --- | --- |
| `background` | The screen's background |
| `panel` | The background of the results, the marked list and windows |
| `text` | Ordinary text |
| `muted` | Details, paths and hints |
| `border` | Borders of what doesn't have the focus |
| `accent` | Folders, the focused border, and the query box in Normal mode |
| `marked` | Marks, the letters a query matched, and the query box in Command mode |
| `success` | Completed searches and the query box in Insert mode |
| `error` | Errors, missing files, and the query box in Replace mode |
| `selection` | The highlighted row's background |
| `current_directory` | The background of the `@` line, the folder you're in |
| `current_directory_text` | The text of the `@` line |
| `link` | Names of symbolic links |
| `executable` | Names of executable files (built-ins use `success`) |
| `empty` | Names of empty files (built-ins use `error`) |
| `function` | Function names on the Keys screen (built-ins use `error`) |
| `staged_folder` | Folders a copy is still writing, and the query box in Visual mode |

File icons and the preview's syntax colors use these same colors, so they
follow your palette too.

## Live editing

While fff runs, saving your configuration file updates the colors at once,
so you can tune a palette with fff open beside your editor. A mistake while
you edit keeps the last working colors and shows the error; fixing it
recovers on its own.

A palette chosen with `gP` stays for the session, though edits to that
palette's own colors still show. `reload_palette` (no key by default; bind
one on the [Keys screen](KEYS.md), or run it from `:`) goes back to the
palette your configuration names.

## Without color

fff honors [`NO_COLOR`](https://no-color.org): set it, and fff draws
without colors, relying on bold text, arrows and labels instead.

## Credits

The built-in palettes follow their projects' published colors; the
[development notes](dev/PALETTES.md#color-sources) list the sources, and
[THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md) the licenses.
