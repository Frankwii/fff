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
