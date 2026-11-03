# Development

The implementation defines current behavior. The [README](../README.md) and user
guides describe it; tests preserve regression coverage. Possible extensions are
listed separately in the [roadmap](ROADMAP.md).

## Code map

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | CLI parsing, configuration setup, completion registration and exit codes. |
| `src/lib.rs` | Session state (`App`): navigation, tabs, rows and their ordering, the highlight, marks and path output. |
| `src/matching.rs` | Fuzzy scoring of names (nucleo, fzf's algorithm) and of paths below their roots, and the matched characters to highlight. |
| `src/policy.rs` | Hidden entries, exclusions and file filters (`Policy`), the kinds listed, and which found paths are rows. |
| `src/marks.rs` | Marked paths in order with constant-time membership. |
| `src/discovery.rs` | Cancellable breadth-first traversal, batches, backpressure, the path limit, freshness and scan reconciliation. |
| `src/grep.rs` | `fff grep`: the contents query, extraction into the text cache, the search on worker threads, and the rows it shows (one per match). |
| `src/ranking.rs` | Ranking rows from a snapshot in parallel pieces, cancellable, on another thread for large listings. |
| `src/known.rs` | The sharded index of known paths with each one's delivering scan, and freeing large listings off the input thread. |
| `src/cache.rs` | Bounded LRU snapshots and verification ages. |
| `src/bookmarks.rs` | Live bookmark forms and atomic, comment-preserving TOML persistence. |
| `src/prefetch.rs` | Bounded child-directory prefetch, cancellation, cooldown and mount eligibility. |
| `src/ui/mod.rs` | The session loop (`Session`): focus states, background polling, the editor's field, drawing and wakeups. |
| `src/ui/dispatch.rs` | One event's route: the running job's `cancel`, pastes, the mouse, then keys in the results and marked list through the keymap to a function (`list_key`, `run_function`), and other focuses' fixed keys (`act`). |
| `src/ui/focus.rs` | Each focus's own keys, one handler per `Focus` variant (`results_key`, `marks_key`, the sheets and choosers). |
| `src/ui/screen.rs` | Terminal lifecycle (enter, suspend for programs, resume) and the input reader. |
| `src/ui/input.rs` | The choosers' fixed Normal-mode keys; text fields (`focused_text` maps each focus to its Neovim field, for typing and paste alike); status timing, numbered keys. |
| `src/ui/configure.rs` | `gc`: the configuration in `$EDITOR`, its backup, and every setting applied to the running session. |
| `src/ui/actions.rs` | Opening and leaving with entries, file dialogs' saves, the scope choice, running programs, bookmark/glob/palette saves, dropped paths. |
| `src/ui/jobs.rs` | File operations as background jobs: checks before confirmations, progress, results, marks, undo records and their messages. |
| `src/ui/mouse.rs` | Clicks, the wheel and drags in the results and lists; the opt-in button bar. |
| `src/view/mod.rs` | Ratatui frame layout (`render`): the query box and its status, the location line, tab strip, notices, display paths. |
| `src/view/results.rs` | The results pane: rows, icons, detail columns, labels and matched letters. |
| `src/view/panes.rs`, `src/view/footer.rs` | The marked list and preview panes; the footer and the button bar. |
| `src/view/text.rs` | Edited text with Neovim's cursor and selection, prompts, wrapped messages. |
| `src/view/sheets.rs` | Operation sheets: confirmations, choices, rename/new/extract, run and go-to prompts. |
| `src/view/choosers.rs` | Bookmark chooser and form, icon picker, palette chooser. |
| `src/view/guide.rs`, `src/view/keys.rs` | The F1/F2 overlay: the Keys screen and the Session tab. |
| `src/preview.rs` | The preview pane: background reads after a rest, cancellation, file-type sniffing, helper programs and the kept previews. |
| `src/highlight.rs` | Syntax colors for text previews from a private headless Neovim (tree-sitter, else `:syntax`), on a thread of its own. |
| `src/details.rs` | Visible entries' details (lstat, link targets, owners) and entry counts, read by background workers, and their formats. |
| `src/watch.rs` | inotify on the current directory, pinned roots and listed folders for live updates. |
| `src/open.rs` | `[open]` rules, opener choice, detached background launches and terminal windows. |
| `src/notify.rs` | Pop-up notifications posted from background threads. |
| `src/location.rs` | The Alt+g prompt: typed paths, bookmarks and zoxide suggestions. |
| `src/run.rs` | gx/gX and E: shell-free word splitting, PATH resolution, Tab completion, running and exec. |
| `src/state.rs` | Per-directory highlights and the last session for `--resume`, saved atomically. |
| `src/operations.rs` | Validation, confirmation snapshots, staged file operations, rollback, deletion review, progress and stopping. |
| `src/trash.rs` | Trash bins per filesystem, their records (found again after a rename, for restoring), and the startup purge. |
| `src/undo.rs` | Undo records of operations and their all-or-nothing reversal (`operations::undo`). |
| `src/archive.rs` | Bounded libarchive reader; entry validation and private extraction staging. |
| `src/editor.rs` | Clean embedded Neovim through MessagePack-RPC. |
| `src/keymap.rs` | Functions, where they work, their default keys, scopes, conflicts and lock-out checks, and `keys.toml`. |
| `src/ui/keys.rs` | The Keys screen's state and editing: searches, adding keys, scopes, saving. |
| `src/config.rs`, `src/palettes.rs`, `src/reload.rs` | TOML configuration, semantic palettes and live color reload. |
| `src/explorer.rs` | `fff explorer`: file dialogs' starting place, query-line hint and answer. |
| `src/dbus.rs` | `fff dbus-file-manager`: a minimal D-Bus client answering `org.freedesktop.FileManager1` ("Show in folder") with the open-folder launcher. |
| `src/setup.rs`, `src/setup/` | `fff setup --desktop` and `--file-chooser`: the desktop entry, the D-Bus service file, the termfilechooser wrapper script and the configuration keys selecting them. |
| `src/completion.rs`, `src/icons.rs` | Dynamic shell completion and filetype glyphs. |

## Design choices

- **No image previews.** fff targets plain terminals such as st, where image
  protocols (kitty, sixel) are unavailable or poor. The [preview
  pane](CONFIGURATION.md#preview-pane) is text only: an image shows its size,
  and `l` (background opener) or `o` show the image itself. Its speed is
  non-negotiable: reads never block a frame or a highlight change, happen on
  their own threads after the highlight rests, and are cancelled when it
  moves. A read stuck on a slow mount cannot be interrupted, so at most 16
  run at once; past that, the pane says it waits for one to end. Syntax
  colors keep to the same rule: a read waits at most 50 ms for them before
  showing the text plain.

## Behavior to preserve

- UI output uses `/dev/tty`. Stdout carries only selected-path bytes
  (`fff pick`), the chosen directory (`fff roam` and `fff seek` without
  `--cd-file`) or a file dialog's answer (`fff explorer`), by the program
  the session ends as (`gm` switches; `Program::default_depth` holds each
  one's depth);
  programs started with gx/gX get `/dev/tty`, never fff's stdout. Relative
  output is based on the invocation directory, never the current browsing location.
- Exactly one current directory is searched alongside ordered pinned startup
  roots. Overlap attribution uses minimum positive depth, then scope order.
- Sort by depth, directories before files, fuzzy score, root order, then original
  absolute pathname bytes; `fff pick` and `fff seek` put fuzzy score before depth. Query edits do not restart discovery. Scans merge
  new paths into the ranked rows within a time budget, in place and with the
  rows' kept scores: rows ranked before every new path are not touched, so a
  breadth-first scan's merge costs about what it inserts. The input thread never
  re-sorts every known path, and discovery work yields to keys waiting to be
  read. Large re-rankings run on another thread; every key except typing into
  the query and Ctrl+c waits for a pending one, so no action uses stale rows.
- Marks are unique, ordered and independent of navigation, policies and filters.
  Ctrl+number is Enter on its row and Alt+number is `l` on it; both address
  the last fully rendered visible labels.
- The input reader hands every handler a shifted letter as legacy terminals
  send it, uppercase with Shift, whatever the kitty protocol reported, so
  typing and key bindings agree everywhere (`keymap::Key::press` folds the
  two encodings).
- Escape preserves the session (twice with an empty query clears the marks,
  nothing more). In Neovim it first exits transient editing modes;
  Ctrl+c cancels with no selected-path output.
- Drawing never touches the filesystem: what a frame shows about an entry
  (details, `[MISSING]`, a folder's slash, a bookmark's availability) comes
  from `Details`, whose workers read it in the background, one set per mount
  so that a stuck mount holds up only its own entries; until they answer,
  the frame shows what they found before. Owner names, possibly from a
  network directory, are looked up in the background too.
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
- Operations and their preparation run as jobs on their own thread; the
  session only polls them, draws their progress and hears Ctrl+c, which the
  engine honors only while stopping changes nothing (checking, staging) or
  puts the rest back (between deleted entries). Every change an operation
  makes is recorded for undo, and undo checks identities before acting.
- Entries go to the trash by rename only, into the bin on their own mount and
  device; the purge deletes only dated records older than `[trash] days`.

## Common changes

Where a change usually has to touch, so that none is missed:

- **Something a key does in the results or the marked list:** a `Function`
  in `src/keymap.rs` (name, group, description in `functions!`), where it
  works (`available`, a design choice per mode), what it runs (`exec`: the
  default key the focus's handlers take, such as an arm in `results_key`
  in `src/ui/focus.rs`, or a session action in `run_function` in
  `src/ui/dispatch.rs`) and its default keys (`defaults`). The tests check
  that defaults never clash and each applies somewhere. Then the README key
  table, [EDITING.md](EDITING.md) and a PTY test.
- **A key named on screen** (hints, messages): never as a literal. Name a
  function's key through `App::bound(Function::Preview)` (the one that
  applies here, in the current mode first; `None` when it has none) or
  `App::bound_in` for one mode; fixed keys of sheets and choosers take
  plain names (`("enter", "Delete")`, `App::key("esc")`).
- **A sheet, prompt or chooser:** a `Focus` variant (`src/ui/mod.rs`), its
  drawing in `render` (`src/view/mod.rs`, with `src/view/sheets.rs` or
  `choosers.rs`), its keys in a handler called from `focus_key`
  (`src/ui/focus.rs`) and, for a text field, a `Field` (`src/editor.rs`)
  mapped in `focused_text` (`src/ui/input.rs`). A `view` frame test pins its
  layout.
- **A file operation:** a `Request` variant and its method after `prepare`
  (`src/operations.rs`), undo steps (`src/undo.rs`), a plan or confirmation
  started as a job from `src/ui/actions.rs`, and the
  [OPERATIONS.md](OPERATIONS.md) guarantees it keeps.
- **A setting:** the struct, default and validation in `src/config.rs`, how
  `gc` applies it to a running session (`apply_settings` in
  `src/ui/configure.rs`),
  [CONFIGURATION.md](CONFIGURATION.md), `docs/config.example.toml`, and a
  test in `tests/configuration.rs`.
- **A command-line argument:** `Args` in `src/main.rs` and its completion
  (see [AGENTS.md](../AGENTS.md)).

Run one test while iterating: `cargo test --offline --lib NAME` or
`python3 tests/terminal_smoke.py TerminalTests.test_NAME` (after `cargo
build`). `src/view/tests.rs` draws frames at a given size (`draw(&mut app,
120, 26)`), which is much faster than a PTY test for layout.

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
the trash, undo, purge, progress and stopping (`src/trash_tests.rs`, with
per-filesystem bins on tmpfs mounts in a namespace),
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
