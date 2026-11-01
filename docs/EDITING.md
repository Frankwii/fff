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
initially start in **Normal mode**. Press `i` or `a` to type. Directory navigation
preserves the initiating field's mode, including Insert, Normal, Replace, Visual
and Select modes. Pending motions/operators and command-line input are cancelled;
visual selections restart in the new query buffer. Returning to an existing
nonempty field preserves its editing state and cursor. The basic backend has no modes.

Borders show the mode. Normal/Visual uses a block cursor; Insert uses a bar.
Horizontal scrolling follows the cursor, and exclusions have a multiline viewport.
Bracketed paste preserves newlines in exclusions and converts them to spaces in
single-line fields.

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

Ctrl+c always cancels fff. Alt shortcuts, Ctrl/Alt+digits, marking with Tab,
and Ctrl+w retain their fff actions in results/marks. Space in Normal mode
marks/unmarks like Tab; in Insert mode it types a space. The marked list always
accepts Space because it has no text field.

In exclusions, **ZZ** (two uppercase keystrokes in Normal mode) applies the rules
and closes the editor. Ctrl+w goes to Neovim instead of applying changes. Insert
mode can type literal `ZZ`. Basic editing also accepts `ZZ`; an unmatched leading
`Z` becomes text when the next key arrives. Escape cancels without applying.
In bookmark forms, Ctrl+s saves from any mode; ZZ saves in Normal/basic mode.
Tab switches fields and Alt+i opens the icon chooser.
Rebind marking and applying with `[keys.normal] mark` and `apply`.

Enter in results navigates or confirms; in locations/bookmarks it navigates; in
palettes it applies the selected palette. Pending Neovim input and command-line
entry receive Enter instead. In exclusions, Enter retains Neovim's behavior.

Escape from Insert, Visual, pending input or command entry returns toward Normal.
Escape when already Normal clears the main query or closes the subordinate view.
It never exits the session. With basic editing it clears/cancels immediately.

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
external programs or a full editor UI are outside this integration. Quitting or
crashing the engine causes fallback without confirming paths.

The requested keyboard protocol reports Escape distinctly where supported. With
ESC-prefix terminal reporting, rapidly typed Escape+digit can look like Alt+digit;
fff preserves Alt shortcut decoding.
