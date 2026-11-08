# Getting started

This walks you through a first session with fff, from installing it to
using it in a script. It takes about fifteen minutes. Follow along in a
terminal: everything here is safe to try on your own files, since
deleting goes to a trash and `u` undoes any change.

## 1. Check your setup

Once fff is [installed](../README.md#install), run:

```sh
fff setup
```

It lists the programs fff can use and whether you have them (with the
command that installs any missing one), then asks a few questions: whether
your terminal shows fff's icons, and which of your programs should open
PDFs, images, videos and office files. It shows what it would write to
your configuration file and asks before writing anything. You can run it
again at any time.

## 2. Let fff change your shell's directory

A program can't change the directory of the shell that started it, so fff
needs a small shell function to do it. Add one line to your shell's
startup file:

```sh
eval "$(fff setup --init zsh)"     # ~/.zshrc
eval "$(fff setup --init bash)"    # ~/.bashrc
fff setup --init fish | source     # ~/.config/fish/config.fish
```

Open a new terminal so the line takes effect. Now when you leave fff, your
shell can be left in the folder you were looking at. [Setup and
integration](SETUP.md) has more, such as completions and man pages.

## 3. The screen

Go to a folder with some files in it, say `cd ~/Documents`, and run `fff`.

```
╭ Query  NORMAL  ─────────────────────────────────────────────────────╮
│ ❯ type to filter names…                                             │
╰──────────────────────────────────────────────────────── ✓ complete ╯
  @  ~/Documents
╭ Files · 4 ─────────────────────────────  1  2  3  ╮ ╭ Marks (0) ─────╮
│›   1    notes/                       43B   now │ │                │
│    2    papers/                      24B   2d  │ │ Collect paths  │
│    3    taxes/                       1.2M  3w  │ │ as you browse. │
│  4/0    README.md                    6B    now │ │                │
╰ depth 1 ──────────────────────────── 1–4 of 4 ╯ ╰────────────────╯
╭ Preview · directory · 2 entries ───────────────────────────────────╮
│ analysis/                                                          │
│ todo.md                                                            │
╰────────────────────────────────────────────────────────────────────╯
  notes  43 B  2 entries  2026-11-08 18:33  drwxr-xr-x  you:you   F1 keys  q quit
```

From top to bottom:

- **The query box**, where you type to filter. Its title shows the editing
  mode (more on that below).
- **The `@` line**, the folder you're in.
- **The results**, numbered, with each entry's size and how long ago it
  changed. The highlighted row has a `›`. The numbers in the top corner are
  your three tabs.
- **The marked list**, empty for now: you'll collect files there.
- **The preview** of the highlighted entry.
- **The footer**, with the highlighted entry's details and how to get help
  (F1) or leave (`q`).

## 4. Moving around

fff uses Vim's keys. Try these:

| Key | Does |
| --- | --- |
| `j` / `k` | Move the highlight down / up |
| `l` | Go into the highlighted folder |
| `h` | Go up to the parent folder |
| `gg` / `G` | Jump to the first / last row |
| `1`…`9` | Highlight the row with that number |
| PageDown / PageUp | Scroll the preview |

Go into a folder with `l` and come back with `h`: the highlight returns to
the folder you came from. Every folder remembers where you left the
highlight, even in a later session.

## 5. Typing: Normal and Insert mode

This is the one idea to get used to. The query box is a real Neovim, so it
has Vim's modes:

- In **Normal mode** (the default), letters are commands: `j` moves, `l`
  goes into a folder.
- In **Insert mode**, letters are typed into the query.

Press `i` and the box's title changes to `INSERT`. Type a few letters of a
name you see, such as `rea` for `README.md`, and the list narrows to what
matches. The letters don't need to be next to each other: `rdm` matches
`README.md` too.

While typing, the movement keys need Alt: Alt+j / Alt+k move the highlight,
Alt+l / Alt+h go into a folder and back up. Enter works as it does in
Normal mode.

Press **Escape** to go back to Normal mode. You're still in the query box
(it keeps the focus, so `h` and `l` move the cursor in your query as in
Vim). Press **Escape again**, or `j`, to give the results the focus. Press
Escape once more in the results and the query is cleared.

That's all of it: `i` to type, Escape to go back. [Keys](KEYS.md) explains
the details, such as which keys work where.

## 6. Finding files below you

fff lists only the folder's own entries at first ("depth 1", in the
results' bottom corner). Press `+` a couple of times: the results now
include what's inside the subfolders too, listed by their path. Type a
query and you search all of them at once, the closer matches first.
Press `-` to go back up.

A few more ways to narrow things down:

| Key | Does |
| --- | --- |
| `+` / `-` | Search one level deeper / shallower |
| `g.` | Show or hide hidden (dot) files |
| `gd` / `gD` | List only files / only folders (press again to undo) |
| `gs` | Sort by name, size, date, kind… |

Starting fff with `--depth 4` begins four levels deep, and
`fff --glob '*.pdf'` lists only PDFs. [Finding files](SEARCHING.md)
covers these and more.

## 7. Searching inside files

Press `i` to type, then **Tab**. A second box, **Contents**, opens beside
the names query. Type a word you know is in one of your files: every match
is a row, with its line (or page, in a PDF) and the text around it, and
the preview shows the match in context.

Tab switches back to the names query, which still decides which files are
searched. To search PDFs, install poppler (`fff setup` says whether you
have it).

## 8. Opening files

With a file highlighted:

- **Enter** opens it and leaves fff. Your shell is left in the file's
  folder.
- **`L`** opens it in the background and keeps fff open.
- **`e`** edits it in your `$EDITOR`, then comes back to fff.

Which program opens which file comes from your configuration: `fff setup`
asks you, and [Opening files](OPENERS.md) shows how to set it by hand.
Files without a program set open in `$EDITOR`.

To leave fff without opening anything:

- **`q`** leaves you where you started.
- **`!`** leaves you in the folder you're browsing.
- **`g!`** leaves you inside the highlighted folder.

## 9. Marks and file operations

Highlight a file and press **Space**: it's marked, and the highlight moves
on so you can mark the next one. Marked files appear in the marked list on
the right. Marks stay when you move to another folder, which is how you
copy and move:

1. Mark a couple of files with Space.
2. Go to another folder (`h`, `l`, …).
3. Press **`p`** to copy the marked files here, or **`v`** to move them.

Some more:

| Key | Does |
| --- | --- |
| `x` | Move the highlighted file (or the marked ones) to the trash |
| `u` | Undo the last operation; again for the one before |
| `n` | Create a new file or folder |
| `r` | Rename the listed files in a Neovim buffer |
| `Ctrl+k` | Move into the marked list, to look at or unmark files |
| `Ctrl+m` | Clear every mark |

Try `x` on a file, then `u`: it comes back. Nothing in fff deletes a file
without asking, except into the trash. [File operations](OPERATIONS.md)
has the rest, including archives and the trash folder (`gt`).

These keys act on the results. While the query has the focus, `x` and `u`
are Neovim's own (delete a character, undo an edit), so press Escape first
if a key seems to edit the query instead.

## 10. Choosing files for other programs

`fff pick` is fff for scripts. It works the same way but **prints** what
you choose instead of changing your directory:

```sh
nvim "$(fff pick)"                  # choose a file and edit it
fff pick -0 | xargs -0 ls -l        # mark several, then Enter
```

It starts typing straight away, three levels deep. Enter prints the marked
files and the highlighted one; `q` prints nothing.

fff can also choose among paths another program lists:

```sh
rg -l TODO | fff | xargs -r nvim
```

[The programs](PROGRAMS.md) describes these, plus `fff seek` (find a file
deep below you) and switching between them in a running session (`gm`).

## 11. Getting help inside fff

- **F1** opens the Keys screen: every function with its keys. Type to
  search ("trash", "hidden"…), and change any key from there.
- **`:`** lists what you can do where you are, to run it by name.
- **F2** shows the session's details: exact paths, depth, filters, and any
  errors in your configuration.

The footer always says how to open the Keys screen and how to leave.

## Where to go next

- [Browsing](BROWSING.md): tabs, the jump list, the go-to prompt and the
  mouse.
- [Workspaces](WORKSPACES.md): save the places and setups you use often.
- [Color palettes](PALETTES.md): press `gP` to try them now.
- [Keys](KEYS.md): change any key to your liking.
- [Configuration](CONFIGURATION.md): everything else you can set.
