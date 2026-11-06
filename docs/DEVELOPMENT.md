# Development

The implementation defines current behavior. The [README](../README.md) and user
guides describe it; tests preserve regression coverage. Possible extensions are
listed separately in the [roadmap](ROADMAP.md).

## Code map

| Module | Responsibility |
| --- | --- |
| `src/main.rs` | CLI parsing, configuration setup, completion registration and exit codes. |
| `src/lib.rs` | Session state (`App`): navigation, tabs, when rows are ranked again or merged (`refresh`), the rows on screen (`shown`), the highlight, marks and path output. |
| `src/matching.rs` | Fuzzy scoring of names (nucleo, fzf's algorithm) and of paths below their roots, and the matched characters to highlight. |
| `src/policy.rs` | Hidden entries, exclusions and file filters (`Policy`), the kinds listed, and which found paths are rows. |
| `src/marks.rs` | Marked paths in order with constant-time membership. |
| `src/discovery.rs` | Cancellable breadth-first traversal, each depth's folders read on several threads and its paths attributed to their roots there, batches, backpressure, the path limit, freshness and scan reconciliation. |
| `src/grep.rs` | The contents query (the `App::grep` field, in every program): extraction into the text cache, the search on worker threads, and the rows it shows (one per match). |
| `src/index.rs` | The contents index: trigram filters of the texts searched, by file identity, folded as the query matches; its segments, merging, and the pruning of it and the text cache. |
| `src/rows.rs` | The results' order (`Order`: the program's `Rank` or a chosen `Sort`), each row's key (`Key`: its score and a sort's value read from the filesystem), natural name comparison, and the ranked rows (`Rows`): sorted runs merged lazily, and the one way to read every row in order (`Rows::all`). |
| `src/ranking.rs` | Ranking rows from a snapshot in parallel pieces, cancellable, on another thread for large listings. |
| `src/known.rs` | The sharded index of known paths with each one's delivering scan, and freeing large listings off the input thread. |
| `src/cache.rs` | Bounded LRU snapshots and verification ages. |
| `src/workspaces.rs` | Live workspace forms (the settings ticked to keep) and atomic, comment-preserving TOML persistence. A workspace and its settings are `config::Workspace` and `config::Setting`; `App::apply_workspace` opens one, `App::snapshot` is what a new one can keep. |
| `src/prefetch.rs` | Bounded child-directory prefetch, cancellation, cooldown and mount eligibility. |
| `src/ui/mod.rs` | The session loop (`Session`): focus states, background polling, the editor's field, drawing and wakeups. |
| `src/ui/dispatch.rs` | One event's route: the running job's `cancel`, pastes, the mouse, then keys in the results and marked list through the keymap to a function (`list_key`), what each function does (`run_function`), and other focuses' fixed keys (`act`). |
| `src/ui/focus.rs` | Each focus's own keys, one handler per `Focus` variant (the sheets and choosers); Enter in the lists (`enter`: the function Enter has there by default, for the mouse and `confirm_row`), the entry Enter's functions act on (`chosen`) and what each does with it (`finish_with_entry`), and the larger functions' code. |
| `src/ui/screen.rs` | Terminal lifecycle (enter, suspend for programs, resume) and the input reader. |
| `src/ui/input.rs` | The choosers' fixed Normal-mode keys; text fields (`focused_text` maps each focus to its Neovim field, for typing and paste alike); status timing, numbered keys. |
| `src/ui/configure.rs` | `gc`: the configuration in `$EDITOR`, its backup, and every setting applied to the running session. |
| `src/ui/actions.rs` | Opening and leaving with entries, file dialogs' saves, the scope choice, running programs, workspace/glob/palette saves, dropped paths. |
| `src/ui/jobs.rs` | File operations as background jobs: checks before confirmations (modal), running operations in the jobs list (`App::jobs`) with the paths each reads and changes (`Claim`, refusing collisions), stopping and undoing them, results, marks, undo records and their messages. |
| `src/ui/mouse.rs` | Clicks, the wheel and drags in the results and lists; the opt-in button bar. |
| `src/view/mod.rs` | Ratatui frame layout (`render`): the query box and its status, the location line, tab strip, notices, display paths. |
| `src/view/results.rs` | The results pane: rows, icons, detail columns, labels and matched letters. |
| `src/view/panes.rs`, `src/view/footer.rs` | The marked list and preview panes; the footer and the button bar. |
| `src/view/text.rs` | Edited text with Neovim's cursor and selection, prompts, wrapped messages. |
| `src/view/sheets.rs` | Operation sheets: confirmations, choices, rename/new/extract, run and go-to prompts. |
| `src/view/choosers.rs` | Workspace chooser and form, icon picker, palette chooser. |
| `src/view/guide.rs` | F2's Session tab. |
| `src/view/keys.rs` | The Keys screen (F1), drawn in place of the results, and its editor sheet. |
| `src/preview.rs` | The preview pane: background reads after a rest, cancellation, file-type sniffing, helper programs and the kept previews. |
| `src/highlight.rs` | Syntax colors for text previews from a private headless Neovim (tree-sitter, else `:syntax`), on a thread of its own. |
| `src/details.rs` | Visible entries' details (lstat, link targets, owners) and entry counts, read by background workers, and their formats. |
| `src/sizes.rs` | Folders' sizes: the files below each added up by a worker per mount, the folders a frame wants first (`Sizes::commit` after each drawn frame), read on several threads. A tree of folders with their own files' bytes and stamps (inode, mtime, ctime), saved to `$XDG_CACHE_HOME/fff/sizes` at exit and read at the first frame; only folders whose stamp changed (or that a watch or F5 marked) are read again. |
| `src/mounts.rs` | The mount table (`/proc/self/mounts`), read by name and again after each scan: a path's mount point and filesystem kind (kernel, local, other). Kernel filesystems are not added up, entered by scans from outside, searched, watched, or read past 4 KiB for the preview. |
| `src/watch.rs` | inotify on the current directory, pinned roots and listed folders for live updates. |
| `src/open.rs` | `[open]` rules, opener choice, detached background launches and terminal windows. |
| `src/notify.rs` | Pop-up notifications posted from background threads. |
| `src/location.rs` | The Alt+g prompt: typed paths, workspaces and zoxide suggestions. |
| `src/run.rs` | gx/gX and E: shell-free word splitting, PATH resolution, Tab completion, running and exec. |
| `src/state.rs` | Per-directory highlights, saved atomically. |
| `src/operations.rs` | Validation, confirmation snapshots, staged file operations, rollback, deletion review, progress and stopping. |
| `src/trash.rs` | Trash bins per filesystem, their records (found again after a rename, for restoring), and the startup purge. |
| `src/undo.rs` | Undo records of operations and their all-or-nothing reversal (`operations::undo`). |
| `src/archive.rs` | Bounded libarchive reader; entry validation and private extraction staging. |
| `src/editor.rs` | Clean embedded Neovim through MessagePack-RPC. |
| `src/program.rs` | The program a session runs as: how programs differ (`Program`: what each lists, prints and starts in, its depth and labels) and what only one keeps (`Running`: a file dialog), swapped by `App::set_program`. |
| `src/keymap.rs` | Functions, where they work (programs are one of its dimensions), their default keys, scopes, conflicts and lock-out checks, and `keys.toml`. |
| `src/ui/keys.rs` | The Keys screen's state and keys: the picker (your own list keys through the keymap), key searches, and the editor's staged changes, scopes and saving. |
| `src/config.rs`, `src/palettes.rs`, `src/reload.rs` | TOML configuration, semantic palettes and live color reload. |
| `src/explorer.rs` | `fff explorer`: file dialogs' starting place, query-line hint and answer. |
| `src/dbus.rs` | `fff dbus-file-manager`: a minimal D-Bus client answering `org.freedesktop.FileManager1` ("Show in folder") with the open-folder launcher. |
| `src/setup.rs`, `src/setup/` | `fff setup --desktop` and `--browser`: the desktop entry, the D-Bus service file, the termfilechooser wrapper script and the configuration keys selecting them. |
| `src/completion.rs`, `src/icons.rs` | Dynamic shell completion and filetype glyphs. |
| `src/ui/tests.rs`, `src/view/tests.rs`, `src/operations_tests.rs`, `src/trash_tests.rs` | Unit tests of the session's keys, of drawn frames, and of the file operations and trash (see [verification](#verification)). |

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

