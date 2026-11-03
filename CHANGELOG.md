# Changelog

User-visible changes in each release of fff (the `fuzzy-file-finder` crate).
Entries before 0.8.0 were written afterwards from the release notes and
history.

## Unreleased

### Added

- **`gd` switches how rows are labelled** (`toggle_labels`): by name, with
  only the parent folders that tell same-named entries apart
  (`folder1/main.pdf`), or by their path below the folder searched. The
  choice stays when `gm` changes program.
- **`R` replaces a key on the Keys screen**: in a function's keys, press the
  new key and it takes the highlighted one's place, at once when nothing
  else has it there; where it clashes, where it applies opens with the clash.
- **A `function` palette color** for function names on the Keys screen, in
  the `:` picker and where a key clashes; each built-in palette's red.

### Changed

- **`fff seek` labels rows by their path** below the folder searched, as
  `fff roam` does; `fff pick` and `fff grep` keep names. `gd` switches.
- **Putting a function's default keys back is `D`** (was `R`), and asks
  first.
- **Clashes read as a table** where a key applies: the keys it clashes with,
  their functions and where they meet, and the ticked values they share are
  marked in the checklists.
- **A function's keys go round**: moving past the last row comes back to the
  first, and before the first to the last.
- **The Keys screen's `r` hint shows in Normal mode only**, where `r` works,
  and its hints take a second line rather than being cut.
- **Enter on a cached row that turned from a file into a folder** (or back)
  waits for the rescan instead of acting on what it was.
- **A sheet too tall for the terminal asks for a larger one** instead of
  being cut, as fff does below 36 × 12; a function's keys first leave out
  the checklists that repeat the highlighted row.

## 0.15.0 — 2026-11-03

`fff grep` searches what files contain, `:` runs a function by name, and
the Keys screen's tables fit what they hold.

### Added

- **`:` in Normal mode picks a function by name**: a small fuzzy picker
  of the functions that work where you are, each with its key there; Enter
  runs the highlighted one as its key would, Escape goes back. In the
  results' query it replaces Neovim's command line.
- **`fff grep` chooses files by what they contain.** A contents query beside
  the names query (Tab switches) lists one row per match, with its page or
  line and the line it is on; the preview shows the lines around it. PDF,
  DjVu and EPUB documents are searched through their text, extracted once
  into `~/.cache/fff/text`; later searches are immediate. The query is a
  case-smart literal that ignores accents unless typed and matches across
  line breaks and hyphenation. Enter prints the chosen files, like
  `fff pick`, which it browses like.
- **Opening at a match.** An `[[open.rules]]` entry's `at_grep_match` adds
  arguments when a file opens from a `fff grep` match (`l`, Alt+l), with
  placeholders for everything fff knows about it: `{page}`, `{line}`,
  `{percent}`, `{query}`, `{match}`, `{nth}`, `{count}` and `{path}`.
  `--page {page} --find {query}` opens zathura on the match's page with the
  query highlighted.
- **`toggle_preview_wrap`** cuts the preview's long lines instead of
  wrapping them; it has no key by default.
- **`gm` switches program**: go on as `fff roam`, `pick`, `seek` or `grep`
  (`switch_program`). The folders, query, highlight, marks, exclusions and
  filters stay; the depth follows the new program's default unless you set
  another; `fff grep`'s contents query waits for coming back to it. The
  session leaves as the program it ends in. Dialogs and `--window` sessions
  do not switch.

### Changed

- **The Keys screen's tables fit their contents**: columns are as wide as
  what they hold, so long program lists are no longer cut while other
  columns sit empty; the details pane lists a function's keys one row each,
  and a key's further places read `or Enter` instead of a bare `or`. Hints,
  prose and checklists line up with the rows they explain.

- **The preview wraps long lines** onto the next rows instead of cutting
  them at the pane's edge.

- **Matching scores like fzf.** A name's score is the best alignment of the
  query's letters, so a whole word or a run deep in a long name beats letters
  scattered from its start: `canard` finds
  `krupa_szmolyan_relaxation_oscillation_canard_explosion.pdf` before
  `minicozzi_colding_a_course_in_minimal_surfaces.djvu`. The highlighted
  letters are the ones that scored. Latin accents are ignored unless the
  query has some: `evaluacion` finds `Evaluación`.
- **`fff pick` ranks the best match first**, as `fff seek` does, whatever its
  depth; depth only breaks ties.
- **Typing highlights the best match.** Each letter typed or deleted moves
  the highlight to the first row, instead of following the entry that was
  highlighted before however far it fell. Rows a scan adds under the same
  query still keep the highlighted entry.

### Fixed

- **Typing in a large listing no longer waits for every keystroke's
  ranking.** A letter typed before the previous one was ranked used to rank
  and relabel every known path again: over 600,000 paths, the match took 1.5
  seconds to show, and now 0.15.

- **`fff grep`'s match preview scrolls**: Ctrl+j and Ctrl+k
  (`scroll_preview_down`, `scroll_preview_up`) move it from the match, past
  the lines kept around it, up to 500 lines either side; another match
  starts at its own match again.

