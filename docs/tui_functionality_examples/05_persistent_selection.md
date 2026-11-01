# Example 05: persistent marked selections

## Given

The picker is at /home/frank/notes/docs. Its only file is hartman_grobman.pdf. The marked collection is empty and discovery is complete.

## When and then

1. Press Tab on hartman_grobman.pdf. Its full path is marked; the cursor advances if a following result exists.
2. Navigate to /home/frank/papers, highlight Turing 1952.pdf, and press Tab. Two full paths are now marked in that order.
3. Change the query, adjust the depth using Alt+Right, and hide or show hidden files using "Alt+.". Both marks persist even if absent from the current results.
4. Meanwhile, the marked-item is somewhere on the screen and its list shows both full paths.
5. Press Alt+Space to highlight the marked list. Select hartman_grobman.pdf and press Tab. This removes the mark, not the file. hartman_grobman.pdf is no longer highlighted but remains in the list until the user leaves the mark list, at which point it is removed from there. Press Escape or Alt+Space again to return.
6. Highlight another file without marking it and press Ctrl+w.
7. The picker returns only /home/frank/papers/Turing 1952.pdf and exits successfully.

## Assertions

- Ctrl+w returns only the marked collection; it never appends or falls back to the highlighted path. With no marks, it does nothing and the picker remains open.
- Enter on a highlighted file returns all marked paths in insertion order, followed by the highlighted file if it is not already marked, then exits. With no marks, it returns only that file. A marked highlighted file is returned once.
- Enter on a highlighted directory navigates into it, preserving marks.
- Enter in an empty directory confirms all marked paths plus the current directory and exits, treating the current directory like a highlighted file. Append the directory only if it is not already marked; with no marks, return only the current directory.
- Tab toggles the mark rather than duplicating it. A directory mark means the directory itself, not its descendants.
- Marks preserve their insertion order across navigation and search refreshes.
- Missing marked paths are displayed as missing and prevent silent partial confirmation.
- Ctrl+2 is a distinct immediate-choice action: it returns only the second numbered visible option, even when the marked collection is nonempty. Example 12 tests that behavior.
