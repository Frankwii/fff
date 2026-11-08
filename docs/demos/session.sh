#!/bin/sh
# Run by the tapes (hidden): replaces the recording's shell with tmux, the
# demo's shell above (env.sh, its keys recorded by keys.py) and the keys
# pressed drawn below it.
here=$(cd "$(dirname "$0")" && pwd)
log=${FFF_DEMO:-/tmp/fff-demo}/keys.json
socket=fff-demo-$$
exec tmux -L "$socket" -f "$here/tmux.conf" new-session \
  "python3 '$here/keys.py' record '$log' -- bash --rcfile '$here/env.sh'" \; \
  split-window -v -l 3 "python3 '$here/keys.py' show '$log'" \; \
  select-pane -t 0