## 0.14.0 — 2026-11-03

F1 opens the Keys screen: search every function and key, rebind them, and
choose where each key applies.

### Added

- **The Keys screen (F1).** A picker laid out like the results: every
  function fff has, by name, with its keys in Insert and Normal mode, and the
  highlighted one's details beside (Ctrl+j/Ctrl+k scroll them). The query
  searches names, descriptions and keys, and your own keys move through it
  (Alt+j/Alt+k, `j`/`k`, Escape clears the query); Tab switches to the Keys
  view, a row per key with its function, mode, programs, lists and
  highlights, and `r` then a key searches for that key there.
  It shows what works where you opened it; Ctrl+a shows everything. Space
  goes a level deeper at every level, Enter is done and Escape goes back:
  Space opens a function's keys in a sheet like the bookmark form (`n` adds,
  `d` deletes, `R` resets), Space on a key opens where it applies, and
  Enter saves to `keys.toml` while Escape drops the changes. F1 closes the
  screen.
- **Where a key applies is four checklists**: the mode, the program, the
  list and what is highlighted, ticked `●` or not `○`, and crossed out `×`
  where the key cannot apply with the other ticks. Columns say `all`, a list
  or `all but` a list, whichever is shorter. A key that applies in places
  one set of ticks cannot say is given again, and its rows read "or".
  Tab and Shift+Tab move between the columns.
- **Functions are named a verb and what it acts on** (`highlight_next`,
  `go_to_parent`, `go_to_highlighted`, `toggle_hidden`, `show_bookmarks`,
  `print_highlighted`, `quit_into_highlighted`...), and each description
  starts with that verb.
- **`go_to_highlighted`**: goes into the highlighted directory, or to the
  highlighted file's directory with the file highlighted. `l` and Alt+l do it
  on a directory, Enter in Insert mode in `fff roam` on a file; give it a key
  for both on the Keys screen.
- **Keys apply where you choose.** A key can be limited to programs (roam,
  window, pick, seek, each kind of explorer dialog), to the results or the
  marked list, and to a highlighted directory, file or nothing. One key does
  one thing wherever it is pressed: when keys clash, the Keys screen offers
  to narrow the new one or take it from the others there.
- **No way to lock yourself in.** Saving keys that would leave nothing to open
  the Keys screen or to leave fff, that would turn typing into commands, or
  that would take `i`, `a` or `:` in Normal mode, is refused with the reason.
  A `keys.toml` with errors gives way to the default keys, and fff says why.
- **`gc` edits the configuration while fff runs** and applies it as soon as
  the editor ends: keys, bookmarks, openers, columns, the mouse, the trash
  and the rest take effect without restarting. A copy of the configuration
  is kept as `config.toml.bak` while you edit; if the edit has errors, fff
  goes on reading the copy, F2 shows what is wrong, and the query box says
  `config errors` until the file loads again (also in later sessions).
- **F2 opens the Session tab** directly, F1 the Keys screen; each key closes
  its own tab.
- **File filters can be turned off and on again** without losing them:
  `gf` in Normal mode, Alt+F while typing.
- **Normal-mode keys for exclusions, filters and palettes**: `gE` edits the
  exclusions and `ge` turns their enforcement on or off, `gF` edits the file
  filters, `gp` opens the palette chooser and `gP` goes back to the
  configuration's palette.
- **The button bar hides once the mouse rests**: with `buttons = "mouse"`,
  it shows while the mouse is moving or clicking and goes after
  `[ui] buttons_seconds` (5 by default; 0 keeps it).
- **Syntax colors in the preview pane**: text files show comments, strings,
  keywords, types, functions and numbers in the palette's colors, worked out
  by a private Neovim with its tree-sitter parsers or `:syntax` files. The
  text never waits for them. `[ui] highlight = false` turns them off.

### Changed

- **Keys live in `keys.toml`** beside the configuration, written in Vim's
  notation (`<M-h>`, `<CR>`, `gg`) by function, in an `[insert]` and a
  `[normal]` table. Each Enter, `l` and `o` case is a function of its own
  (`quit_into_highlighted`, `open_and_quit`, `print_highlighted`, `go_to_highlighted`,
  `open_in_background`, `open_in_terminal`...), so each can be bound apart. A
  configuration with `[keys]` says where they went.
- **Keys inside choosers, prompts and sheets are fixed**: Enter, Escape, Tab,
  and `j`, `k`, `gg`, `G`, `q` and `!` in Normal mode.
- **Moves, copies and links act at once** when nothing would be overwritten,
  and the message says `u` undoes them. Overwriting still asks first, and so
  do files dropped onto the terminal.
- **Default depth: 1 in `fff roam` and `fff explorer`, 3 in `fff pick`**
  (was 2); `fff seek` keeps 6.
- **`+` and `-` change the depth in Normal mode**, instead of Right and Left,
  and **`g/` opens the go-to prompt** instead of `/`.
