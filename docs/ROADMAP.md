# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- Content search beyond `fff grep` (below).
- Using fff while a long file operation runs. Operations and their checks
  already run on another thread with their progress shown and Ctrl+c to stop
  them; meanwhile only Ctrl+c is heard. Keeping navigation, marks and other
  operations usable would need rules for what a second operation may touch and
  how live-update rescans interact. Archive validation before the extraction
  prompt still runs on the interface thread.
- Preview pane follow-up: previewing the marked list's rows from the
  results.
- Follow-ups to the Keys screen (below): programs as commands, the
  `keys.toml` layout, and keys inside choosers.
- Switching modes without restarting (below): from `fff roam` to `fff seek`,
  for example.
- Resizing the font from fff (below).
- Opening folders from other applications. `--window` exists so that fff,
  set up as the desktop's file manager, can be opened from programs such as
  Telegram (opening a download's folder), but Telegram does not open fff yet.
  What remains is to find how it asks (`xdg-open`, the D-Bus FileManager1
  service or the portal's OpenURI) and make that path reach fff.
- An AUR package once the project nears 1.0.

The preview pane stays text only; there will be no image previews: see
[design choices](DEVELOPMENT.md#design-choices).

## The Keys screen, continued

The Keys screen (F1) and `keys.toml` came in 0.14.0. What remains:

- **Programs as commands.** Keys apply per program, and two programs are
  still options: `fff roam --window` and `fff explorer --multiple /
  --directory / --save`. They would become `fff window` and `fff explorer
  open | open-multiple | folder | save PATH`, named as on the Keys screen,
  with the launchers and portal wrapper `fff setup` writes calling them.
- **`keys.toml` itself.** Its layout (one table per mode, scopes as inline
  tables) is a first version, to be settled.
- **Keys inside choosers, prompts and sheets** are fixed for now; they could
  become functions with places of their own.

## Content search, continued

`fff grep` searches contents: a contents query beside the names
query, one row per match, documents read through fff's own text cache and
searched on its threads, not by running rga, which starts a preprocessor for
every file on every search (1.85 s and about 25 CPU-seconds a search over
2,400 cached PDFs, against 0.02 s here). What remains:

- **In the other programs.** A contents query in `fff roam` and `fff pick`
  too, toggled on, so that browsing can narrow a folder by what its files
  hold.
- **More formats.** Office documents and archives could fill the same cache
  through `rga-preproc`, one run per file.
- **A `/regex/` form** of the contents query.
- **E at the match.** E would open `$EDITOR` at a text match's line when
  the editor accepts `+N`, and a document at its page where the viewer
  takes one (`zathura --page`).
- **Pruning the cache.** Entries are never removed yet; old ones would go by
  age and total size. The text of every PDF in a home folder takes a few
  hundred megabytes at most.
- **OCR stays out.** A scanned document without a text layer cannot match;
  OCR takes seconds a page, too slow here, and `ocrmypdf` adds the text
  layer to the file once, for every program.

## Switching modes

`fff roam`, `pick`, `seek` and `explorer` are chosen when fff starts. A key
could switch the running session to another mode, say from browsing with
`fff roam` to `fff seek`'s flat list of files below here, keeping the query,
marks and directory where they still make sense. A small mode picker, a
chooser like the palette's, would list the modes with what each one does and
returns. What a switch does to output matters: `fff pick` prints paths and
`fff roam` changes the shell's directory, so a session started from a script
(`$(fff pick)`) or a file dialog (`fff explorer`) must not become one that
answers differently from what its caller expects; those would offer only the
modes that answer the same way.

## Resizing the font

Shift with `+` and `-` would make the terminal's font larger or smaller, for
reading from a distance or on a large screen. A terminal program cannot set
the font itself; it asks the terminal, and only some terminals listen: kitty
through its remote control (`kitty @ set-font-size`, when allowed), xterm and
a few others through the OSC 50 font sequence, others not at all. fff would
use what the terminal supports and otherwise say that it cannot. `+` and `-`
change the search depth in Normal mode, and on many layouts Shift+`-` is `_`,
so the chords need checking against what terminals report (the kitty keyboard
protocol tells them apart).

Behavior, keybindings and persistence for these features remain to be designed
when the work is requested.

## Post-launch

Ideas for after 1.0, once the features above are done.

### A tree view

A key would show the results as a tree, drawn with branch lines the way
`pass` and `tree` do: folders with their contents indented below them, so
the shape of a folder is visible at a glance. Rows would keep working as
rows (marks, operations, the preview), and the query would show the matching
entries with the folders leading to them.

### Folders as matches

A query names a file today, and `/` in it matches folders on the way
(`pde/stab`). In flat lists from deep trees (`fff pick`, `fff seek`,
`fff grep`) a folder's name could match instead: `canard` would list the
files inside a folder called `canard`, ranked by how well the folder
matched, without typing the slash, and the `/` form would go. Elsewhere,
where results are a folder's own entries, the current behavior stays. To try
out: which contexts it suits, how a folder match ranks against a file match,
and how the row shows which part matched.

### Sorting

Results are ranked by depth and match quality today (match quality first in
`fff pick` and `fff seek`). A key would open a
sorting picker, a chooser like the others: type or move to choose the
criterion, Tab switches between ascending and descending. Criteria:

- name, alphabetically (and naturally, so `file2` comes before `file10`);
- size (a folder by its entry count, as the size column shows it);
- last modification, last access, and creation where the filesystem records
  it;
- extension, then name;
- kind: folders first, then files, then links;
- depth, then name.

The order applies among the fuzzy matches of the current query: the query
still decides what is listed, the sort only how. The chosen order would show
on the results' border and could be the default in the configuration.

### Permissions

A key would show each entry's permissions, owner and group, the way `nnn`
does, beside its name (`-rw-r--r--  frank  users`), toggled on and off. The
`permissions` [column](CONFIGURATION.md#result-details) exists and the footer
shows the highlighted entry's `user:group`; this would make them a quick
toggle for every row rather than a configuration change, with owner and
group as columns of their own, and could show octal modes too.

The current release is 0.14.0. Version 1.0 is planned once the roadmap before
[Post-launch](#post-launch) is implemented. Earlier versions will not be retroactively renumbered.
