# Example 11: unusual names, missing files, and errors

STATUS: Delayed until future versions. Ignore this functionality; it has not been reviewed by me.

## Given

A directory contains these illustrative names: lecture notes.tex; Cónsul–Jorba.pdf; --draft.tex; what's next?.md; one name containing a newline; and another containing a terminal control character or non-UTF-8 bytes. It also contains an unreadable subdirectory and a directory symlink back to an ancestor.

The fixture specifies the exact original filename bytes. Display labels and numeric option labels are distinct from those bytes.

## When and then

1. Browse and filter the entries. Display labels escape characters that could split rows or control the terminal, while preserving a way to distinguish different original names.
2. Mark unusual names with Tab and confirm with Ctrl+w in machine-oriented output mode. The original absolute path bytes are returned exactly with NUL separators.
3. In another run, press Ctrl+o on --draft.tex. The opener receives that pathname as data rather than interpreting it as an option.
4. Start recursive discovery. An unreadable subdirectory reports an inspectable error, while accessible locations continue to yield sorted results.
5. Delete the highlighted file from another terminal and press Ctrl+o. A missing-file error appears without exiting or opening the next row's file.
6. Press Ctrl+r to refresh; surviving marks and query settings remain.

## Assertions

- Spaces, quotes, wildcard characters, newlines, and leading hyphens do not corrupt selection or output.
- Fuzzy matching supports non-ASCII text; the final case/accent-normalization policy is a separate decision.
- Terminal control characters are not executed by rendering filenames.
- Directory symlinks are not followed by recursive discovery by default, preventing cycles; explicitly navigating into one remains possible.
- A missing target of Ctrl+2 reports failure and keeps the session open; it does not accept a different row or emit partial path output.
- Resizing cannot discard the current query, depth, roots, or marks.
