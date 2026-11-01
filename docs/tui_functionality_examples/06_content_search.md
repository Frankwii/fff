# Example 06: content search and document results

STATUS: Delayed until future versions. Ignore this functionality; it has not been reviewed by me.

## Given

The active roots are /home/frank/notes and /home/frank/papers, at maximum depth 3. They contain LaTeX sources and PDFs with the literal phrase stable manifold. The filename and content views retain separate queries. Content matching starts in literal mode.

## When and then

1. Press Ctrl+g and type stable manifold. Content search runs over the active roots and depth, and results appear with their filenames and matching excerpts.
2. Press Alt+d, enter *.tex, and press Enter. Matching source files remain in the result list. Clearing this constraint through the same prompt allows document matches again.
3. Highlight a LaTeX match and press Ctrl+o. Neovim opens the original file at the matching line if a reliable source-line position is available. Leaving Neovim restores the search session.
4. Highlight a PDF match and press Ctrl+o. The configured document viewer opens the PDF, using a page only when reliable page metadata and opener support are available.
5. Press Alt+Right. Maximum depth becomes 4, retaining the content query, matching mode, and marks.
6. Press Ctrl+f. The filename view returns with its earlier filename query and the updated shared depth.

## Assertions

- A content query and a filename constraint are distinct controls.
- Alt+r toggles literal/regular-expression content matching; the mode is visible. An invalid regular expression reports feedback and does not quit.
- Across matching files, effective depth is the primary ordering key. Within the same depth, file pathname order is deterministic; a file's match rows use source order when positions are available.
- Fuzzy scores apply to filename filtering, not to the matching excerpt itself.
- A numbered result row identifies one match; Ctrl+2 returns that row's containing file path and exits, without an excerpt or line number in the path output.
- Repeated matches can be inspected, but marking the containing file toggles its one shared file mark rather than creating duplicate output paths.
- Adapter failures are reported per file while other searches remain usable.
- Stale content-query results cannot overwrite a newer query's results.
