# picker specification

Status: pre-implementation. This document records intended behavior; it does not
claim the Cargo scaffold implements any picker functionality.

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
2. Active curated examples in `tui_functionality_examples/`, subject to that
   directory's `AGENTS.md`.
3. This consolidation and `tui_functionality_examples/KEYBINDINGS.md`, subordinate
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
| Current directory | Exactly one browsed location, initially the first startup root. |
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

The relationship between fixed startup roots and navigation-driven listings needs
clarification before that part is implemented; see section 12.

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
shown with full paths and can receive focus.

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
| Alt+l | Location entry; final acceptance behavior unresolved | 04 |
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
| Navigation versus fixed roots | Examples require directory browsing to follow navigation, and startup roots to remain fixed. They do not fully define how the two scopes compose, especially outside a root. |
| Location entry | Example 04 enters `/home/frank` via Alt+l and then says Enter returns `turing.pdf`. It does not explain why accepting a directory location confirms the previously highlighted file or whether another Enter is intended. |
| Parent-navigation fixture | Example 02 preserves `sh` when returning to `short_notes` but describes an empty directory even though `proofs` exists. That phrase may mean no matches. Its next step expects `short_notes` and `scratch` to match `sh`, although `scratch` lacks an `h` for strict subsequence matching. Clarify the fixture/matching expectation. |
| History | Parent navigation preserves the current query and resets scroll/highlight per active example 02. The older transcript proposed restoring per-directory query/cursor/scroll history; revisits and back/forward behavior are not fully specified by active examples. |
| Ctrl+d | The examples README mentions Ctrl+C or Ctrl+D as explicit exits. No active example establishes a Ctrl+d binding; keep it out of the active binding table pending a decision. |
| Query terminology | Active example 08 says content query, while content search is deferred. Its Escape/session-preservation assertions remain applicable; the wording does not activate content search. |
| Matching policy | Exact scoring, basename versus root-relative-path matching, and normalization beyond the case-insensitive examples need decisions. |
| Glob behavior | Example 03 defines traversal through nonmatching parents and a file-only result fixture. General directory-display behavior under glob constraints and CLI syntax remain undecided. |
| Exclusions | Hidden visibility and exclusion enforcement are distinct. Rule syntax, ordinary ignore-file integration, startup policy, and editor lifecycle need decisions. |
| Empty results | Enter behavior is specified for an empty directory; numbered-choice fallback is specified for any zero-visible-results state. Enter with a no-match query or incomplete empty scan is not settled. |
| Missing marks | Active example 05 disallows silent partial confirmation. The recovery interaction and handling of different confirmation commands need decisions. |
| Defaults and CLI | Startup depth 3 is proposed. Flag names, missing-root/default-root handling, output defaults, exit codes, remapping configuration, and supported platforms/terminals are undecided. |

## 13. Acceptance-source map

| Active example | Primary contract |
| --- | --- |
| [01](../tui_functionality_examples/01_directory_browsing.md) | Subsequence filtering, directory entry, first-visit query, no-match survival. |
| [02](../tui_functionality_examples/02_navigation_history.md) | Parent navigation, query retention, empty-directory confirmation. |
| [03](../tui_functionality_examples/03_recursive_search.md) | Live depth, file glob traversal, immediate depth eligibility. |
| [04](../tui_functionality_examples/04_multiple_search_roots.md) | Fixed ordered roots, effective depth, overlap deduplication. |
| [05](../tui_functionality_examples/05_persistent_selection.md) | Persistent marks, marked-list focus, confirmation rules, missing marks. |
| [07](../tui_functionality_examples/07_visibility_and_exclusions.md) | Hidden visibility, editable exclusions, enforcement toggle. |
| [08](../tui_functionality_examples/08_escape_and_cancellation.md) | Escape preserves discovery/session; explicit cancellation. |
| [10](../tui_functionality_examples/10_return_paths.md) | Invocation-time path/output configuration. |
| [12](../tui_functionality_examples/12_numbered_immediate_choice.md) | Visible numbered choice, marks bypass, fallback, output bytes, terminal decoding. |
| [13](../tui_functionality_examples/13_depth_ordering.md) | Exact sort tuple, live insertion, rendered targets, highlight stability. |
