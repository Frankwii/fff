#!/bin/sh
# Records docs/demos/contents.gif on a virtual X display of its own: st and
# fff on one side, and the PDF viewer the contents query opens beside them,
# which VHS (a terminal alone) cannot show. Needs Xvfb, i3, st, xdotool,
# zathura and ffmpeg. Nothing reaches your own display: DISPLAY is a new
# one, and i3, st and zathura read only the demo's configuration.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
export FFF_DEMO=${FFF_DEMO:-/tmp/fff-demo-x}
size=2400x1080
n=99
while [ -e "/tmp/.X11-unix/X$n" ] || [ -e "/tmp/.X$n-lock" ]; do n=$((n + 1)); done
export DISPLAY=:$n

cargo build --release --offline --manifest-path "$repo/Cargo.toml"
sh "$here/fixture.sh" "$FFF_DEMO"
video=$FFF_DEMO/contents.mkv
pids=
cleanup() {
  for pid in $pids; do kill "$pid" 2> /dev/null || true; done
  wait 2> /dev/null || true
}
trap cleanup EXIT INT TERM

Xvfb "$DISPLAY" -screen 0 "${size}x24" -nolisten tcp > /dev/null 2>&1 &
pids="$! $pids"
sleep 1
# The demo's configuration only, for i3 and everything it starts.
export XDG_CONFIG_HOME=$FFF_DEMO/config
unset DBUS_SESSION_BUS_ADDRESS WAYLAND_DISPLAY I3SOCK SWAYSOCK
i3 -c "$here/i3.conf" > /dev/null 2>&1 &
pids="$! $pids"
sleep 1
cd "$repo"
st -f "JetBrainsMono Nerd Font Mono:pixelsize=19" -e "$here/session.sh" > /dev/null 2>&1 &
pids="$! $pids"
sleep 2.5

ffmpeg -v error -y -f x11grab -draw_mouse 0 -framerate 30 -video_size "$size" \
  -i "$DISPLAY" -c:v libx264rgb -preset ultrafast -crf 0 "$video" &
ffmpeg=$!
sleep 0.5
python3 "$here/contents-x.py"
kill -INT "$ffmpeg"
wait "$ffmpeg" || true

ffmpeg -v error -y -i "$video" -vf "fps=15,scale=1600:-1:flags=lanczos,split[a][b];[a]palettegen=stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle" \
  "$here/contents.gif"
echo "Wrote $here/contents.gif" >&2
