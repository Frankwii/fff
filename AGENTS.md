# Working on fff

The current implementation is the behavior baseline. Read [README.md](README.md)
and the relevant guides before making changes. When documentation disagrees with
code, verify the behavior and correct the documentation. Preserve existing behavior
unless the user requests a change; tests provide regression coverage.

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): code map, invariants and verification.
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md): TOML settings and discovery/cache behavior.
- [docs/OPERATIONS.md](docs/OPERATIONS.md): file operations, safety guarantees and archive safety.
- [docs/EDITING.md](docs/EDITING.md): Neovim integration and input routing.
- [docs/DESKTOP.md](docs/DESKTOP.md): `fff setup`, the desktop entry and the portal file chooser.
- [docs/PALETTES.md](docs/PALETTES.md): palette catalog and sources.
- [docs/ROADMAP.md](docs/ROADMAP.md): possible future work, not current requirements.

Before finishing a change, run the [verification](docs/DEVELOPMENT.md#verification)
commands: `cargo test --offline`, `cargo clippy --offline --all-targets -- -D warnings`
and, after building, `python3 tests/terminal_smoke.py` for anything the terminal shows.

Record user-visible changes in [CHANGELOG.md](CHANGELOG.md) as they are made,
under an "Unreleased" heading that becomes the version's section at release:
what changed for users, grouped under a single Added / Changed / Removed / Fixed
heading each, not a commit list. Commit subjects likewise say what changed for
users (see `git log`).

Keep documentation current when behavior changes. Update the relevant guide rather
than appending chronological decisions or maintaining a separate historical spec.
Keep stdout limited to selected paths (`fff pick`) or the chosen directory
(`fff roam` without `--cd-file`), and preserve original filesystem paths
independently of abbreviated display labels.

When adding or changing a CLI argument whose values make sense to complete
(paths, enumerations, palette or other names, structured values like key chords),
wire up shell completion (`value_hint`, a `ValueEnum`/possible values, or an
`ArgValueCompleter` in `src/completion.rs`) and keep the completion tests in
`src/main.rs` and `tests/cli.rs` passing. Free-form values such as numbers or
globs need no completion; list them in the `FREE_FORM` test constant in
`src/main.rs` instead.
