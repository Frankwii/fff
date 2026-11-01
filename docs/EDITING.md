# Text editing

Fuzzy file finder uses a clean embedded Neovim by default. `nvim` must be on PATH; when no
backend is explicitly chosen and it cannot start, fff quietly falls back to
basic typing/Backspace. `--editor basic` / `--editor nvim` overrides TOML:

```toml
[editor]
backend = "basic"
```

An explicitly requested Neovim that cannot start reports the fallback in the
status line. An engine failure after startup also reports the problem and
preserves the last synchronized text in basic mode. Restart to change backends;
palette reload applies only to colors.

## Fields and modes

The query, location prompt, exclusions, bookmark name/path, bookmark and icon
filters, and palette filter all
initially start in **Normal mode**. New file/directory naming buffers start in
**Insert mode**, ready for typing; rename and extraction buffers start in Normal.
Press `i` or `a` to type in a Normal field. Directory navigation
preserves the initiating field's mode, including Insert, Normal, Replace, Visual
and Select modes. Pending motions/operators and command-line input are cancelled;
visual selections restart in the new query buffer. Returning to an existing
nonempty field preserves its editing state and cursor. The basic backend has no modes.

In query, new-name and extraction-destination buffers, Normal-mode `o` and `O`
enter Insert mode at the cursor without adding a line. In name/destination
buffers Enter does nothing in Normal or Insert mode; command-line Enter still
executes commands such as `:wq`. Query Enter keeps its navigation/confirmation
action. Rename and exclusion buffers retain multiline `o`, `O` and Enter.

In results Normal mode, `v`, `p`, `x`, `r`, `e`, `n` and `s` invoke file
operations; Insert mode keeps them as text. Choosers and operation buffers retain
their editing keys.

Borders show the mode. Normal/Visual uses a block cursor; Insert uses a bar.
Horizontal scrolling follows the cursor, and exclusions have a multiline viewport.
Neovim command entry appears on a separate line below the buffer, keeping its
contents visible while entering commands such as `:wq`.
Bracketed paste preserves newlines in exclusions and rename lists, and converts
them to spaces in single-line fields.

Neovim provides motions, operators, counts, registers, undo and Unicode editing.
Fields have separate scratch buffers, cursors and undo histories. Pending input
such as `f` or `r` shows **PENDING** without blocking fff; switching fields
cancels transient pending input or selection.

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

For example, type `ihello world`, press Escape, then `0dw`: the query becomes
`world`. `u` restores it and Ctrl+r reapplies the deletion.

## Application keys and Escape

In Normal mode, `q` exits and returns the invocation directory; `!` exits and
returns the current browsing directory. Both ignore marks and use the normal
path output format (`--relative` and `--null` apply). With `--no-dir`, they exit
successfully without output. They also work in the marked list. Insert/basic
fields treat them as text. Configure `quit` and `quit_current` in `[keys.normal]`.

Ctrl+c always cancels fff. Alt shortcuts, Ctrl/Alt+digits, marking with Tab,
and Ctrl+w retain their fff actions in results/marks. Space in Normal mode
marks/unmarks like Tab; in Insert mode it types a space. The marked list always
accepts Space because it has no text field.

Exclusions, rename lists, extraction destinations and new names
use Neovim's buffer lifecycle. Press `ZZ` to write and close (apply), or `ZQ`
to close without writing (discard); `:wq` and `:q!` do the same. In these
buffers `ZZ` always writes, so it applies even when nothing was edited (native
`ZZ` would close an unchanged buffer without writing). `:w` records a snapshot but keeps editing; closing later applies that last
written snapshot and discards any edits made since. Unchanged exclusions leave
the stored data alone. Validation errors keep the
previous data, close the editor, and show a short error at the bottom. Exclusion
errors include the line number; blank lines are ignored.

File operation details and the temporary backup guard are described in
[the operation guide](OPERATIONS.md).

Apart from that `ZZ` mapping, these editors have no fff Escape or Ctrl+s
bindings in Neovim: those keys go to Neovim, including its native `ZQ` (quit
without writing). Escape in Normal mode leaves the editor open. Exclusions return
to the main screen.

Bookmark forms are not buffer-lifecycle editors: Enter saves and Escape in
Normal mode cancels, like the location prompt. Escape from Insert/Visual mode
and Enter during command-line or pending input still go to Neovim; `ZZ`/`:wq`
and `ZQ`/`:q!` also save or cancel. Tab/Shift+Tab or Up/Down switch fields and
Alt+i opens the icon chooser. Save errors keep the form open. Basic editing uses Ctrl+s to apply and Escape to discard;
`ZZ` is literal text. Rebind list marking with `[keys.normal] mark`; the legacy
`apply` setting is accepted but ignored.

Enter in results navigates or confirms; in locations/bookmarks it navigates; in
bookmark forms it saves; in palettes it applies the selected palette. Pending Neovim input and command-line
entry receive Enter instead. In exclusions, Enter retains Neovim's behavior.

Escape from Insert, Visual, pending input or command entry returns toward Normal.
Escape when already Normal clears the main query, closes subordinate choosers
or cancels the bookmark form; exclusion and operation buffers keep Escape inside
Neovim.
It never exits the session. With basic editing it clears/closes immediately.

Digits are Vim counts in Normal mode and query text in Insert/basic mode.
Alt+j/Alt+k move list highlights in every mode. Up/Down control results and
choosers; in exclusions and during command-line editing they go to Neovim.

## Normal-mode list keys

These defaults apply in results, bookmarks, bookmark icons and palettes when the field is Normal,
and always in the marked list, which has no text field:

| Key | Results | Bookmark / palette chooser | Marked list |
| --- | --- | --- | --- |
| `j` / `k` | Next / previous result | Next / previous entry | Next / previous mark |
| `h` | Parent directory | Neovim motion | No action |
| `l` | Enter highlighted directory | Neovim motion | No action |
| `gg` / `G` | First / last result | First / last entry | First / last mark |

The palette chooser waits for a highlight to stay still for at least 100 ms
before previewing it. Moving again restarts the wait; Enter applies immediately. The location
prompt and exclusion editor keep Neovim's own `hjkl`, `gg` and `G` motions.
Insert, Visual, Replace, operator, pending and command-line modes keep their
editing behavior. Alt bindings remain available in every mode.

Unbound keys go to Neovim. A binding prefix such as `g` waits as **PENDING**; an
unmatched continuation sends both keys to Neovim, preserving commands such as
`gU`, `ge`, `gu` and `g~`. Escape after a lone `g` cancels it without clearing the
query. Configure sequences in [keys.normal](CONFIGURATION.md#normal-mode-keys).

## Isolation and terminal behavior

One private `nvim --embed --headless -u NONE -i NONE --noplugin -n` process edits
scratch buffers through [MessagePack-RPC](https://neovim.io/doc/user/api/#rpc-connecting).
No personal init, plugins, ShaDa, swap, backups, undo files or persistent history
are used. Logging goes to `/dev/null`; listener sockets are stopped. Communication
uses private child stdin/stdout, separate from selected-path output. RPC has a
bounded timeout and the child is reaped on exit.

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
