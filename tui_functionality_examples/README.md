# Terminal file picker: behavior examples

Revision 2. Rust custom TUI. These documents describe behavior before implementation and are intended to become acceptance tests. All filesystem fixtures are illustrative.

## Required direction

- The picker runs in a terminal, supports fuzzy filename matching, and remains responsive during searches.
- Its current directory, maximum search depth, active roots, and selected-item list are visible or immediately inspectable.
- Filename discovery has a live-adjustable maximum depth. Increasing or decreasing it preserves the query and session.
- Matches at lesser depths always precede matches at greater depths. Directory-first ordering applies within a depth, followed by fuzzy ranking.
- A numbered shortcut such as Ctrl+2 immediately returns the second option shown and terminates successfully. This is distinct from navigating into a directory.
- Escape must not destroy the entire session. That should be reserved to Ctrl+C or Ctrl+D

## Terms and conventions

Current directory: the location being browsed. Search roots: directories searched recursively. Selected items: marked output items, independent of the roots. Highlight: the item under the cursor. Visible option: a selectable result row, excluding titles, separators, progress, and errors.

The filename view covers both ordinary browsing at depth 1 and recursive discovery at greater depths. The proposed startup depth is 3, matching rofi-ff. There is no separate recursion toggle that must be enabled before changing depth. Content search is a separate view over the same roots and depth settings.

Immediate children have depth 1; a child of a child has depth 2. Across overlapping roots, a path appears once at its minimum depth relative to an active root. Ordering applies to known matching candidates; an incomplete scan is visibly labeled, and newly discovered shallower candidates are inserted before known deeper ones.

The exact sorting priority is: depth ascending; directories before files; fuzzy score descending; startup search-root order; absolute original pathname in bytewise ascending order as the final tie-breaker. An empty filename query gives every candidate the same fuzzy score. Root order takes precedence over pathname order when the preceding criteria tie; it does not override depth. For a path found under overlapping roots, use its minimum relative depth and the earliest startup root that yields that depth.

## Keybindings

[KEYBINDINGS.md](KEYBINDINGS.md) defines the proposed common keymap, shortcut numbering, terminal compatibility, and input conventions. Each example also lists its relevant keys. These bindings are proposals except for the requested numbered immediate-choice behavior; Ctrl+digit reporting remains terminal-dependent.

Printable characters always edit the focused query or prompt. Plain digits remain valid query text. Names such as main.tex or paths in tables are data, not commands to execute.

## Independent examples

1. [Directory navigation with fuzzy filtering](01_directory_browsing.md)
2. [Parent navigation, history, and empty directories](02_navigation_history.md)
3. [Live depth changes](03_recursive_search.md)
4. [Multiple and overlapping roots](04_multiple_search_roots.md)
5. [Persistent marked selections](05_persistent_selection.md)
6. [Content search and document results](06_content_search.md)
7. [Hidden files, ignore rules, and exclusions](07_visibility_and_exclusions.md)
8. [Escape, cancellation, and continued interaction](08_escape_and_cancellation.md)
9. [Opening a file and resuming the picker](09_open_and_resume.md)
10. [Returning exact paths to the caller](10_return_paths.md)
11. [Unusual names, missing files, and errors](11_filenames_and_errors.md)
12. [Numbered immediate choice](12_numbered_immediate_choice.md)
13. [Shallower-first display ordering during live updates](13_depth_ordering.md)

## Proposed defaults and open decisions

The startup depth of 3, most shortcut assignments, content-search defaults, ignore-file defaults, selection confirmation rules, and Escape priority are reviewable proposals. Depth priority and immediate numbered choice reflect explicit requirements. The examples establish observable outcomes without prescribing a widget layout, thread model, or elapsed-time targets.

All keys are remappable. Each test fixture should specify its mode, roots, maximum depth, query, policy settings, marked paths, viewport, and completed scan results as relevant. Tests involving live updates should inject events deterministically rather than depending on filesystem timing. Physical terminal key decoding and application behavior should be tested separately.