- **Alt keys for features work only while typing**: in Normal-mode results,
  Alt+b, Alt+g, Alt+t, Alt+., Alt+d, Alt+D, Alt+v, Alt+e, Alt+H, Alt+f,
  Alt+F, Alt+p and Alt+P say which Normal-mode key to use instead (`b`, `g/`,
  `gt`, `.`, `d`, `D`, `P`, `gE`, `ge`, `gF`, `gf`, `gp`, `gP`). Alt keys
  that move around work in both modes.
- **`e` opens in `$EDITOR` and `E` extracts archives**, swapped from before.
- **Pinned roots that do not fit point to F2**, whose Session tab lists them.
- **A rule in the preview pane sets an entry's facts** (a PDF's pages,
  where a trashed entry came from) apart from its content.

### Removed

- **`[keys.normal]`, `[keys.map]` and `--map-key`**, replaced by `keys.toml`
  and the Keys screen; `depth_digits = false` is now `depth = "none"` in
  `[normal]`.
- **The printed key guide**, replaced by the Keys screen.

## 0.13.0 — 2026-11-03

The trash opens like a folder and R restores from it, the preview pane
scrolls, and the screen names keys as you configured them.

### Added

- **The trash opens like any folder: Alt+t** (`gt` in Normal mode). Browse,
  filter, preview, mark and rename there as anywhere; pressed again, it goes
  on to the trash of another filesystem. The location line says you are in
  the trash, and the preview pane says where an entry came from and when.
