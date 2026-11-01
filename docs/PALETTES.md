# Built-in palettes and live reload

Set `[ui].palette` in your TOML file, or pass `--palette NAME`:

```toml
[ui]
palette = "catppuccin-mocha"
```

| Family | Names |
| --- | --- |
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
| Picker originals | `midnight`, `light`, `forest` |

Names are lowercase and use dashes. Catppuccin is spelled `catppuccin`; Frappé's
identifier is ASCII `frappe`. Any built-in can be the `extends` base of a custom
palette. See [configuration](CONFIGURATION.md) for the semantic color roles.

When a config file is enabled, saving changes to its palette selection or color
roles updates the running interface automatically, normally within 500 ms. File
reading and parsing happen in a background thread. Invalid intermediate edits
keep the last working colors and show an error; correcting the file recovers
automatically. Atomic-save/rename workflows work too. `--palette` pins the chosen
name but still reloads custom colors for that name. `--no-config` disables file
reload. This reload applies to colors only; bookmarks/cache settings need a new
invocation. Query, cursor, marks, discovery, and numbered targets are preserved.

## Switching palettes while running

| Key | Action |
| --- | --- |
| Alt+p | Open the palette chooser: every built-in in the order above, then the config file's custom palettes. Type to fuzzy-filter names; moving the highlight previews it across the whole interface. Enter applies it for this session; Escape (or Alt+p again) restores the previous colors. |
| Alt+P | Re-read the config file now and apply its palette, discarding any Alt+p session choice. With `--palette NAME`, re-read that name's definition instead. |

The chooser never writes the config file. After Enter, background reload keeps
following the *chosen* palette's definition (editing `[palettes.NAME]` still
updates it) and ignores `[ui].palette` until Alt+P returns control to the file.
If that re-read fails, the current colors stay and the error is shown; the
session choice is still cleared, so fixing the file recovers automatically.
Alt+P works with `--no-config` too (it restores the default or pinned palette).
Both chords can be remapped, e.g. `--map-key ctrl+t=alt+p --map-key ctrl+r=alt+P`.

## Color sources

The numeric colors were checked against these upstream sources on 2026-11-01:

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

Picker maps those upstream colors to its own UI roles; it does not reproduce an
editor's syntax-highlight rules. Catppuccin uses base/mantle/text, subtext0,
overlay0, blue/yellow/green/red, and surface0 for the main roles. Nord uses its
Polar Night backgrounds, Snow Storm text, Frost accents, and Aurora statuses.
Gruvbox follows how the Vim/Neovim colorscheme *feels* rather than its first
palette slots: a flat `bg0` canvas (`#282828` / `#fbf1c7`) with `bg2`/`bg3`
borders and the gray comment color (`#928374`) for secondary text. Orange — bright
`#fe8019` in dark, faded `#af3a03` in light — is the accent: focused borders,
directories, the highlight caret and the current-directory badge, with yellow for
marks and bright/faded green and red for status.
Tokyo Night uses exported primary, normal, and bright terminal colors.

The families added later follow the same flat-canvas approach, each with its
signature accent on directories, focus and the `@` badge:

- **Dracula**: Background/Current Line/Comment, purple accent, pink marks.
- **Everforest** (medium): `bg0`, `grey1` muted, `bg4` borders, green accent,
  yellow marks, aqua success.
- **Kanagawa**: wave uses sumiInk3, fujiWhite/fujiGray, crystalBlue, carpYellow and
  waveBlue2 selection; dragon uses dragonBlack3, dragonBlue2 and a dragonOrange badge.
- **One Dark**: `#282c34`, blue accent, purple badge, and the One Dark Pro comment
  gray `#7f848e` (the original `#5c6370` is too dim for secondary text).
- **Rosé Pine**: base, subtle, highlight med/high, rose accent, gold marks, foam
  (pine in dawn) success, love errors.
- **Solarized**: base03/base3 canvases, blue accent, yellow marks, green/red status.

Light variants keep upstream accents even where their contrast is lower; a test
keeps every built-in's text at 4:1 or better and other text roles at 2:1 or better. All
values are compiled into the binary; runtime palette selection needs no network.
