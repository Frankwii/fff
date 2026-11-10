# Text editing

Every text field is a clean embedded Neovim (no user configuration or
plugins); `nvim` must be on PATH. When it cannot start, fff exits with an error
naming the problem, and if it fails during a session, the session ends with that
error.

## Fields and modes

The query, exclusions, file filters, workspace and icon filters, and palette
filter all initially start in **Normal mode**. New file/directory naming
buffers, the go-to prompt (`/`), the gx/gX command prompt (zsh users get zsh's own line instead), the workspace
form's name and path, and the query of `fff pick`, `fff seek`,
`fff explorer save` and a list read from stdin start in **Insert mode**, ready for typing; rename and
extraction buffers and the contents query start in Normal. Tab
(`switch_query`), in either mode, moves between the names and contents
queries, in every program, the file dialogs included, and the other query is typed
into in the mode it was pressed in (Insert stays Insert, Normal stays
Normal; pending input and command-line entry are cancelled, and a visual
selection starts again there). Each keeps its own text, cursor and undo
history. Switching programs (`gm`) keeps the mode it was pressed in, in
whichever query the new program types into.
Press `i` or `a` to type in a Normal field. Directory navigation
preserves the initiating field's mode, including Insert, Normal, Replace, Visual
and Select modes, except from the go-to prompt: its Insert mode is for
typing the path, and the query starts in Normal mode in the folder it goes
to, so keys act on the results at once. Pending motions/operators and command-line input are cancelled;
visual selections restart in the new query buffer. Returning to an existing
nonempty field preserves its editing state and cursor.

In query, new-name and extraction-destination buffers, Normal-mode `o` and `O`
enter Insert mode at the cursor without adding a line. In name/destination
buffers Enter applies from Normal or Insert mode; command-line Enter still
executes commands such as `:s`. Query Enter keeps its navigation/confirmation
action. Rename, exclusion and filter buffers retain multiline `o`, `O` and Enter.

In results Normal mode, `v`, `p`, `x`, `X`, `u`, `r`, `R`, `E`, `n` and `s` invoke file
operations, in the results only (not the marked list, nor the query in focus); Insert mode keeps them as text. Likewise `n`, `e`, `d` and `r` add,
edit, delete and reload workspaces in the workspace chooser's Normal mode (Alt+n,
Alt+e, Alt+d and Alt+r work in every mode). Other choosers and operation buffers
retain their editing keys.

Titles show the mode. The query box (names and contents) shows it as a
filled label after its title, as a statusline does, and takes the mode's
color on its border, title and prompt: `accent` in Normal, `success` in
Insert, `staged_folder` in Visual/Select, `error` in Replace and `marked` in
Command, Operator and Pending modes. Other boxes keep their border color and
add `· MODE` to their title. Normal/Visual uses a block cursor; Insert uses a bar.
Horizontal scrolling follows the cursor, and exclusions and filters have a multiline viewport.
Neovim command entry appears on a separate line below the buffer, keeping its
contents visible while entering commands such as `:wq`.
Bracketed paste preserves newlines in exclusions, filters and rename lists, and converts
them to spaces in single-line fields.

Neovim provides motions, operators, counts, registers, undo and Unicode editing.
Fields have separate scratch buffers, cursors and undo histories. Pending input
such as `f` or `r` shows **PENDING** without blocking fff; switching fields
cancels transient pending input or selection.

The table lists Neovim's own keys, as they work in fields without fff
bindings (exclusions, filters, rename lists, prompts). In the results' query in
focus, the default keys typed as text edit it as Neovim's do, while keys
not typed as text (Enter, Tab, arrows, Alt and Ctrl keys) still act on the
results below. `:` there opens the command picker instead
of Neovim's command line: the functions that work where you are, to run one
by name (Enter) or go back (Escape).

