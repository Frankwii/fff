# Everyday tasks

Short recipes for things people do with fff every day, each recorded in a
home that looks lived in: seven years of teaching material (well over a
thousand files), a cluttered Downloads folder, holiday photos and a few
forgotten large files. The keys pressed are shown under each recording.

If you haven't used fff before, the [tutorial](GETTING_STARTED.md) explains
the ideas these recipes rely on, such as Normal and Insert mode.

## Find one file among thousands

You know roughly where a file lives but not the exact folder:
`teaching/2024-2025/linear-algebra/week-07/solutions.tex`. Type a few
letters from each part of the path, in order:

```sh
fff seek
```

![Finding a file with fff seek](demos/task-find.gif)

`lin07sol` matches the week 7 solutions of every year. Starting again with
`2024lin07sol` (Ctrl+u clears the line) keeps only the year you want, and
Enter leaves your shell in that file's folder.

- `fff seek` starts typing straight away and looks six folders deep. In
  plain `fff`, press `+` a few times to look deeper, then `i` to type.
- The letters don't need to be next to each other, but they must come in
  order. Starting each part of the path with its first letters works best.

## Find which file says something

You remember an exam question about the Cauchy–Schwarz inequality, but not
which exam it was in. Search the contents, then narrow by name:

```sh
cd ~/teaching && fff -d 4
```

![Searching contents, then narrowing by name](demos/task-contents.gif)

1. `i`, then **Tab** opens the Contents query. Type `schwarz`: every match
   is a row, with its line, and the preview shows it in context.
2. **Tab** goes back to the names query. Type `exam`: now only the exams
   are searched.
3. Escape, then `j` and `k` to look through the matches.

This works for PDFs too, if you have poppler installed. [Finding
files](SEARCHING.md#searching-contents) has the details.

## Free up disk space

The disk is full and you don't know why. Sort by size and look a few levels
deep:

![Finding the largest files](demos/task-space.gif)

1. `gs`, type `size`, Enter. Folders show their total size, largest first.
2. `+` a few times to search deeper, and `gd` to list only files: now the
   largest files come first, wherever they are.
3. `x` moves the highlighted one to the trash. `u` would bring it back.

Files in the trash still take up space; it's emptied after 30 days, or you
can delete for good with `X` instead (it lists what goes and asks first).

## Tidy up Downloads

Move every PDF out of Downloads into your papers folder in one go:

```sh
cd ~/Downloads && fff
```

![Moving every PDF out of Downloads](demos/task-tidy.gif)

1. `i`, type `.pdf`, then Escape twice: only the PDFs are listed.
2. `m` marks everything listed.
3. `/` opens the go-to prompt. Type `~/papers` and Enter. You arrive
   typing, ready to filter the new folder, so press Escape twice.
4. `v` moves the marked files here (`p` would copy them).

Marks stay while you browse, so you can collect files from several folders
before moving them. `u` undoes the move.

## Rename many files at once

Turn `IMG_2031.jpg`, `IMG_2032.jpg`… into `mallorca-2031.jpg`…:

```sh
cd ~/Pictures/mallorca-2026 && fff
```

![Renaming photos in a Neovim buffer](demos/task-rename.gif)

1. `r` opens the names in a Neovim buffer, one per line.
2. Edit them with anything Neovim offers. Here, `:%s/IMG_/mallorca-/`.
3. `ZZ` renames them all (`ZQ` cancels).

To rename only some files, filter them with the query first: `r` lists only
what matches. If any new name clashes, nothing is renamed. `u` undoes it.

## Hand files to another program

`fff pick` prints the files you choose, so any command can use them:

```sh
nvim "$(fff pick)"                                # edit one file
fff pick -0 | xargs -0 -r tar czf week7.tar.gz    # archive the files you mark
fff pick ~/papers --glob '*.pdf' -0 | xargs -0 -r zathura   # open PDFs
```

Mark files with Space, then Enter prints the marked ones and the
highlighted one; `q` prints nothing. [The programs](PROGRAMS.md) has more.

## Come back to the same places

If you find yourself typing the same `cd` and the same options, save them:

- **`gw`** opens your [workspaces](WORKSPACES.md); `n` there saves the
  current folder (and, if you like, the depth, filters and sorting) under a
  name.
- **`/`** goes to a folder by typing part of its name: it suggests your
  workspaces and, with [zoxide](https://github.com/ajeetdsouza/zoxide), the
  folders you use most.
- **Ctrl+o** and **Ctrl+i** go back and forward through the folders you've
  visited, as in Vim.
