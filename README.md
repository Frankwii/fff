# picker

A planned Rust terminal file picker inspired by Doom Emacs's directory-oriented,
fuzzy file completion and the scripts in `reference_scripts/`.

This repository currently contains only a Cargo binary scaffold. Picker behavior
has not been implemented, and no third-party dependencies have been chosen.

Start with [the program specification](docs/SPECIFICATION.md) for the consolidated
behavior, source precedence, acceptance-source map, and unresolved decisions.
Future agents should also read [AGENTS.md](AGENTS.md).

## Project direction

- Fast, robust, interactive terminal operation with persistent session state.
- A visible current directory, search depth, search roots, and marked paths.
- One filename view: immediate children at depth 1, recursive discovery at greater
  depths, with live depth adjustment. The proposed startup depth is 3.
- Fuzzy filtering independent of filesystem discovery.
- Ordering by depth, directories before files within a depth, fuzzy score,
  startup root order, then original absolute pathname bytes.
- Fixed startup search roots, overlap deduplication using minimum relative depth,
  and marked selections independent of roots and current results.
- Numbered Ctrl+digit shortcuts immediately return a visible path and exit,
  using the most recently rendered labels; actual terminal decoding needs validation.
- Escape clears a query or leaves a subordinate view without destroying the session.
- Invocation-time output settings for absolute or invocation-relative paths and
  script-friendly delimiters, including NUL.

The proposed stack is Ratatui + Crossterm, Nucleo for fuzzy matching, and filesystem
discovery using `std::fs` and/or `ignore`. This is a design direction, not an
implemented dependency choice.

## Requirements and references

Read `tui_functionality_examples/AGENTS.md`, its `README.md`, and all active examples
before implementation. Active examples take precedence over `KEYBINDINGS.md`;
explicit user instructions take precedence over both. Examples marked
`STATUS: Delayed until future versions` are outside the current scope. Currently
these are 06 (content search), 09 (opening and resuming), and 11 (additional
filename/error scenarios).

The shell scripts are historical references, not an implementation specification.
Preserve their useful intent without reproducing process-restart state management
or shell pathname parsing.

Open decisions and source inconsistencies are tracked in section 12 of the
specification. Do not silently resolve them by copying older transcript proposals.

## Scaffold validation

```sh
cargo check --offline
```
