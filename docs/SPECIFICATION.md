# picker specification

Status: first implementation. The user authorized implementation on 2026-10-31.
The active examples remain authoritative; sections 14–19 record decisions made for
this version. See README.md for usage, implementation limits, and verification.

## 1. Purpose and source authority

Build a fast, robust, interactive Rust terminal file picker inspired by the
directory-oriented fuzzy file picker remembered from Doom Emacs. The user wants
to see where they are, type to narrow candidates, enter directories, broaden
discovery, collect paths across locations, and return paths to a caller.

The exact visual frontend is flexible. The essential experience is an ongoing
session with visible context. Navigation, search changes, empty listings, and
Escape must not accidentally discard the session as the historical rofi scripts
did.

Use these sources in this order:

1. Explicit user instructions and subsequent decisions.
2. Active curated examples in `docs/tui_functionality_examples/`, subject to that
   directory's `AGENTS.md`.
3. This consolidation and `docs/tui_functionality_examples/KEYBINDINGS.md`, subordinate
   to those examples.
4. Earlier conversation proposals and `reference_scripts/` for historical intent.

Check status markers when reading examples. Files marked
`STATUS: Delayed until future versions` supply no current acceptance requirements
or bindings. Currently these are 06, 09, and 11. Do not activate them indirectly
through references in other documents. Conversely, a requirement independently
present in an active example remains active.

The curated documents call most defaults and shortcut assignments proposals.
Implementations should follow the documented baseline once implementation is
authorized, while distinguishing those choices from the user's explicit depth
priority and immediate numbered-choice requirements. Contradictions are collected
in section 12 rather than silently resolved here.

## 2. Current scope

The initial program is a terminal application with a single filename view. At
depth 1 it shows immediate children; at larger depths it includes descendants.
There is no separate recursion toggle or separate project-wide search view that
must be enabled before changing depth.

Current capabilities are navigation, fuzzy filename filtering, live maximum-depth
changes, multiple fixed startup roots, persistent marked paths, visibility and
exclusion controls, numbered immediate choice, and configurable returned paths.

Content search and opening a chosen file while resuming the picker are deferred.
Do not add `rg`/`rga` integration or opener bindings from the old transcript.
Detailed scenarios in deferred example 11 are also outside the current acceptance
scope. Live root addition/removal/reset is explicitly unavailable initially.

## 3. Session concepts

Keep the following concepts distinct, regardless of the chosen code structure:

| Concept | Meaning and lifetime |
| --- | --- |
| Invocation directory | Process working directory at startup; stable base for relative output. |
| Current directory | Exactly one browsed location, initially the first explicit startup root, or the invocation directory when no roots were supplied. |
| Search roots | Ordered, deduplicated directories supplied at startup; fixed during the session. |
| Maximum depth | Common discovery limit for all roots; minimum 1; proposed initial value 3. |
| Filename query | Text used to fuzzy-filter discovered candidates. |
| File glob constraint | Positive filename constraint, independent of fuzzy query and exclusions. |
| Visibility policies | Hidden-entry visibility and whether the exclusion list is respected. |
| Exclusions | Inspectable, editable rules; matching a directory prunes its subtree. |
| Marked paths | Ordered collection of output items independent of roots, query, and current results. |
| Highlight | Candidate identified by its path, not just its current row index. |
| Viewport and focus | Visible page, scroll position, and focused results/list/editor. |
| Discovery status | Incomplete or completed scan and eligibility of incoming results. |
| Rendered choice targets | Paths addressed by numbered labels in the last completed frame. |
| Output settings | Invocation-time path representation and record delimiter. |

Maintain actual filesystem paths separately from presentation labels. Relative
labels must not become output paths whose meaning changes after navigation.

Depth changes preserve current directory, roots, filename query, glob constraint,
and marks. Visibility/exclusion changes preserve current directory, roots, depth,
query, and marks. Marks survive navigation and search refreshes.

The user clarified that discovery combines descendants of the current directory
and all fixed startup roots. Exactly one current directory is retained; see section 14.

## 4. Discovery, matching, and order

Immediate children have depth 1. Each further path component adds one level.
Changing the fuzzy query filters known candidates without restarting filesystem
discovery. Matching is case-insensitive ordered-subsequence matching: `dwn`
matches `Downloads`, and `rpx` matches `report.tex`. A scoring policy ranks fuzzy
matches; the exact algorithm and text being scored need a documented decision.

A positive file glob must not block traversal through parent directories needed
to discover matching files. Example 03 uses `*.tex` and omits directory candidates
from its constrained result list.

Deduplicate a path discovered through multiple roots. Its effective depth is the
minimum relative depth among active roots. For root-order ties, attribute it to
the earliest startup root that yields that minimum depth. A duplicate startup
root creates neither another root nor another result.

Sort known matching candidates by this exact tuple:

1. Effective depth ascending.
2. Directories before files within that depth.
3. Fuzzy score descending within that depth and kind.
4. Startup root order.
5. Original absolute pathname bytes ascending.

An empty query assigns equal fuzzy scores. A shallow file precedes a deeper
directory, even if the deeper directory has a stronger fuzzy score. Nonmatching
shallow candidates are omitted. For example 13's empty query, final order is:

```text
alpha/                  depth 1, directory
zebra.txt               depth 1, file
alpha/beta/             depth 2, directory
alpha/aa.txt            depth 2, file
alpha/beta/00.txt       depth 3, file
```

