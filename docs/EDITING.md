# Experimental Neovim editing

Picker can run a clean Neovim process as the text-editing engine inside its own
interface. This is **opt-in**; the default `basic` backend retains the original
append/Backspace editing and Escape behavior.

```sh
cargo run -- --editor nvim
```

Or enable it in your TOML config:

```toml
[editor]
backend = "nvim"
```

`--editor basic` / `--editor nvim` overrides the config. Neovim must be available
as `nvim` on PATH; the implementation was tested with Neovim 0.12.6. If startup or
RPC fails, picker reports the problem and switches to basic editing, preserving
the last synchronized text. Palette hot reload does not switch editing backends;
restart to change `[editor]` settings.

## Fields and modes

All editable fields use the engine:

- filename query;
- location entry (Alt+g);
- multiline exclusion editor (Alt+e);
- bookmark-name filter (Alt+b).

Fields start in **INSERT**, so ordinary typing still filters immediately. Their
borders show the current mode. **Escape** enters **NORMAL**; `i`, `a`, `I`, `A`,
`o`, and the other usual Neovim editing commands behave through the real engine.
Normal/Visual mode uses a block cursor; Insert uses a bar. Horizontal scrolling
keeps the text cursor visible, and the exclusion editor follows its active line.

Examples:

| Action | Keys |
| --- | --- |
| Move within text | Escape, then `h`, `l`, `w`, `b`, `0`, `$` |
| Delete/change a word | `dw`, `ciw` |
| Delete a line | `dd` |
| Undo / redo | `u` / Ctrl+r |
| Characterwise selection | `v`, motions, then `y`, `d`, or `c` |
| Linewise/block selection | `V` / Ctrl+v |
| Paste an internal register | `p` / `P` in Normal mode |
| Edit multiple exclusion lines | `o`, `O`, `j`, `k`, `gg`, `G`, line operators |
| Substitute buffer text | `:s/old/new/`, then Enter |

Neovim owns motions, operators, counts, registers, undo, Unicode handling, and
pending commands. A partially entered command such as `f` or `r` is shown as
**PENDING** and waits for its next character without blocking picker. The fields
have separate scratch buffers, cursor positions, and undo histories. Switching
fields cancels a transient pending command/selection. Directory navigation resets
the query field to Insert mode, including when the new query is empty.

Queries, locations, and bookmark filters use a compact single-line presentation;
exclusions have a multiline viewport. Bracketed paste is supported; pasted
newlines are converted to spaces in the compact fields and preserved in exclusions.

## Picker keys still belong to picker

| Key / context | Behavior |
| --- | --- |
| Ctrl+c | Quit picker without selected paths, in every mode. |
| Alt bindings, Ctrl+digits, Alt+digits | Existing picker navigation/selection shortcuts. |
| Tab in results | Mark the highlighted result. |
| Ctrl+w in results/marks | Confirm marked paths. |
| Ctrl+w in exclusions | Apply exclusion edits. |
| Enter in results | Normal picker Enter, except while completing a Neovim command. |
| Enter in location/bookmarks | Navigate to the entered/selected directory. |
| Enter in exclusions | Neovim's newline/motion behavior; Ctrl+w applies. |
| Escape in Insert/Visual/pending/command mode | Return toward Normal mode inside the editor. |
| Escape when already Normal | Clear the main query, or cancel/leave the subordinate field. |

In Normal mode, digits are Vim counts; in Insert mode, plain digits remain query
text. Alt+j/Alt+k move the result/marked highlight in every mode. Up/Down
remain chooser controls in results/bookmarks; in exclusions they edit/move through
the buffer. During Neovim command entry they go to Neovim instead.

## Normal-mode list keys

In **NORMAL** mode, list views accept plain Vim-style motions for the highlight:

| Key | Results | Bookmark chooser | Marked list |
| --- | --- | --- | --- |
| `j` / `k` | Same as Alt+j / Alt+k | Same as Alt+j / Alt+k | Same as Alt+j / Alt+k |
| `h` | Same as Alt+h (parent directory) | Goes to Neovim | Nothing |
| `l` | Same as Alt+l (enter highlighted directory) | Goes to Neovim | Nothing |
| `gg` / `G` | First / last result | First / last bookmark | First / last mark |

Insert, Visual, Replace, pending, and command-line modes are unaffected: there
the letters edit or move through text as usual. The multiline exclusion editor
and location prompt are not lists, so `hjkl`, `gg`, and `G` keep their Vim
meaning there. The marked list has no text field, so it accepts these keys
regardless of the editing backend. Alt bindings keep working in every mode.

Keys that are not bound still go to Neovim. A key that starts a binding (`g`)
waits, shown as **PENDING**; if the next key does not complete a binding, both
keys reach Neovim unchanged, so `gU`, `ge`, `gu`, and `g~` still edit the field.
Escape after a lone `g` cancels it inside Neovim without clearing the query.

The bindings are configurable in `[keys.normal]`; see
[CONFIGURATION.md](CONFIGURATION.md#normal-mode-list-keys).

For example, type `hello world`, press Escape, then `0dw`: the query becomes
`world`. `u` restores the query and Ctrl+r reapplies the deletion. Enter can then
confirm a matching file using the usual mark rules.

On terminals supporting the requested keyboard protocol, Escape is reported
distinctly. Legacy terminals can encode a rapidly typed Escape+digit exactly like
Alt+digit; picker preserves the existing Alt shortcut decoding.

## How clean embedding works

Investigation confirmed that Neovim's documented [MessagePack-RPC embedding
API](https://neovim.io/doc/user/api/#rpc-connecting) works as a field-editing engine.
Picker starts one private `nvim --embed --headless -u NONE -i NONE --noplugin -n`
process, uses unnamed `nofile` scratch buffers, disables backups/undo files/modelines
and persistent history, and redirects logging to `/dev/null`. No personal init,
plugins, ShaDa, swap, or editor temp files are loaded/written. Unneeded Neovim
listener sockets are stopped; communication uses private child stdin/stdout.
Selected path output stays isolated on picker stdout.

This embeds the text engine, not Neovim's full window UI. Text-local commands such
as substitutions are supported. Commands that open windows/buffers, run external
programs, or expect a complete Neovim UI are outside this integration. Quitting
or crashing the engine causes basic-mode fallback; it does not confirm paths.
RPC has a bounded timeout, and the child is shut down/reaped when picker exits.

Tests exercise the real installed Neovim: operators, find/replace pending input,
Unicode, visual selection/yank/paste, undo/redo, substitutions, field switching,
multiline edits, process failure, and cleanup. Terminal tests verify every field,
clean-init isolation, fallback, mode/cursor rendering, and exact stdout bytes.
Neovim-specific tests explicitly skip when `nvim` is not installed.
