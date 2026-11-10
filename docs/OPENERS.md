# Opening files

How fff decides which program opens a file, the arguments it passes, and
how to run any other program on your files.

## Ways to open a file

| Key | Opens the file… | Then |
| --- | --- | --- |
| `L` | In the background | fff stays open |
| Enter (`fff roam`) | As fff leaves | Your shell is left in the file's folder |
| Ctrl+`1`…`9` (`fff roam`) | On that numbered row, as fff leaves | The same |
| `e` | In your `$EDITOR`, in this terminal | fff comes back when you close it |
| `open_in_terminal` (no key by default) | With its opener, in this terminal | fff comes back when it ends |

**Graphical programs** (a PDF viewer, an image viewer) start on their own:
closing fff or the terminal leaves them open. Their output goes to a log
file, `~/.local/state/fff/open.log`; if one fails, a notice says so and
where the log is.

**Terminal programs** (an editor, a pager) need a terminal. With Enter they
take fff's place in this one. With `L` they open in a new terminal window
(see [the terminal](#the-terminal) below), since fff keeps this one.

## Openers

Which program opens which files is set in `[[open.rules]]` in the
[configuration file](CONFIGURATION.md). `fff setup` writes rules for the
programs you have; you can add your own:

```toml
[[open.rules]]
command = "zathura"
extensions = ["pdf", "djvu", "epub"]

[[open.rules]]
command = "imv"
extensions = ["png", "jpg", "jpeg", "gif", "webp"]

[[open.rules]]
command = "mpv --fs"
extensions = ["mkv", "mp4", "webm"]

[[open.rules]]
command = "nvim"
patterns = ["Makefile", "*.conf"]
terminal = true
```

Rules are tried in order and the **first match wins**. A file that matches
no rule opens in your `$EDITOR`, as a terminal program.

| Key | Means |
| --- | --- |
| `command` | The program and its arguments. The file's path is added at the end. Required. |
| `extensions` | Extensions it opens, without the dot, in any case. `"tar.gz"` works too. |
| `patterns` | Globs matched against the file name, such as `"Makefile"` or `"*.conf"`. |
| `terminal` | `true` for a program that runs in a terminal, such as an editor. Default `false`. |
| `at_grep_match` | Extra arguments when opening at a contents match; see [below](#opening-at-a-contents-match). |

A rule needs a `command` and at least one extension or pattern.

**How the command is read.** `command` is split into words like a shell
command line: spaces separate words, quotes and backslashes work as in a
shell, and `~` at the start of a word is your home. But no shell runs it,
so characters that only mean something to a shell (`|`, `>`, `$`, `*`…)
are refused unless you quote them. The program must be on
your `PATH` (or be a path to an executable), and fff checks it exists
before running anything. The file is always passed as one separate
argument, so no file name, however odd, is read as an option or as shell
syntax.

When several files are opened at once (`open_in_terminal` on marked
files), files sharing an opener go to one invocation: three PDFs open in
one zathura.

## Opening at a contents match

When you [search contents](SEARCHING.md#searching-contents) and press `L`
on a match, fff can open the file **at the match**: on its page, on its
line, with the words highlighted. Add `at_grep_match` to the rule, with the
arguments that do it for your program:

```toml
[[open.rules]]
command = "zathura"
extensions = ["pdf", "djvu"]
at_grep_match = "--page {page} --find {query}"
```

Opening a match in `Gilbarg-Trudinger.pdf` on page 233 then runs:

```sh
zathura --page 233 --find 'Sobolev embedding' /home/you/Gilbarg-Trudinger.pdf
```

The command line is `command`, then `at_grep_match`, then the file. Opened
any other way (from a file row, with Enter), the rule's `command` runs on
its own.

These **placeholders** are filled in from the match:

| Placeholder | Becomes | Example |
| --- | --- | --- |
| `{page}` | The match's page in a PDF or DjVu, from 1. In a text file or an EPUB, which have no pages, its line. | `233` |
| `{line}` | Its line, from 1. In a document, the line in its extracted text, which a viewer doesn't know. | `14873` |
| `{percent}` | How far into the file the match is, from 0 to 100. The useful position in an EPUB. | `42` |
| `{query}` | The contents query as you typed it, without spaces at its ends. | `Sobolev embedding` |
| `{match}` | The text that matched, as the file spells it (its case, its accents). | `Sobolev Embedding` |
| `{nth}` | Which of the file's matches this is, from 1. | `3` |
| `{count}` | How many matches the file has (at most 200). | `7` |
| `{path}` | The file's full path. Used anywhere in `at_grep_match`, the file goes there instead of at the end. | `/home/you/notes/Brezis.pdf` |

Placeholders are filled in after the line is split into words, so a value
with spaces (a query, a path) stays one argument. A placeholder can also be
part of a word: `--page={page}`, `+{line}`. Unknown braces, such as `{x}`,
are left alone.

Some examples:

```toml
# zathura: on the match's page, with the query found and highlighted.
at_grep_match = "--page {page} --find {query}"

# mupdf takes the page after the file.
at_grep_match = "{path} {page}"

# Okular.
at_grep_match = "--page {page} --find {query}"

# Neovim (with terminal = true): on the line, the cursor on the match.
at_grep_match = "+{line} -c /{query}"

# Your own script, given everything fff knows.
command = "my-opener"
at_grep_match = "--path {path} --page {page} --line {line} --percent {percent} --query {query} --match {match} --nth {nth} --count {count}"
```

**Doing more with a shell.** To run several commands (a notification, then
the viewer), use `sh -c`, but pass the values as the shell's arguments
rather than writing placeholders inside the script, so they stay quoted:

```toml
[[open.rules]]
command = "sh -c"
extensions = ["pdf"]
at_grep_match = '''
'notify-send "Match $3 of $4" "$2"; exec zathura --page "$1" --find "$2" "$5"'
fff {page} {query} {nth} {count} {path}
'''
```

## The terminal

Terminal programs opened in the background need a new terminal window.
fff finds one on its own: the terminal it runs in, else `$TERMINAL`, else
the first known terminal installed (st, Alacritty, kitty, WezTerm, Ghostty,
foot, Konsole, GNOME Terminal, xterm and others). To choose yourself:

```toml
[open]
terminal = "alacritty -e"      # runs a program in a new window
fff_terminal = "st -c fff-picker -e"   # fff's own windows; see below
```

`terminal` must start a window running the rest of the line: `st -e`,
`alacritty -e`, `kitty`, `foot`. `fff_terminal` is the same, for the
windows in which the desktop integration starts fff itself; giving them a
class lets your window manager float them (see [Setup and
integration](SETUP.md#floating-windows)).

`fff setup --check-config` reports the terminal it found and any opener
whose program isn't installed.

## Running programs

`gx` runs any program on the highlighted or marked files, and `gX` runs it
in fff's place.

**When your shell is zsh**, the command line is your own zsh, at the bottom
of fff's screen, with the files already on it after the cursor:

```
Run ❯  'my notes.md' draft.tex
```

Type the command in front of them, such as `wc -l` or `e` (your alias for
`nvim`), and press Enter. It runs exactly as in your terminal: your
aliases, functions, Tab completion, history (Up) and keys, pipes and globs.
An empty line or Ctrl+C runs nothing. Your `.zshrc` loads each time, as in
a new terminal.

**With another shell**, a prompt asks for the command:

1. Type it, such as `wc -l` or `dragon --and-exit`. **Tab** completes
   program names and file paths. The line below says which program it will
   run, or that there's none.
2. **Enter** shows the exact command, the program and the files, in order.
   Enter again runs it; Escape cancels.

   There's no shell here: the files are added at the end as separate
   arguments, and characters that would need a shell (`|`, `>`, `$`,
   `*`…) are refused with a message.

Either way, with `gx` the program gets the terminal and fff comes back when
it ends. If it failed, fff waits for Enter so you can read its errors. With
`gX`, fff ends and the program takes its place; in `fff roam` your shell is
then left in the folder you were browsing. Programs run in the folder
you're browsing.

`e` is the quick version for your editor: it opens the highlighted or
marked files in `$EDITOR` with no prompt. `$EDITOR` can have arguments,
such as `nvim -p` or `code --wait`.
