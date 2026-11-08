#!/usr/bin/env python3
"""Shows the keys pressed during a recording, as captions under the terminal.

    keys.py record FILE -- COMMAND...   run COMMAND in a terminal of its own,
                                        writing the keys typed into it to FILE
    keys.py show FILE                   draw FILE's keys, redrawn as it changes

The recording runs both in two tmux panes (see session.sh). Keys typed in
quick succession make one caption, as screenkey shows them: a word typed
into the query (`stab`), a command line (`fff --depth 3`) or a sequence
(`gP`). Other keys are named: Esc, Enter, Tab, Space, arrows, Ctrl+c; the
terminal's own reports (focus, replies to queries) are left out.
"""
import json
import os
import pty
import select
import signal
import struct
import sys
import termios
import time
import tty
import fcntl

# Keys closer together than this are one caption.
JOIN = 0.25
# Captions older than this are drawn dimmed.
FRESH = 3.0
NAMES = {
    "\r": "Enter", "\x1b": "Esc", "\t": "Tab", "\x7f": "Backspace", " ": "Space",
    "\x1b[A": "↑", "\x1b[B": "↓", "\x1b[C": "→", "\x1b[D": "←",
    "\x1bOA": "↑", "\x1bOB": "↓", "\x1bOC": "→", "\x1bOD": "←",
    "\x1b[5~": "PageUp", "\x1b[6~": "PageDown", "\x1b[Z": "Shift+Tab",
}


def name(chunk):
    """A key's caption, or None for text to join into the current one."""
    if chunk in NAMES:
        return NAMES[chunk]
    if len(chunk) == 1 and ord(chunk) < 32:
        return "Ctrl+" + chr(ord(chunk) + 96)
    if len(chunk) == 2 and chunk[0] == "\x1b":
        return "Alt+" + chunk[1]
    if chunk.startswith("\x1b"):
        return ""
    return None


def record(path, argv):
    captions = []  # [text, time, joinable]

    def save():
        with open(path + ".tmp", "w") as f:
            json.dump(captions[-40:], f)
        os.replace(path + ".tmp", path)

    save()
    pid, master = pty.fork()
    if pid == 0:
        os.execvp(argv[0], argv)

    def resize(*_):
        size = fcntl.ioctl(0, termios.TIOCGWINSZ, b"\0" * 8)
        fcntl.ioctl(master, termios.TIOCSWINSZ, size)

    resize()
    signal.signal(signal.SIGWINCH, resize)
    old = termios.tcgetattr(0)
    tty.setraw(0)
    try:
        while True:
            try:
                ready, _, _ = select.select([0, master], [], [])
            except InterruptedError:
                continue
            if master in ready:
                try:
                    data = os.read(master, 65536)
                except OSError:
                    break
                if not data:
                    break
                os.write(1, data)
            if 0 in ready:
                data = os.read(0, 1024)
                os.write(master, data)
                chunk, now = data.decode("utf-8", "replace"), time.time()
                key = name(chunk)
                if key == "":
                    continue  # the terminal's own reports, not keys
                last = captions[-1] if captions else None
                if key is None or (key == "Space" and last and last[2] and now - last[1] < JOIN):
                    if last and last[2] and now - last[1] < JOIN:
                        last[0] += chunk
                        last[1] = now
                    else:
                        captions.append([chunk, now, True])
                else:
                    captions.append([key, now, False])
                save()
    finally:
        termios.tcsetattr(0, termios.TCSADRAIN, old)
    os.waitpid(pid, 0)


def show(path):
    # Catppuccin Mocha, as the recordings' palette.
    base, text, chip, newest, dim = "30;30;46", "205;214;244", "69;71;90", "137;180;250", "127;132;156"
    sys.stdout.write("\x1b[?25l")
    drawn = None
    while True:
        try:
            with open(path) as f:
                captions = json.load(f)
        except (OSError, ValueError):
            captions = []
        now = time.time()
        width = os.get_terminal_size().columns
        state = ([c[0] for c in captions], [now - c[1] < FRESH for c in captions])
        if state != drawn:
            drawn = state
            chips, used = [], 0
            for i, (label, at, _) in enumerate(reversed(captions)):
                label = label.replace("\x1b", "")
                cell = len(label) + 2
                if used + cell + 1 > width - 4:
                    break
                used += cell + 1
                if i == 0 and now - at < FRESH:
                    colors = f"48;2;{newest};38;2;{base};1"
                elif now - at < FRESH:
                    colors = f"48;2;{chip};38;2;{text}"
                else:
                    colors = f"48;2;{base};38;2;{dim}"
                chips.append(f"\x1b[{colors}m {label} \x1b[0;48;2;{base}m")
            line = " ".join(reversed(chips))
            pad = max(0, (width - used) // 2)
            sys.stdout.write(f"\x1b[48;2;{base}m\x1b[2J\x1b[2;1H{' ' * pad}{line}")
            sys.stdout.flush()
        time.sleep(0.03)


if __name__ == "__main__":
    if sys.argv[1:2] == ["record"] and "--" in sys.argv:
        record(sys.argv[2], sys.argv[sys.argv.index("--") + 1:])
    elif sys.argv[1:2] == ["show"]:
        show(sys.argv[2])
    else:
        sys.exit(__doc__)
