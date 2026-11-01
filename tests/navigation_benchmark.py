#!/usr/bin/env python3
"""Manual Linux PTY benchmark; report measurements, never impose timing assertions.

cargo build --offline --release
python3 tests/navigation_benchmark.py --binary target/release/fff
Uses warm OS metadata and first in-app visits, depth 1, basic editing, no icons.
The terminal screen decoder and Python scheduling are included in elapsed time.
"""
import argparse
import os
from pathlib import Path
import statistics
import tempfile
import time
import terminal_smoke as terminal
from terminal_screen import Screen


class Frames(Screen):
    def __init__(self, *args):
        super().__init__(*args)
        self.history = []

    def _csi(self, params, code):
        super()._csi(params, code)
        if params == "?2026" and code == "l":
            self.history.append(self.text)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/fff"))
    parser.add_argument("--visits", type=int, default=30)
    parser.add_argument("--entries", type=int, default=128)
    args = parser.parse_args()
    if args.visits < 1 or args.entries < 1:
        parser.error("visits and entries must be positive")
    terminal.BINARY = str(args.binary.resolve())
    terminal.Screen = Frames
    with tempfile.TemporaryDirectory(prefix="fff-nav-bench-") as folder:
        root = Path(folder)
        for n in range(args.visits):
            child = root / f"d{n:03}"
            child.mkdir()
            for k in range(args.entries):
                (child / f"file{k:04}").touch()
        session = terminal.Session(root, "--depth", "1", "--no-config", "--no-icons", implicit_root=True)
        elapsed = []
        loading_frames = 0
        try:
            for n in range(args.visits):
                name = f"d{n:03}"
                child = root / name
                session.send(name.encode(), f"❯ {name}")
                session.screen.history.clear()
                start = time.perf_counter()
                session.send(b"\x1bl", ("cwd", child))
                elapsed.append((time.perf_counter() - start) * 1000)
                child_frames = [frame for frame in session.screen.history if any(
                    str(child) in line for line in frame.splitlines() if line.lstrip().startswith("@ "))]
                loading_frames += sum("file0000" not in frame for frame in child_frames)
                session.send(b"\x1bh", ("cwd", root))
            session.send(b"\x03")
            assert session.finish() == (130, b"")
        finally:
            session.close()
        print(f"{args.visits} first visits, {args.entries} files each, depth 1, warm OS cache")
        print(f"Complete-frame latency: median {statistics.median(elapsed):.2f} ms; max {max(elapsed):.2f} ms")
        print(f"Frames at the new location before its files appeared: {loading_frames}")


if __name__ == "__main__":
    main()