Apply ordering during discovery, not just after completion. New shallower results
are inserted before deeper known results. Keep a surviving highlighted path
highlighted when insertion changes its row. Clearly label incomplete discovery.

Increasing depth extends discovery without restarting the application and keeps
input usable. Decreasing depth removes ineligible known candidates immediately;
late events cannot restore them. Repeated decreases at 1 leave the session open.
Increasing beyond the deepest existing entry still updates the depth indicator.

## 5. Navigation and empty states

Enter on a highlighted directory navigates into it and preserves marks and depth.
A first visit begins with an empty query. Parent navigation via Alt+h retains the
current query and resets scroll/highlight, following active example 02 rather
than the transcript's earlier history-restoration proposal.

The current directory and maximum depth are visible. Roots and each result's
effective depth and root-relative path are inspectable. The marked collection is
shown with compact relative labels and can receive focus. Exact absolute paths
remain inspectable in F1 details (user display decision in section 16).

An empty directory and a query with no matches leave the session usable. Users
can edit the query, clear it, or navigate upward. Enter in an empty directory
confirms marks plus the current directory, as specified below. Active examples
do not define Enter on every kind of empty result set; do not generalize that
rule to a filtered nonempty directory or an incomplete scan without a decision.

## 6. Marks and confirmation

Tab toggles the highlighted path's mark and advances if a following result exists.
Marks are unique and retain insertion order. Marking a directory selects that
directory itself, not its descendants. Filtering, depth limits, hidden-file
changes, and exclusions never silently discard marks.

Alt+Space focuses the marked-item list and returns to results when pressed again.
Inside that list, Tab toggles the focused mark. An unmarked row remains visible
until the user leaves the list; Escape or Alt+Space returns to results and then
removes unmarked rows. Complete marked-list navigation is not specified yet.

| Action in main results | Output and outcome |
| --- | --- |
| Enter on directory | Navigate; do not confirm output. |
| Enter on file | Return marks in insertion order, append the file if absent, then exit successfully. |
| Enter in empty directory | Return marks in insertion order, append current directory if absent, then exit successfully. |
| Ctrl+w with marks | Return only marks in insertion order and exit successfully. |
| Ctrl+w without marks | Stay open and output nothing. |
| Numbered choice | Return only its target, ignoring marks, and exit successfully. |
| Ctrl+c | Exit with no selected-path output. |

With no marks, the two Enter confirmation cases return only their target. A marked
highlighted target appears once. Active example 05 also requires missing marked
paths to appear as missing and prevent silent partial confirmation; the exact
interaction for resolving missing marks remains undecided.

## 7. Numbered immediate choice

Numbered shortcuts act on selectable rows visible in the most recently fully
rendered main-results frame. Titles, separators, progress, and errors do not
consume numbers. Targets change when filtering or paging changes rendered rows.

- Ctrl+1 through Ctrl+9 choose that visible position.
- If the requested position exceeds the visible count, choose the last visible row.
- Ctrl+0 always chooses the last visible row, including when more than ten rows fit.
- Rows beyond the first nine that are not the last row have no numeric shortcut.
- With zero visible candidates, every numbered shortcut chooses only the current
  directory, including when a query has no matches. Ignore marks in this case too.

The shortcut returns exactly one target and exits successfully. A directory is
returned immediately rather than entered. No Enter, opener, or confirmation
prompt follows. Pending discovery updates cannot redirect the shortcut to a path
that has not yet been rendered under its label.

For example 12's absolute/NUL mode, Ctrl+2 emits only the second visible path
followed by exactly one NUL byte. It ignores both the highlight and any marks.

Physical Ctrl+digit chords must be decoded distinctly. If terminal input cannot
distinguish a chord from another key, do not guess. Plain digits edit the query.
Decoded-event behavior and real terminal compatibility require separate checks.
The terminal protocol and supported terminal matrix remain implementation decisions.

## 8. Visibility and exclusions

Alt+. toggles hidden-file visibility independently of exclusions. Alt+H (uppercase
H) toggles respect for the exclusion list. Alt+e edits that list; Ctrl+w in the
exclusion editor applies edits rather than confirming marked output.

Edits made while exclusion enforcement is disabled do not immediately hide entries.
Re-enabling enforcement applies the edited rules. Excluded known candidates
disappear immediately; allowing previously excluded paths triggers discovery.
Directory exclusions prune descendants. Active policies and all exclusion rules
are inspectable, and marks on excluded paths remain marked.

Editing in a Neovim/editor buffer is a stated preference, similar to nnn's rename
interface. The editor mechanism, rule syntax, full editor keymap, and defaults
for ordinary ignore files are not settled. Do not equate hidden visibility with
disabling exclusions or assume every ignore-file convention from a traversal crate.

## 9. Input and session safety

Printable text always edits the focused query or prompt. Lowercase and uppercase
bindings are distinct. All bindings are intended to be remappable; configuration
format is undecided. The active baseline is:

