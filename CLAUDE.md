@AGENTS.md

Rules for the curated examples directory (always in force):

@docs/tui_functionality_examples/AGENTS.md

## Orientation for Claude Code

User-facing behavior and keys: `README.md`. Config, palettes and Neovim editing:
`docs/CONFIGURATION.md`, `docs/PALETTES.md`, `docs/EDITING.md`. `reference_scripts/`
records historical intent only and is not a compatibility contract.

### Verification (run all before reporting a behavior change done)

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo build --offline            # terminal_smoke.py runs target/debug/picker
python3 tests/terminal_smoke.py  # Linux PTY: real Crossterm decoding, exact stdout bytes
```

The Neovim tests (`tests/nvim_editor.rs`, the nvim cases in the smoke suite) use the
installed `nvim` and skip when it is missing.

### Code map

- `src/lib.rs`: `App` session state, `Policy`, fuzzy `score`, ordering, output
  (`write_paths`, `relative_to`). Pure state with no terminal or filesystem timing.
- `src/discovery.rs`: cancellable background traversal that sends batches of `Event`s.
- `src/cache.rs`: bounded LRU discovery snapshots used to replay revisits (spec §17–18).
- `src/ui.rs`: the event loop on `/dev/tty`, key dispatch, and numbered-key decoding.
- `src/view.rs`: Ratatui rendering. It returns the exact numbered targets of each frame.
- `src/editor.rs`: the experimental clean Neovim engine over MessagePack-RPC (spec §19).
- `src/config.rs`, `src/palettes.rs`, `src/reload.rs`: TOML config, built-in
  palettes, and the live palette reload.
- `src/keys.rs`: `--map-key` remapping. `src/main.rs`: CLI (clap) and exit codes.
- `tests/acceptance.rs`: example-driven checks that inject discovery events.
  `tests/terminal_screen.py` is the screen model the PTY suite uses.

### Conventions

- Stdout carries only selected paths. All UI output goes to `/dev/tty`.
- Keep filesystem paths separate from display labels (`.../` abbreviations are
  for display only).
- When the user makes a new product decision, record it in a new numbered section
  of `docs/SPECIFICATION.md` (as in §14–19) and update `README.md`. Never edit the
  curated examples to fit an implementation.
