# Working on fff

The current implementation is the behavior baseline. Read [README.md](README.md)
and the relevant guides before making changes. When documentation disagrees with
code, verify the behavior and correct the documentation. Preserve existing behavior
unless the user requests a change; tests provide regression coverage.

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): code map, invariants and verification.
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md): TOML settings and discovery/cache behavior.
- [docs/EDITING.md](docs/EDITING.md): Neovim integration and input routing.
- [docs/PALETTES.md](docs/PALETTES.md): palette catalog and sources.
- [docs/ROADMAP.md](docs/ROADMAP.md): possible future work, not current requirements.

Keep documentation current when behavior changes. Update the relevant guide rather
than appending chronological decisions or maintaining a separate historical spec.
Keep stdout limited to selected paths and preserve original filesystem paths
independently of abbreviated display labels.

When adding or changing a CLI argument whose values make sense to complete
(paths, enumerations, palette or other names, structured values like key chords),
wire up shell completion (`value_hint`, a `ValueEnum`/possible values, or an
`ArgValueCompleter` in `src/completion.rs`) and keep the completion tests in
`src/main.rs` and `tests/cli.rs` passing. Free-form values such as numbers or
globs need no completion; list them in `FREE_FORM` in `src/main.rs` instead.