| Key | Action | Active examples |
| --- | --- | --- |
| Enter | Navigate or confirm as in section 6 | 01, 02, 04, 05 |
| Alt+h | Parent, retaining query and resetting scroll/highlight | 02 |
| Alt+j | Next highlighted result | 02 |
| Alt+k | Previous highlighted result or marked-list item | Explicit user decision |
| Alt+l | Enter highlighted directories only; never confirm output | User decision, section 17 |
| Alt+g | Location entry | User decision, section 17 |
| Alt+b | Bookmark chooser | User decision, section 17 |
| F5 | Force discovery refresh, bypassing the current snapshot | User decision, section 17 |
| Alt+Right / Alt+Left | Increase/decrease depth by 1, minimum 1 | 03, 04, 05, 13 |
| Tab | Toggle mark; advance when possible | 05 |
| Alt+Space | Switch between results and marked list | 05 |
| Ctrl+w | Confirm marks, or apply edits inside exclusion editor | 05, 07 |
| Alt+. | Toggle hidden entries | 05, 07 |
| Alt+H | Toggle exclusion enforcement | 07 |
| Alt+e | Edit exclusions | 07 |
| Escape | Clear main query, or leave marked list | 01, 02, 05, 08 |
| Ctrl+c | Quit without selected paths | 08 |
| Ctrl+1 ... Ctrl+9, Ctrl+0 | Immediate visible choice | 03, 05, 12, 13 |
| Alt+1 ... Alt+9, Alt+0 | Activate visible choice using Enter behavior | 14 / explicit user decision |
| PageUp / PageDown | Change visible page and numbered targets | 12 |

In main results, Escape clears a nonempty query while discovery continues. With
an empty query it changes nothing. It must not terminate the session. Subordinate
view behavior is only established where explicitly described; a universal prompt
cancel/help keymap has not yet been agreed. No complete previous-result keymap
is established either.

## 10. Caller interface

Output settings are supplied at invocation and remain fixed while browsing.
Support absolute paths, paths relative to the invocation directory, and
script-friendly record framing including NUL termination. The path basis and
delimiter are separate concerns; precise CLI flags and default format are undecided.

Navigation must not change the base used for relative output. Keep display labels
separate from returned paths. Terminal UI and status text must not contaminate
selected-path output. Successful numbered choices terminate successfully; Ctrl+c
returns no selected paths. Exact non-success exit codes remain undecided.

## 11. Technical direction and verification

Rust is the chosen language. The conversation proposes Ratatui + Crossterm for
the terminal, Nucleo for fuzzy matching, and `std::fs::read_dir` and/or `ignore`
for discovery. No dependencies are installed yet; these are candidate components,
not an obligation to introduce all of them.

Recommended architecture from the conversation:

- One persistent application state with explicit input/state transitions.
- Separate rendering, discovery, fuzzy matching, and output formatting.
- Background discovery that reports batches without blocking input.
- Cancellation or generation identifiers so obsolete results cannot overwrite
  current state; eligibility checks enforce depth and policy changes immediately.
- Explicit rendered shortcut targets and path-based highlight identity.
- Original filesystem paths retained independently of labels; subprocess arguments,
  if needed, passed directly rather than assembled through shell word splitting.

Threads/channels versus an async runtime, widget layout, caching, worker count,
and elapsed-time budgets are undecided. Judge responsiveness through evidence;
do not invent performance claims or require a particular concurrency framework.

Turn active examples into acceptance checks as implementation proceeds. Fixtures
should state roots, current directory, depth, query, glob/policy settings, marks,
viewport, and discovered candidates. Inject live discovery events deterministically
and render frames explicitly rather than relying on filesystem timing. Verify
ordering, stale-event rejection, overlap deduplication, path identity, persistent
marks, confirmation combinations, and exact output bytes. Keep physical key
decoding checks separate from application tests with decoded events.

Historical scripts explain the desired capabilities, but their process restarts,
lost query/selection state, unchecked navigation, empty-list exits, relative-path
ambiguity, and shell argument handling are reasons for the replacement. They are
not compatibility contracts to reproduce.

## 12. Open decisions and source inconsistencies

Do not invent answers to these product questions while claiming they are already
specified. Clarify the relevant item when implementation reaches it; unrelated
work can proceed once authorized.

| Item | Established facts and unresolved detail |
| --- | --- |
| Navigation versus fixed roots | Resolved by the user: descendants of the single current directory and all fixed startup roots. See section 14. |
| Location entry | Enter in the location prompt navigates without confirmation; the prompt moved to Alt+g when the user requested navigation-only Alt+l (section 17). |
| Parent-navigation fixture | Example 02 describes an empty directory even though `proofs` exists; this can be presented as no matching results. The previous consolidation incorrectly claimed `scratch` lacks an `h`: its final letter is `h`, and `sh` is a valid subsequence. Both `short_notes` and `scratch` match, with adjacency favoring `short_notes`. Curated examples are unchanged. |
| History | Parent navigation preserves the current query and resets scroll/highlight per active example 02. The older transcript proposed restoring per-directory query/cursor/scroll history; revisits and back/forward behavior are not fully specified by active examples. |
| Ctrl+d | The examples README mentions Ctrl+C or Ctrl+D as explicit exits. No active example establishes a Ctrl+d binding; keep it out of the active binding table pending a decision. |
| Query terminology | Active example 08 says content query, while content search is deferred. Its Escape/session-preservation assertions remain applicable; the wording does not activate content search. |
| Matching policy | First-version basename/subsequence scoring approved; see section 14. Additional normalization remains open. |
| Glob behavior | First version uses `--glob` on file basenames only and traverses parents; approved in section 14. |
| Exclusions | First version uses explicit glob exclusions, enforced by default, and a built-in session editor; section 14. External editor lifecycle remains open. |
| Empty results | Approved first-version behavior: Enter does nothing on no-match results in a nonempty directory or during an incomplete empty scan. Numbered fallback remains unchanged. |
| Missing marks | Approved first-version behavior: block Enter/Ctrl+w until missing marks are unmarked; numbered choice bypasses marks. |
| Defaults and CLI | Approved first-version defaults are in section 14; implemented flags and key remapping are in README.md. Physical terminal compatibility and non-Unix support remain unverified. |