## How rows are kept and ordered

The results are the known paths a query matches (`App::known`, every path
scans delivered), ranked in one order. `src/rows.rs` holds that order and
the rows; `App::refresh` decides when they are ranked again or merged.

- **The order** is one value, `rows::Order`, which every comparison of rows
  goes through (`Order::cmp`, on a row and its key). It is the program's
  ranking (`Program::rank`): `Rank::Depth` (depth, folders first, best
  match, root order, then the path's bytes, which break every tie) or
  `Rank::Score` (`fff pick` and `fff seek`: best match first, then as
  `Depth`); with a sort chosen in the sorting picker (`App::sort`, `gs`,
  `[ui] sort`), the `Sort`'s criterion first and the ranking for its ties.
  `App::order` puts the two together. The order is part of what the rows
  were ranked for (`RefreshBasis`), so another order ranks them again.
- **Keys.** Each row is ranked by a `rows::Key`: its match score and, for
  a sort by size, dates or kind, a value read from the filesystem (an
  `lstat`; a folder's size from the totals `sizes::Totals` holds, which
  sort last until added up). `Order::key` reads it when a row is scored,
  in `Job::rank_piece` and `App::merge`, so comparisons never touch the
  filesystem; rows ranked again for a longer query keep their values.
  Names, extensions and depth need no value. `App::set_sort` ranks every
  known path again, reading the values afresh.
- **The rows** (`rows::Rows`) are sorted runs (`Run`: rows and their scores,
  in order), each sorted on its own. A ranking produces one run. A scan's
  new paths are filtered, scored and sorted in rounds (`App::merge`), and
  each round is pushed as a run of its own: no row already ranked moves.
  Runs are merged pairwise as they are pushed only while the one before is
  not more than twice as large, so there are about log₂ of the rows over
  the round size of them (about ten for `/usr`), the largest first, and each
  row is moved once per doubling (a merge takes whole stretches of a run
  that rank before the other's next row, found by searching, rather than
  comparing row by row).
- **Lazy merging.** The first `rows::LAZY` rows (2048) are kept merged
  as runs are pushed (`Rows::head`: the new run's first rows merged into
  them, when any rank before their last), so reading them (`Rows::row`,
  `iter_from`, `position`) costs what reading a slice does: what a frame
  draws (the rows in view, those a page above and below, the highlight) is
  there. Reading past them, or every row, merges every run once
  (`Rows::all`); from then on the rows stay merged, each new round inserted
  among them in place (`rows::insert`, moving the rows after it, as before
  runs), until a ranking replaces them. Reading past the last row merges
  nothing.
- **Every row in order** goes through one function: `App::shown` (the rows,
  or a contents query's matches), which calls `Rows::all`. `Rows` also
  dereferences to that slice (so `app.rows[0]` and `app.rows.iter()` read
  the full order); nothing can read a half-merged list. The count
  (`App::shown_len`, `Rows::len`) needs no merging. What reads every row,
  or rows past the screen, in order:
  - `G` and `gg` (`move_by` to an end), `PageDown` near the end (the offset
    clamped to the count), the wheel's clamp, single steps that wrap;
  - `m`/`M` and batch rename `r` (`shown_paths`), the marks they make and
    so what is printed;
  - Alt/Ctrl+number (`highlight_numbered`), through the drawn rows and the
    highlight;
  - the mouse: clicks (`highlight_row`), drags and the drag held at an edge
    (`ui/mouse.rs` `drag`, over `shown`);
  - the contents query, which queues the listed files in order and lists
    its matches by them (`poll_grep`);
  - the discovery cache's snapshot of the first rows (`Cache::capture`) and
    the rows a new scan replays (`Discovery::restart`);
  - the F2 Session tab's highlighted path, the highlight's index
    (`highlighted`, by a hint, then by search) and `move_past`.
- **Kept queries** (`earlier`) keep a clone of the rows (shared runs) with
  their scores, so that returning to one restores its rows and later merges
  rank against their kept scores. A change to the listing drops them.
- **Rankings on another thread** (`ranking.rs`) score a snapshot of the
  rows or of every known path (`Job::base`, shared runs) in parallel
  pieces, sort each and merge them there into the one run they hand back.

Where the features of the [roadmap](ROADMAP.md#post-launch) plug in:

- **Another sort criterion**: a `rows::Criterion` with its comparison in
  `Sort::cmp` and, when it needs the filesystem, its value in `value`.
- **A tree view**: an order comparing paths by their components (a folder
  before its contents, siblings by name), with the listing also keeping the
  folders leading to each match (`Listing::listed`, `App::listed`).
- **Folders as matches**: what a row matches and how well is
  `Matcher::score_candidate` (`src/matching.rs`), called by `Job::rank_piece`
  and `App::merge`; the order then sorts by that score.

Risks this design carries: once anything reads past the first `LAZY` rows
(the view scrolled that far, the highlight there, `G`, a contents query, a
mouse drag), the rest of the scan merges as it did before runs, no slower
and no faster. The first `LAZY` rows are copied into `head` and merged
again with each round that ranks among them. `Rows::all` copies the row
pointers when there is more than one run. Kept queries and pending rankings
share runs, so merging a run they share copies it, as it did before.

## Behavior to preserve

- UI output uses `/dev/tty`. Stdout carries only selected-path bytes
  (`fff pick`: `Program::prints_paths`), the chosen directory (`fff roam` and `fff seek` without
  `--cd-file`) or a file dialog's answer (`fff explorer`), by the program
  the session ends as (`gm` switches; `Program::default_depth` holds each
  one's depth);
  programs started with gx/gX get `/dev/tty`, never fff's stdout. Relative
  output is based on the invocation directory, never the current browsing location.
- Exactly one current directory is searched alongside ordered pinned startup
  roots. Overlap attribution uses minimum positive depth, then scope order.
- Sort by depth, directories before files, fuzzy score, root order, then original
  absolute pathname bytes; `fff pick` and `fff seek` put fuzzy score before
  depth (`Program::rank`); a sort from the sorting picker comes before
  either, which then breaks its ties (see [how rows are kept](#how-rows-are-kept-and-ordered)).
  Query edits do not restart discovery. Scans merge new paths into the ranked
  rows within a time budget, as sorted runs of their own: no row already
  ranked moves, so a scan's merge costs about sorting what it inserts. Every
  row in order is read only through `App::shown` (`Rows::all`), which merges
  the runs first; nothing reads a half-merged list. The input thread never
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
  (details, `[MISSING]`, a folder's slash, a workspace's availability) comes
  from `Details`, whose workers read it in the background, one set per mount
  so that a stuck mount holds up only its own entries; until they answer,
  the frame shows what they found before. Owner names, possibly from a
  network directory, are looked up in the background too, and folders'
  sizes are added up by workers that a frame only tells which folders it
  wants (`Details::size`, `want_size`).
- Generation checks and cancellation prevent obsolete work from changing the view.
  Reuse recomputes attribution. Partial or truncated snapshots cannot establish
  completeness, and cached empty directories require verification before Enter.
- Verification age survives revisits and checkpoints. F5 bypasses replay. Prefetch
  stays bounded and cannot evict visited snapshots or spawn successors behind a
  stuck speculative worker. See [cache settings](CONFIGURATION.md#discovery-cache-and-performance).
- Workspace writes occur only on explicit save/delete. Preserve unrelated config,
  symlinks and permissions; refuse conflicting workspace edits. `--no-config`
  stays session-only. `tests/workspaces.rs` covers persistence and failures.
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
  works (`available`, a design choice per mode), what it does (its arm in
  `run_function` in `src/ui/dispatch.rs`, which may call a method in
  `src/ui/focus.rs` or on `App`) and its default keys (`defaults`). One
  acting on the current entry or the marks is a `ScopedAction`
  (`src/ui/actions.rs`), run through `Session::scoped`, which asks which
  when the two differ. The tests check
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
- **A picker** (named choices filtered as you type, like `:` and `gs`): a
  `ui::Picker` variant, its choices (`Picker::all`, as `ui::Choice`s), what
  Enter does with one in `picker_key` (`src/ui/dispatch.rs`) and its title
  and columns in `picker` (`src/view/choosers.rs`); the matching, keys,
  field and list are shared.
- **A file operation:** a `Request` variant and its method after `prepare`
  (`src/operations.rs`), undo steps (`src/undo.rs`), a plan or confirmation
  started as a job from `src/ui/actions.rs`, and the
  [OPERATIONS.md](OPERATIONS.md) guarantees it keeps.
- **Something that differs between programs** (roam, pick, seek, a
  window, the file dialogs): a method on `Program` in `src/program.rs`
  (`navigates`, `prints_paths`, `dialog`, `short_labels`, ...) read as
  `app.program()`, rather than a flag of its own; state only one program
  keeps goes in its `Running` variant. Where keys apply is the scopes'
  business (`available`, `defaults`), and what Enter does is the function
  Enter is bound to there.
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
cargo fmt --check
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
archive limits. `src/ui/jobs_stress.rs` runs random operations on generated
trees side by side on real threads (slowed down so that they overlap), stops
and undoes them, and checks that nothing is lost, half written or left in a
staging folder, and that undoing everything restores the trees exactly. They
are heavy (gigabytes of disk traffic), so `cargo test` skips them; run them
after changing file operations, the jobs list, the trash or undo
(`src/operations.rs`, `src/undo.rs`, `src/trash.rs`, `src/ui/jobs.rs`), one
round per test, or more before a release, and replay a failure by its seed:

```sh
cargo test --offline --lib stress -- --ignored
FFF_STRESS_ROUNDS=30 cargo test --offline --lib stress -- --ignored --nocapture --test-threads=1
FFF_STRESS_SEED=<seed from the failure> cargo test --offline --lib stress -- --ignored
```

The PTY suite also kills fff with SIGKILL at several moments of a copy and a
deletion, and checks that nothing is ever partly there under its name. PTY tests cover Normal-only operation keys, prompts, save/discard
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

## Packaging

`packaging/aur/PKGBUILD` builds the Arch package `fuzzy-file-finder` from the
release tag `v$pkgver` on GitHub: the binary, the manual pages and the bash,
zsh and fish completions (generated with `fff setup`, run as plain `fff` so
that they call the installed binary), and the documentation. It conflicts
with the AUR's `fff`, which also installs `/usr/bin/fff`. It is not on the
AUR yet; until then it is installed from this repository.

Installing it, from `packaging/aur` (the build's files there are ignored by
git):

```sh
makepkg -si      # builds, runs the tests, asks for sudo to install
```

The package's `/usr/bin/fff` comes before `~/.local/bin` and other folders
of your own on a usual `PATH`; remove any `fff` of your own there (a link to
`target/release/fff`, a `cargo install` in `~/.cargo/bin`) and manual pages
`fff setup --man` wrote to `~/.local/share/man`, which `man` would read
before the package's. `fff setup --desktop` and `--browser` write the path
of the `fff` that ran them into their files: run them again with
`/usr/bin/fff`, once.

On each release, after its commit is tagged `v$pkgver` and the tag pushed:

1. In `PKGBUILD`, set `pkgver` to the release and `pkgrel` to 1 (a change to
   the package alone, for the same release, raises `pkgrel` instead).
2. `updpkgsums` fills `sha256sums` from the release's tarball.
3. `makepkg --printsrcinfo > .SRCINFO`.
4. `makepkg -si` builds and installs it (`makepkg -sci` also removes the
   build's files after).
5. Commit `PKGBUILD` and `.SRCINFO`, and once the package is on the AUR,
   push both to its repository too.

The desktop, D-Bus and file chooser files keep pointing at `/usr/bin/fff`
across updates; nothing else needs running again.
