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

- **In the other programs?** To decide: a contents query in `fff roam`,
  `fff pick` and `fff seek` too, since it sits well in the current
  interface. A row per match would appear only while the contents query is
  not empty; with it empty, the results stay one row per entry, as now. The
  query bars would swap sides, the names query on the left where it is
  today and the contents query on the right, and long queries need a rule:
  which bar gives way, and how a cut query shows that it goes on.
- **An index of the content cache.** Each search reads every cached text
  again; an index built beside the cache (by word, or by trigram so that
  literal substrings still match) would find the candidate files first, so
  that searches over large trees read only those. It must follow the cache:
  a changed file is a new entry, and pruning would drop its index entries
  too.
- **More formats.** Office documents and archives could fill the same cache
  through `rga-preproc`, one run per file.
- **A `/regex/` form** of the contents query.
- **E at the match.** E would open `$EDITOR` at a text match's line when
  the editor accepts `+N`, and a document at its page where the viewer
  takes one (`zathura --page`).
- **Pruning the cache.** Entries are never removed yet; old ones would go by
  age and total size. The text of every PDF in a home folder takes a few
  hundred megabytes at most.
- **A viewer that follows the highlight.** Extracted text loses
  formulas: subscripts, fractions and symbols come out as scattered
  characters. The preview stays text (no images, see [design
  choices](DEVELOPMENT.md#design-choices)), so the page itself belongs in a
  PDF viewer. zathura answers on D-Bus (`org.pwmt.zathura`): one window,
  opened by a key, could follow the highlighted match, going to its page and
  highlighting it as the highlight moves, the way `at_grep_match` opens a file
  there once.
- **Panning a cut match line.** With `toggle_preview_wrap` off, the match's
  line is cut to keep the match in view; keys of their own could pan it left
  and right. Ctrl+h and Ctrl+l are out (Ctrl+h is Backspace on many
  terminals), and stepping between a file's matches needs no keys of its
  own, since each match is a row.
- **OCR stays out.** A scanned document without a text layer cannot match;
  OCR takes seconds a page, too slow here, and `ocrmypdf` adds the text
  layer to the file once, for every program.

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

### Resizing the font

Keys that make the terminal's font larger or smaller, for reading from a
distance or on a large screen. A first version asked kitty and xterm
through escape sequences and was removed, unreliable, before release. The terminal
to aim at is st, which has no escape sequence for its font: its zoom is
bound in its `config.h` (Ctrl+Shift+PageUp, Ctrl+Shift+PageDown,
Ctrl+Shift+Home). To investigate: whether fff can trigger st's zoom (st sets
`$WINDOWID`, so the keys could be sent to its own window), or whether a small
st patch accepting an escape sequence (such as xterm's OSC 50) is the cleaner
way, and how the screen should settle after the size changes.

### Permissions

A key would show each entry's permissions, owner and group, the way `nnn`
does, beside its name (`-rw-r--r--  frank  users`), toggled on and off. The
`permissions` [column](CONFIGURATION.md#result-details) exists and the footer
shows the highlighted entry's `user:group`; this would make them a quick
toggle for every row rather than a configuration change, with owner and
group as columns of their own, and could show octal modes too.

### Faster scanning

Listing a large tree takes about ten times as long as `fd`: 1.6 seconds for
the 620,000 entries of `/usr`, against 0.14, so a query typed at once waits
for the scan to reach its match. Measured, the time splits in halves:

- The walk reads one folder at a time on one thread. Reading each depth's
  folders on several threads, one depth after another, would keep what the
  scan guarantees today: shallower entries arrive first, and `max_paths`
  still stops at the shallowest ones.
- The walker then waits about as long for the interface thread, which merges
  every batch into the fully sorted rows. Kept as fzf keeps its input,
  in chunks sorted on their own and merged only as far as the screen shows,
  a batch would cost little more than appending it. Whatever needs every row
  in order (`G`, the count, `m`, batch rename, the printed marks) would
  finish the merge first, so nothing changes in what fff lists or does.

How close this can come to fzf. Once a tree is listed, nothing in fff
needs a slower algorithm than fzf's: each keystroke scores the same names
with the same algorithm (nucleo is fzf's, in Rust), narrows the previous
matches as fzf does, and sorts by keys as cheap as fzf's. That case already
takes 0.17 seconds over `/usr`, against fzf's 0.02, and what the rest is
spent on has not been measured; the embedded Neovim adds a round trip per
keystroke, which costs milliseconds, not tenths. While a tree is being
listed fff does work fzf leaves to others or does not do at all, and these
set its limit:

- fzf reads a stream it never revisits; fff keeps every known path in a map
  (one entry per path, at its shallowest depth, for overlapping roots,
  rescans, live updates and the discovery cache). That is a hash insertion
  per path, a cost fzf does not pay, but a small and constant one (sharded
  already; a faster hasher than the standard one would cut it further).
- Labels in `fff pick` and `fff seek` depend on every listed entry with the
  same name, so a new path can change another row's label. Kept by name and
  updated per group as paths arrive, as the labels of additions already are,
  they cost a lookup per path; computing them only for the rows on screen
  would make them nearly free. The full relabelling a listing change
  triggers today is the part to avoid.
- Each path is its own allocation (a `PathBuf` in an `Arc`), where fzf packs
  its input into chunks. This is the deepest change, and the last to make.

None of these changes what fff can do, and none grows faster than the
number of paths: a scan within about twice the time of `fd | fzf` looks
reachable, where it is about eight times today. Matching fzf exactly is not
the aim; what is lost to the features above is the price of them.

The current release is 0.15.0. Version 1.0 is planned once the roadmap before
[Post-launch](#post-launch) is implemented. Earlier versions will not be retroactively renumbered.