## 13. Acceptance-source map

| Active example | Primary contract |
| --- | --- |
| [01](tui_functionality_examples/01_directory_browsing.md) | Subsequence filtering, directory entry, first-visit query, no-match survival. |
| [02](tui_functionality_examples/02_navigation_history.md) | Parent navigation, query retention, empty-directory confirmation. |
| [03](tui_functionality_examples/03_recursive_search.md) | Live depth, file glob traversal, immediate depth eligibility. |
| [04](tui_functionality_examples/04_multiple_search_roots.md) | Fixed ordered roots, effective depth, overlap deduplication. |
| [05](tui_functionality_examples/05_persistent_selection.md) | Persistent marks, marked-list focus, confirmation rules, missing marks. |
| [07](tui_functionality_examples/07_visibility_and_exclusions.md) | Hidden visibility, editable exclusions, enforcement toggle. |
| [08](tui_functionality_examples/08_escape_and_cancellation.md) | Escape preserves discovery/session; explicit cancellation. |
| [10](tui_functionality_examples/10_return_paths.md) | Invocation-time path/output configuration. |
| [12](tui_functionality_examples/12_numbered_immediate_choice.md) | Visible numbered choice, marks bypass, fallback, output bytes, terminal decoding. |
| [13](tui_functionality_examples/13_depth_ordering.md) | Exact sort tuple, live insertion, rendered targets, highlight stability. |
| [14](tui_functionality_examples/14_numbered_enter.md) | Alt+digit activation follows Enter; split results/marked panes. |


## 14. First-version decisions (2026-10-31)

The user explicitly authorized implementation, clarified discovery as
“Descendants of the current directory *and* all fixed roots,” and approved the
proposed first-version defaults and choices below. These decisions supersede the
corresponding open items in section 12; unaddressed questions remain open.

- Keep startup roots fixed and ordered, with exactly one current directory.
  Discovery uses their union with the current directory. Effective depth is the
  minimum positive relative depth across these locations. For implementation
  ties, the additional current-directory scope follows fixed roots; duplicate
  scope paths are omitted.
- Use basename matching with case-insensitive ordered subsequences. The initial
  scoring policy rewards adjacency and word starts and favors early matches.
  No normalization beyond Unicode lowercase conversion is added.
- Default depth is 3. Without explicit root arguments, start browsing at the
  invocation directory with no fixed roots; directory/parent navigation moves
  the sole discovery scope. Explicit root arguments remain fixed. Output defaults
  to absolute newline-separated paths; independent relative and NUL flags are
  available. Relative paths always use the invocation directory as their base.
- Enforce explicit glob exclusions initially, hide hidden entries, and do not
  load ordinary ignore files automatically. Positive file globs show only files
  while permitting traversal through nonmatching directories.
- Use a built-in exclusion editor initially. Ctrl+w validates and applies edits;
  rules remain session-local. Escape cancels prompts. A full external-editor
  lifecycle is left for future work.
- Missing marks block Enter/Ctrl+w until unmarked. Numbered selection bypasses
  marks as established in example 12. Enter on no-match results in a physically
  nonempty directory does nothing; incomplete empty scans cannot confirm.
- Alt+l accepts a directory location and navigates; it never confirms a previously
  highlighted file. Directory navigation clears the query; parent navigation
  preserves it and resets scroll/highlight.

Implementation choices: Crossterm terminal I/O on `/dev/tty`, a bounded worker
channel with generation checks and cancellation, explicit original path storage,
CLI key remapping, arrow-key movement in results/marks, and a scrollable F1 details
view. No async runtime, automatic ignore rules, content search, or opener is used.
Success exits 0, cancellation 130, startup/I/O failure 1. Symlink directories are
not recursively followed. Built-in editors currently offer append/backspace
editing. These are initial implementation choices, not additions to the curated
acceptance examples.

Verification separates deterministic state/discovery checks from Linux PTY
integration. CSI-u Ctrl+digit events are decoded through Crossterm; ambiguous
legacy input is not interpreted as a numbered chord. Physical terminal/hardware
compatibility and non-Unix support remain unverified. A richer editor, persistent
configuration, history beyond the specified first-visit/parent behavior, and a
terminal compatibility matrix remain open.

User follow-up: Alt+k moves the highlight upward in results and the marked list,
alongside the Up arrow.


Navigation bug fix following the user's report with `cargo run`: an implicit
starting directory is a browsing location, not an explicit fixed root. Previously
pinning it left old entries visible after directory and parent navigation. Keep
fixed-root union behavior when root arguments are explicitly supplied. Omit the
current directory itself from candidate results so it cannot be re-entered as
its own highlighted result through an ancestor scope. This implements the active
01/02 navigation contracts while retaining the user's explicit fixed-root union
rule. Regression checks exercise nested Enter (legacy and CSI-u) and parent
navigation at depth 1 with a retained query.


## 15. Numbered Enter and interface revision

The user requested Alt+number to activate the same visible target as Ctrl+number,
with the behavior of manually highlighting that entry and pressing Enter. Active
example 14 records this decision. Both use the last completed frame's labels,
1–9 with clamping and 0 for the last row. Directory targets navigate without
confirmation; file targets confirm ordered marks plus the target once and exit.
Alt+number uses the same missing-mark checks as Enter. With zero visible rows it
follows Enter's empty-state rules, including staying open for a no-match query in
a nonempty directory or incomplete scan. Ctrl+number retains example 12's
single-path output and unconditional current-directory fallback.

