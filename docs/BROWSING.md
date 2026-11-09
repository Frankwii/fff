# Browsing

How to move around in fff, and what the screen tells you. The keys below
are the defaults in Normal mode; [Keys](KEYS.md) explains modes and how to
change any of them.

## The query or the results

Keys act on whichever has the **focus**: the results or the query box
above them. The one without it is drawn dimmed. fff starts with the
results focused, and every key in this guide works there.

The query box gets the focus when you type into it (`i`). Escape leaves
Insert mode but keeps the focus in the query, so you can edit it with
Vim's keys. There, letters, digits and symbols are Neovim's: `h` and `l`
move the cursor, `0` goes to the start of the line, `/` searches the query
and `x` deletes a character. Only `j` and `k` keep their meaning: they
give the results the focus, on the first or last row.

Keys that aren't text work from the query too: the arrows, PageUp and
PageDown, Enter, and the Ctrl, Alt and function keys. So if a key
seems to edit your query instead of doing what this guide says, press
Escape (or `j`) to focus the results and try again.

## Moving

| Key | Does |
| --- | --- |
| `j` / `k` | Next / previous row |
| `J` / `K` | A page down / up |
| `gg` / `G` | First / last row |
| `1`…`9` | Highlight the row with that number on screen |
| `l` | Go into the highlighted folder; on a file, go to the folder it's in |
| `h` | Go up to the parent folder |
| `/` | Go to a folder you type (see [below](#going-to-a-folder)) |
| `Ctrl+o` / `Ctrl+i` | Back / forward through the folders you've been in |
| `gt` | Open the [trash](OPERATIONS.md#the-trash) |

While typing in Insert mode, use Alt with the same letters: Alt+j and
Alt+k move, Alt+l and Alt+h go into and out of folders.

Going up with `h` keeps your query, so the parent's entries are filtered by
it too. Going into a folder with `l` starts it with an empty query.

**fff remembers where you were.** Each folder keeps the row you last
highlighted in it, so going up lands on the folder you came from, and
coming back to a folder puts the highlight where you left it. This is kept
between sessions too (in `~/.local/state/fff/state`), and fff always starts
on the row you last highlighted in that folder.

## Going to a folder

`/` opens a prompt for a folder to go to, typing straight away:

- Type a path, such as `~/papers/2024` or `../notes`. **Tab** completes the
  last folder name.
- Or type a few words: the prompt suggests your
  [workspaces](WORKSPACES.md) and, if you use
  [zoxide](https://github.com/ajeetdsouza/zoxide), the folders you visit
  most. **Up** / **Down** choose among the suggestions.
- **Enter** goes to the chosen suggestion, else to the folder you typed if
  it exists, else to the best suggestion, back in Normal mode. **Escape**
  cancels.

## Tabs

fff has three tabs, like nnn's contexts, numbered in the top-right corner
of the results. Each tab has its own folder and its own back/forward
history; the query, the marks and the search settings are shared.

| Key | Does |
| --- | --- |
| Up / Down | Next / previous tab |
| Ctrl+Tab | Next tab |

Tabs work in either mode. They're handy for file operations: mark files in
one tab, switch to another tab in the destination folder, and copy them
there with `p`. You can also click a tab's number.

## The jump list

`Ctrl+o` goes back to the folder you were in before, and `Ctrl+i` forward
again, as in Vim. Each tab keeps its own list. (`Ctrl+i` needs a terminal
that tells it apart from Tab, as most modern terminals do.)

## What the rows show

Each row shows an icon, the entry's name and, on the right, its size and
how long ago it changed (`5m`, `3h`, `2d`, `4mo`). A folder's size is the
total of every file in it, added up in the background; it shows `…` while
it's still counting. Folder sizes are remembered for the next session.

Names are colored by kind: folders, symbolic links, executables and empty
files each have their color. Links show `→ target` after their name.

The **footer** shows more about the highlighted entry: its full path, exact
size, date, permissions and owner.

**Labels.** In `fff roam` rows show their path below the folder you're
in. In `fff pick` they show bare names, with just enough parent folders to
tell same-named files apart (`2023/report.pdf`, `2024/report.pdf`). `gl`
switches between the two.

The columns can be changed with `[ui] columns` in the
[configuration](CONFIGURATION.md#rows-and-details).

## The preview

The bottom half of the screen previews the highlighted entry:

- a text file's first lines, with syntax colors;
- a folder's entries;
- a PDF's title, page count and the text of its first pages (with poppler);
- an archive's contents, without extracting it;
- an image's dimensions, or a song's or video's length, streams and tags
  (with FFmpeg);
- for anything else, what kind of file it is.

| Key | Does |
| --- | --- |
| PageDown / PageUp | Scroll the preview |
| `gpp` | Hide or show the preview |
| `gpw` | Wrap long lines, or cut them at the edge |

The preview is text only, by design: no images, so it looks the same in
every terminal. It's read in the background and never slows down browsing.
To start with it hidden, set `[ui] preview = false`.

## Things change while you look

When files appear, disappear or change in the folders you're looking at,
the results follow on their own. F5 rescans from disk, if you ever need
it. (macOS lacks the Linux feature this uses; there, the listing refreshes
on a timer instead, every few seconds for a folder's own entries.)

The query box's bottom border shows what's going on: `◌ scanning,
incomplete` while a search is running, `✓ complete` when it's done, and
`filters on` or `exclusions on` while some entries are being left out.

## The mouse

The mouse works as in a graphical file manager:

- **Click** a row to highlight it. **Double-click** to go into a folder or
  open a file in the background.
- **Drag** to mark a range of rows.
- **The wheel** scrolls.
- **Click a tab's number** to switch to it.
- **Drop files** from a graphical file manager onto the terminal to copy
  them here (in terminals that support it), after a confirmation.

While fff has the mouse, your terminal's own text selection usually needs
Shift held. To give the mouse back to the terminal, set
`[ui] mouse = false`. For mouse-only use, `[ui] buttons` adds a bar of
buttons (Back, Up, Open, Mark, Select…); see
[configuration](CONFIGURATION.md#mouse-and-buttons).

## Session details

**F2** shows everything about the current session: the exact folders being
searched, the depth, filters and exclusions, the palette, every tab's
folder, your marks, and any errors in your configuration file.
