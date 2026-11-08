#!/bin/sh
# Builds the folder the demo recordings browse: a home of its own with
# notes, papers (PDFs made with pdflatex) and downloads, and fff's own
# configuration, state and caches beside it, so that recording never reads
# or writes yours. Usage: fixture.sh DIR (removed and made again).
set -eu
demo=${1:?usage: fixture.sh DIR}
rm -rf "$demo"
home=$demo/home
mkdir -p "$home" "$demo/config/fff" "$demo/state" "$demo/cache" "$demo/data"

mkdir -p "$home/research/pde/notes" "$home/research/analysis" \
  "$home/research/turing_patterns/figures" "$home/notes" "$home/Downloads" \
  "$home/papers" "$home/projects/fff/src"

cat > "$home/research/pde/notes/stability.tex" <<'TEX'
\section{Stability of the heat equation}
Let $u$ solve $u_t = \Delta u$ in $\Omega$. By the Sobolev embedding
$H^1_0(\Omega) \hookrightarrow L^{2^*}(\Omega)$ the energy decays.
% TODO: check the constant in the Poincaré inequality.
TEX
cat > "$home/research/pde/heat_equation.tex" <<'TEX'
\section{The heat equation}
The fundamental solution is the Gaussian kernel.
TEX
cat > "$home/research/analysis/sobolev.md" <<'MD'
# Sobolev spaces

- Sobolev embedding theorem: $W^{k,p} \subset L^q$ for $1/q = 1/p - k/n$.
- Rellich–Kondrachov: the embedding is compact on bounded domains.
- TODO: write up the trace theorem.
MD
cat > "$home/research/turing_patterns/model.py" <<'PY'
import numpy as np

def laplacian(u, h):
    """Five-point Laplacian with periodic boundaries."""
    return (np.roll(u, 1, 0) + np.roll(u, -1, 0) + np.roll(u, 1, 1)
            + np.roll(u, -1, 1) - 4 * u) / h**2

def step(u, v, a=2.8e-4, b=5e-3, tau=0.1, k=-0.005, dt=1e-3, h=1e-2):
    du = a * laplacian(u, h) + u - u**3 - v + k
    dv = (b * laplacian(v, h) + u - v) / tau
    return u + dt * du, v + dt * dv
PY
printf 'P3\n1 1\n255\n0 0 0\n' > "$home/research/turing_patterns/figures/heat.ppm"
cat > "$home/notes/todo.md" <<'MD'
# To do

- Referee report for the journal
- TODO: prepare the seminar on Turing patterns
- Book the room for Thursday
MD
cat > "$home/notes/seminar.md" <<'MD'
# Seminar: reaction–diffusion

Turing's 1952 paper shows how diffusion can destabilise a uniform state.
MD
cat > "$home/notes/reading-list.md" <<'MD'
# Reading list

1. Evans, *Partial Differential Equations*
2. Brezis, *Functional Analysis, Sobolev Spaces and PDE*
3. Gilbarg & Trudinger, *Elliptic PDE of Second Order*
MD
for name in invoice-2026-10.pdf boarding-pass.pdf photo-001.jpg photo-002.jpg \
  dataset.csv slides-draft.odp; do
  printf 'demo\n' > "$home/Downloads/$name"
done
printf 'fn main() {\n    println!("hello");\n}\n' > "$home/projects/fff/src/main.rs"
printf '# fff\n' > "$home/projects/fff/README.md"

# Papers with real text, for the contents query.
if command -v pdflatex > /dev/null; then
  tex=$(mktemp -d)
  paper() {
    cat > "$tex/$1.tex" <<TEX
\\documentclass{article}
\\begin{document}
\\section*{$2}
$3
\\newpage
\\section*{Main result}
$4
\\end{document}
TEX
    (cd "$tex" && pdflatex -interaction=batchmode "$1.tex" > /dev/null)
    mv "$tex/$1.pdf" "$home/papers/$1.pdf"
  }
  paper brezis-functional-analysis 'Functional Analysis' \
    'We recall the Hahn--Banach theorem and weak topologies.' \
    'The Sobolev embedding theorem gives continuity of the inclusion of
    $W^{1,p}$ into $L^{p^*}$ for $p < n$.'
  paper evans-pde 'Partial Differential Equations' \
    'Four important linear equations: transport, Laplace, heat and wave.' \
    'Energy methods prove uniqueness for the heat equation.'
  paper turing-1952 'The Chemical Basis of Morphogenesis' \
    'A system of chemical substances, reacting together and diffusing.' \
    'Diffusion can make a uniform state unstable and form patterns.'
  rm -rf "$tex"
fi

cat > "$demo/config/fff/config.toml" <<'TOML'
[ui]
palette = "catppuccin-mocha"

[[workspaces]]
name = "Notes"
path = "~/notes"

[[workspaces]]
name = "Papers"
program = "pick"
roots = ["~/papers"]
depth = 4
filters = ["*.pdf"]
TOML

# The X recording (record-x.sh) opens PDFs with zathura, at a contents
# match's page with the query found, in the recordings' colors.
cat >> "$demo/config/fff/config.toml" <<'TOML'

[[open.rules]]
command = "zathura"
extensions = ["pdf"]
at_grep_match = "--page {page} --find {query}"
TOML
mkdir -p "$demo/config/zathura"
cat > "$demo/config/zathura/zathurarc" <<'RC'
set default-bg "#1e1e2e"
set statusbar-bg "#313244"
set statusbar-fg "#cdd6f4"
set inputbar-bg "#1e1e2e"
set inputbar-fg "#cdd6f4"
set highlight-color "rgba(249,226,175,0.6)"
set highlight-active-color "rgba(137,180,250,0.6)"
set adjust-open "width"
set guioptions "s"
set recolor true
set recolor-lightcolor "#1e1e2e"
set recolor-darkcolor "#cdd6f4"
RC