The user also authorized a visual redesign and requested marks on the right.
The implementation now uses Ratatui with the existing Crossterm input layer.
At widths of at least 80 columns, results and marked paths appear side by side;
narrower terminals stack panes and give marks more room when focused. Borders,
selection backgrounds, directory colors, matched-letter emphasis, explicit
shortcut labels, and textual progress/missing states distinguish information.
Full marked paths wrap; session details and prompts use dedicated panels. A
36-column by 12-row minimum avoids assigning shortcuts to invisible rows. UI
output still uses `/dev/tty`; selected path bytes remain isolated on stdout.
Tests cover both state transitions and complete rendered frames, including
paging, resize, focus, and real Crossterm decoding of ESC+digit / CSI-u Alt+digit.


## 16. Compact relative display paths

The user requested display paths relative to the current root and `.../` in place
of a long chain of parent-directory steps. Use the single current browsing
directory as the common display base for results, marked paths, and the selected
path footer. Descendants show `topic/file.tex`; siblings one level up retain
`../file.tex`; two or more leading `..` components collapse to `.../`, for example
`../../papers/file.tex` becomes `.../papers/file.tex`. Labels update after
navigation, including for results discovered through fixed roots and persistent
marks. The current-directory header remains an absolute anchor; F1 details retain
exact absolute paths and discovery-root-relative labels. This explicit user
request supersedes the earlier full-absolute-path marked-pane display requirement.

Abbreviation is presentation only. Filesystem identity, discovery attribution,
sort order, numbered targets, navigation, mark storage, and output formatting
continue to use original paths. In particular, `--relative` output still uses the
invocation directory as its base and never substitutes `.../` abbreviations.


## 17. TOML configuration, bookmarks, caching, and navigation revision

The user explicitly requested configurable TOML palettes and bookmarks,
navigation-only Alt+l, caching of completed searches for quick revisits with
thorough tests, and a more distinctive current-directory display. The full
implemented format and controls are documented in [CONFIGURATION.md](CONFIGURATION.md)
and [the sample config](config.example.toml).

- Optional config lookup uses XDG_CONFIG_HOME or HOME/.config. `--config`,
  `--no-config`, `--palette`, and `--check-config` provide explicit overrides and
  validation. No config is created or rewritten automatically. Invalid fields,
  colors, palettes, bookmark names, and cache limits fail before entering the TUI.
- Built-in midnight/light/forest palettes and named custom palettes control all
  semantic UI colors, including current-directory banner foreground/background.
  The header highlights the basename with bold/underline and a colored band.
- Alt+l enters only a highlighted directory and never selects files or confirms
  an empty-directory output. It preserves marks/depth like directory Enter.
  The location prompt moves to Alt+g. This supersedes example 04's Alt+l binding.
- Alt+b opens an ordered, fuzzy-name-filtered bookmark chooser. Enter navigates
  directories; unavailable paths remain recoverable errors in the chooser. Names
  are unique; paths support ~/ and config-directory-relative paths. Bookmark
  navigation clears the query while preserving marks, fixed roots, and depth.
- Completed successful scans populate a bounded session LRU cache. Keys include
  current directory, ordered scopes, depth, hidden/exclusion policies and rules,
  and file glob. Query/highlight/marks are not restored. Cache hits install an
  initial candidate subset synchronously and revalidate in the background, with
  visible incomplete status, stale-event guards, and reconciliation of deletions
  and kind changes. Empty cached results cannot confirm until freshly verified.
  Cancelled/failed/stale scans are not stored. Defaults: 16 entries, 4096 paths
  each, expiry after 60 seconds. F5 bypasses the current snapshot; config can
  disable the cache. Cache tests inject time/events and use filesystem fixtures.

Current curated scenarios remain unchanged; explicit user decisions in this
section take precedence where earlier bindings or visual defaults disagree.


## 18. Immediate revisit replay and upstream palettes

User request (2026-11-01): make revisiting any visited directory immediate, add
all Catppuccin and Tokyo Night variants plus Nord and Gruvbox, use dashed names,
and support live color reload. Upstream definitions were verified online; see
[PALETTES.md](PALETTES.md) for exact identifiers, role mappings, and sources.

A cache hit now receives a rendered frame before incoming discovery batches are
processed. Snapshot storage is shared and bounded; navigation checkpoints the
current matches, including provisional prefixes of unfinished visits. An
unfinished prefix cannot replace a complete snapshot. TTL marks snapshots stale
rather than discarding them: they remain available for provisional replay until
LRU eviction, and background discovery still reconciles filesystem changes.
This supersedes section 17's completed-only/expiry-removal cache policy.

Palette-only config reload runs off the input thread, retains working colors on
invalid edits, honors a CLI-pinned palette name, and preserves session state.
The cache/palette revision is committed before the separate experimental text
editing work requested by the user.


## 19. Experimental clean Neovim text editing

The user requested a separate experimental commit providing Vim-like editing in
all text fields, preferably a clean Neovim instance. Investigation/prototyping
confirmed the documented stdio MessagePack-RPC API supports this. `--editor nvim`
or `[editor] backend="nvim"` enables the real clean engine; default `basic` remains
available and is the fallback on missing/broken Neovim. All four text fields use
independent scratch buffers with real motions/operators/undo/registers, modal
Escape, visible mode/cursor/selection, command-line editing, and bracketed paste.

