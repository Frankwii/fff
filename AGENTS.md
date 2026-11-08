# Working on fff

The current implementation is the behavior baseline; tests provide regression
coverage. Preserve existing behavior unless the user asks for a change.

The documentation has two layers. The user guides, [README.md](README.md)
and `docs/*.md`, teach people to use fff: task first, short, with examples.
The developer reference in `docs/dev/` describes the complete behavior,
edge cases included; read the relevant part first:

- [docs/dev/DEVELOPMENT.md](docs/dev/DEVELOPMENT.md): code map, invariants, where common changes go, verification, recording the demos.
- [docs/dev/USAGE.md](docs/dev/USAGE.md): the programs, browsing, matching, searching contents and the keys.
- [docs/dev/CONFIGURATION.md](docs/dev/CONFIGURATION.md): TOML settings, keys, discovery/cache behavior.
- [docs/dev/OPERATIONS.md](docs/dev/OPERATIONS.md): file operations, safety guarantees and archive safety.
- [docs/dev/EDITING.md](docs/dev/EDITING.md): Neovim integration and input routing.
- [docs/dev/DESKTOP.md](docs/dev/DESKTOP.md): `fff setup`, the desktop entry, Show in folder and the portal file chooser.
- [docs/dev/PALETTES.md](docs/dev/PALETTES.md): palette catalog and sources.
- [docs/dev/ROADMAP.md](docs/dev/ROADMAP.md): possible future work, not current requirements.

Before finishing a change, run the [verification](docs/dev/DEVELOPMENT.md#verification)
commands: `cargo fmt` (the code is kept formatted), `cargo test --offline`, `cargo clippy --offline --all-targets -- -D warnings`
and, after building, `python3 tests/terminal_smoke.py` for anything the terminal shows.
The stress tests are skipped by default because they are heavy; run them
(`cargo test --offline --lib stress -- --ignored`) only when a change touches
file operations, the jobs list, the trash or undo.

Documentation describes current behavior. When it disagrees with the code,
verify the behavior and correct it; when behavior changes, update the
reference in `docs/dev/` and, when users would notice, the user guide that
covers it (and the roadmap, if an item is done) rather than appending
chronological notes. User guides stay readable: what a person needs, not
every limit and edge case, which the reference keeps. Record user-visible changes in [CHANGELOG.md](CHANGELOG.md)
under "Unreleased", grouped under a single Added / Changed / Removed / Fixed
heading each: what changed for users, not a commit list. Commit subjects
likewise say what changed for users (see `git log`).

Keep stdout limited to what each program outputs (see [behavior to
preserve](docs/dev/DEVELOPMENT.md#behavior-to-preserve)), and keep original
filesystem paths separate from abbreviated display labels.

When adding or changing a CLI argument whose values make sense to complete
(paths, enumerations, palette or other names, key chords), wire up shell
completion (`value_hint`, a `ValueEnum`/possible values, or an
`ArgValueCompleter` in `src/completion.rs`) and keep the completion tests in
`src/main.rs` and `tests/cli.rs` passing. Free-form values such as numbers or
globs need no completion; list them in the `FREE_FORM` test constant in
`src/main.rs` instead.
