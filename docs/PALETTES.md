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

## Color sources

The numeric colors were checked against these upstream sources on 2026-11-01:

- [Catppuccin palette](https://catppuccin.com/palette/) and
  [official palette JSON](https://github.com/catppuccin/palette/blob/main/palette.json), version 1.8.0.
- [Nord's official colors and palettes](https://www.nordtheme.com/docs/colors-and-palettes/).
- [Gruvbox's original color definitions](https://github.com/morhetz/gruvbox/blob/master/colors/gruvbox.vim).
- Tokyo Night's official Alacritty exports:
  [night](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_night.toml),
  [storm](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_storm.toml),
  [moon](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_moon.toml),
  [day](https://github.com/folke/tokyonight.nvim/blob/main/extras/alacritty/tokyonight_day.toml).

Picker maps those upstream colors to its own UI roles; it does not reproduce an
editor's syntax-highlight rules. Catppuccin uses base/mantle/text, subtext0,
overlay0, blue/yellow/green/red, and surface0 for the main roles. Nord uses its
Polar Night backgrounds, Snow Storm text, Frost accents, and Aurora statuses.
Gruvbox uses its original dark/light backgrounds with bright/faded accents.
Tokyo Night uses exported primary, normal, and bright terminal colors. All
values are compiled into the binary; runtime palette selection needs no network.
