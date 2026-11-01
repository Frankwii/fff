# Example 07: hidden files, ignore rules, and exclusions

## Given

The current root contains main.tex, .draft.tex, and build/output.log. An ignore parameter passed on program invocation excludes build. The maximum depth is 2, no filename glob constraint is set, and the fuzzy query is empty.

For this test, hidden entries are hidden and ignore rules are respected. Whether ordinary depth-1 browsing should respect ignore rules by default remains reviewable.

## When and then

1. Let discovery finish. main.tex appears, while .draft.tex and the ignored build subtree do not.
2. Press "Alt+.". .draft.tex becomes available; build remains ignored.
3. Press Alt+H (notice the caps). build and build/output.log become available. All depth-1 entries precede the depth-2 log, with directories before files within depth 1.
4. Press Alt+e, and start editing the exclusion list. It currently contains "build". Create another line and enter *.tex. Press Ctrl+w to apply changes to the exclusion list. Everything remains remains visible. Press Alt+H and the search is empty.

## Assertions

- If any file is marked and then hidden by some filter on the exclusion list, it should remain marked.
- Dotfiles and respect for exclusion list.
- The active policies and full exclusion list are inspectable.
- Depth, query, current directory, roots, and marks survive these policy changes.
- Entries excluded by the new policy disappear immediately. When un-excluded a new file discovery is triggered.
- An exclusion matching a directory prunes its subtree.
- An empty result set leaves the picker open.
- Ideally, the exclusion list is edited in a neovim/editor buffer. Similar to nnn's renaming interface.
