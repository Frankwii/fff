# Sourced by the tapes (hidden): fff from this checkout, with the demo's
# home, configuration, state and caches, and a plain prompt.
demo=${FFF_DEMO:-/tmp/fff-demo}
repo=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
export PATH="$repo/target/release:$PATH"
export HOME="$demo/home"
export XDG_CONFIG_HOME="$demo/config" XDG_STATE_HOME="$demo/state"
export XDG_CACHE_HOME="$demo/cache" XDG_DATA_HOME="$demo/data"
export EDITOR=nvim COLORTERM=truecolor
[ -n "$TMUX" ] || export TERM=xterm-256color
unset NO_COLOR
eval "$(fff setup --init bash)"
PS1='\[\e[1;34m\]\w\[\e[0m\] $ '
cd "$HOME"
