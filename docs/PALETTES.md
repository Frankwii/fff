# Built-in palettes and live reload

Set `[ui].palette` in your TOML file, or pass `--palette NAME`:

```toml
[ui]
palette = "catppuccin-mocha"
```

| Family | Names |
| --- | --- |
| Fuzzy file finder originals | `midnight`, `light`, `forest` |
| Catppuccin | `catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`, `catppuccin-mocha` |
| Nord | `nord` |
| Gruvbox | `gruvbox-dark`, `gruvbox-light` (`gruvbox` aliases dark) |
| Tokyo Night | `tokyonight-night`, `tokyonight-storm`, `tokyonight-moon`, `tokyonight-day` (`tokyonight` aliases night) |
| Dracula | `dracula` |
| Everforest | `everforest-dark`, `everforest-light` (`everforest` aliases dark; medium contrast) |
| Kanagawa | `kanagawa-wave`, `kanagawa-dragon` (`kanagawa` aliases wave) |
| One Dark | `onedark` |
| Rosé Pine | `rose-pine`, `rose-pine-moon`, `rose-pine-dawn` |
| Solarized | `solarized-dark`, `solarized-light` (`solarized` aliases dark) |
| Ayu | `ayu-dark`, `ayu-light`, `ayu-mirage` |
| Monokai | `monokai` |
| Material | `material-darker` |
| Zenburn | `zenburn` |
| Ocean | `ocean` |
| Eighties | `eighties` |
| Ashes | `ashes` |
| Atelier | `atelier-dune` |
| Black Metal | `black-metal` |
| Tomorrow | `tomorrow-night` |

Names are lowercase and use dashes. Catppuccin is spelled `catppuccin`; Frappé's
identifier is ASCII `frappe`. Any built-in can be the `extends` base of a custom
palette. See [configuration](CONFIGURATION.md) for the semantic color roles.

When a config file is enabled, saving changes to its palette selection or color
roles updates the running interface automatically, normally within 500 ms. File
reading and parsing happen in a background thread. Invalid intermediate edits
keep the last working colors and show an error; correcting the file recovers
automatically. Atomic-save/rename workflows work too. `--palette` pins the chosen
name but still reloads custom colors for that name. `--no-config` disables file
reload. This reload applies to colors only; bookmarks reload with Alt+r in their chooser; cache settings need a new
invocation. Query, cursor, marks, discovery, and numbered targets are preserved.

## Switching palettes while running

| Key | Action |
| --- | --- |
| Alt+p | Open the palette chooser: every built-in in the order above, then the config file's custom palettes. Press `i` to fuzzy-filter names (the chooser starts in Normal mode); a highlight held for at least 100 ms previews it across the whole interface. Moving again restarts the wait. Enter applies it immediately for this session; Escape (or Alt+p again) restores the previous colors. |
| Alt+P | Re-read the config file now and apply its palette, discarding any Alt+p session choice. With `--palette NAME`, re-read that name's definition instead. |

The chooser never writes the config file. After Enter, background reload keeps
following the *chosen* palette's definition (editing `[palettes.NAME]` still
updates it) and ignores `[ui].palette` until Alt+P returns control to the file.
If that re-read fails, the current colors stay and the error is shown; the
session choice is still cleared, so fixing the file recovers automatically.
Alt+P works with `--no-config` too (it restores the default or pinned palette).
Both chords can be remapped, e.g. `--map-key ctrl+t=alt+p --map-key ctrl+r=alt+P`.

## Color sources

Built-in colors come from these upstream definitions:

- [Catppuccin palette](https://catppuccin.com/palette/) and
  [official palette JSON](https://github.com/catppuccin/palette/blob/main/palette.json), version 1.8.0.
- [Nord's official colors and palettes](https://www.nordtheme.com/docs/colors-and-palettes/).
- [Gruvbox's original color definitions](https://github.com/morhetz/gruvbox/blob/master/colors/gruvbox.vim).
- [Dracula specification](https://spec.draculatheme.com/).
- [Everforest palette](https://github.com/sainnhe/everforest/blob/master/palette.md).
- [Kanagawa colors](https://github.com/rebelot/kanagawa.nvim/blob/master/lua/kanagawa/colors.lua).
- [One Dark](https://github.com/joshdick/onedark.vim/blob/main/colors/onedark.vim) and
  One Dark Pro's comment color.
- [Rosé Pine palette](https://rosepinetheme.com/palette/ingredients/).
- [Solarized](https://ethanschoonover.com/solarized/).
- Tokyo Night's official Alacritty exports:
  [night](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_night.toml),
  [storm](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_storm.toml),
  [moon](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_moon.toml),
  [day](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_day.toml).

Fuzzy file finder maps upstream colors to semantic UI roles. It uses each family's canvas,
text and signature accents for directories, focus, marks, status and the `@`
badge; syntax-highlighting rules are not reproduced. The numeric mappings live
in `src/palettes.rs`, with Fuzzy file finder's own palettes in `src/config.rs`.

Light variants keep upstream accents even where their contrast is lower; a test
keeps every built-in's text at 4:1 or better and other text roles at 2:1 or better. All
values are compiled into the binary; runtime palette selection needs no network.

## Base16 sources

The following palettes use [Tinted Theming definitions](https://github.com/tinted-theming/schemes/tree/spec-0.11/base16):

| Name | Author credited by source | Exact definition |
| --- | --- | --- |
| `ayu-dark` | Tinted Theming / Ayu Theme | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/ayu-dark.yaml) |
| `ayu-light` | Tinted Theming / Ayu Theme | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/ayu-light.yaml) |
| `ayu-mirage` | Tinted Theming / Ayu Theme | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/ayu-mirage.yaml) |
| `monokai` | Wimer Hazenberg | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/monokai.yaml) |
| `material-darker` | Nate Peterson | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/material-darker.yaml) |
| `zenburn` | elnawe | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/zenburn.yaml) |
| `ocean` | Chris Kempson | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/ocean.yaml) |
| `eighties` | Chris Kempson | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/eighties.yaml) |
| `ashes` | Jannik Siebert | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/ashes.yaml) |
| `atelier-dune` | Bram de Haan | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/atelier-dune.yaml) |
| `black-metal` | metalelf0 | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/black-metal.yaml) |
| `tomorrow-night` | Chris Kempson | [Base16 YAML](https://github.com/tinted-theming/schemes/blob/spec-0.11/base16/tomorrow-night.yaml) |

These are Fuzzy file finder mappings of those Base16 definitions. Background and panel use
base00, text base05, muted base04, borders and selection base02, accent/base badge
base0D, marks base0A, success base0B, error base08; badge text uses base00.
For readable selections, Atelier Dune uses base01. Ayu Light uses purple base0E
for marks because its yellow is too faint on the light canvas. Values remain
upstream colors. Every scheme participates in the existing text/selection
contrast checks, dynamic completion, live chooser, reload, and custom inheritance.
The source license is retained in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).
