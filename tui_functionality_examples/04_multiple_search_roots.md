# Example 04: multiple and overlapping search roots

## Given

The picker starts with these search roots, in this order: /home/frank/notes and /home/frank/papers. Its initial current directory is /home/frank/notes, the first search root, and its maximum depth is 2. The fixture includes:

- /home/frank/notes/appendix/turing.pdf, at depth 2 relative to notes.
- /home/frank/papers/turing.pdf, at depth 1 relative to papers.

There are no marked items. The fuzzy query is turing, so only file results match and no directory is highlighted.

## When and then

1. Let discovery finish. papers/turing.pdf appears before notes/appendix/turing.pdf because its depth is lesser, regardless of startup root order. In case of a tie the root order should break it. The current directory remains /home/frank/notes.
2. Press Alt+Right. The common maximum depth becomes 3 for both roots; the current directory remains /home/frank/notes and papers/turing.pdf remains highlighted.
3. Press Alt+l, enter /home/frank, and press Enter. The program outputs the path of turing.pdf then halts.

## Overlap assertion

In a fresh run, supply /home/frank/notes, /home/frank/papers, and /home/frank/notes/appendix as startup roots, in that order. The initial current directory is still /home/frank/notes. The appendix/turing.pdf file has effective depth 1, appears once, and is sorted using that depth. In a separate run without the nested root, that file has depth 2 relative to notes.

## Additional assertions

- The picker always has exactly one current directory, including when multiple search roots are active. Search roots broaden the search scope; they do not replace or eliminate the current directory.
- The initial current directory is the first startup search root. Subsequent navigation can change the current directory without changing the search roots or their order.
- Search roots are fixed for the session. Adding, removing, or resetting roots live is unavailable in the initial version of the program; changing them requires a new invocation.
- Supplying a duplicate startup root creates no duplicate root or result.
- The active roots and each result's effective depth and root-relative path are inspectable.
- Alt+Right increases the common maximum depth for every active root. Similarly for Alt+Left.
- Marked output paths are independent of the search-root list and persist across navigation.
