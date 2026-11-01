# Example 14: numbered Enter with Alt+digits

Source: explicit user request. Alt+number has the same effect as manually
highlighting the corresponding visible entry and pressing Enter. Ctrl+number
retains example 12's immediate, single-target confirmation.

## Given

The current directory is `/home/frank/notes`, depth 1. A complete rendered frame
shows these rows, with highlight on row 1:

| Label | Path | Kind |
| --- | --- | --- |
| 1 | /home/frank/notes/algorithms | Directory |
| 2 / 0 | /home/frank/notes/report.tex | File |

`/home/frank/papers/Turing.pdf` is already marked.

## When and then, using fresh sessions

- Alt+1 enters `algorithms`, clears the query, preserves marks and depth, and keeps
  the session open. Startup roots, if explicitly supplied, remain fixed.
- Alt+2 highlights `report.tex` and confirms the marked path followed by
  `report.tex`, exactly as Enter on that file would. It exits successfully.
- Alt+0 and Alt+9 also activate `report.tex`: 0 means the last visible row, and
  out-of-range digits clamp to the last visible row.
- Ctrl+2 still returns only `report.tex`, ignoring marks.

## Assertions

- Both shortcut families use the same visible labels from the last fully rendered
  results frame. After paging/filtering, labels refer to the new rendered rows.
  Pending discovery cannot redirect a shortcut to an unrendered path.
- Alt+number does not bypass missing-mark checks or duplicate a marked target.
- With no visible rows, Alt+number follows Enter's empty-result behavior: confirm
  marks plus the current directory only for a completed physically empty directory;
  stay open for a no-match query in a nonempty directory or incomplete empty scan.
  Ctrl+number retains its unconditional current-directory fallback.
- Plain digits edit the query. Alt+digits only activate entries in results focus;
  they do not activate background results while a prompt or marked pane is focused.
- Legacy ESC+digit and distinct CSI-u Alt+digit events decode as Alt chords.

## Interface request

The marked list appears to the right of results in a terminal wide enough for both
panes. It remains focusable with Alt+Space; full paths and missing-path indicators
remain inspectable. Narrow terminals may use a stacked layout. Color and pane
borders distinguish focus and selection without replacing shortcut labels or
textual missing/progress indicators.