Picker shortcuts retain their application actions. Insert/Visual/pending Escape
first returns toward Normal; Normal Escape clears/cancels as appropriate. During
pending Vim input and command-line entry, editing input (including Enter) goes to
Neovim. This explicit experimental mode supersedes the prior unconditional
printable-query/Escape behavior only when enabled. No personal config/plugins or
persistent editor files are used. The process is private, time-bounded on RPC,
and reaped on exit. See [EDITING.md](EDITING.md) for tested behavior and the
boundary between field editing and a full Neovim window UI.

## 20. Neovim default, palette keys, Normal-mode list keys, icons, integrated location line, completions (2026-11-01)

The user requested six changes on 2026-11-01. They supersede conflicting earlier
text in this specification and the earlier key bindings; curated examples are
unchanged. The subsections interact as follows: Neovim being the default (20.1)
makes Normal mode reachable in every list view, so the Normal-mode list keys
(20.3) also apply in the palette chooser (20.2), where every highlight move
previews; filetype icons (20.4) decorate result and marked rows only, never the
`@` location line or pinned roots (20.5); and every new command-line flag with a
completable value is wired into shell completion (20.6; `--no-icons` takes no
value).

### 20.1 Neovim editing is the default

The user requested: "Make neovim editing the default." This supersedes section
19's opt-in rule. With no `--editor` flag and no `[editor] backend` in TOML, all
text fields use the clean embedded Neovim engine. `--editor basic` or
`[editor] backend = "basic"` keeps append/Backspace editing and the original
Escape behavior; the CLI flag overrides TOML.

Fallback must be graceful and quiet: when the backend was not chosen explicitly
and `nvim` is missing from PATH or fails to start, picker starts normally in
basic editing with no status message, stderr output, or startup failure. An
explicit `nvim` choice (CLI or TOML) still reports `Using basic editing: …` in the
status line, because the user asked for an engine that is unavailable. A Neovim
engine that fails after startup still falls back to basic editing with that status
message, preserving the last synchronized text. The section 19 engine boundaries,
isolation, and tested behavior are otherwise unchanged.

### 20.2 Live palette switching (Alt+p chooser, Alt+P config reload)

User request (2026-11-01): "Make color live-reloadable (not just via a terminal
argument; I want a keymapping for this during execution)." Clarified to both:

- **Alt+p** opens a palette chooser listing every built-in palette in catalog
  order, then the config file's custom palettes. Its name filter is a text field
  using the active editing engine, like the bookmark filter (fuzzy subsequence
  match, list order kept). Moving the highlight or changing the filter previews
  the highlighted palette across the whole interface; with no match the
  pre-chooser colors show. Enter applies the highlighted palette for the rest of
  the session. Escape (from Normal mode when Neovim editing is active) or Alt+p
  again closes the chooser and restores the pre-chooser colors. The config file
  is never written.
- **Alt+P** (uppercase; Alt+Shift+p in either legacy or kitty-protocol encoding)
  re-reads the configuration immediately and applies its palette, clearing any
  session choice. With `--palette NAME` it re-reads that pinned name's definition.
  It also works with `--no-config`, restoring the default/pinned built-in. A
  failed re-read keeps the current colors and reports the error; the session
  choice is still cleared, so background reload recovers once the file is fixed.
  Alt+P inside the chooser closes it and reloads.
- The background watcher (section 18) stays. After an Enter in the chooser it
  follows the chosen name's definition instead of `[ui].palette` until Alt+P.
  Reloads computed for a superseded choice are discarded. A reload while the
  chooser is open updates the colors that Escape restores and keeps the preview.
- Both chords are listed in the footer (where width allows), F1 details, and
  README, and are remappable with `--map-key` (for example `ctrl+r=alt+P`).
  Session state (query, marks, discovery, numbered targets) is preserved.

### 20.3 Normal-mode list keys

User request (2026-11-01): "Add keybindings that are specific to being in Normal
mode while editing: movement for selecting an option with hjkl (remove the 'Alt'
from the existing insert-mode mappings), pressing gg [and G] for navigating to
the top/bottom of the search result. Try to make them configurable in the config
file too." Clarified: `gg` highlights the first result and `G` the last.

These keys apply only to list views: the main results, the marked list, the
bookmark chooser, and the palette chooser (section 20.2, where moving the
highlight also previews the palette). While the view's field is in Neovim Normal mode, plain `h`/`j`/`k`/`l` do exactly what Alt+h/j/k/l do in
that view; an action the view does not implement (for example `h`/`l` in the
bookmark and palette choosers) behaves as unbound. The marked list has no text field and
always accepts them. The multiline exclusion editor and location prompt keep
Vim's own `hjkl`/`gg`/`G`. Alt bindings keep working in every mode; Insert,
Visual, pending, and command-line modes are unchanged.

Unbound Normal-mode keys still go to Neovim. A key that begins a configured
sequence waits (shown as PENDING); if the next key does not continue it, the
pending keys and that key reach Neovim intact, so `gU`, `ge`, and similar
commands keep working. Bindings are configurable in TOML `[keys.normal]`
(`left`, `down`, `up`, `right`, `first`, `last`; a sequence or list; `"none"`,
empty, or `[]` disables). Sequences are plain printable keys (`<space>`, `<lt>`
allowed), must not duplicate or prefix each other, and are validated at startup
and by `--check-config`. See [EDITING.md](EDITING.md) and
[CONFIGURATION.md](CONFIGURATION.md). This supersedes section 19's statement
that only Alt+j/Alt+k move the list highlight while editing.

