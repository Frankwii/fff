#!/bin/sh
# Records the README's GIFs with VHS (https://github.com/charmbracelet/vhs;
# on Arch, pacman -S vhs). Run from anywhere; builds fff first. With
# arguments, records only those tapes: record.sh find contents.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
export FFF_DEMO=${FFF_DEMO:-/tmp/fff-demo}
cargo build --release --offline --manifest-path "$repo/Cargo.toml"
cd "$repo"
if [ $# -eq 0 ]; then set -- "$here"/*.tape; fi
for tape in "$@"; do
  case $tape in *.tape) ;; *) tape=$here/$tape.tape ;; esac
  # Each recording starts from the same files, remembered rows and caches.
  sh "$here/fixture.sh" "$FFF_DEMO"
  vhs "$tape"
done
