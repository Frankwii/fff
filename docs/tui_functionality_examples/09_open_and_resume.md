# Example 09: open a file and resume the picker

STATUS: Delayed until future versions. Ignore this functionality; it has not been reviewed by me.

## Given

The filename view highlights /home/frank/notes/sources/hartman_grobman/main.tex. Its depth is 3, its fuzzy query is main, and two other files are marked. Discovery is complete. Neovim is the configured text editor.

## When and then

1. Press Ctrl+o. Neovim opens the highlighted source file and takes over terminal input and output normally.
2. Edit the file and leave Neovim.
3. The picker resumes at the current terminal size, with query main, depth 3, the same roots, the same two marks, and the same highlighted path if it still exists.
4. Press Alt+g. The filename view browses the highlighted file's containing directory with that file highlighted. This explicit reveal clears the filename query so it cannot conceal the revealed entry.
5. Press Ctrl+r if needed. The listing refreshes without losing surviving marks or changing depth.

## Assertions

- Ctrl+o opens only the highlighted file and does not confirm any output or quit the picker.
- A directory highlight is handled by Enter for navigation; Ctrl+o on a directory leaves the picker unchanged with feedback in this proposed keymap.
- The editor receives the original full path as data, including spaces and punctuation.
- Input mode, cursor visibility, and rendering are restored after the editor returns, including a nonzero editor exit.
- A launch failure is reported without discarding the session.
- An explicitly opened PDF uses its configured document viewer. Interactive terminal viewers use the same suspend/resume behavior; external viewers that return immediately do not block continued use of the picker.
- Ctrl+2 remains a different action: return the second shown path and terminate rather than open an editor.
