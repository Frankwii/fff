# Development

The implementation defines current behavior. The [README](../README.md) and user
guides describe it; tests preserve regression coverage. Possible extensions are
listed separately in the [roadmap](ROADMAP.md).

## Code map

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | CLI parsing, configuration setup, completion registration and exit codes. |
| `src/lib.rs` | Session state, navigation, tabs, policies, matching, ordering, marks and path output. |
| `src/marks.rs` | Marked paths in order with constant-time membership. |
| `src/discovery.rs` | Cancellable breadth-first traversal, batches, freshness and scan reconciliation. |
| `src/cache.rs` | Bounded LRU snapshots and verification ages. |
| `src/bookmarks.rs` | Live bookmark forms and atomic, comment-preserving TOML persistence. |
| `src/prefetch.rs` | Bounded child-directory prefetch, cancellation, cooldown and mount eligibility. |
| `src/ui/mod.rs` | The session: focus states, wakeups and key dispatch. |
| `src/ui/screen.rs` | Terminal lifecycle (enter, suspend for programs, resume) and the input reader. |
| `src/ui/input.rs` | Normal-mode list dispatch, text-field editing and paste, status timing, numbered keys. |
| `src/ui/actions.rs` | Scope choice, file operations, running programs and their messages, bookmark/palette saves, the mouse. |
| `src/view/mod.rs` | Ratatui frame layout: query, location line, results and tab strip, marks, footer, display paths. |
| `src/view/sheets.rs` | Operation sheets: confirmations, choices, rename/new/extract, run and go-to prompts. |
| `src/view/choosers.rs` | Bookmark chooser and form, icon picker, palette chooser. |
| `src/view/guide.rs` | The F1 key guide and session details. |
| `src/preview.rs` | The preview pane: background reads after a rest, cancellation, file-type sniffing, helper programs and the kept previews. |
| `src/details.rs` | Visible rows' details (lstat, link targets, owners), background entry counts and their formats. |
| `src/watch.rs` | inotify on the current directory for live updates. |
| `src/open.rs` | `[open]` rules, opener choice, detached background launches and terminal windows. |
| `src/notify.rs` | Pop-up notifications posted from background threads. |
| `src/location.rs` | The Alt+g prompt: typed paths, bookmarks and zoxide suggestions. |
| `src/run.rs` | gx/gX and E: shell-free word splitting, PATH resolution, Tab completion, running and exec. |
| `src/state.rs` | Per-directory highlights and the last session for `--resume`, saved atomically. |
| `src/operations.rs` | Validation, confirmation snapshots, staged file operations, rollback and deletion review. |
| `src/archive.rs` | Bounded libarchive reader; entry validation and private extraction staging. |
| `src/editor.rs` | Clean embedded Neovim through MessagePack-RPC. |
| `src/normal_keys.rs`, `src/keys.rs` | Normal-mode action sequences and CLI chord remapping. |
| `src/config.rs`, `src/palettes.rs`, `src/reload.rs` | TOML configuration, semantic palettes and live color reload. |
| `src/explorer.rs` | `fff explorer`: file dialogs' starting place, query-line hint and answer. |
| `src/setup.rs`, `src/setup/` | `fff setup --desktop` and `--file-chooser`: the desktop entry, the termfilechooser wrapper script and the configuration keys selecting them. |
| `src/completion.rs`, `src/icons.rs` | Dynamic shell completion and filetype glyphs. |

## Design choices

- **No image previews.** fff targets plain terminals such as st, where image
  protocols (kitty, sixel) are unavailable or poor. The [preview
  pane](CONFIGURATION.md#preview-pane) is text only: an image shows its size,
  and `l` (background opener) or `o` show the image itself. Its speed is
  non-negotiable: reads never block a frame or a highlight change, happen on
  their own threads after the highlight rests, and are cancelled when it
  moves.

## Behavior to preserve

- UI output uses `/dev/tty`. Stdout carries only selected-path bytes
  (`fff pick`) or the chosen directory (`fff roam` without `--cd-file`);
  programs started with gx/gX get `/dev/tty`, never fff's stdout. Relative
  output is based on the invocation directory, never the current browsing location.
- Exactly one current directory is searched alongside ordered pinned startup
  roots. Overlap attribution uses minimum positive depth, then scope order.
- Sort by depth, directories before files, fuzzy score, root order, then original
  absolute pathname bytes. Query edits do not restart discovery.
- Marks are unique, ordered and independent of navigation, policies and filters.
  Ctrl+number is Enter on its row and Alt+number is `l` on it; both address
  the last fully rendered visible labels.
- Escape preserves the session. In Neovim it first exits transient editing modes;
  Ctrl+c cancels with no selected-path output.
- Generation checks and cancellation prevent obsolete work from changing the view.
  Reuse recomputes attribution. Partial or truncated snapshots cannot establish
  completeness, and cached empty directories require verification before Enter.
- Verification age survives revisits and checkpoints. F5 bypasses replay. Prefetch
  stays bounded and cannot evict visited snapshots or spawn successors behind a
  stuck speculative worker. See [cache settings](CONFIGURATION.md#discovery-cache-and-performance).
- Bookmark writes occur only on explicit save/delete. Preserve unrelated config,
  symlinks and permissions; refuse conflicting bookmark edits. `--no-config`
  stays session-only. `tests/bookmarks.rs` covers persistence and failures.
- Original paths remain separate from display abbreviations and icons. Keep
  palette attribution and [third-party notices](THIRD_PARTY_NOTICES.md).

- File operations must pass through `Engine`; no UI branch may mutate a user
  path directly. Validate the full batch, bind destructive steps to confirmed
  snapshots, stage, then publish with no-replace semantics, rolling back on
  failure. See [file operations](OPERATIONS.md).

## Verification

For behavior changes:

```sh
cargo test --offline
cargo clippy --offline --all-targets -- -D warnings
cargo build --offline
python3 tests/terminal_smoke.py
```

`src/operations_tests.rs` covers collisions, confirmation binding, injected
races and rollback, mount points (bind mounts in a user namespace via `unshare`),
deferred signals, source changes, permissions, byte paths, links, malicious archives and
archive limits. PTY tests cover Normal-only operation keys, prompts, save/discard
and mark reconciliation. Some safety tests create temporary Unix sockets and
need an environment that permits them.

The Rust tests cover state transitions, discovery, caching, configuration,
completion, frames and exact output bytes. `tests/acceptance.rs` injects decoded
input and discovery events; `tests/discovery_cache.rs` covers filesystem fixtures
and snapshot reconciliation. Neovim tests use the installed engine and skip when
it is unavailable.

The Linux PTY suite runs `target/debug/fff`; `tests/terminal_screen.py` models
its committed frames. It checks real Crossterm decoding, modes, navigation,
policies, marks, paging, cancellation and separate stdout. It does not establish
a physical keyboard/terminal compatibility matrix.

For navigation performance:

```sh
cargo build --offline --release
python3 tests/navigation_benchmark.py --binary target/release/fff
```

This measures first in-app visits at depth 1 with warm OS metadata, Neovim editing
and no icons. Report the workload and build mode alongside latency or memory
measurements. Slow storage and large trees still require progressive updates.
