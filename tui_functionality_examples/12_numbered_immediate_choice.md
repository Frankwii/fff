# Example 12: numbered immediate choice

## Given

The main filename results are fully rendered. These three options are visible and labeled as follows; all have depth 1, directories first. The query is empty. The highlight is on option 1.

| Visible shortcut label | Path | Kind |
| --- | --- | --- |
| 1 | /home/frank/notes | Directory |
| 2 | /home/frank/papers | Directory |
| 3 | /home/frank/todo.txt | File |

An unrelated path /home/frank/old.txt is already marked. Machine-oriented output is enabled. For the keyboard layer of this scenario, the terminal delivers a distinct decoded Ctrl+2 event.

## When

Press Ctrl+2 once.

## Then

- The picker returns exactly /home/frank/papers followed by one NUL byte and exits successfully.
- It does not enter papers, launch an opener, wait for Enter, or ask for confirmation.
- It does not return the highlighted notes directory or the marked old.txt path.
- No interface text or status message appears in the returned path data.

## Further assertions using fresh runs

- Ctrl+3 returns only /home/frank/todo.txt.
- Ctrl+4, Ctrl+5, ..., Ctrl+9, Ctrl+0 with only three visible options returns the third option (last visible).
- Ctrl+0 returns the last visble option.
- After PageDown/PageUp, numbers refer to the newly visible rows, not absolute indexes from the beginning of the whole result list. Additional rows beyond the first nine visible options that are not the last one (which is marked with a zero) have no numeric shortcut.
- Typing plain 2 edits the query and never triggers an immediate choice.
- Headers, depth separators, error messages, and progress lines do not consume option numbers.
- Filtering updates labels to match the displayed result rows.
- Selecting via these keybindings has preference over anything marked.
- With no visible results, any numbered-choice shortcut returns only the current directory and exits successfully, ignoring all marks. This applies to both an empty directory and a query with no matches.

## Separate terminal integration check

A physical Ctrl+2 must reach the application as its own chord. Where the input stream cannot distinguish it from another key, the application must not guess.
