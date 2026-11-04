# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- An AUR package once the project nears 1.0.

The preview pane stays text only; there will be no image previews: see
[design choices](DEVELOPMENT.md#design-choices).

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
(`pde/stab`). In flat lists from deep trees (`fff pick`, `fff seek`, and
contents matches) a folder's name could match instead: `canard` would list the
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
- size (a folder by the files below it, as the size column shows it, once
  added up);
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
shows the highlighted entry's permissions and owner; this would make them a quick
toggle for every row rather than a configuration change, with owner and
group as columns of their own, and could show octal modes too.

### Names a running operation will take

Creating a file or folder whose name a running copy, move or extraction will
publish is refused until it finishes. A prompt could offer the choices
instead: name the new entry `name (1).ext`, rename the arriving one to
`name (1).ext` when it lands, overwrite it when it lands, or abort.

### Named workspaces

`fff --resume` keeps only the last session, so a recurring setup (the papers
of one project with their roots pinned and a depth of 3, a folder of notes
with a content search in it, a codebase with its exclusions) is rebuilt by
hand or started from a script of its own. A workspace would be a session
saved under a name: its roots, folders and tabs, depth, filters, exclusions,
toggles and query, and perhaps its marks.

Workspaces would live in the bookmark chooser beside bookmarks, a bookmark
being the simplest workspace: a single folder. The form that adds a bookmark
would offer to save the current session instead, and opening a workspace
would replace the session as `--resume` does. `fff --workspace NAME`
(completed from the saved names) would start in one. To decide: whether
workspaces are kept in the configuration with bookmarks or in the state
directory beside the session, which they resemble more; whether opening one
saves changes back to it or leaves it as saved until saved again; and how the
chooser tells a workspace from a bookmark.

### Shell integration

fff lists what it scans; other programs could supply the list instead, the
way `fzf` reads its candidates: `ls | fff`, `git ls-files | fff pick`, `rg
-l TODO | fff seek`, `locate -i thesis | fff`. Rows would still be paths
(marks, previews, operations and opening would work on them) but the
program, not a folder scan, would decide which. To explore before designing
anything: whether a list comes in on stdin (fff then reads keys from
`/dev/tty`, as `fzf` does) or as a command fff runs and can run again on
reload; which commands accept one (`pick` and `seek` naturally, `roam`
perhaps not, since navigating leaves the list); whether lines that are not
existing paths are allowed, and how relative ones resolve; whether the list
streams in while fff is open; and what live updates, depth and exclusions
mean for it. Other integrations to weigh alongside: a key binding for zsh
that inserts `fff pick`'s choice at the cursor (`fzf`'s Ctrl+t), and a
`cd` into the folder `fff roam` ended in.

### Faster scanning

