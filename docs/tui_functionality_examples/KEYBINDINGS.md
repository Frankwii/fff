# Keybindings and interaction contract

The non-delayed functionality examples take precedence over this document. Bindings below come from active examples or explicit user decisions. Explicit user instructions supersede both. Delayed examples do not define active bindings.

Printable characters edit the focused query or input. Plain digits remain query text. Ctrl means Control; Alt may also be called Meta. Lowercase and uppercase bindings are distinct.

## Main results

| Key | Action | Example |
| --- | --- | --- |
| Enter | Enter a highlighted directory. On a highlighted file, return all marked paths plus that file and exit. In an empty directory, return all marked paths plus the current directory and exit. | 01, 02, 04, 05 |
| Alt+h | Navigate to the parent, preserving the query and resetting scroll and highlight. | 02 |
| Alt+j | Move the highlight to the next result. | 02 |
| Alt+k | Move the highlight to the previous result or marked-list item. | Explicit user decision |
| Alt+l | Enter the highlighted directory without ever confirming files or empty-directory output. | Explicit user decision; supersedes 04 |
| Alt+g | Location entry; Enter navigates, Escape cancels. | Explicit user decision |
| Alt+b | Open/close the bookmark chooser; type to filter names, Enter to visit. | Explicit user decision |
| F5 | Bypass the current discovery snapshot and refresh. | Explicit user decision |
| Alt+Right | Increase maximum discovery depth by 1. | 03, 04, 05, 13 |
| Alt+Left | Decrease maximum discovery depth by 1, clamped at 1. | 03, 04, 13 |
| Tab | Toggle the highlighted item's mark, then advance if a following result exists. | 05 |
| Alt+Space | Focus the marked-item list; press again to return to main results. | 05 |
| Ctrl+w | Return only marked paths and exit; with no marks, do nothing. | 05 |
| Alt+. | Toggle hidden-file visibility. | 05, 07 |
| Alt+H | Toggle respect for the exclusion list. | 07 |
| Alt+e | Edit the exclusion list. | 07 |
| Escape | Clear the query while discovery continues; with an empty query, leave the session unchanged. | 01, 02, 08 |
| Ctrl+c | Exit without returning paths. | 08 |
| Ctrl+1 through Ctrl+9 | Immediately return only the corresponding visible option and exit; if the requested index exceeds the visible count, return the last visible option. | 03, 05, 12, 13 |
| Ctrl+0 | Immediately return only the last visible option and exit. | 12 |
| Alt+1 through Alt+9 | Activate the corresponding visible option exactly like highlighting it and pressing Enter; clamp to the last visible option. | 14 |
| Alt+0 | Activate the last visible option using Enter behavior. | 14 |
| PageUp / PageDown | Change the visible page of results; update numbered shortcuts accordingly. | 12 |

Search roots are fixed at startup. No live root-editing bindings are defined. Depth adjustments preserve the query, current directory, roots, and marks.

## Confirmation

On a file, Enter returns the marked collection in insertion order, followed by the highlighted file if it is not already marked. With no marks, it returns that file alone. A path already marked is returned once.

In an empty directory, Enter uses the same confirmation rule with the current directory as the target: return all marks plus that directory once, then exit.

Ctrl+w returns only the marked collection. With no marks, it does nothing: the picker stays open and outputs nothing.

Numbered Ctrl+digit shortcuts return only their target, ignoring all marks. These are the selection shortcuts referred to here; other Ctrl bindings retain their own actions, including Ctrl+w and Ctrl+c.

Returned paths use the startup output configuration described in example 10. Example 12 specifically uses absolute paths with a trailing NUL byte.

## Marked-item list and exclusion editing

In the marked-item list, Tab toggles the focused item's mark. An unmarked item remains visible until the user leaves the list. Escape or Alt+Space returns to main results, at which point unmarked entries disappear from the list. The examples do not yet specify all navigation bindings within this list.

While editing exclusions, Ctrl+w applies the edited list rather than confirming output paths. The examples do not yet establish a complete editor keymap. Changes to exclusions preserve marks, including marks on paths hidden by the new rules.

## Numbered-choice contract

The first nine visible result rows are addressable with Ctrl+1 through Ctrl+9. Ctrl+0 addresses the last visible result, even when more than ten rows are visible. Intermediate rows beyond the first nine have no shortcut. With fewer than nine visible results, higher digit shortcuts select the last visible result. With no visible results, any numbered shortcut returns only the current directory and exits, ignoring marks. This includes queries with no matches as well as empty directories.

Headers, separators, and messages do not consume numbers. Filtering and paging update the targets. A shortcut uses the labels from the most recently fully rendered results; queued discovery updates cannot redirect it before rendering. It returns the target path and exits, for both files and directories, without navigating or including marks.

## Numbered Enter

Alt+digits use the same rendered labels and paging rules as Ctrl+digits. Directories
are entered; files confirm marks plus that file and exit, exactly like Enter.
Missing marks block confirmation. With no visible rows, Alt+digits follow Enter: a
completed physically empty directory confirms marks plus the current directory;
no-match results in a nonempty directory or an incomplete scan stay open. Unlike
Ctrl+digits, Alt+digits do not bypass marks or force an empty-results fallback.
Both shortcut families are active only in main results focus.

## Terminal input

Example 12 requires physical Ctrl+digit chords to reach the application distinctly. If the input stream cannot distinguish a chord from another key, the application must not guess. Application tests that inject decoded events and physical-terminal integration checks are separate.
