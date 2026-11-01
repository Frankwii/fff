# Example 03: change recursive depth live

## Given

The filename view searches /home/frank/notes with maximum depth 1. The filename glob constraint is *.tex, the fuzzy query is empty, and no items are marked. The fixture contains exactly these files and their necessary parent directories:

| Relative file path | Depth |
| --- | --- |
| zebra.tex | 1 |
| topic/alpha.tex | 2 |
| topic/details/beta.tex | 3 |

A positive file glob limits result files; it does not prevent traversal through their parent directories. In this fixture directory entries are omitted from the constrained result list.

## When and then

| Step | Input | Expected file results after the requested discovery completes |
| --- | --- | --- |
| 1 | No input | zebra.tex only; the depth indicator says 1. |
| 2 | Alt+Right | Depth 2; zebra.tex, then topic/alpha.tex. |
| 3 | Alt+Right | Depth 3; zebra.tex, then topic/alpha.tex, then topic/details/beta.tex. |
| 4 | Alt+Left | Depth 2; the depth-3 file disappears immediately. |
| 5 | Ctrl+2 | Return only /home/frank/notes/topic/alpha.tex and exit successfully. |

The second label belongs to alpha.tex even though alphabetic ordering alone would have placed it before zebra.tex. Numeric labels follow the displayed order after applying the depth rule.

## Assertions

- Increasing depth extends the current search without requiring a separate view or a new application invocation.
- Decreasing depth immediately excludes known results beyond the new limit; late deeper results cannot reappear.
- The root, fuzzy query, glob constraint, marked paths, and current directory survive a depth adjustment.
- Repeated Alt+Left at depth 1 leaves depth at 1 and the picker open.
- Increasing from depth 3 to 4 when no further descendants exist changes the indicator but retains the same results.
- Scan progress is visible and input remains usable while new levels are being discovered.
- Example 13 defines ordering when files and directories are mixed or worker results arrive out of order.