Typing gives the query the focus: `i`, `a` or any other key no binding has
in the results goes to Neovim and moves the focus up to the query, and
Escape from Insert mode stays there. In the query's Normal mode `h`, `l`,
`x`, `p`, `u`, `v`, `:` and the other keys typed as text edit it, but `j`
and `k`, which go down into the results to the first or last row
(`highlight_first`, `highlight_last`), and `gc`, which edits the
configuration, unless you
bound one there yourself (a key you bind always comes first; its
[focus](CONFIGURATION.md#the-query-in-focus) decides where). Enter, Tab and
the Alt and Ctrl keys still act on the results. Whichever of the two lacks
the focus is drawn dimmed: the query's border, or the results' border and
highlight bar. Escape in the query's Normal mode (`toggle_query_focus`)
gives the results the focus back. The Keys screen works the same way.

| Action | Keys in Normal mode |
| --- | --- |
| Enter text | `i`, `a`, `I`, `A` |
| Move within text | `w`, `b`, `0`, `$` |
| Delete/change a word | `dw`, `ciw` |
| Delete a line | `dd` |
| Undo / redo | `u` / Ctrl+r |
| Characterwise selection | `v`, then motions and `y`, `d` or `c` |
| Linewise/block selection | `V` / Ctrl+v |
| Paste an internal register | `p` / `P` |
| Edit exclusion lines | `o`, `O`, motions and line operators |
| Substitute text | `:s/old/new/`, then Enter |

For example, type `ihello world`, press Escape, then `0cwbye` and Escape: the
query becomes `bye world`. `u` restores it and Ctrl+r reapplies the change.

## Application keys and Escape

In Normal mode, `q` exits choosing nothing (no output, no directory change,
a dialog cancelled); `!` exits with the current browsing directory: `fff
roam` changes to it (`fff window` starts a shell there), `fff pick` and
folder dialogs print it in the normal path output format (`--relative` and
`--null` apply), and a save dialog saves there. Both ignore marks. In
`fff seek`, whose directory never changes, `!` does nothing. They also work
in the marked list. Insert-mode fields treat them as text. They are `quit`,
`quit_here`, `print_here` and `save_here` on the
[Keys screen](CONFIGURATION.md#keys).

Ctrl+c always cancels fff. Alt and Ctrl shortcuts, Ctrl/Alt+digits and
Ctrl+Enter retain their fff actions in results/marks, in every mode. Space in
Normal mode marks/unmarks; in Insert mode it types a space. Tab never marks:
it switches between the names and contents queries.
The marked list always accepts Space because it has no text field. In Normal mode `m` marks every
listed row (all filtered results, not only those on screen, or every row of the
marked list) and `M` inverts the listed rows' marks, like nnn's `a` and `A`;
marks on paths that are not listed stay as they are.

Multiline buffers (exclusions, file filters and rename lists)
use Neovim's buffer lifecycle. Press `ZZ` to write and close (apply), or `ZQ`
to close without writing (discard); `:wq` and `:q!` do the same. In these
buffers `ZZ` always writes, so it applies even when nothing was edited (native
`ZZ` would close an unchanged buffer without writing). `:w` records a snapshot but keeps editing; closing later applies that last
written snapshot and discards any edits made since. Unchanged exclusions or filters leave
the stored data alone. Validation errors keep the
previous data, close the editor, and show a short error at the bottom. Exclusion
errors include the line number; blank lines are ignored.

Apart from that `ZZ` mapping, these editors have no fff Escape or Ctrl+s
bindings in Neovim: those keys go to Neovim, including its native `ZQ` (quit
without writing). Escape in Normal mode leaves the editor open. Exclusions and
file filters return to the main screen.

Single-line editors (new names, extraction destinations and workspace forms)
are not buffer-lifecycle editors: Enter applies and Escape in Normal mode
cancels, like the location prompt. Validation errors keep them open. Escape from Insert/Visual mode
and Enter during command-line or pending input still go to Neovim; `ZZ`/`:wq`
and `ZQ`/`:q!` also apply or cancel. In workspace forms, Tab/Shift+Tab or
Up/Down switch fields and settings, as j/k do on the icon and settings rows
and in a Normal-mode name or path; Space on a setting ticks it, and Space
on the icon row opens the icon chooser.

Enter in results navigates or confirms; in locations it navigates, in workspaces it opens one; in
workspace forms it saves; in palettes it applies the selected palette. Pending Neovim input and command-line
entry receive Enter instead. In exclusions and filters, Enter retains Neovim's behavior.

Escape from Insert, Visual, pending input or command entry returns toward Normal.
With the query in focus, Escape in Normal mode gives the results the focus.
Escape when already Normal in the results clears the main query (with a contents query, the query
being typed into, or the other one once it is empty), closes subordinate choosers
or cancels the workspace form; exclusion and operation buffers keep Escape inside
Neovim. Escape never clears the marks.
It never exits the session.

Digits are query text in Insert mode. In Normal-mode results 1–9 highlight
that row (`highlight_row` on the Keys screen; `"none"` gives them back), and
Ctrl+Alt+1–9 set the search depth; elsewhere in Normal mode they are Vim
counts. `j`/`k` move list highlights in Normal mode, Alt+j/Alt+k while typing. The rows of
the choosers (workspaces, workspace icons, palettes) and the `:` and sorting
pickers are numbered as the results are: Alt+1 through Alt+9 is Enter on
that row, Alt+0 on the last, in every mode; a click highlights a row, the
wheel moves the highlight and a double click is Enter on it. Up/Down switch
to the next/previous tab in the results, in either mode. Up/Down move
choosers' highlights (in the workspace chooser they reorder workspaces); in
exclusions, filters and during command-line editing they go to Neovim.

## Normal-mode list keys

These defaults apply in results, workspaces, workspace icons, palettes and
the `:` and sorting pickers when the field is Normal, and always in the marked
list, which has no text field:

| Key | Results | Choosers and pickers | Marked list |
| --- | --- | --- | --- |
| `j` / `k` | Next / previous result | Next / previous entry | Next / previous mark |
| `h` | Parent directory | Neovim motion | No action |
| `l` | Enter the highlighted directory, or go to the highlighted file's folder | Neovim motion | Enter the highlighted directory, or go to the highlighted file's folder |
| `gg` / `G` | First / last result | First / last entry | First / last mark |
| `m` / `M` | Mark all results / invert their marks | Neovim command | Mark all rows / invert their marks |
| `gc` | [Edit the configuration](CONFIGURATION.md#editing-while-fff-runs), then apply it | Neovim command | Edit the configuration, then apply it |

The palette chooser waits for a highlight to stay still for at least 100 ms
before previewing it. Moving again restarts the wait; Enter applies immediately. The location
prompt and exclusion editor keep Neovim's own `hjkl`, `gg` and `G` motions.
Insert, Visual, Replace, operator, pending and command-line modes keep their
editing behavior. Alt bindings remain available in every mode.

Unbound keys go to Neovim. A binding prefix such as `g` waits as **PENDING**; an
unmatched continuation sends both keys to Neovim, preserving commands such as
`gU`, `g~` and `g?` (not `gu`: the `u` after the `g` undoes a file operation) (`gc`, Neovim's comment operator, edits the
configuration instead). Escape after a lone `g` cancels it without
clearing the query. In the results the default `p`, `x` and `u` copy marked
entries, trash and undo file operations; with the query in focus they are
Neovim's put, delete and undo. Every key is rebound on the [Keys screen](CONFIGURATION.md#keys) (F1).

## Isolation and terminal behavior

One private `nvim --embed --headless -u NONE -i NONE --noplugin -n` process edits
scratch buffers through [MessagePack-RPC](https://neovim.io/doc/user/api/#rpc-connecting).
No personal init, plugins, ShaDa, swap, backups, undo files or persistent history
are used. Logging goes to `/dev/null`; listener sockets are stopped. Communication
uses private child stdin/stdout, separate from selected-path output. RPC has a
bounded timeout and the child is reaped on exit.

The preview pane's [syntax colors](CONFIGURATION.md#syntax-colors) come from
another private process, `nvim --embed --headless --clean -n -i NONE`, which
never edits: it adds only the parsers and queries under Neovim's
`site` data directory to its runtime path.

Text-local commands such as substitutions work. Commands needing Neovim windows,
external programs or a full editor UI are outside this integration. Closing a
field captures its written contents and starts a fresh clean engine;
this resets Neovim undo histories and registers. Writing a scratch buffer records
an in-memory snapshot; fff applies it only when the editor closes. A query close retains the
query and returns to Normal editing. An unexpected engine crash causes fallback
without confirming paths.

The requested keyboard protocol reports Escape distinctly where supported. With
ESC-prefix terminal reporting, rapidly typed Escape+digit can look like Alt+digit;
fff preserves Alt shortcut decoding.
