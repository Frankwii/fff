# Possible future development

The current implementation is the baseline. These are potential extensions,
not implemented features or commitments:

- Publishing the package in `packaging/aur` to the AUR once the project nears 1.0.

The preview pane stays text only; there will be no image previews: see
[design choices](DEVELOPMENT.md#design-choices).

## Before 1.0

The goals for the first release.

### For new users

Gaps that writing the [user guides](../GETTING_STARTED.md) showed, in the
order a newcomer meets them:

- **Typing in Normal mode.** The empty query says `type to filter names…`
  in Normal mode too, where typing runs commands instead: `s` links the
  marks, `o` starts Insert mode, `x` trashes. In Normal mode with the
  results in focus the hint should name the key that starts typing
  (`i to filter names…`, through `App::bound`).
- **A setting to start typing.** `[ui] start_in = "insert"` (or similar)
  for `fff roam` and `fff window`, as `fff pick` and `fff seek` already
  start, for people who think of fff as a finder first and of Vim second.
- **The shell function, found missing.** Leaving `fff roam` with `!`, `g!`
  or Enter on a file without `--cd-file` prints the folder to a terminal
  where nothing reads it, and the shell stays where it was. When stdout is
  a terminal, a line on stderr could say how to set the function up
  (`fff setup --init`), once.
- **Setup that sets up.** `fff setup` lists the shell line and completions
  under **Next** for the user to copy; it could offer to add them itself
  (append the `eval` line to `~/.zshrc`, save `_fff` where `$fpath` finds
  it, run `--man`), each asked and shown first, as it does the
  configuration.
- **What a `g` waits for.** About twenty functions start with `g` (`gs`,
  `gw`, `gP`, `gm`, `gc`, `gx`, `g.`, `gd`, `gE`, `gF`, `gl`, `gt`, `g!`,
  `gpp`…). While a prefix waits as PENDING, a small sheet listing what can
  follow and what each does (as which-key does in Neovim) would teach them
  without F1, from the keymap, so rebound keys show as bound.
- **Terminals without the kitty keyboard protocol** (st, urxvt, xterm,
  tmux by default) never send Shift+Enter, Ctrl+Enter, Ctrl+digits or a
  Ctrl+m distinct from Enter, so `print_marks` and `print_highlighted` have
  no working default key there. fff knows whether the terminal answered
  the protocol query: it could say so once (and `--check-config` with it),
  naming the functions left without a key, or give them fallback keys.
- **`fff tutor`.** A practice session, like `vimtutor`: a temporary folder
  of files whose previews are the lessons (move, type, go deeper, search
  contents, mark, copy, undo, pick), each telling the next key to press,
  so that the tutorial is learnt in fff rather than read beside it.
  `docs/demos/fixture.sh` already builds such a folder.
- **`.gitignore`.** Users coming from `fd` expect ignored files to be left
  out. An opt-in `[exclusions] gitignore = true`, reading the ignore files
  of the repositories being scanned, with `ge` turning them off like the
  other exclusions.
- **Key presets.** A few `keys.toml` presets for people coming from
  ranger, lf, nnn or yazi (`fff setup --keys ranger`), so that their hands
  keep working on the first day.

### Distribution

Beside the AUR package: a Homebrew formula (or tap) for macOS, and
prebuilt Linux and macOS binaries on GitHub's releases for people without
a Rust toolchain. `packaging/aur/PKGBUILD` installs `docs/*.md` and
`config.example.toml`: add `keys.example.toml`, which [KEYS.md](../KEYS.md)
links, at the next release.

The crate is 5.8 MiB compressed (0.23.0), about 5.4 MiB of it the README's
recordings in `docs/demos`, which building never reads. An `exclude` for
that folder in `Cargo.toml` would bring it to a few hundred KiB; the
README links the recordings by relative paths, which crates.io resolves
against the `repository` URL, so check with `cargo package --list` and on
the published page that they still show.

## Post-launch

Ideas for after 1.0, once the features above are done.

### Scanning, continued

Listing the 620,000 entries of `/usr` takes about 0.38 seconds headless
(`tests/scan_timing.rs`), in `fff roam` and `fff pick` alike, against 0.28
for `fd | fzf --filter ''`. Of the interface thread's share, storing the
paths among the known ones takes about 0.13 seconds, filtering each round
of new paths 0.035, sorting it 0.07 and pushing it among the rows 0.07.
What could still go:

- **Packed paths.** Each path is its own allocation (a `PathBuf` in an
  `Arc`), where fzf packs its input into chunks. It is no longer the larger
  cost, and the deepest change to make.

Tried and left out, as they changed nothing measurable: keys of 8 bytes
rather than 16 (the score and a sort's value, kept apart so that the
default order moves less), and merging three or four runs at once.

### A tree view

A key would show the results as a tree, drawn with branch lines the way
`pass` and `tree` do: folders with their contents indented below them, so
the shape of a folder is visible at a glance. Rows would keep working as
rows (marks, operations, the preview), and the query would show the matching
entries with the folders leading to them.

### Improvements to matching

A change to how a query matches: queries that tolerate a typo.

**Queries with a typo.** A query matches when its letters appear in order, so a letter left out is
forgiven (`thre lnes` finds `three lines theorem.pdf`) but a wrong, extra or
swapped one is not (`three lnies`). nucleo has no tolerance for typos. Both
queries would get it as a fallback, run only when the query as typed finds
(next to) nothing, so a query without typos costs what it costs today and
ranks as it does.

- **Names.** When the names query lists fewer than a few dozen rows, the
  names are matched again allowing one typo, a letter of the query left
  unmatched or replaced; with letters of the name skipped as usual, a swap
  is one typo. [frizbee](https://github.com/saghen/frizbee) does this (MIT,
  Smith–Waterman like fzf's, with SIMD): by its author's figures, one typo
  over 1.4 million paths takes 61 ms on one thread, against nucleo's 91 ms
  without, so over a home folder (about 190,000 entries to depth 6 with
  dotfiles) the first such keystroke would take some milliseconds and the
  next ones, narrowing the typo matches, less. frizbee compares bytes and
  does not fold accents (`á` against `a` is a typo), so the fallback folds
  them on both sides first, as nucleo's smart normalization does. Rows found
  this way rank after those the query matches as typed. The matched letters
  are highlighted from frizbee's indices, along the path below the root
  as the query matches it.
- **Contents.** A fuzzy match over lines of text matches noise, and an
  approximate scan defeats the contents index: allowing a typo in `three
  lines` leaves too few trigrams to require, and nearly every English text
  passes, so the search would read the whole text cache (seconds, against
  0.02 s). The query is corrected instead: the index would also keep the
  vocabulary of the text it has seen (the distinct words, folded as the
  query is), and when a contents search finds nothing, which the index
  tells at once, each query word missing from the vocabulary is replaced by
  its nearest word within one typo (`lnies` → `lines`), preferring words the
  listed files' filters hold, and the corrected query is searched as usual.
  The query's border shows what was searched.
- **Both at once.** The names query chooses the files and the contents
  query searches them, as now: the names are never matched against the
  match rows. Each falls back on its own, the names first, since they decide
  which files the contents search reads.

Typo matches would join the soft order as a third term: a row matched as
typed has no typo, one found by the names fallback one, and a contents match
as many as words were corrected. Their scores are frizbee's, not nucleo's,
so the simplest order keeps the two apart, the typo matches after the
others, each ranked by score and depth; a single order needs frizbee's
scores brought to nucleo's scale.

To decide: the threshold for the names fallback; how the vocabulary is
stored (a compact sorted set beside the index's segments), kept and pruned
(a stale word costs a correction that finds nothing); whether a key accepts
or refuses a correction; and how a row shows that it matched with a typo.

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

The current release is 0.23.0. Version 1.0 is planned once the roadmap before
[Post-launch](#post-launch) is implemented. Earlier versions will not be retroactively renumbered.
