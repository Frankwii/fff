# Example 01: directory navigation with fuzzy filtering

## Given

The filename view is at /home/frank with depth 1, no query, no marked items, and completed discovery. Hidden and ignored entries are absent from this fixture. It contains:

| Name | Kind |
| --- | --- |
| Documents | Directory |
| Downloads | Directory |
| builds | Directory |
| todo.txt | File |

Downloads contains report.tex. The current directory and maximum depth are visible. The cursor initially highlights Documents, the first directory in bytewise pathname order.

## When and then

| Step | Input | Expected result |
| --- | --- | --- |
| 1 | Type dwn | Downloads matches the ordered letters d, w, n with gaps and case-insensitive matching. It becomes the highlighted result. |
| 2 | Enter | The current directory changes to /home/frank/Downloads; the depth remains 1; the query starts empty on this first visit. |
| 3 | Type zzzz | An empty-results message appears; the current directory and session remain available. |
| 4 | Escape | With discovery complete and no subordinate view open, the query clears and report.tex reappears. |
| 3 | Type rpx | report.tex matches by fuzzy subsequence. |
| 4 | Enter | The program outputs /home/frank/Downloads/report.tex and halts. |

## Assertions

- Typing edits the query; no character requires a separate search command.
- At depth 1 all candidates are immediate children. At greater depths the same view includes descendants.
- Depth is the primary sort key; directories precede files within that depth, followed by fuzzy score and pathname ties.
- Changing the fuzzy query does not restart filesystem discovery.
- A no-match query does not quit or lose marked items.
- The highlighted entry is identified by its path rather than only by its row number.
