# Example 08: Escape, cancellation, and continued interaction

## Given

A file search over several roots is running at depth 3. Its content query is nonempty, partial results are visible, and three files are marked. No prompt or help view is open.

## When and then

1. Press Escape. The active discovery continues but the query clears. 
2. Press Escape again. The active discovery continues or is finished and nothing else happens.
3. Press Ctrl+c. The application exits without selected-path output.