- **R restores entries from the trash** to where they were deleted from,
  only while in the trash. It finds the way even after an entry was renamed
  there, by fff or by hand (records now carry the entry's inode). When
  something has taken the place, a sheet asks to overwrite it (into the
  trash), keep both as `name (1)`, or cancel. `u` puts them back into the
  trash. Configurable as `restore` and `trash` in `[keys.normal]`.
- **Ctrl+j / Ctrl+k scroll the preview pane** by half its height without
  moving the highlight. With the pane hidden, they say how to open it.

### Changed

- **The screen names keys as you configured them.** The F1 guide, sheet
  borders, footers, hints and messages show a chord remapped in `[keys.map]`
  or `--map-key` by the key that now does its work, and `[keys.normal]`
  actions by their sequences; keys that no longer do anything are left out.

### Fixed

- **The purge follows entries renamed in the trash** instead of keeping them
  forever without a record.

## 0.12.1 — 2026-11-03

Large trees list faster, and a stuck mount holds up only its own entries.

### Fixed

- **Large trees list faster.** New paths merge into the results without
  copying every row each time, so a scan of over a million entries fills the
  list up to about three times sooner, with shorter pauses between keys.
- **A stuck network or FUSE mount no longer holds up details elsewhere.**
  Entries on other mounts keep their sizes, dates and counts, owners show by
  number until their names arrive, and stuck previews are capped: past 16, the
  preview pane says it waits for one to end.
- **Ctrl+c leaves at once** while a large listing is still being sorted.

## 0.12.0 — 2026-11-03

fff never waits for a slow disk to draw.

### Fixed

- **A slow network or FUSE mount no longer freezes the screen.** Drawing used
  to check entries on disk: result rows the first time they showed, every
  visible mark on every frame (and every mark twice in F1), the rows of
  confirmations and bookmarks' folders. These details are now read in the
  background, and a frame shows the last ones known until fresh ones arrive.

## 0.11.1 — 2026-11-03

Choosing files starts typing, `fff seek` looks deeper and ranks by score,
marking moves to Shift+Space, and the query box's border carries less.

### Changed

- **`fff pick`, `fff seek` and `fff explorer --save` start in Insert mode**,
  ready to type the query or the name to save under. So do the bookmark
  form's name and path.
- **`fff seek` searches 6 levels deep by default** (2 elsewhere), and ranks
  the best fuzzy matches first, however deep, instead of shallower first.
- **Shift+Space marks**, in Normal and Insert mode alike (it needs a terminal
  with the kitty keyboard protocol); Space still marks in Normal mode, and Tab
  only while typing. The `mark` key in `[keys.normal]` stands for Shift+Space,
  and `[keys.map]` chords that pointed at `tab` to mark now name `shift+space`.
- **The depth is shown on the results' bottom border**, under the count, and
  `files only` / `folders only` beside the count, instead of on the query
  box's border.
- **The progress marker (`✓ complete`, `◌ scanning`) shows only while a scan
  runs and for three seconds after.** Quiet background refreshes no longer
  show it.

### Added

- **Escape twice clears the marks**: in Normal mode with the query empty.

## 0.11.0 — 2026-11-03

The preview pane is shown from the start.

### Changed

- **The preview pane is shown when fff starts.** `[ui] preview = false`
  starts with it hidden, as before.

## 0.10.0 — 2026-11-03

A trash with undo, progress and Ctrl+c for long operations, a folders-only
listing, and many fixes: operations finish safely when the terminal closes,
Enter never leaves with an entry that is gone, and the status on the query
box reads like its title.

### Added

- **A trash.** `x` moves entries to the trash at once, without a
  confirmation: `$XDG_DATA_HOME/fff/trash` (`~/.local/share/fff/trash`), or a
  `.fff-trash-UID` folder at the top of another filesystem, so trashing is
  always a rename. Entries larger than `[trash] ask_above` (10 GiB) ask first,
  in one sheet for the whole selection: delete them permanently or trash them
  anyway. Entries that cannot go to the trash are offered permanent deletion.
  When fff starts, a background thread deletes what has been in the trash for
  more than `[trash] days` (30). `[trash] enabled = false` keeps permanent
  deletion.
- **`X` deletes permanently**, after the size review `x` used to show.
- **`u` undoes file operations**, last first: trashing, moves, renames,
  copies, links, new entries and extractions (created entries go to the
  trash), and overwrites (the overwritten entries come back from the trash).
  An undo checks that every entry is still the one the operation left and
  happens completely or not at all. Configurable as `undo` in `[keys.normal]`.
- **Progress for long operations.** Copies, extractions, deletions, and the
  checks before every operation run on their own thread; one taking more than
  a moment shows a sheet with a bar, the bytes or entries done, the speed and
  the time. Ctrl+c stops it while stopping changes nothing (and between the
  entries of a permanent deletion, putting the rest back); confirmations of
  large selections no longer freeze the screen while they are sized.
- **D hides files**, listing only folders (Alt+D while typing); again to show
  them. `d` and `D` switch between files only and folders only. Configurable
  as `files` in `[keys.normal]`.

### Changed

- `x` moves to the trash instead of deleting permanently; overwritten
  entries of a move or copy also go to the trash when they can.
- **`u` in Normal-mode results undoes file operations**, no longer query
  edits; `:u` still undoes those, or set `undo` to another key or `"none"`.
- **`q` in `fff pick` prints nothing**, like Ctrl+c, instead of the launch
  directory: a script such as `fff pick | xargs -r zathura` no longer opens
  the folder you started in. `!` still prints the browsing directory.
- **Shorter Normal-mode keys:** `d` hides directories (was `zd`) and `P`
  shows the preview pane (was `zp`); `p` still copies. In the results, `d`,
  `D` and `P` no longer reach Neovim as delete and put.
- A key set in `[keys.normal]` takes over a default binding of the same
  key instead of refusing to start, so a configuration with `delete = "D"`
  keeps working and `files` is then left unbound.
- **`fff seek` has no tabs at all:** Up / Down in Normal mode move the
  highlight there instead of reporting that navigation is disabled. Since
  it lists only files, `d` and `D` are not bound there either and reach
  Neovim; `P` shows the preview pane as elsewhere.
- **Small polish across the screen.** The F1 guide shortens a long
  description at its end, keeping the words that say what the key does, and
  its session details say "no query" and spell out whether hidden entries
  are shown and exclusions enforced. Notices and session details wrap
  between words. A compact marked list shows marks without blank lines
  between them, and long mark names keep their extension in view. The
  current / selection / all question names the current entry once, in a
  shorter sentence. `x` or `X` with nothing to act on says "Nothing to
  delete", and refusals to change or undo around the current directory say
  to go up first.
- **The status on the query box's border reads like its title**,
  `✓ complete · depth 2 · hidden shown`, instead of shaded boxes that cut
  through the border; the finished scan shows a check mark rather than a dot
  that small fonts drew as `·`.

### Removed

- **Ctrl+w only returns marks where several paths can be the answer**:
  `fff pick` and `fff explorer --multiple`. In `fff roam` it no longer opens
  the marked files and leaves (`o` opens them here and comes back), in `fff
  seek` it no longer leaves into the last mark's folder, and in other dialogs
  it no longer answers with a mark: in a save dialog that replaced a marked
  file without asking. There it now only says so. The Select & Exit button
  follows suit.

### Fixed

- **fff ends when its terminal closes, also under `nohup`.** With the hangup
  signal ignored it kept running forever, one thread spinning a CPU core on
  the closed terminal; during an operation that thread spun until the
  operation finished.
- **Quitting fff while it empties old entries from the trash** could leave a
  half-deleted entry with its record, looking whole and restorable. Entries
  now leave `files/` and lose their record before they are deleted, and the
  next start finishes an interrupted purge.
- **A `gX` program that cannot start leaves your shell where it was.** The
  shell integration still changed to the browsing directory.
- **`$` and backticks inside double quotes are refused** in `gx` / `gX`
  lines, `[[open.rules]]` commands and `$EDITOR`, as they already were outside
  quotes: a shell would expand them, and fff passed them on literally. Write
  `\$` or `'$'` for a literal one.
- **Copies and moves into a folder below one you can enter but not list**
  (such as another user's home of mode 0711) work; they failed with
  "Permission denied" while checking the destination was not inside a
  source.
- **Tab completes the options of a program typed as a path** (`./script`)
  from its `--help`; it asked `man`, which formatted the file itself as a
  manual page.
- **`[[open.rules]]` extensions with several parts match**: `"tar.gz"` never
  matched anything, since only the part after the last dot was compared.
  An empty extension is now refused when the configuration loads.
- **Enter while typing in `fff seek` chooses the file**, leaving into its
  folder as in Normal mode. It tried to reveal the file in its folder
  instead, which seek forbids, so a file below the starting folder only got
  "Navigation is disabled in fff seek" until Esc left Insert mode.
- **Ctrl+number and double clicks act on their row while typing a name in
  `fff explorer --save`.** They saved under the typed name instead, so Ctrl+1
  on a folder saved in the current one and Ctrl+number on a file did not
  offer to replace it.
- **Operations on large folders no longer freeze fff afterwards.** With live
  updates on, the thousands of changes from deleting, copying or extracting a
  large tree took seconds to process, during which the screen stood still.
- **Capitals and shifted symbols type correctly in kitty, foot, ghostty and
  WezTerm.** With the kitty keyboard protocol, Shift+a reached every text
  field as `a` (and `:` as `;`), so names, commands and `ZZ` could not be
  typed.
- **Pasting into the gx/gX command prompt works**; the text was dropped.
- **Escape leaves Insert mode in the gx/gX command prompt**, and Escape again
  cancels it. Escape was swallowed there, so it could not cancel the prompt.
- Escape leaving Insert mode in the bookmark chooser keeps the highlighted
  bookmark instead of jumping to the first one.
- `fff setup --check-config` no longer fails on a machine without `st` when
  the configuration does not name it: the built-in terminal defaults are not
  checked.
- The Alt+g prompt suggests the folder fff was started in from zoxide's
  list; zoxide left out the directory it ran in, which was always that one.
- Bookmark names and paths with line breaks or other control characters (as
  `dd` then `P` in the form leaves) are refused instead of being written to
  the configuration.
- `--resume` drops relative paths from a damaged session file instead of
  resolving them against wherever the new session runs; a relative tab
  folder makes the file count as damaged.
- An unknown palette's error lists every built-in palette, not three.
- **Normal-mode Enter on a cached row whose entry was deleted stays** and
  says so, until the refresh drops the row. It left into the entry's folder,
  which `fff pick` printed although nobody chose it.
- **Insert-mode Enter that reveals a file in the current folder keeps
  Insert mode**, as revealing one elsewhere does. The query dropped to
  Normal mode, so the next letters acted as commands (`x` trashed the file).
- Enter on a marked-list row whose path was deleted says so instead of
  printing (`fff pick`) or opening the missing path.
- In `fff seek`, Normal-mode Enter with nothing listed stays instead of
  leaving into the starting folder, and Ctrl+o / Ctrl+i say navigation is
  disabled instead of reaching Neovim, where Ctrl+o made the next letter a
  command.
- Ctrl+number or a double click on a row that a just-finished re-sort
  removed does nothing, instead of acting as Enter with nothing highlighted.
- fff no longer keeps a core busy while a large listing is re-sorted on
  another thread, or during a scan after a change on disk.
- The preview pane no longer fails on a file holding only a SQLite header,
  and shows the line count of a text file of exactly 64 KiB.
- PageDown at the end of the results keeps a full page in view.
- Ctrl+s in the new-name and extraction prompts keeps them open on a
  validation error, as Enter does.
- **Stopping a permanent deletion right after an entry went no longer
  reports that entry as only partly deleted**, keeps its mark or leaves an
  empty `.fff-stage-*` folder behind. Ctrl+c is heard before an entry is
  deleted, never after, and a stop before anything of an entry was deleted
  says it was not deleted.
- `e` with an empty destination extracts beside the archive in a folder
  browsed through a symlink; it failed with "Not a directory".
- `u` refuses to undo a trashing while you browse inside the trashed folder
  in the trash, as it does for other undos that would move the current
  directory away.
- The trash purge keeps entries whose recorded date is impossible (a 13th
  month, a 31st of April) instead of reading them as a later real date.
- **The Select & Exit button in `fff explorer` is Enter on the highlighted
  row**, as documented: saving over a file asks first, and a folder is
  entered. It answered with the row as it was, so in a save dialog it
  overwrote a file without asking.
- Enter while typing in an empty folder no longer closes a `--window`
  session, or cancels a dialog choosing files in `fff explorer`; only where
  a folder is an answer (`fff pick`, `fff roam`, folder dialogs) does it
  answer with the folder.
- Live updates no longer watch the targets of symlinked folders listed
  below the browsed one, which fff never lists the insides of: changes in
  `/tmp` behind a `tmp` link no longer rescan the results, and a link into
  an unreachable mount cannot hang the screen.
- Closing the terminal while a file operation runs lets the operation
  finish before fff ends. fff ended at once, stopping it partway and leaving
  entries in hidden `.fff-stage-*` folders.
- The remembered-locations and session files are readable only by you; they
  hold your folders, marks and queries.
- **Lowering the depth after `max_paths` stopped a scan lists the shallower
  results completely**, instead of stopping again at once and showing
  deleted files until F5.
- A change on disk noticed just as you left a folder no longer lets fff show
  the old listing as current when you come back.
- The chosen Alt+g suggestion stays visible on short terminals, and editing
  the typed path from Neovim's command line clears the choice, so Enter
  never goes to a row you cannot see.
- Greek queries with Σ, σ or ς match names ending in a capital Σ.
- **The first "Show in folder" of a session works.** When the desktop started
  fff to answer it, that call was lost and the asking program waited 25
  seconds for nothing; only later calls opened fff.
- A leading `~` in `[open]` commands (`terminal`, `fff_terminal`, a rule's
  `command`) no longer fails the configuration with "must name a program".
- Tab after a bare `~` in the gx/gX prompt completes inside the home folder
  instead of turning it into a relative path.
- A `file://` URI with `%+5`-style escapes is no longer decoded as a byte,
  and the short `file:/path` form is understood. Files dropped on the
  terminal decode their URIs the same way; a `%` without two hex digits
  after it no longer turns the whole drop into query text.
- Tab in the gx/gX prompt no longer treats a backslash inside single quotes
  as an escape, which made it complete the wrong word.
- `fff explorer` never answers a save dialog with a path containing a
  newline, which the portal would read as two paths.
- **Read-only folders can be copied, deleted and overwritten.** A folder
  without write permission for its owner (mode 555, say) failed with
  "Permission denied" where `cp -r` and `rm -r` succeed; the copy keeps the
  folder's mode.
- Extraction refuses archive entry names containing C1 control characters
  such as U+009B, which a terminal reads like Escape, as it already did for
  other control characters.
- **Renaming many entries no longer quits fff.** Saving a rename list larger
  than about 64 KiB (thousands of rows, as in a deep `fff pick` or
  `fff seek`) ended the session with "Neovim stopped: Neovim RPC
  disconnected"; such lists now rename like short ones. Any large multiline
  field closed with ZZ or `:wq` had the same problem.
- **Small folders open in one frame, complete.** Entering a folder, going up
  or starting fff no longer draws the location first empty or from the cache
  (with `◌ cached · refreshing`) and then again a moment later: a scan that
  finishes within about a frame is shown only once it has, so the label and
  the flicker are left to listings that really take a while. Quiet rescans
  after live updates no longer flash the label either.
- Tab in the Alt+g prompt no longer crashes on a path ending in `/.` after
  a name with accented letters, and completes after the text as typed:
  `~` and `..` complete inside those folders, and `only/.` offers hidden
  folders, instead of gluing a name onto the text.
- Long result names shortened to fit a narrow window no longer crash the
  match highlighter.

## 0.9.1 — 2026-11-02

Path-aware matching, "Show in folder" with the file highlighted, live updates
across the whole listing, more of the mouse, and huge trees that stay
responsive while they load and re-rank.

### Changed

- **Live updates reach deeper results and pinned roots.** With `[ui] watch`,
  files created, removed or renamed in any listed folder (not only the
  current directory) or in a pinned root appear and disappear without F5, and
  a folder's entry count follows. Up to 4096 folders are watched, shallowest
  first.
- **Browsing stays responsive while huge trees load.** Moving the highlight
  during a scan of hundreds of thousands of paths (say `$HOME` with hidden
  files at depth 9) no longer stalls for a second at a time: keys are handled
  before discovery work, and new paths are merged into the results a little
  at a time instead of re-sorting everything, shallowest first. Such scans
  also finish several times faster and take less memory.
- **The highlight stays on the first row while a location loads**, instead of
  following whichever row was first when the scan began, until you move it
  (or a remembered row, such as the folder you came up from, is listed).
- **Re-ranking a large listing no longer freezes the screen.** Changing
  depth, hidden files, exclusions, filters or Alt+d, and typing a query, rank
  big listings on other threads in parallel; the previous results stay up,
  with `◌ sorting` on the status line, until the new ones are ready. Typing
  never waits for them; keys that act on rows do.
- `fff pick` and `fff seek` update their labels for shared names only where
  they changed, so a large tree no longer slows them down as it fills.

### Added

- **"Show in folder" highlights the file.** `fff setup --desktop` also
  installs a D-Bus service for `org.freedesktop.FileManager1`, so Firefox's
  "Show in folder" (and other programs' "Open containing folder") opens fff in
  the file's folder with the file highlighted, instead of only the folder.
  Run `fff setup --desktop` again to add it.
- **The mouse in the marked list and the choosers.** A click on a mark
  focuses the marked list there and a double click acts like `l` on it; in
  the bookmark, icon and palette choosers and the Alt+g suggestions, a click
  selects a row and a double click chooses it. The wheel moves through them.
- **Dragging past the results' edge scrolls them**, marking rows as it goes,
  for as long as the button is held there.
- **Path-aware matching.** A query containing `/` matches the path below
  each result's root instead of its name: `pde/stab` finds
  `pde/notes/stability.tex` but not `notes/stability.tex`, and `pde/` lists
  what is inside folders matching `pde`. The parts match components in
  order, the last one the name; whole and consecutive components rank
  higher, and the matched letters are highlighted along the whole path.
  Queries without `/` match as before.
- **`[performance] max_paths`** (default 1,000,000) bounds the paths a view
  holds, so browsing a huge tree cannot exhaust memory. A scan that finds
  more stops there and the status line shows `◌ partial`; a lower depth shows
  the rest.

## 0.9.0 — 2026-11-02

A text-only preview pane, `fff --resume`, and a command for each way of using
fff: `fff pick`, `fff seek` and `fff setup` join `fff explorer`, and plain `fff`
is `fff roam`.

### Added

- **A text-only preview pane.** Alt+v (`zp` in Normal mode) shows the
  highlighted entry in the bottom half of the screen: a text file's first
  lines, a folder's entries, an archive's or EPUB's contents, a PDF's title,
  author, pages and first pages' text (with poppler), an image's dimensions,
  and audio and video duration, streams and tags (with `ffprobe`). Reading
  happens in the background after the highlight rests and never holds up the
  list. `[ui] preview = true` starts with it shown; rebind it as `preview` in
  `[keys.normal]`.
- **`fff --resume`** continues the last `fff roam` session as it ended: its
  folder and highlighted row, tabs and their history, marks, query, depth,
  filters, exclusions and toggles. Only the last session is kept; `fff pick`,
  `fff seek` and `fff explorer` sessions are never saved.
- **Alt+d (`zd` in Normal mode) hides directories**, listing only the files
  found down to the current depth; press it again to show them. Navigation
  keeps working, and the status line shows `files only` meanwhile. Rebind it
  as `directories` in `[keys.normal]`.
- **`fff roam`** names what fff does without a command: browse, and change the
  shell's directory on leaving. `fff` alone stays the same as `fff roam`.

### Changed

- **`--output-paths` is now `fff pick`**, the only command that prints paths:
  `nvim "$(fff pick)"`, `fff pick --relative -0`. It takes ROOTS and
  `--browse` like `fff roam`. Its rows show bare names, with only the parent
  directories that tell same-named entries apart, as `--no-dir` did. Scripts
  that combined `--output-paths` with `--no-dir` and `--glob` use
  `fff pick --glob ...`, which lists only the matching files.
- **`--no-dir` is now `fff seek`**: `fff seek ~/papers --glob '*.pdf'`. It
  behaves as `--no-dir` did without `--output-paths`, leaving into the chosen
  file's folder, and rejects `--browse`, `--window` and `--resume`.
- **Setting up is `fff setup`**: `--init SHELL`, `--completions SHELL`,
  `--desktop`, `--file-chooser` (or `--browser`) and `--check-config` replace
  `fff --init`, `--completions`, `--setup desktop`, `--setup file-chooser`
  and `--check-config`. Update the shell lines in your startup files, e.g.
  `eval "$(fff setup --init zsh)"` and `source <(fff setup --completions
  zsh)`. Each moved option now fails with a pointer to its replacement.
  Launchers and wrappers written by `fff --setup` keep working and are
  replaced when setup runs again.
- `--glob` and `--exclude` also work after a command: `fff explorer --glob
  '*.pdf'` lists only PDFs in a file dialog.
- **Every session's footer says how to leave**: `q quit` (`Ctrl+c quit` while
  typing) beside `F1 keys`, not only in `fff explorer`.

### Removed

- **Automatic resuming.** A new session starts in the current directory, on
  the row last highlighted there; use `fff --resume` to continue the last
  session. `[ui] resume` is still accepted and does nothing, and `--here` is
  accepted for scripts but no longer needed.

## 0.8.1 — 2026-11-02

Folders opened from the desktop now behave like a file manager window.

### Changed

- **Folders opened from the desktop are browsed, not left.** fff started by
  `xdg-open`, a launcher or "Show in folder" has no shell to change into, so
  Normal-mode Enter on a folder (or the Select button) now enters it instead
  of closing the window, and `!` turns the window into a shell (`$SHELL`) in
  the folder being browsed. Enter on a file still opens it and closes the
  window. Run `fff --setup desktop` again to update the launcher.

### Added

- **`--window`**, for fff running in a terminal window of its own: Enter
  enters folders, `!` starts a shell in the current folder, and nothing is
  printed. The desktop launcher passes it.

## 0.8.0 — 2026-11-02

fff can now be the desktop's file manager and Firefox's file chooser, and the
keys that open or finish something were reworked into one consistent model.

### Added

- **`fff --setup desktop`** makes fff the program that opens folders: it
  installs a desktop entry and a small launcher script, makes it the default
  for `inode/directory` with `xdg-mime` (checking that it took effect and
  saying how to restore the previous default), so `xdg-open`, application
  launchers and "Show in folder" open fff in a terminal window.
- **`fff --setup file-chooser`** lets Firefox and other applications that use
  the desktop portal pick and save files with fff, through
  xdg-desktop-portal-termfilechooser: it writes the portal wrapper and selects
  it in the portal configuration, editing existing files one key at a time.
- **`fff explorer [--multiple] [--directory] [--save] [PATH]`**, a mode made
  for answering a file dialog. It starts beside the suggested file, prints the
  answer one path per line, and keeps the usual keys: Enter chooses a file
  and enters a folder (in either editing mode), `!` takes the current folder,
  Ctrl+w the marks; `q` cancels. The empty query line says how to choose, and
  the footer shows `q quit` beside `F1 keys`.
- **Saving from a dialog.** The query box is the file name, and its title
  shows `Save as NAME`. `!` or Ctrl+s (also while typing) saves under the
  typed or suggested name in the current folder, and Enter does while typing.
  The name highlights similar files without hiding anything. Saving over a
  file always asks: Enter replaces, `k` keeps both (`paper (1).pdf`), Escape
  goes back.
- `--browse`: start browsing at a path instead of pinning it as a search root;
  a file starts in its folder, highlighted.
- `[open] fff_terminal` (default `st -c fff-picker -e`): the terminal for
  fff's own windows from `--setup`, with a window class that a window manager
  rule can float.
- Enter or Ctrl+number with nothing to act on now says why (nothing matches,
  still searching, nothing listed) instead of doing nothing.
- In open and save dialogs, `l` on a file explains how to choose it instead
  of trying to open it; a double click is Enter on the row.
- New guide: [desktop integration](docs/DESKTOP.md), with a full
  `fff explorer` reference.

### Changed

- **Normal-mode Enter on a file opens it and leaves** (default mode): a
  terminal opener such as an editor replaces fff in the terminal, like `gX`; a
  GUI opener starts detached, like `l`; the shell then lands in the file's
  folder. Together, `l` stays, `o` comes back, Enter leaves.
- **Ctrl+w in the default mode opens the marked files and leaves**, grouped
  by opener like `o`, instead of only changing to the last mark's folder.
- **Ctrl+number is Enter on that row**, in the current editing mode, instead
  of returning the row's path directly.
- With `--output-paths`, Normal-mode Enter on a file (and Enter in the marked
  list) prints the file instead of its folder.
- The Select & Exit button without marks acts like Normal-mode Enter.
- General options (`--depth`, `--hidden`, `--config`, ...) may follow a
  subcommand.
- fff asks the terminal what it supports and sends the kitty keyboard
  protocol and synchronized output only where confirmed; frames are written
  in one piece. Terminals such as st no longer log a complaint on every frame.

## 0.7.0 — 2026-11-02

### Added

- `[ui] columns`: result rows end with details, by default the size (a
  directory's entry count) and age; permissions are available too. Columns
  drop from the left on narrow terminals. Only visible rows are read, and
  entry counts come from a background worker.
- A richer footer: the highlighted entry's link target, exact size or entry
  count, date, permissions and owner, dropping the least important when
  narrow.
- Links show `→ target` after their name; broken links use the error color,
  and a link's size is its target's.
- `[ui] watch`: the current directory follows changes as they happen, with
  coalesced rescans and details refreshed in place.
- `[ui] mouse`: a click highlights a row and clears the marks, a double click
  acts like `l`, dragging marks the rows it spans, the wheel scrolls, and
  clicking a tab number switches tabs.
- `[ui] buttons` (`off`, `mouse` or `always`): an opt-in button bar for
  mouse-only use, in the results and in the transfer and delete
  confirmations.
- Files dropped onto the terminal (for example st with the drag-n-drop patch)
  are offered for copying into the current directory.

### Changed

- Much faster with many rows or marks: constant-time marks, a windowed marks
  pane, kept results for shorter queries (`[performance] cached_queries`) and
  a remembered highlight index.

## 0.6.0 — 2026-11-01

### Added

- **Openers.** `[open]` rules choose a program per extension or name pattern
  (first match wins, otherwise `$EDITOR`). `l`, Alt+l and Alt+number open a
  file in the background: GUI programs detached from the terminal, terminal
  programs in a new window through `[open] terminal`. `o` opens the current
  or marked files in this terminal, grouped by opener. `--check-config`
  reports missing opener programs.
- Pop-up notices when a background program fails.
- Normal-mode `m` marks every listed row and `M` inverts their marks.

### Changed

- Normal-mode Enter exits into the highlighted entry's folder (also for deep
  results; printed with `--output-paths`). Enter while typing enters
  directories and returns or reveals files. Alt+number acts like `l`.
- Tabs: Up goes to the next tab and Down to the previous one; the tab numbers
  sit in the results' top-right corner.

### Removed

- The basic built-in editor and `--editor`: Neovim is now required.

## 0.5.0 — 2026-11-01

### Added

- **Three tabs**, each with its own directory and Ctrl+o / Ctrl+i history,
  sharing the marks: Alt+Up / Alt+Down, or Up / Down in Normal mode.
- Plain Normal-mode keys in the results: Left / Right and 1–9 for the depth,
  `.` for hidden entries, `/` for the go-to prompt and `b` for bookmarks, all
  rebindable in `[keys.normal]`.
- `[keys.map]`: remap or disable any chord from the configuration, like
  `--map-key` (which still wins).
- The go-to prompt (Alt+g) suggests bookmarks and zoxide's frequent folders.
- Ctrl+o / Ctrl+i walk back and forward through the folders visited.
- File operations are rebindable in `[keys.normal]`.
- `gx` / `gX` complete options from the program's man page or `--help`.
- The F1 guide explains exclusions and file filters.
