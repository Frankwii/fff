# The programs

fff is one program with several ways to start it, depending on what you
want back when you leave:

| Command | You get back |
| --- | --- |
| `fff` (or `fff roam`) | Your shell in another folder |
| `fff pick` | The paths you chose, printed |
| `fff seek` | Your shell in the folder of a file found deep below |
| `COMMAND \| fff` | One of the paths COMMAND listed, printed |
| `fff window` | Nothing: fff as a file manager in its own window |
| `fff explorer …` | An answer to a file dialog |

They share everything else: the keys, the query, marks, previews, file
operations and your configuration. The search options (`--depth`, `--glob`,
`--exclude`, `--hidden`, `--workspace`) and the interface options
(`--palette`, `--no-icons`, `--config`, `--no-config`) work with every one of
them. `fff --help` lists them all, and `fff pick --help` (and so on)
describes each command.

## `fff`: browse and change directory

Plain `fff` browses from the current folder. With the [shell
function](SETUP.md#change-directory-on-exit) set up, leaving fff can
leave your shell somewhere else:

| Key | Leaves you |
| --- | --- |
| `q` | Where you started |
| `!` | In the folder you're browsing |
| `g!` | Inside the highlighted folder |
| Enter on a file | In the file's folder, after opening the file |

Enter on a file opens it with its [opener](OPENERS.md) as fff leaves. A
program that runs in the terminal, such as your editor, takes fff's place
there. Enter on a folder does nothing in `fff`: use `l` to go into it, or
`g!` to leave into it.

Without the shell function, `fff` prints the folder to change to instead,
so `cd "$(command fff)"` also works.

## `fff pick`: choose paths for a script

`fff pick` prints the paths you choose, one per line, and changes no
directory. It's for command substitution and pipes:

```sh
nvim "$(fff pick)"
fff pick -0 | xargs -0 ls -ld
fff pick ~/papers --glob '*.pdf' -0 | xargs -0 zathura
```

It starts in Insert mode, ready to type, and three levels deep.

| Key | Prints |
| --- | --- |
| Enter | The marked paths and the highlighted one |
| Shift+Enter | Only the highlighted path |
| Ctrl+Enter | Only the marked paths |
| `!` | The folder you're browsing |
| `q` | Nothing (exit status 0) |
| Ctrl+c | Nothing (exit status 130) |

With nothing listed, Enter in Normal mode prints the folder you're browsing.

Paths are absolute. `--relative` prints them relative to where you started
fff, and `-0` (`--null`) ends each with a NUL byte instead of a newline, for
`xargs -0` and file names with odd characters.

Rows show bare file names in `fff pick`, with just enough of their folders
to tell same-named files apart; `gl` shows whole paths instead. Either way,
what's printed is always the full path.

## `fff seek`: find a file deep below

`fff seek` lists only files, from every folder down to six levels below,
best match first. You can't change folders in it: it's for finding one
file fast.

```sh
fff seek ~/projects
```

Enter leaves your shell in the chosen file's folder. To print the file
instead, use `fff pick` with a filter, which also lists only matching
files: `fff pick --glob '*.tex'`.

## Piping paths in: `COMMAND | fff`

When paths come in on stdin, `fff` chooses among them instead of browsing,
and prints your choice:

```sh
rg -l TODO | fff | xargs -r nvim
git ls-files | fff
zoxide query -l | fff
fd -0 -e pdf | fff -0 | xargs -0r zathura
```

- Paths appear as they're read, so you can start typing while a slow
  command is still writing.
- Lines that aren't existing files or folders are skipped (the status line
  counts them), and duplicates are listed once.
- Your choice is printed as the input spelled it (`./src/main.rs` stays
  `./src/main.rs`). `-0` reads and prints NUL-separated paths.
- Enter, Shift+Enter, Ctrl+Enter and `q` work as in `fff pick`.

Only choosing works here: the queries (contents too), marks, the preview,
sorting and palettes. There are no folders to go to and no file operations,
and the search options (`--depth`, `--glob`…) are errors, since the list is
used as given. Filter in the command that writes it instead.

## Switching program in a running session

`gm` turns the running session into another program, keeping your place:
press `gm`, then `r` for `fff roam`, `p` for `fff pick` or `s` for
`fff seek`. The folders, tabs, query, marks, filters and preview all carry
over. The depth carries over if you changed it, and otherwise becomes the
new program's default.

For example: start `fff`, browse to `~/papers`, press `gm` `s` to find a
file six levels deep there, then `gm` `r` to go back to browsing. How the
session ends follows the program you end it in.

## `fff window` and `fff explorer`

Two more commands make fff part of your desktop:

- `fff window [PATH]` browses in a terminal window of its own, as your
  file manager. There's no shell to return to, so Enter goes into folders,
  opening a file closes the window, and `!` turns the window into a shell
  in the folder you're in.
- `fff explorer open|open-multiple|folder|save [PATH]` answers a file
  dialog, printing the chosen file, files, folder or place to save.

[Setup and integration](SETUP.md#the-desktops-file-manager) shows how to
make fff open folders for other programs and answer Firefox's file
dialogs, which run these for you.

## Starting from a workspace

`--workspace NAME` starts in a saved [workspace](WORKSPACES.md): its
folder, and any settings it keeps (roots, depth, filters, even the
program). Options you give on the command line win over the workspace's.
A workspace can stand in for a script:

```sh
fff pick -0 --workspace papers | xargs -0 -r zathura
```

## Exit status

| Code | Means |
| --- | --- |
| 0 | A normal exit: a folder was chosen, paths were printed, or you left with `q` |
| 1 | An error at startup, in the configuration, or reading or writing files |
| 2 | Wrong command-line usage, such as an unknown option |
| 130 | Cancelled with Ctrl+c: nothing printed, no directory change |

After `gX` (run a program in fff's place; see [opening
files](OPENERS.md#running-programs)), the exit status is that program's.
