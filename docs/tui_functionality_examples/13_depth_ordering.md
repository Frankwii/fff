# Example 13: shallower-first display ordering during live updates

## Given

The current root is /home/frank/example, maximum depth is 3, and the fuzzy query is empty. Ignore and hidden-file policies do not exclude any fixture entry. These are all the result candidates:

| Relative path | Kind | Effective depth |
| --- | --- | --- |
| alpha | Directory | 1 |
| zebra.txt | File | 1 |
| alpha/beta | Directory | 2 |
| alpha/aa.txt | File | 2 |
| alpha/beta/00.txt | File | 3 |

Every empty-query fuzzy score is equal, and the main viewport fits all five rows.

## When

Inject discovery events in the deliberately scrambled order: alpha/beta/00.txt, alpha/aa.txt, zebra.txt, alpha/beta, alpha. Render a completed frame after each event.

## Then

After every frame, known candidates are sorted by effective depth first. On completion, the displayed rows must be exactly:

| Visible label | Relative path | Reason |
| --- | --- | --- |
| 1 | alpha | Depth 1 directory. |
| 2 | zebra.txt | Depth 1 file, before every depth-2 candidate. |
| 3 | alpha/beta | Depth 2 directory, before files at depth 2. |
| 4 | alpha/aa.txt | Depth 2 file, despite aa sorting before beta alphabetically. |
| 5 | alpha/beta/00.txt | Depth 3 file, despite its basename sorting before all the others. |

Ctrl+2 now returns /home/frank/example/zebra.txt and terminates successfully. A deeper directory must not outrank that shallower file.

## Additional assertions using fresh runs

- With a nonempty query, a matching shallow candidate still precedes a better-scoring deeper match. Nonmatching shallow entries are omitted, not forced into the results.
- Within one depth, matching directories precede matching files. Within one kind and depth, higher fuzzy scores come first; remaining ties use startup search-root order, then bytewise absolute original pathname order. For overlapping roots, use the earliest startup root that yields the path's minimum relative depth.
- Newly discovered shallower entries are inserted before known deeper entries. Progress identifies incomplete discovery; no claim is made about entries not yet discovered.
- Alt+Left from depth 3 to 2 immediately removes the last row. A late event for it cannot reintroduce it.
- Alt+Right back to 3 restores eligibility and eventually the same completed order.
- Overlapping roots use minimum relative depth for an identical absolute path and produce one candidate.
- Discovery updates retain a highlighted path when it still matches, even if its row number changes. Numbered shortcuts follow the last rendered labels rather than the highlight or a not-yet-rendered candidate array.
