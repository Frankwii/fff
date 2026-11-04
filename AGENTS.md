# Working on fff

The current implementation is the behavior baseline; tests provide regression
coverage. Preserve existing behavior unless the user asks for a change. Read
[README.md](README.md) and the relevant guide first:

- [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md): code map, invariants, where common changes go, verification.
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md): TOML settings, keys, discovery/cache behavior.
- [docs/OPERATIONS.md](docs/OPERATIONS.md): file operations, safety guarantees and archive safety.
- [docs/EDITING.md](docs/EDITING.md): Neovim integration and input routing.
- [docs/DESKTOP.md](docs/DESKTOP.md): `fff setup`, the desktop entry, Show in folder and the portal file chooser.
- [docs/PALETTES.md](docs/PALETTES.md): palette catalog and sources.
- [docs/ROADMAP.md](docs/ROADMAP.md): possible future work, not current requirements.

Before finishing a change, run the [verification](docs/DEVELOPMENT.md#verification)
commands: `cargo fmt` (the code is kept formatted), `cargo test --offline`, `cargo clippy --offline --all-targets -- -D warnings`
and, after building, `python3 tests/terminal_smoke.py` for anything the terminal shows.
The stress tests are skipped by default because they are heavy; run them
(`cargo test --offline --lib stress -- --ignored`) only when a change touches
file operations, the jobs list, the trash or undo.

Documentation describes current behavior. When it disagrees with the code,
verify the behavior and correct it; when behavior changes, update the relevant
guide (and the roadmap, if an item is done) rather than appending
chronological notes. Record user-visible changes in [CHANGELOG.md](CHANGELOG.md)
under "Unreleased", grouped under a single Added / Changed / Removed / Fixed
heading each: what changed for users, not a commit list. Commit subjects
likewise say what changed for users (see `git log`).

Keep stdout limited to what each program outputs (see [behavior to
preserve](docs/DEVELOPMENT.md#behavior-to-preserve)), and keep original
filesystem paths separate from abbreviated display labels.

When adding or changing a CLI argument whose values make sense to complete
(paths, enumerations, palette or other names, key chords), wire up shell
completion (`value_hint`, a `ValueEnum`/possible values, or an
`ArgValueCompleter` in `src/completion.rs`) and keep the completion tests in
`src/main.rs` and `tests/cli.rs` passing. Free-form values such as numbers or
globs need no completion; list them in the `FREE_FORM` test constant in
`src/main.rs` instead.