### 20.4 Filetype icons

User request (2026-11-01): "Add little icons for shown entries based on filetype.
Maybe use nerd icons for this." Result rows and marked-pane rows show a Nerd Font
glyph chosen from well-known basenames (for example `Cargo.toml`, `Makefile`,
`.gitignore`) and then the lowercase extension; directories use a folder glyph
(`.git` a git glyph) and unknown files a generic file glyph. Lookup uses only the
name and the already-known directory flag, so rendering never stats result rows.
A compact built-in table is used instead of a crate: it keeps offline builds
dependency-free and limits glyphs to single-column BMP private-use code points.

The icon replaces the two-column `▸`/`·` kind marker, so row alignment, elision
that keeps the basename visible, metadata columns, and numbered targets are
unchanged; width is measured rather than assumed. Icon colors reuse palette
roles (directories `accent`, code `success`, media/archives `marked`, documents
`text`, other/config `muted`), so custom palettes and `NO_COLOR` apply unchanged;
the trailing `/` on directories still conveys kind without color or icon fonts.
`[ui] icons = true|false` (default `true`, validated by `--check-config`) and the
`--no-icons` flag (for terminals without a Nerd Font) restore the plain markers.
Icons are presentation only: matching, sorting, numbering, and output ignore them.

### 20.5 Integrated location line and pinned roots

User request (2026-11-01): “Have the current directory appear somewhere between
the query prompt and the list of files. Remove the current bar at the top and
make it into something more integrated. The 'PS1' for this may be simply '@'.
When there's more than one root, they should appear explicitly as 'pinned roots'
somewhere there too, but try to make it compact. Remember these are separate
from the working directory.”

- The top current-directory banner and its separate status row are removed. The
  filter panel is now the first row group; directly below it a single line reads
  `@ <absolute current directory>`. The `@`/path segment uses the
  `current_directory` background and `current_directory_text` foreground; `@`
  and the basename are bold and the basename underlined, so the line remains
  distinct under `NO_COLOR`. Long paths elide from the left, keeping the basename.
- Explicit startup roots are shown as `pinned r1 notes · r2 papers` (basenames,
  numbered like the `rN` scope labels on result rows), right-aligned on the `@`
  line when both fit, otherwise on one extra line under it. Entries that do not
  fit are summarized as `+N (F1)`; F1 lists every scope path and now calls these
  “pinned root”. When the current directory is a pinned root, that entry is bold
  and underlined. The `@` line also shows the current directory's own scope
  number (for example `r3` once navigation leaves every root), keeping the
  working directory visibly separate from pinned roots. Without explicit roots
  nothing pinned is shown.
- Depth, discovery progress (`● complete`, `◌ scanning (incomplete)`, `◌ cached ·
  refreshing (incomplete)`), hidden/exclusion policies, and an active file glob
  appear on the filter panel's bottom border. Narrow frames first drop trailing
  policies (still in F1), then shorten progress to `◌ incomplete`; depth and an
  explicit incomplete/complete label always remain.
- This supersedes the banner/header wording of sections 16 and 17. The 36 × 12
  minimum, numbered-target rules, and stacked narrow layout are unchanged; the
  freed rows go to the result panes.

### 20.6 Shell completions and descriptive help

User request (2026-11-01): "Add terminal completions and in general improve the
'--help' messages to be more descriptive. Add, as an agentic rule, that every
terminal argument should be completable." Follow-ups: a library may be used to
make terminal output look nice, and the rule applies to "parameters that it makes
sense to autocomplete; don't take it too literally".

Completion is dynamic: the shell script calls back into the installed binary
(`COMPLETE=<shell> picker -- <words>`), so candidates always match the running
version. `picker --completions SHELL` prints the registration script (bash, zsh,
fish, elvish, powershell) and is identical to `COMPLETE=SHELL picker`; neither
starts the interface or reads the configuration. Completion covers every option,
root directories, `--config` files, `--editor` backends, `--completions` shells,
`--palette` names (built-ins plus `[palettes.NAME]` tables from the file the
completed command line would load: `--config` with a leading `~`, `$NAME` or
`${NAME}` expanded as the shell will, none with `--no-config`, else the default
path; unreadable files contribute nothing), and `--map-key` chords one segment at
a time (modifiers, key names, `none` as target; a source `space` only after
`ctrl+`/`alt+`, since printable sources are rejected). `--depth`, `--glob`
and `--exclude` take free-form values and deliberately have no completion.

`-h` gives a one-line-per-option summary with short examples; `--help` adds a
description, grouped options (Search, Output, Interface, Configuration, Shell
integration) with defaults and possible values, examples, a key summary pointing
to the README table and F1, the configuration path, exit status (0 selected or
valid configuration, 1 startup/configuration/I/O error including an unknown
`--palette`, 2 usage error: unknown options or malformed `--depth`, `--map-key`,
`--glob` or `--exclude` values, 130 Ctrl+c) and completion installation. Help uses clap styles (colored headings,
literals and placeholders) and is plain under `NO_COLOR` or when not a terminal.


## 21. Bookmark icons, gruvbox, Normal-mode entry, key guide, `--no-dir`, config path (2026-11-01)

A second set of user requests on 2026-11-01. Each subsection records one request
and its implementation; they supersede earlier text where they conflict.

### 21.1 Bookmark icons

