# Finding files

fff finds files in two ways: by **name**, with the query box at the top,
and by **contents**, with a second query that searches inside files. Both
search every folder down to the current **depth**.

## The names query

Press `i` and type. The results narrow to the entries whose path (below
the folder you're in) contains your letters **in order**, though not
necessarily next to each other:

- `rdm` finds `README.md`.
- `pdestab` finds `pde/notes/stability.tex`, and so does `pde/stab`.
- `pde/` lists what's inside folders matching `pde`.

Matching ignores case, and accents unless you type some: `evaluacion`
finds `Evaluación`. Matches are ranked as fzf ranks them: runs of letters
and letters at the start of words (after `_`, `-`, `.`, `/` or a space)
count most, so the closest match comes first.

To clear the query, press Escape until the results have the focus, then
Escape once more. Your query stays when you go up a folder with `h`.

## Depth

By default fff lists only the current folder's own entries: depth 1. To
search below it:

| Key | Does |
| --- | --- |
| `+` / `-` | One level deeper / shallower |
| Ctrl+Alt+`1`…`9` | Go straight to that depth |

From the command line, `--depth N` (or `-d N`) sets where to start. The
current depth is shown in the results' bottom border.

Deeper results are listed by their path. When you search several levels
deep, a good match deep down still beats a poor one near the top, but at
equal quality the shallower one comes first. `[ui] temperature` in the
[configuration](CONFIGURATION.md#sorting-and-ranking) sets how much depth
counts.

`fff pick` starts at depth 3 and `fff seek` at depth 6; see [the
programs](PROGRAMS.md).

## Showing and hiding entries

| Key | Does |
| --- | --- |
| `g.` | Show or hide hidden entries (names starting with a dot) |
| `gd` | Hide folders, listing only files (again to show them) |
| `gD` | Hide files, listing only folders (again to show them) |

`--hidden` starts with hidden entries shown.

## File filters

A file filter lists only the files whose name matches a pattern, such as
`*.pdf`. Folders aren't listed, but are still searched, so with some depth
you see every PDF below you:

```sh
fff --depth 5 --glob '*.pdf' --glob '*.djvu'
```

A file is listed when it matches any of the patterns. Quote them, so that
your shell doesn't expand them.

| Key | Does |
| --- | --- |
| `gF` | Edit the filters in a Neovim buffer, one pattern per line; `ZZ` saves, `ZQ` cancels |
| `gf` | Turn the filters off, or on again |

Patterns use the usual glob syntax: `*`, `?`, `[abc]` and `{tex,pdf}`.

## Exclusions

Exclusions hide entries you never want to see, such as `node_modules` or
`.cache`. An excluded folder is not searched at all, which also makes
searches faster.

```sh
fff --exclude node_modules --exclude '*.o'
```

- A pattern without a `/` matches names anywhere: `node_modules`, `*.o`.
- A pattern with a `/` matches paths below the folder being searched:
  `build/cache`.
- A pattern starting with `/` or `~/` matches one place on disk:
  `~/backup`. You can still go into that folder yourself; it's only left
  out of searches from outside it.

To exclude things every time, list them in the configuration:

```toml
[exclusions]
rules = ["node_modules", "~/.cache", "~/backup"]
```

| Key | Does |
| --- | --- |
| `gE` | Edit this session's exclusions in a buffer, one per line |
| `ge` | Turn the exclusions off, or on again |

`--no-exclusions` starts with them off. fff doesn't read `.gitignore`
files.

## Searching contents

Press **Tab** to open the **Contents** query beside the names query. Type
in it, and the rows become the matches: one row per match, with the file,
its line (`l. 40`), page (`p. 12`) or position in an e-book (`42%`), and
the text around it. The preview shows the lines around the highlighted
match.

Tab switches between the two queries, each keeping its text. The names
query still applies: it chooses which files are searched. So does
everything else on this page, such as the depth and the filters. For
example, to search your PDFs for a word:

```sh
fff ~/papers --depth 6 --glob '*.pdf'
```

then press `i`, Tab, and type the word.

**What a contents query matches.** It looks for your text as typed, not a
regular expression. It ignores case unless you type a capital letter, and
accents unless you type some. A space matches any spacing, line breaks
included, and a word broken by a hyphen at the end of a line is still
found.

**Which files it reads.** Text files are read as they are, and binary files
are skipped. PDF, DjVu and EPUB documents are searched through their text,
with `pdftotext` (from poppler) and `djvutxt` (from DjVuLibre). A scanned
document without a text layer has nothing to find; `ocrmypdf` can add one.

**Speed.** The first search through many documents takes a while, since
their text is extracted once. After that, fff keeps the text and an index of
what each file contains (in `~/.cache/fff`), so later searches are
immediate and only open the files that could match. Both clean themselves
up over time, and are safe to delete.

**Limits.** A file shows its first 200 matches, and a search stops after
20000. The count beside the query says so when that happens.

**Acting on matches.** A match row stands for its file: Enter, `L` and
Space act on the file. Marking a match marks its file and moves on to the
next file. With the right [opener settings](OPENERS.md#opening-at-a-contents-match),
`L` opens a PDF on the match's page with the words highlighted, or an
editor on the match's line.

Escape clears the query you're typing into, then the other one.

## Sorting

The results are ranked by how well they match, with shallower entries
first at equal quality. `gs` opens the sorting picker to sort by something
else instead:

| Order | Sorts by |
| --- | --- |
| `name` | Name, A first, numbers by value (`file2` before `file10`) |
| `size` | Size, the largest first (folders by their total) |
| `modified`, `accessed`, `created` | That date, the newest first |
| `extension` | Extension, then name |
| `kind` | Folders, then files, then links |
| `depth` | Depth, the shallowest first, then name |
| `shallowest` | Depth, folders first, then the best match |
| `log` | The best match, with a penalty for depth |
| `program` | Back to the default order |

Tab in the picker switches between descending and ascending. The query
still decides what's listed; sorting only changes the order. The results'
bottom border names the current order (`sorted by size ↓`). Sorted by
size, folders are added up in the background, and they take their place
in the list as their totals arrive.

To start in another order, set `[ui] sort` in the
[configuration](CONFIGURATION.md#sorting-and-ranking).

## Pinned roots

Folders named on the command line are **pinned roots**: they stay in the
results wherever you browse.

```sh
fff ~/notes ~/papers --depth 3
```

Here the results combine what's below `~/notes` and `~/papers` with what's
below the folder you're browsing, so your notes and papers are always one
query away. To start browsing in a folder instead of pinning it, add
`--browse`: `fff --browse ~/notes`.