Listing a large tree takes several times as long as `fd`: about 0.86
seconds for the 620,000 entries of `/usr` (headless, until the rows are
settled; `tests/scan_timing.rs`), against 0.15, and 1.1 seconds in a
terminal with the frames drawn meanwhile, so a query typed at once waits
for the scan to reach its match. In `fff pick`, with its labels, it takes
2.3 seconds headless and 2.8 in a terminal. Each depth's folders are read on
several threads, and paths are attributed to their roots there, which took
the walk out of the way; the rows are kept as sorted runs merged lazily (see
[how rows are kept](DEVELOPMENT.md#how-rows-are-kept-and-ordered)), so a
batch no longer moves the rows ranked before it (inserting `/usr`'s paths
took 0.11 seconds in `fff roam` and 0.17 in `fff pick`, now 0.1 and 0.09).
What remains is the interface thread:

- Storing each path among the known ones takes about 0.5 seconds of it.
  This is memory latency, not hashing: a faster hasher and keys sharing the
  candidate's path were tried and changed nothing measurable, since each
  lookup follows pointers through a table of 620,000 entries.
- Filtering, scoring and sorting each round of new paths takes about 0.12
  seconds in `fff roam`; in `fff pick`, grouping them by name for the labels
  adds about 0.6, and relabelling the groups they change about 0.95 (see
  below). Pushing the sorted rounds as runs, and merging the runs as they
  double, takes the 0.1 seconds above; merging three or four runs at once
  instead of two would move each row about half as often.
- **Limiting redraws while a scan runs.** Each batch merged marks the screen
  as changed, and the session loop draws on its next pass: a scan of `/usr`
  draws about 115 frames in `fff roam` and 400 in `fff pick`, on the same
  interface thread that stores and merges the paths. They, with starting
  Neovim, are the 0.25 to 0.5 seconds the terminal adds to the headless
  time; what each frame costs alone has not been measured.
  Drawing at most every few tens of milliseconds while scanning (and at once
  after a key, as now), or only when what the screen shows changed (the rows
  in view, the count, the progress marker), would leave that time to the
  scan. Frames read only the rows in view from the runs, so they no longer
  grow with the listing.

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
  per path, a cost fzf does not pay, and today the largest one: constant
  per path, but bound by memory latency (see above), so it would shrink
  with the paths packed as fzf packs them rather than with a faster hash.
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
reachable, where it is several times today. Matching fzf exactly is not
the aim; what is lost to the features above is the price of them.

### Semantic search

A query by meaning rather than by letters: `heat equation stability` would
find the notes and papers about it in Spanish, Catalan or English, and the
photos and figures of it, ranked together. Models run locally on the CPU,
and images are embedded directly, never captioned into text first.

The case to aim at: the name query `brezis` and the semantic query
`boundary conditions in the weak formulation` list the sections of Brezis's
book that cover it first, the right one at the top, and Enter opens zathura
on its page with its title highlighted. The two queries do different work:
an author's name says little to an embedding, and the passage may not
contain it, so the name query picks the files (as it does for the contents
query) and the semantic query ranks passages within them. Within one book
that is a few hundred chunks, ranked at once; a cross-encoder reranker
(such as the multilingual `bge-reranker-v2-m3`) could reorder the best
twenty in a second or two. The contents query already lists every page
saying "boundary condition", dozens of them; what this adds is the section
*about* it ranked above the pages that only mention it.

- **Sections as chunks.** A PDF's outline (`mutool show FILE outline`)
  gives its sections' titles and pages, so a chunk is a section, split
  further if long, and a row reads `brezis.pdf  §8.4  Examples of boundary
  value problems  p. 2xx` rather than a page somewhere. Without an outline,
  headings found in the text or page windows instead.
- **Opening at the section.** `at_grep_match` already opens zathura on a
  contents match's page with the query found. A semantic match has no
  literal text to find, so `{query}` would be the section's title or the
  chunk's sentence closest to the query; the preview shows the chunk with
  those sentences highlighted.
- **Formulas.** `pdftotext` mangles them and embedders do not read
  mathematics, so a query described in prose works; a query by formula
  (`‖u‖ ≤ C‖∇u‖`) does not.
- **Trying it first.** Before any of this is built, a throwaway script
  embedding one real folder of notes and papers with e5-small, asked the
  searches actually made lately, would show whether it finds what the
  contents query misses. This query is the first to try.

Two models by default: a multilingual text embedder for documents, and a
multilingual CLIP-style model whose image and text encoders share a space,
for images. A query is embedded by both text encoders, each store searched,
and the rankings fused. The CLIP-style models' text sides are trained on
captions, so they embed a document chunk poorly; a text embedder does not
see images at all.

| Model | Role | Parameters | Weights (f32 / int8) | Notes |
| --- | --- | --- | --- | --- |
| `intfloat/multilingual-e5-small` | text, the default | 118M | 470 MB / 120 MB | 384 dimensions, 512 tokens, about 100 languages (XLM-R vocabulary), MIT |
| `Alibaba-NLP/gte-multilingual-base` | text, if quality needs it | 305M | 1.2 GB / 305 MB | 768 dimensions (truncatable to 128), 8192 tokens, 70+ languages, Apache-2.0 |
| `sentence-transformers/clip-ViT-B-32-multilingual-v1` with CLIP ViT-B/32's image encoder | images, the default | 135M text + 88M image | 890 MB / 225 MB | 512 dimensions, aligned on 50+ languages including Catalan and Spanish, Apache-2.0 |
| `google/siglip2-base-patch16-224` | images, if quality needs it | 375M (about 280M of it text, mostly its 256,000-token vocabulary) | 1.5 GB / 375 MB | 768 dimensions, 64 text tokens, multilingual pretraining, Apache-2.0 |

The default pair is about 340M parameters: 1.4 GB in f32, 340 MB in int8
where the runtime quantizes. A session only searches, so it loads the two
text encoders (about 250M); the image encoder is needed only while
indexing. Most of a multilingual model is its vocabulary table, and with
the weights memory-mapped the rows of words never seen stay on disk, so
what is resident is less than the file. The vectors themselves are small:
100,000 chunks at 384 dimensions in int8 are 38 MB.

With little memory, one model alone: multilingual CLIP for both documents
and images (225M parameters, 225 MB in int8), at the cost of weak document
matches. `jinaai/jina-clip-v2` is one model that does both well (89
languages, 8192 tokens, 512×512 images), but it is not the lighter
choice: 865M parameters, 1.7 GB in f16, a ViT-L over 1,369 patches per
image, and a non-commercial license (CC BY-NC 4.0).

Measured on one P-core of an Intel Core Ultra 7 155H (4.8 GHz), with
PyTorch's reference implementations in f32; int8 quantizes the linear
layers dynamically. Weights were read cold from the NVMe disk. The query is
a short Spanish sentence, the chunk 512 tokens, the image 1024×768 resized
to the model's input.

| Model | Load, cold / warm | Query, f32 / int8 | Chunk, f32 / int8 | Image |
| --- | --- | --- | --- | --- |
| multilingual-e5-small | 4.8 s / 0.39 s | 20 / 7 ms | 251 / 175 ms | |
| gte-multilingual-base | 3.1 s / 0.94 s | 76 / 21 ms | 1,140 / 720 ms | |
| multilingual CLIP, text | 3.1 s / 0.19 s | 28 / 8 ms | 434 / 281 ms | |
| CLIP ViT-B/32, image | 0.8 s / 0.43 s | | | 140 ms |
| SigLIP 2 base | 3.1 s / 0.29 s | 128 / 64 ms (64 tokens) | | 360 ms |
| jina-clip-v2 | 5.8 s / 4.4 s | 1,160 ms | 4,700 ms (800 tokens) | 10.3 s |

- The cold loads are mostly Python and transformers starting up: reading
  the weights themselves took 0.25 to 1.2 seconds for whole repositories
  (about 4 GB/s), which hold the weights in several formats. Memory-mapped
  safetensors in Rust would load the default pair in well under a second.
- A query costs the default pair under 50 ms on one core (e5 and
  multilingual CLIP's text encoder), under 20 ms in int8: fine after a pause
  in typing.
- Indexing is the slow part. At 175 ms per chunk in int8, 100,000 chunks
  take about 5 core-hours, half an hour on all cores; 10,000 images take
  about 25 core-minutes. gte-multilingual-base is four times slower per
  chunk than e5-small, and jina-clip-v2 is 25 to 70 times slower than the
  default pair (without its GPU-only attention kernels), which settles it
  out for a CPU.

- **Indexing offline.** A command (such as `fff index ROOTS`) would embed
  overlapping chunks of each file's text, from the text cache the contents
  query already fills, with the path, and each image whole. Entries would be
  keyed by file identity as the contents index's are, so running it again
  embeds only what changed, and pruned the same way. Half an hour or so for
  a large tree the first time (see above).
- **Searching.** The query is embedded once, after a pause in typing or on
  Enter (partial words embed badly), and compared with every stored vector
  exactly: at personal scale, about 100,000 int8 vectors, that takes a few
  milliseconds, so no approximate index (FAISS, HNSW) is needed unless the
  store grows past a million or so. The model loads lazily on a worker
  thread, so starting fff stays instant.
- **Ranking.** The two models' scores are not comparable, and even within
  one shared space text-to-text similarities run systematically higher than
  text-to-image ones, so results would be fused by rank (reciprocal rank
  fusion), with the fuzzy and contents matches too, rather than by score.
- **In the interface.** A query of its own, beside the name and contents
  queries, with one row per matching chunk (its lines) or image. Unlike the
  contents query, it is a ranking of the closest, not every match.
- **Building.** An optional Cargo feature, so the default build and its
  dependencies stay as they are; `candle` keeps the build pure Rust and
  offline, `ort` is faster but needs ONNX Runtime. Model weights live outside
  the binary, at a path in the configuration.

## Content search, continued

A contents query beside the names query (Tab) searches what files contain,
in `fff roam`, `fff pick` and `fff seek`, one row per match: documents read
through fff's own text cache and searched on its threads, through an index
that rules out the files that cannot match, not by running rga, which
starts a preprocessor for every file on every search (1.85 s and about 25 CPU-seconds a search over
2,400 cached PDFs, against 0.02 s here). What remains:

- **More formats.** Office documents and archives could fill the same cache
  through `rga-preproc`, one run per file.
- **A `/regex/` form** of the contents query.
- **`e` at the match.** `e` (`edit_files`) would open `$EDITOR` at a text
  match's line when the editor accepts `+N`, and a document at its page where the viewer
  takes one (`zathura --page`).
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

The current release is 0.18.0. Version 1.0 is planned once the roadmap before
[Post-launch](#post-launch) is implemented. Earlier versions will not be retroactively renumbered.