User request: customizable bookmark icons as a separate `icon` key, e.g.
`icon = "󱃩"` beside `name` and `path`. The optional value is trimmed and must be
one or two terminal columns wide; `--check-config` rejects anything else. The
bookmark chooser draws it before the name in the bookmark's color. Bookmarks
without an icon use the Nerd Font folder glyph when `[ui] icons` is enabled.
Explicit icons are shown even when file icons are disabled, since the user chose
them; names stay aligned in either case. Icons are presentation only.

### 21.2 Gruvbox closer to the Neovim colorscheme

User request: make gruvbox "a bit more similar to the one in nvim. I remember it
being quite orange", since the earlier mapping used palette slots rather than the
memorable colors. `gruvbox-dark` and `gruvbox-light` now use a flat `bg0` canvas,
the gray comment color for muted text, `bg2`/`bg3` borders, and orange (bright
`#fe8019`, faded `#af3a03`) as accent and current-directory color. Values remain
upstream gruvbox colors; see PALETTES.md.

### 21.3 Normal mode after entering a location

User request: "Make normal be the default editing mode when entering a new
location." With the Neovim backend, every change of current directory (directory
Enter or Alt+digit, Alt+l/`l`, Alt+h/`h`, the location prompt, bookmarks) resets
the query buffer into Normal mode, with the cursor on the last character of a
retained query. This supersedes section 19/20.1's "navigation resets to Insert".
The first field activation at startup and the subordinate fields still begin in
Insert mode. The basic backend has no modes and is unchanged.

### 21.4 F1 key guide and visual overhaul

User request: an F1 binding "that shows a guide for all other keybinds", removing
them from the bottom; an appearance overhaul, e.g. a nicer "depth 3 …" line; and
"what even is the 'r1' column?" — labels "could be made to look nicer", with the
design left to the implementer's taste.

- F1 opens an overlay with two tabs, **Keys** (default) and **Session**. Tab,
  BackTab or Left/Right switch tabs; Up/Down, j/k and page keys scroll; Escape, q
  or F1 close. Keys lists every binding in groups (Navigate, Select and return,
  Search, Normal mode in lists, Look), in two columns when wide. Normal-mode
  entries show the configured `[keys.normal]` sequences; with `--no-dir` the
  navigation entries are crossed out with a note. Session holds the former F1
  details (exact paths, scopes as pinned/current, highlighted path with its depth
  and root, policies, palette, marks).
- The footer is one line: the highlighted path (parent dimmed) or a status message
  (truncated at its end), and an `F1 keys` pointer. The footer's key hint lines
  and the marked pane's Alt+Space hint are removed; numbered row labels remain.
  Results gain the two freed rows.
- The filter's bottom border shows pills: progress (`● complete`, `◌ scanning ·
  incomplete`, `◌ cached · refreshing`, or `◌ incomplete` when narrow) and
  `depth N`, then only non-default settings: glob, `files only`, `hidden shown`,
  `exclusions off`. F1 lists every policy.
- The per-row `dN rN` metadata column is removed. Labels are relative to the
  current directory, so they already show where a row lives; F1 gives exact depth
  and root. Pinned roots are listed by name (`pinned  notes  papers`, with a pin
  glyph when icons are on) instead of `rN` numbers. This supersedes the "scope
  number" display in sections 15 and 20.5; depth ordering is unchanged.
- The `@` line is a badge in the current-directory colors followed by the path,
  with `~` for the home directory, a dimmed parent and a bold basename. The query
  prompt is `❯`; marks show `●`/`○` instead of `[x]`/`[ ]`.

### 21.5 Static file choice with `--no-dir`

User request: a `--no-dir` flag that "makes directories don't show up on the
search" and disables Alt+l "or other navigation-related keybindings": "a static
search where I need to choose some files out of the ones in the specified
directory and leave, but cannot navigate".

- Directory candidates are omitted from results (discovery still traverses them,
  so files down to `--depth` appear). Depth, visibility and exclusion changes
  still work; they do not move the current directory.
- The current directory never changes: Alt+h, Alt+l, Normal-mode `h`/`l`, Alt+g
  and Alt+b leave the session as is and show "Navigation is disabled with
  --no-dir". A directory candidate (e.g. from a cached kind change) is not entered.
- No directory is ever returned: Enter in an empty directory and Ctrl+number with
  no visible rows stay open instead of returning the current directory (this
  replaces example 12's fallback only in this explicit mode). Files confirm with
  marks exactly as usual.

### 21.6 Default configuration path

User request: "Make the default config path be `$XDG_CONFIG_HOME/picker/config.toml`".
That was already the lookup order (section 17); following the XDG base directory
specification, `~/.config/picker/config.toml` remains the fallback only when
`XDG_CONFIG_HOME` is unset, empty, or not absolute. Documentation and `--help` now
lead with the XDG path, and `--check-config` names the file it validated (or says
that no file was found and built-in settings apply).


## 22. Pinned-root underline, fading messages, more palettes, responsiveness (2026-11-01)

A third set of user requests on 2026-11-01.

### 22.1 Pinned current root underline

When the current directory is also a pinned root, only that root's name is bold
and underlined; the two-space gap before it is a separate unstyled span (it was
previously underlined too).

### 22.2 Status messages fade

User request: "make the red notification messages at the bottom disappear after
a few seconds." Status messages (footer and prompt error lines) clear after
`[ui] message_seconds` (default 4, range 0..3600, 0 keeps them until replaced).
Expiry is handled once per event-loop iteration: each key handler starts from an
empty status, so a message reported again by a key, even with identical text,
restarts its timer, while a message left untouched keeps its deadline. Messages
from background work (palette reload, discovery) start their timer when they
first appear.
