#!/usr/bin/env python3
"""Linux PTY integration: real Crossterm decoding with separated stdout.

Run after `cargo build --offline`. CSI-u tests exercise protocol decoding, not
physical keyboard hardware or a terminal emulator's chord configuration.
"""
import errno
import fcntl
import os
from pathlib import Path
import pty
import select
import signal
import struct
import termios
import tempfile
import time
import unittest
from terminal_screen import Screen

BINARY = str(Path(__file__).resolve().parents[1] / "target/debug/picker")


class Session:
    def __init__(self, root, *args, size=(30, 160), implicit_root=False, environment=None):
        master, slave = pty.openpty()
        self.master = master
        fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", *size, 0, 0))
        reader, writer = os.pipe()
        self.output = reader
        self.pid = os.fork()
        if self.pid == 0:
            os.close(master)
            os.close(reader)
            os.setsid()
            fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
            os.dup2(slave, 0)
            os.dup2(writer, 1)
            os.dup2(slave, 2)
            os.close(slave)
            os.close(writer)
            if implicit_root:
                os.chdir(root)
            os.environ["TERM"] = "xterm-256color"
            os.environ["COLORTERM"] = "truecolor"
            os.environ.pop("NO_COLOR", None)
            if environment: os.environ.update(environment)
            if "--config" not in args and "--no-config" not in args and not environment:
                args=(*args,"--no-config")
            os.execv(BINARY, [BINARY, *([] if implicit_root else [str(root)]), *args])
        os.close(slave)
        os.close(writer)
        self.ui = b""
        self.screen = Screen(*size)
        self.status = None
        self.wait_ui("● complete" if size[0] >= 12 and size[1] >= 36 else " complete")

    def wait_ui(self, token, after=None):
        deadline = time.monotonic() + 5
        location = token[1] if isinstance(token, tuple) else None
        token = token.decode("utf-8") if isinstance(token, bytes) else token
        def matches():
            if location is not None:
                lines = self.screen.text.splitlines()
                return str(location) in lines[0] and "● complete" in lines[1]
            return token in self.screen.text
        if after is None and matches():
            return
        while time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.05)[0]:
                try:
                    chunk = os.read(self.master, 65536)
                except OSError as e:
                    if e.errno == errno.EIO:
                        break
                    raise
                self.ui += chunk
                self.screen.feed(chunk)
                if matches() and (after is None or self.screen.frames > after):
                    return
        raise AssertionError(f"UI did not show {token!r}:\n{self.screen.text}")

    def send(self, data, wait=None):
        after = self.screen.frames
        os.write(self.master, data)
        if wait is not None:
            self.wait_ui(wait, after=after)

    def finish(self):
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.status = os.waitstatus_to_exitcode(status)
                break
            # Drain UI so the child cannot block writing to the PTY.
            if select.select([self.master], [], [], 0.02)[0]:
                try:
                    chunk = os.read(self.master, 65536)
                    self.ui += chunk
                    self.screen.feed(chunk)
                except OSError:
                    pass
        if self.status is None:
            raise AssertionError("picker did not exit")
        output = b""
        while chunk := os.read(self.output, 65536):
            output += chunk
        return self.status, output

    def close(self):
        if self.status is None:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except ProcessLookupError:
                pass
        os.close(self.master)
        os.close(self.output)


class TerminalTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="picker-pty-")
        self.root = Path(self.tmp.name)
        self.session = None

    def tearDown(self):
        if self.session:
            self.session.close()
        self.tmp.cleanup()

    def start(self, *args, **kwargs):
        self.session = Session(self.root, *args, **kwargs)
        return self.session

    def test_numbered_choice_ignores_marks_and_returns_exact_nul(self):
        (self.root / "a").mkdir()
        (self.root / "b").mkdir()
        (self.root / "c.txt").touch()
        s = self.start("--null", "--depth", "1")
        s.send(b"\t", b"Marks (1)")
        s.send(b"\x1b[50;5u")  # CSI-u: Ctrl+2
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "b") + b"\0"))

    def test_plain_digit_escape_then_cancel(self):
        (self.root / "2.txt").touch()
        (self.root / "other").touch()
        s = self.start()
        s.send(b"2", b"Query: 2")
        s.send(b"\x1b", b"other")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_empty_directory_enter(self):
        s = self.start("--null")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root) + b"\0"))

    def test_no_matches_numbered_falls_back_to_current_directory(self):
        (self.root / "file").touch()
        s = self.start("--null")
        s.send(b"zzzz", b"No matching results")
        s.send(b"\x1b[57;5u")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root) + b"\0"))

    def test_depth_and_exclusion_editor(self):
        (self.root / "build").mkdir()
        (self.root / "build" / "log").touch()
        (self.root / "main.tex").touch()
        s = self.start("--depth", "1", "--exclude", "build")
        s.send(b"\x1b[1;3C", b"depth 2")  # Alt+Right
        s.send(b"\x1bH", b"exclusions: false")
        s.send(b"\x1be", b"one glob per line")
        before_apply = len(s.ui)
        s.send(b"\r*.tex\x17", b"Query:")
        self.assertNotIn(b"No matching results", s.ui[before_apply:])
        s.send(b"\x1bH", b"No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_mark_list_unmark_then_confirm_file(self):
        (self.root / "a").touch()
        (self.root / "b").touch()
        s = self.start()
        s.send(b"\t", b"Marks (1)")
        s.send(b"\x1b ", "› [x]")
        s.send(b"\t", "› [ ]")
        s.send(b"\x1b", b"Marks (0)")
        s.send(b"\x1bk", "›   1   · a")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "a") + b"\n"))

    def test_page_zero_and_higher_digit_clamping(self):
        for i in range(30):
            (self.root / f"file{i:02}").touch()
        s = self.start(size=(20, 160))  # 9 visible result rows in the split layout
        s.send(b"\x1b[6~", b"file17")  # PageDown
        s.send(b"\x1b[48;5u")  # Ctrl+0 => last of the next 9 rows
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file17") + b"\n"))

    def test_filtered_enter_stays_open_and_missing_marks_block(self):
        marked = self.root / "a"
        marked.touch()
        (self.root / "b").touch()
        s = self.start()
        s.send(b"\t", b"Marks (1)")
        marked.unlink()
        s.send(b"zzzz", b"No matching results")
        s.send(b"\r\x17", b"Missing marked paths")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_tiny_terminal_has_no_numbered_rows(self):
        (self.root / "file").touch()
        s = self.start(size=(5, 160))
        s.send(b"\x1b[49;5u")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root) + b"\n"))

    def test_location_navigation_preserves_marks_and_fixed_root(self):
        (self.root / "a").touch()
        with tempfile.TemporaryDirectory(prefix="picker-location-") as other:
            target = Path(other) / "b"
            target.touch()
            s = self.start()
            s.send(b"\t", b"Marks (1)")
            s.send(b"\x1bg", b"Location (Enter navigates")
            s.send(os.fsencode(other) + b"\r", ("cwd", other))
            self.assertIn(other, s.screen.text)
            s.send(b"b", b"Query: b")
            s.send(b"\r")
            expected = os.fsencode(self.root / "a") + b"\n" + os.fsencode(target) + b"\n"
            self.assertEqual(s.finish(), (0, expected))

    def test_default_browsing_enters_nested_directories(self):
        nested = self.root / "alpha" / "beta"
        nested.mkdir(parents=True)
        target = nested / "file.txt"
        target.touch()
        for enter in (b"\r", b"\x1b[13u"):
            with self.subTest(enter=enter):
                s = self.start("--depth", "1", implicit_root=True)
                s.send(enter, ("cwd", self.root / "alpha"))
                s.send(enter, ("cwd", nested))
                s.send(enter)
                self.assertEqual(s.finish(), (0, os.fsencode(target) + b"\n"))
                s.close()
                self.session = None

    def test_default_parent_navigation_leaves_old_scope(self):
        parent = self.root
        self.root = parent / "child"
        self.root.mkdir()
        (self.root / "leaf-only").touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"leaf-only", b"Query: leaf-only")
        s.send(b"\x1bh", ("cwd", parent))
        s.wait_ui(b"No matching results")
        self.assertIn(str(parent), s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_alt_number_enters_directory_then_confirms_empty_with_marks(self):
        (self.root / "a").mkdir()
        directory = self.root / "b"
        directory.mkdir()
        marked = self.root / "c.txt"
        marked.touch()
        for alt_two, alt_zero in [(b"\x1b2", b"\x1b0"), (b"\x1b[50;3u", b"\x1b[48;3u")]:
            with self.subTest(alt_two=alt_two):
                s = self.start("--null", "--depth", "1", implicit_root=True)
                s.send(b"c.txt", b"Query: c.txt")
                s.send(b"\t", b"Marks (1)")
                s.send(b"\x1b", b"Files \xc2\xb7 3")
                s.send(alt_two, ("cwd", directory))
                self.assertIn("Marks (1)", s.screen.text)
                s.send(alt_zero)
                self.assertEqual(s.finish(), (0, os.fsencode(marked) + b"\0" + os.fsencode(directory) + b"\0"))
                s.close()
                self.session = None

    def test_alt_file_appends_marks_and_clamps_to_last(self):
        for name in ("a", "b", "c"):
            (self.root / name).touch()
        for chord in (b"\x1b0", b"\x1b9"):
            with self.subTest(chord=chord):
                s = self.start("--null")
                s.send(b"\t", b"Marks (1)")
                s.send(chord)
                self.assertEqual(s.finish(), (0, os.fsencode(self.root / "a") + b"\0" + os.fsencode(self.root / "c") + b"\0"))
                s.close()
                self.session = None

    def test_alt_no_matches_stays_open(self):
        (self.root / "a").touch()
        s = self.start()
        s.send(b"\t", b"Marks (1)")
        s.send(b"zzzz", b"No matching results")
        s.send(b"\x1b9", b"No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_alt_number_after_paging_uses_visible_labels(self):
        for i in range(30):
            (self.root / f"file{i:02}").touch()
        s = self.start(size=(20, 160))
        s.send(b"\x1b[6~", b"file17")
        s.send(b"\x1b2")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file10") + b"\n"))

    def test_right_hand_marks_focus_and_resize(self):
        (self.root / "file").touch()
        s = self.start()
        title = next(line for line in s.screen.text.splitlines() if "Files ·" in line)
        self.assertGreater(title.index("Marks (0)"), title.index("Files ·"))
        self.assertIn(b"38;2;", s.ui)
        s.send(b"\t", b"Marks (1)")
        s.send(b"\x1b ", "› [x]")
        # Resize to a compact stacked layout, then exercise the same marked row.
        fcntl.ioctl(s.master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 60, 0, 0))
        s.screen = Screen(24, 60)
        os.kill(s.pid, signal.SIGWINCH)
        s.wait_ui("Marks (1)")
        s.send(b"\t", "› [ ]")
        s.send(b"\x1b", b"Marks (0)")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_alt_l_navigates_directories_but_never_confirms_files_or_empty(self):
        directory = self.root / "folder"
        directory.mkdir()
        target = directory / "file"
        target.touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"\x1bl", ("cwd", directory))
        s.send(b"\t", b"Marks (1)")
        s.send(b"\x1bl", b"Marks (1)")
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(target) + b"\n"))

    def test_configured_bookmarks_filter_missing_target_and_preserve_marks(self):
        marked = self.root / "marked"
        marked.touch()
        with tempfile.TemporaryDirectory(prefix="picker-bookmarks-") as config_dir:
            base = Path(config_dir)
            target = base / "notes folder"
            target.mkdir()
            (target / "report.tex").touch()
            config = base / "config.toml"
            config.write_text("[[bookmarks]]\nname='My notes'\npath='notes folder'\n[[bookmarks]]\nname='Missing'\npath='missing'\n")
            s = self.start("--config", str(config), "--null", implicit_root=True)
            s.send(b"\t", b"Marks (1)")
            s.send(b"\x1bb", b"Bookmarks")
            s.send(b"Missing\r", b"not an accessible directory")
            s.send(b"\x1b", b"Marks (1)")
            s.send(b"\x1bb", b"Bookmarks")
            s.send(b"mnt\r", ("cwd", target))
            self.assertIn("Marks (1)", s.screen.text)
            s.send(b"\x17")
            self.assertEqual(s.finish(), (0, os.fsencode(marked) + b"\0"))

    def test_toml_palette_cli_override_and_directory_banner(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="picker-palette-") as config_dir:
            config = Path(config_dir) / "config.toml"
            config.write_text("[ui]\npalette='custom'\n[palettes.custom]\nextends='light'\ncurrent_directory='#123456'\n")
            s = self.start("--config", str(config))
            self.assertIn("CURRENT DIRECTORY", s.screen.text.splitlines()[0])
            self.assertIn(b"48;2;18;52;86", s.ui)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))
            s.close()
            self.session = None
            s = self.start("--config", str(config), "--palette", "forest")
            self.assertIn(b"48;2;49;94;69", s.ui)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_revisit_after_file_changes_and_force_refresh(self):
        directory = self.root / "folder"
        directory.mkdir()
        old = directory / "old"
        old.touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"\x1bl", ("cwd", directory))
        s.send(b"\x1bh", ("cwd", self.root))
        old.unlink()
        new = directory / "new"
        new.touch()
        s.send(b"\x1bl", ("cwd", directory))
        self.assertIn("new", s.screen.text)
        s.send(b"\x1b[15~", "● complete")  # F5
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(new) + b"\n"))

    def test_remapped_numbered_choice(self):
        (self.root / "file").touch()
        s = self.start("--map-key", "ctrl+n=ctrl+1")
        s.send(b"\x0e")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))


if __name__ == "__main__":
    unittest.main()
