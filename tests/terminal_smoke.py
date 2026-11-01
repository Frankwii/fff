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
import tomllib
import shutil
from terminal_screen import Screen

BINARY = str(Path(__file__).resolve().parents[1] / "target/debug/fff")


class Session:
    # Tests pin basic editing unless they choose --editor or opt out with editor=None
    # (which exercises the configured/default backend).
    def __init__(self, root, *args, size=(30, 160), implicit_root=False, environment=None, editor="basic"):
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
            if editor and "--editor" not in args:
                args=(*args,"--editor",editor)
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
                # The integrated "@" line sits between the filter and results.
                at = [l for l in self.screen.text.splitlines() if l.lstrip().startswith("@ ")]
                return any(str(location) in l for l in at) and "● complete" in self.screen.text
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

    def wait_absent(self, token, timeout=5):
        deadline = time.monotonic() + timeout
        while token in self.screen.text and time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.05)[0]:
                chunk = os.read(self.master, 65536)
                self.ui += chunk
                self.screen.feed(chunk)
        if token in self.screen.text:
            raise AssertionError(f"UI still shows {token!r}:\n{self.screen.text}")

    def wait_bytes(self, token, after=0):
        # Wait for the committed frame containing the color, not a partial write.
        def committed():
            data = self.ui[after:]
            position = data.find(token)
            return position >= 0 and b"\x1b[?2026l" in data[position:]
        deadline = time.monotonic() + 5
        while not committed() and time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.02)[0]:
                chunk = os.read(self.master, 65536)
                self.ui += chunk
                self.screen.feed(chunk)
        if not committed():
            raise AssertionError(f"UI did not emit {token!r} in a completed frame")

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
            raise AssertionError("fff did not exit")
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
        self.tmp = tempfile.TemporaryDirectory(prefix="fff-pty-")
        self.root = Path(self.tmp.name)
        self.session = None

    def tearDown(self):
        if self.session:
            self.session.close()
        self.tmp.cleanup()

    def start(self, *args, **kwargs):
        self.session = Session(self.root, *args, **kwargs)
        return self.session

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_normal_quit_returns_launch_directory_and_ignores_marks(self):
        (self.root / "child").mkdir()
        (self.root / "child" / "file").touch()
        s = self.start("--editor", "nvim", "--depth", "1", implicit_root=True)
        s.send(b"\t", "Marks (1)")
        s.send(b"l", ("cwd", self.root / "child"))
        s.send(b"q")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root) + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_normal_quit_current_uses_relative_nul_and_insert_keeps_text(self):
        (self.root / "child").mkdir()
        (self.root / "child" / "file").touch()
        s = self.start("--editor", "nvim", "--relative", "--null", "--depth", "1", implicit_root=True)
        s.send(b"iq!", "❯ q!")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "❯ type to filter")
        s.send(b"l", ("cwd", self.root / "child"))
        s.send(b"!")
        self.assertEqual(s.finish(), (0, b"child\0"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_quit_from_pinned_root_uses_invocation_not_root(self):
        (self.root / "file").touch()
        s = self.start("--editor", "nvim")
        s.send(b"q")
        self.assertEqual(s.finish(), (0, os.fsencode(Path.cwd().resolve()) + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_no_dir_quit_returns_no_directory(self):
        (self.root / "file").touch()
        s = self.start("--editor", "nvim", "--no-dir")
        s.send(b"!")
        self.assertEqual(s.finish(), (0, b""))

    def test_bookmark_alt_digits_use_visible_filtered_rows_and_clamp(self):
        names = [f"place{i:02}" for i in range(12)]
        for name in names:
            (self.root / name).mkdir()
            (self.root / name / "file").touch()
        config = self.root / "config.toml"
        config.write_text("".join(f"[[bookmarks]]\nname='{n}'\npath='{n}'\n" for n in names))
        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"\x1bb", "Bookmarks")
        s.send(b"\x1b2", ("cwd", self.root / names[1]))
        s.send(b"\x1bb", "Bookmarks")
        s.send(b"place1", "place11")
        s.send(b"\x1b9", ("cwd", self.root / names[11]))
        s.send(b"\x1bb", "Bookmarks")
        s.send(b"\x1b[6~", "place05")
        s.send(b"\x1b[6~\x1b[6~", "place11")
        s.send(b"\x1b0", ("cwd", self.root / names[11]))
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

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
        s.send(b"2", "❯ 2")
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
        s.send(b"\x1bH", "exclusions off")
        s.send(b"\x1be", b"one glob per line")
        before_apply = len(s.ui)
        s.send(b"\r*.tex\x13", "❯")
        self.assertNotIn(b"No matching results", s.ui[before_apply:])
        s.send(b"\x1bH", b"No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_mark_list_unmark_then_confirm_file(self):
        (self.root / "a").touch()
        (self.root / "b").touch()
        s = self.start()
        s.send(b"\t", b"Marks (1)")
        s.send(b"\x1b ", "› ●")
        s.send(b"\t", "› ○")
        s.send(b"\x1b", b"Marks (0)")
        s.send(b"\x1bk", "›   1    a")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "a") + b"\n"))

    def test_filetype_icons_default_cli_and_config_disable(self):
        (self.root / "docs").mkdir()
        (self.root / "main.rs").touch()
        s = self.start("--editor", "basic", "--depth", "1")
        s.wait_ui(" docs/")
        s.wait_ui(" main.rs")
        s.send(b"\t", "●  docs")  # marked rows carry the same icon
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        with tempfile.TemporaryDirectory(prefix="fff-icons-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\nicons=false\n")
            for args in (("--no-icons",), ("--config", str(config))):
                s = self.start("--editor", "basic", "--depth", "1", *args)
                s.wait_ui("▸ docs/")
                s.wait_ui("· main.rs")
                self.assertNotIn("", s.screen.text)
                s.send(b"\x1b[49;5u")  # Ctrl+1: numbering is unaffected
                self.assertEqual(s.finish(), (0, os.fsencode(self.root / "docs") + b"\n"))
                s.close()
            self.session = None

    def test_page_zero_and_higher_digit_clamping(self):
        for i in range(30):
            (self.root / f"file{i:02}").touch()
        s = self.start(size=(16, 160))  # 9 visible result rows in the split layout
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
        with tempfile.TemporaryDirectory(prefix="fff-location-") as other:
            target = Path(other) / "b"
            target.touch()
            s = self.start()
            s.send(b"\t", b"Marks (1)")
            s.send(b"\x1bg", b"Location (Enter navigates")
            s.send(os.fsencode(other) + b"\r", ("cwd", other))
            self.assertIn(other, s.screen.text)
            s.send(b"b", "❯ b")
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
        s.send(b"leaf-only", "❯ leaf-only")
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
                s.send(b"c.txt", "❯ c.txt")
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
        s = self.start(size=(16, 160))
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
        s.send(b"\x1b ", "› ●")
        # Resize to a compact stacked layout, then exercise the same marked row.
        fcntl.ioctl(s.master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 60, 0, 0))
        s.screen = Screen(24, 60)
        os.kill(s.pid, signal.SIGWINCH)
        s.wait_ui("Marks (1)")
        s.send(b"\t", "› ○")
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

    def test_f1_opens_key_guide_and_tab_switches_to_session(self):
        (self.root / "file").touch()
        s = self.start()
        self.assertIn("F1 keys", s.screen.text)
        self.assertNotIn("Alt+#", s.screen.text)
        s.send(b"\x1bOP", "SELECT AND RETURN")  # F1
        self.assertIn("Alt+Space", s.screen.text)
        s.send(b"\t", "CURRENT DIRECTORY")
        s.send(b"\t", "SELECT AND RETURN")
        s.send(b"\x1b[27u", "F1 keys")  # Escape closes
        self.assertNotIn("SELECT AND RETURN", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_status_messages_fade_after_configured_seconds(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-status-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\nmessage_seconds=1\n")
            s = self.start("--no-dir", "--config", str(config))
            s.send(b"\x1bh", "disabled with --no-dir")
            started = time.monotonic()
            s.wait_absent("disabled with --no-dir", timeout=4)
            self.assertGreaterEqual(time.monotonic() - started, 0.8)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_no_dir_lists_only_files_and_disables_navigation(self):
        folder = self.root / "folder"
        folder.mkdir()
        nested = folder / "nested.txt"
        nested.touch()
        top = self.root / "top.txt"
        top.touch()
        s = self.start("--no-dir", "--depth", "2", implicit_root=True)
        s.send(b"", b"nested.txt")
        self.assertNotIn("folder/ ", s.screen.text)
        # Parent, enter, location and bookmarks all stay put with a notice.
        for key in (b"\x1bh", b"\x1bl", b"\x1bg", b"\x1bb"):
            s.send(key, "disabled with --no-dir")
            self.assertTrue(any(" ".join(l.split()).startswith(f"@ {self.root}") for l in s.screen.text.splitlines()))
            self.assertNotIn("Go to directory", s.screen.text)
            self.assertNotIn("Bookmarks ·", s.screen.text)
        # A no-match query: Ctrl+number does not fall back to the directory.
        s.send(b"zzzz", b"No matching results")
        s.send(b"\x1b[49;5u")  # CSI-u: Ctrl+1
        s.send(b"\x7f\x7f\x7f\x7f", b"top.txt")
        s.send(b"\t", b"Marks (1)")
        s.send(b"\r")
        self.assertEqual(
            s.finish(), (0, os.fsencode(top) + b"\n" + os.fsencode(nested) + b"\n")
        )

    def test_live_bookmark_save_edit_icon_cancel_and_delete(self):
        config = self.root / "config.toml"
        config.write_text("# Keep my comment\n[ui]\npalette='nord'\n")
        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"\x1bb", "Bookmarks")
        s.send(b"\x1bn", "Add bookmark")
        s.send(b"\x7f" * len(self.root.name) + b"Notes", "Notes")
        s.send(b"\x1bi", "Bookmark icons")
        s.send(b"home", "Home house")
        s.send(b"\r", "Add bookmark")
        s.send(b"\x13", "Bookmark saved")
        saved = tomllib.loads(config.read_text())["bookmarks"][0]
        self.assertEqual(saved, dict(name="Notes", path=str(self.root), icon="\uf015"))
        self.assertIn("# Keep my comment", config.read_text())
        s.send(b"\x1be", "Edit bookmark")
        s.send(b"\x7f" * 5 + b"Renamed", "Renamed")
        s.send(b"\x1bi", "Bookmark icons")
        s.send(b"automatic", "Automatic")
        s.send(b"\r", "Edit bookmark")
        s.send(b"\x13")
        s.wait_absent("Edit bookmark")
        s.wait_absent("Add bookmark")
        s.wait_ui("Bookmark saved")
        saved = tomllib.loads(config.read_text())["bookmarks"][0]
        self.assertEqual(saved, dict(name="Renamed", path=str(self.root)))
        before = config.read_text()
        s.send(b"\x1be", "Edit bookmark")
        s.send(b"changed")
        s.send(b"\x1b")
        s.wait_absent("Edit bookmark")
        self.assertEqual(config.read_text(), before)
        s.send(b"\x1bd", "Delete bookmark?")
        s.send(b"\x1b")
        s.wait_absent("Delete bookmark?")
        self.assertEqual(config.read_text(), before)
        s.send(b"\x1bd", "Delete bookmark?")
        s.send(b"\r", "Bookmark deleted")
        self.assertEqual(tomllib.loads(config.read_text()).get("bookmarks", []), [])
        self.assertTrue(self.root.is_dir())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_bookmark_save_creates_missing_default_config_and_survives_restart(self):
        config_home = self.root / "settings"
        config = config_home / "fff" / "config.toml"
        environment = {"XDG_CONFIG_HOME": str(config_home)}
        s = self.start(implicit_root=True, environment=environment)
        self.assertFalse(config.exists())
        s.send(b"\x1bb", "Bookmarks")
        s.send(b"\x1bn", "Add bookmark")
        s.send(b"\x13", "Bookmark saved")
        self.assertEqual(tomllib.loads(config.read_text())["bookmarks"][0]["path"], str(self.root))
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        self.session = None
        s = self.start(implicit_root=True, environment=environment)
        s.send(b"\x1bb", "Bookmarks")
        self.assertIn(self.root.name, s.screen.text)
        s.send(b"\r", ("cwd", self.root))
        s.wait_absent("Bookmarks ·")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_live_bookmarks_session_only_and_external_reload(self):
        s = self.start(implicit_root=True)
        s.send(b"\x1bb", "Bookmarks")
        s.send(b"\x1bn", "Add bookmark")
        s.send(b"\x13", "for this session")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        self.session = None
        config = self.root / "config.toml"
        config.write_text("[[bookmarks]]\nname='Before'\npath='.'\n")
        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"\x1bb", "Before")
        s.send(b"\x1be", "Edit bookmark")
        config.write_text("[[bookmarks]]\nname='External'\npath='.'\n")
        s.send(b"\x13", "Alt+r")
        self.assertIn("External", config.read_text())
        s.send(b"\x1b")
        s.wait_absent("Edit bookmark")
        s.send(b"\x1br", "Bookmarks reloaded")
        self.assertIn("External", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is unavailable")
    def test_live_bookmark_nvim_form_and_icon_picker(self):
        s = self.start(implicit_root=True, editor="nvim")
        s.send(b"\x1bb", "Bookmarks")
        s.send(b"\x1bn", "Add bookmark")
        self.assertIn("NORMAL", s.screen.text)
        s.send(b"ccNotes", "Notes")
        s.send(b"\x1bi", "Bookmark icons")
        self.assertIn("NORMAL", s.screen.text)
        s.send(b"ihome", "Home house")
        s.send(b"\x1b", "NORMAL")
        s.send(b"\r", "Add bookmark")
        s.send(b"\x1b[27u:wq\r")
        s.wait_absent("Edit bookmark")
        s.wait_absent("Add bookmark")
        s.wait_ui("Bookmark saved")
        self.assertIn("Notes", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is unavailable")
    def test_bookmark_neovim_writes_apply_only_on_close(self):
        config = self.root / "config.toml"
        config.write_text("[[bookmarks]]\nname='Before'\npath='.'\n")
        s = self.start("--config", str(config), editor="nvim", implicit_root=True)
        s.send(b"\x1bb", "Before")
        s.send(b"\x1be", "Edit bookmark")
        s.send(b"ccDiscarded", "Discarded")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "Edit bookmark")
        s.send(b"\x13", "Edit bookmark")
        self.assertEqual(tomllib.loads(config.read_text())["bookmarks"][0]["name"], "Before")
        s.send(b":q!\r", "Bookmarks ·")
        s.wait_absent("Edit bookmark")
        self.assertEqual(tomllib.loads(config.read_text())["bookmarks"][0]["name"], "Before")
        s.send(b"\x1be", "Edit bookmark")
        s.send(b"ccWritten", "Written")
        s.send(b"\x1b[27u:w\r", "Edit bookmark")
        self.assertEqual(tomllib.loads(config.read_text())["bookmarks"][0]["name"], "Before")
        s.send(b"ccUnsaved", "Unsaved")
        s.send(b"\x1b[27u:q!\r", "Bookmark saved")
        s.wait_absent("Edit bookmark")
        self.assertEqual(tomllib.loads(config.read_text())["bookmarks"][0]["name"], "Written")
        s.send(b"\x1be", "Edit bookmark")
        before = config.read_text()
        s.send(b"dd:wq\r", "Bookmark name must not be empty")
        s.wait_absent("Edit bookmark")
        self.assertEqual(config.read_text(), before)
        # An unchanged, explicitly written existing bookmark is a no-op.
        s.send(b"\x1be", "Edit bookmark")
        s.send(b":wq\r", "Bookmarks ·")
        s.wait_absent("Edit bookmark")
        self.assertEqual(config.read_text(), before)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_configured_bookmarks_filter_missing_target_and_preserve_marks(self):
        marked = self.root / "marked"
        marked.touch()
        with tempfile.TemporaryDirectory(prefix="fff-bookmarks-") as config_dir:
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
        with tempfile.TemporaryDirectory(prefix="fff-palette-") as config_dir:
            config = Path(config_dir) / "config.toml"
            config.write_text("[ui]\npalette='custom'\n[palettes.custom]\nextends='light'\ncurrent_directory='#123456'\n")
            s = self.start("--config", str(config))
            lines = s.screen.text.splitlines()
            self.assertNotIn("CURRENT DIRECTORY", s.screen.text)
            self.assertTrue(" ".join(lines[3].split()).startswith(f"@ {self.root}"), s.screen.text)
            self.assertIn(b"48;2;18;52;86", s.ui)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))
            s.close()
            self.session = None
            s = self.start("--config", str(config), "--palette", "forest")
            self.assertIn(b"48;2;49;94;69", s.ui)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_location_line_and_pinned_roots_replace_the_top_banner(self):
        for name in ("notes", "papers"):
            (self.root / name).mkdir()
            (self.root / name / f"{name}.txt").touch()
        (self.root / "notes" / "topic").mkdir()
        s = Session(self.root / "notes", str(self.root / "papers"), "--editor", "basic", "--depth", "1")
        self.session = s
        lines = s.screen.text.splitlines()
        self.assertIn("Query", lines[0])
        self.assertIn("● complete   depth 1", lines[2])
        self.assertNotIn("hidden", lines[2])
        self.assertTrue(" ".join(lines[3].split()).startswith(f"@ {self.root / 'notes'} "), s.screen.text)
        self.assertTrue(lines[3].rstrip().endswith("pinned  notes  papers"), s.screen.text)
        self.assertIn("Files ·", lines[4])
        s.send(b"topic", "❯ topic")
        s.send(b"\r", ("cwd", self.root / "notes" / "topic"))
        at = next(l for l in s.screen.text.splitlines() if l.lstrip().startswith("@ "))
        self.assertIn(f"@ {self.root / 'notes' / 'topic'} ", " ".join(at.split()))
        self.assertIn("pinned  notes  papers", at)
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
        self.assertIn("old", s.screen.text)  # recent snapshot intentionally stays usable
        s.send(b"\x1b[15~", "● complete")  # F5 forces freshness
        self.assertIn("new", s.screen.text)
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(new) + b"\n"))

    def test_current_directory_refreshes_without_keyboard_input(self):
        (self.root / "old").touch()
        with tempfile.TemporaryDirectory(prefix="fff-freshness-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[cache]\nfresh_seconds=1\nprefetch=false\n")
            s = self.start("--depth", "1", "--config", str(config), implicit_root=True)
            (self.root / "newly-created").touch()
            s.wait_ui("newly-created")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_palette_live_reload_survives_invalid_edits(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-reload-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\npalette='nord'\n")
            s = self.start("--config", str(config))
            config.write_text("[ui]\npalette='catppuccin-mocha'\n")
            s.wait_ui("Palette reloaded")
            self.assertIn(b"48;2;30;30;46", s.ui)
            config.write_text("not valid TOML")
            s.wait_ui("Palette reload failed")
            config.write_text("[ui]\npalette='tokyonight-day'\n")
            s.wait_ui("Palette reloaded")
            self.assertIn(b"48;2;225;226;231", s.ui)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_query_operators_undo_redo_and_exact_stdout(self):
        (self.root / "world.txt").touch()
        s = self.start("--editor", "nvim")
        s.send(b"ihello world", "❯ hello world")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0dw", "❯ world")
        s.send(b"u", "❯ hello world")
        s.send(b"\x12", "❯ world")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "world.txt") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_location_and_bookmark_filter(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-vim-fields-") as folder:
            target = Path(folder) / "notes"
            target.mkdir()
            (target / "note").touch()
            config = Path(folder) / "config.toml"
            config.write_text("[editor]\nbackend='nvim'\n[[bookmarks]]\nname='Notes'\npath='notes'\n")
            s = self.start("--config", str(config), implicit_root=True, editor=None)
            s.send(b"\x1bg", "NORMAL")
            s.send(b"ijunk " + os.fsencode(target), b"junk")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"0dw\r", ("cwd", target))
            s.send(b"\x1bb", "Bookmarks")
            s.send(b"iJunk Notes", "Find: Junk Notes")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"0dw", "Find: Notes")
            s.send(b"\r", ("cwd", target))
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_buffer_closes_keep_query_and_validate_exclusions(self):
        (self.root / "file").touch()
        s = self.start("--editor", "nvim", "--exclude", "build")
        s.send(b"ifile", "file")
        s.send(b"\x1b[27u:q!\r", "Query · NORMAL")
        self.assertNotIn("Using basic editing", s.screen.text)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1be", "Edit exclusions")
        s.send(b"ggdGifile", "file")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "Edit exclusions")
        self.assertIn("Edit exclusions", s.screen.text)
        s.send(b":q!\r", "Query · NORMAL")
        s.wait_absent("Edit exclusions")
        s.send(b"\x1be", "Edit exclusions")
        self.assertIn("build", s.screen.text)
        s.send(b"ggdGifile", "file")
        s.send(b"\x1b[27u:wq\r", "No matching results")
        s.wait_absent("Edit exclusions")
        s.send(b"\x1be", "Edit exclusions")
        s.send(b"ggdGi[", "[")
        s.send(b"\x1b[27u:wq\r", "Exclusion line 1:")
        s.wait_absent("Edit exclusions")
        self.assertIn("No matching results", s.screen.text)
        s.send(b"\x1be", "Edit exclusions")
        self.assertIn("file", s.screen.text)
        s.send(b":q!\r", "No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_multiline_exclusions_and_pending_find(self):
        (self.root / "main.tex").touch()
        (self.root / "build").mkdir()
        s = self.start("--editor", "nvim", "--exclude", "build")
        s.send(b"\x1be", "NORMAL")
        s.send(b"o*.tex", "*.tex")
        s.send(b"\x1b[27u:wq", ":wq")
        lines = s.screen.text.splitlines()
        command_row = next(i for i, line in enumerate(lines) if ":wq" in line)
        for rule in ["build", "*.tex"]:
            self.assertTrue(any(rule in line for line in lines[:command_row]))
        s.send(b"\r", "No matching results")
        # Applying exclusions returns to the main query, initially Normal.
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0f", "PENDING")
        s.send(b"x", "NORMAL")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_ignores_personal_init_files(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-clean-nvim-") as home:
            config = Path(home) / ".config/nvim"
            config.mkdir(parents=True)
            marker = Path(home) / "init-loaded"
            (config / "init.lua").write_text("vim.fn.writefile({'loaded'}, " + repr(str(marker)) + ")")
            s = self.start("--editor", "nvim", "--no-config", environment={"HOME": home})
            self.assertIn("NORMAL", s.screen.text)
            self.assertFalse(marker.exists())
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_list_keys_and_g_passthrough(self):
        for name in ("alpha", "beta", "gamma"):
            (self.root / name).touch()
        s = self.start("--editor", "nvim", "--no-config")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"g", "PENDING")
        s.send(b"g", "NORMAL")
        # G = last, k = Alt+k, gg = first, j = Alt+j; the query is untouched.
        s.send(b"Gkggjj")
        s.send(b"k\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "beta") + b"\n"))
        s.close()
        s = self.session = Session(self.root, "--editor", "nvim", "--no-config")
        s.send(b"igamma", "❯ gamma")
        s.send(b"\x1b[27u", "NORMAL")
        # Unbound continuations reach Neovim intact: gU, ge.
        s.send(b"0gUiw", "❯ GAMMA")
        s.send(b"u", "❯ gamma")
        s.send(b"$ge", "NORMAL")  # plain e would stay on the last letter
        s.send(b"ix", "❯ xgamma")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"dl", "❯ gamma")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "gamma") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_h_and_l_navigate_like_alt(self):
        directory = self.root / "folder"
        directory.mkdir()
        (directory / "file").touch()
        s = self.start("--editor", "nvim", "--no-config", "--depth", "1", implicit_root=True)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"l", ("cwd", directory))
        # Normal-mode navigation stays Normal, so h needs no Escape first.
        self.assertIn("Query · NORMAL", s.screen.text)
        s.send(b"h", ("cwd", self.root))
        self.assertIn("Query · NORMAL", s.screen.text)
        # i returns to Insert; typing filters as usual.
        s.send(b"ifol", "❯ fol")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_keys_in_marks_bookmarks_and_config(self):
        for name in ("alpha", "beta", "gamma"):
            (self.root / name).touch()
        with tempfile.TemporaryDirectory(prefix="fff-normal-keys-") as folder:
            for name in ("one", "two", "three"):
                (Path(folder) / name).mkdir()
            config = Path(folder) / "config.toml"
            config.write_text(
                "[keys.normal]\nmark='none'\nfirst='<space>t'\nlast=['G','<space>b']\ndown='none'\n"
                + "".join(f"[[bookmarks]]\nname='{n}'\npath='{n}'\n" for n in ("one", "two", "three"))
            )
            s = self.start("--editor", "nvim", "--config", str(config), implicit_root=True)
            s.send(b"\t\t\t", b"Marks (3)")
            s.send(b"\x1b ", "Marks (3)")
            # Marks list: last, then first via the remapped sequence; Tab unmarks.
            s.send(b"G\t", b"Marks (2)")
            s.send(b" t\t", b"Marks (1)")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"\x1bb", "Bookmarks")
            # j is unbound here, so it reaches Neovim and leaves "one" highlighted.
            s.send(b" b t")
            s.send(b"j\r", ("cwd", Path(folder) / "one"))
            s.send(b"\x17")
            self.assertEqual(s.finish(), (0, os.fsencode(self.root / "beta") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_keys_in_palette_chooser(self):
        (self.root / "file").touch()
        s = self.start("--editor", "nvim", "--no-config")
        s.send(b"\x1bp", "Palettes · Enter applies · Esc reverts · NORMAL")
        # G previews and applies the last palette; gg then j select the second.
        mark = len(s.ui)
        s.send(b"G")
        s.send(b"\r", "Palette tomorrow-night applied")
        self.assertIn(b"48;2;29;31;33", s.ui[mark:])
        s.send(b"\x1bp", "Palettes ·")
        s.send(b"ggj\r", "Palette light applied")
        # A g not followed by a binding still edits the filter inside Neovim.
        s.send(b"\x1bp", "Palettes ·")
        s.send(b"inord", "Find: nord")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0gUiw", "Find: NORD")
        s.send(b"\r", "Palette nord applied")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_navigation_preserves_insert_and_normal_modes(self):
        directory = self.root / "folder"
        directory.mkdir()
        (directory / "file").touch()
        s = self.start("--editor", "nvim", "--depth", "1", implicit_root=True)
        s.send(b"ifol", "❯ fol")
        s.send(b"\x1bl", ("cwd", directory))
        self.assertIn("Query · INSERT", s.screen.text)
        s.send(b"file", "❯ file")
        s.send(b"\x1bh", ("cwd", self.root))
        self.assertIn("Query · INSERT", s.screen.text)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "❯")
        s.send(b"l", ("cwd", directory))
        self.assertIn("Query · NORMAL", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_space_marks_in_normal_and_types_in_insert(self):
        for name in ("a", "b"):
            (self.root / name).touch()
        s = self.start("--editor", "nvim")
        s.send(b" ", "Marks (1)")
        s.send(b"i ", "No matching results")
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "❯")
        s.send(b"\x1b ", "Marks (1)")
        s.send(b" ", "Marks (0)")
        s.send(b"\x1b ", "Marks (0)")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_exclusions_native_zz_and_ctrl_w_keep_neovim_behavior(self):
        (self.root / "main.tex").touch()
        (self.root / "build").mkdir()
        s = self.start("--editor", "nvim", "--exclude", "build")
        s.send(b"\x1be", "Exclusions")
        s.send(b"0iZZ", "ZZbuild")
        self.assertIn("Exclusions", s.screen.text)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0xxo*.tex", "*.tex")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x17", "PENDING")
        self.assertIn("Exclusions", s.screen.text)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"Z", "PENDING")
        self.assertIn("Exclusions", s.screen.text)
        s.send(b"Z", "No matching results")
        self.assertNotIn("Exclusions ·", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_configured_apply_sequence_does_not_intercept_neovim(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-action-keys-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[keys.normal]\nmark='x'\nquit='none'\napply='qq'\n")
            s = self.start("--editor", "nvim", "--config", str(config))
            s.send(b"x", "Marks (1)")
            s.send(b"\x1be", "Edit exclusions")
            s.send(b"inot-present", "not-present")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"qq", "Edit exclusions")
            self.assertIn("Edit exclusions", s.screen.text)
            s.send(b":wq\r")
            s.wait_absent("Edit exclusions")
            s.send(b"\x17")
            self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    def test_palette_chooser_starts_on_the_canonical_palette_for_an_alias(self):
        (self.root / "file").touch()
        s = self.start("--editor", "basic", "--palette", "gruvbox")
        s.send(b"\x1bp", "Palettes ·")
        s.send(b"\r", "Palette gruvbox-dark applied")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_basic_exclusions_use_explicit_save_and_discard(self):
        (self.root / "file").touch()
        s = self.start("--editor", "basic")
        s.send(b"\x1be", "Exclusions")
        s.send(b"ZZ", "ZZ")
        self.assertIn("Edit exclusions", s.screen.text)
        s.send(b"\x1b[27u", "Query")
        s.wait_absent("Edit exclusions")
        s.send(b"\x1be", "Exclusions")
        s.send(b"file\x13", "No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_normal_only_and_empty_selection(self):
        (self.root / "file").write_text("original")
        s = self.start(editor="nvim", implicit_root=True)
        for key in [b"v", b"p", b"s"]:
            s.send(key, "No files selected")
            self.assertEqual((self.root / "file").read_text(), "original")
        s.send(b"ivpxrens", "❯ vpxrens")
        self.assertEqual(sorted(p.name for p in self.root.iterdir()), ["file"])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_copy_move_and_symlink_clear_all_selected(self):
        for key in ["p", "v", "s"]:
            prefix = "file-" + key + "-"
            names = [prefix + "a", prefix + "b"]
            target = self.root / ("target-" + key)
            target.mkdir()
            for name in names:
                (self.root / name).write_text("payload")
            s = self.start(editor="nvim", implicit_root=True)
            s.send(b"i" + prefix.encode(), "❯ " + prefix)
            s.send(b"\x1b[27u\t\t", "Marks (2)")
            s.send(b"\x1b[27ui" + target.name.encode(), "❯ " + target.name)
            s.send(b"\x1b[27u\r", ("cwd", target))
            s.send(key.encode(), "Operation complete")
            for name in names:
                self.assertEqual((target / name).read_text(), "payload")
                self.assertEqual((self.root / name).exists(), key != "v")
                self.assertEqual((target / name).is_symlink(), key == "s")
            self.assertIn("Marks (0)", s.screen.text)
            s.send(key.encode(), "No files selected")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))
            s.close()
            self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_failed_file_operations_preserve_all_selected(self):
        source = self.root / "source"
        source.write_text("keep")
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"\t", "Marks (1)")
        for key in [b"v", b"p", b"s"]:
            # The destination is the source's current directory: collision.
            s.send(key, "Destination already exists")
            self.assertIn("Marks (1)", s.screen.text)
            self.assertEqual(source.read_text(), "keep")
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(source) + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_new_file_directory_and_discard(self):
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"n", "Create a file")
        s.send(b"f", "New file · INSERT")
        s.send(b"discarded", "discarded")
        s.send(b"\x1b[27u:q!\r", "Query · NORMAL")
        self.assertFalse((self.root / "discarded").exists())
        for choice, name in [(b"f", "new-file"), (b"d", "new-folder")]:
            s.send(b"n", "Create a file")
            s.send(choice, "New file · INSERT" if choice == b"f" else "New directory · INSERT")
            s.send(name.encode(), name)
            s.send(b"\x1b[27u:wq\r", "Operation complete")
            s.wait_absent("New file" if choice == b"f" else "New directory")
            self.assertTrue((self.root / name).exists())
            self.assertEqual((self.root / name).is_dir(), choice == b"d")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_rename_buffer_validation_and_mark_updates(self):
        (self.root / "alpha").write_text("a")
        (self.root / "beta").write_text("b")
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"\t", "Marks (1)")
        s.send(b"r", "Rename files")
        self.assertIn("alpha", s.screen.text)
        self.assertIn("beta", s.screen.text)
        s.send(b"ggdd:wq\r", "Rename row count changed")
        self.assertTrue((self.root / "alpha").exists())
        self.assertTrue((self.root / "beta").exists())
        s.send(b"r", "Rename files")
        s.send(b"gg0wCrenamed", "renamed")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "Rename files")
        s.send(b":wq\r", "Operation complete")
        self.assertFalse((self.root / "alpha").exists())
        self.assertEqual((self.root / "renamed").read_text(), "a")
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "renamed") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_delete_current_or_selected_and_cancel(self):
        (self.root / "alpha").write_text("aaa")
        (self.root / "beta").write_bytes(b"b" * 20000)
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"\t", "Marks (1)")  # alpha marked, beta highlighted
        s.send(b"x", "Delete current (c) or selected (s)")
        s.send(b"c", "permanently deleted")
        self.assertIn("beta", s.screen.text)
        self.assertIn("19.5 KiB", s.screen.text)
        s.send(b"\x1b[27u", "Query · NORMAL")
        self.assertTrue((self.root / "beta").exists())
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"xs", "permanently deleted")
        self.assertIn("alpha", s.screen.text)
        s.send(b"\r", "Operation complete")
        self.assertFalse((self.root / "alpha").exists())
        self.assertTrue((self.root / "beta").exists())
        self.assertIn("Marks (0)", s.screen.text)
        s.send(b"x", "permanently deleted")
        s.send(b"\x1b[27u", "Query · NORMAL")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_deleting_unselected_current_file_keeps_other_selected_files(self):
        selected = self.root / "alpha"
        current = self.root / "beta"
        selected.write_text("selected")
        current.write_text("current")
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"\t", "Marks (1)")
        s.send(b"xc", "permanently deleted")
        s.send(b"\r", "Operation complete")
        self.assertFalse(current.exists())
        self.assertTrue(selected.exists())
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(selected) + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_extract_empty_and_explicit_destination(self):
        import zipfile
        archive = self.root / "input.zip"
        with zipfile.ZipFile(archive, "w") as z:
            z.writestr("unpacked", "payload")
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"e", "empty extracts beside the archive")
        s.send(b":wq\r", "Operation complete")
        self.assertEqual((self.root / "unpacked").read_text(), "payload")
        # Filter ensures extraction continues to target the archive alone.
        s.send(b"iinput.zip", "❯ input.zip")
        s.send(b"\x1b[27ue", "Extract archive")
        s.send(b"ioutput", "output")
        s.send(b"\x1b[27u:wq\r", "Operation complete")
        s.wait_absent("Extract archive")
        self.assertEqual((self.root / "output/unpacked").read_text(), "payload")
        self.assertTrue(archive.exists())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_extract_rejects_non_archives_before_opening_a_buffer(self):
        import zipfile
        directory = self.root / "folder"
        directory.mkdir()
        plain = self.root / "not-an-archive.zip"
        plain.write_text("plain text")
        corrupt = self.root / "corrupt.zip"
        with zipfile.ZipFile(corrupt, "w", compression=zipfile.ZIP_STORED) as z:
            z.writestr("data", b"PAYLOAD")
        raw = bytearray(corrupt.read_bytes())
        raw[raw.index(b"PAYLOAD")] ^= 1
        corrupt.write_bytes(raw)
        s = self.start(editor="nvim", implicit_root=True)
        for name in [directory.name, plain.name, corrupt.name]:
            s.send(b"i" + name.encode(), "❯ " + name)
            s.send(b"\x1b[27ue", "Cannot extract this file:")
            self.assertNotIn("Extract archive", s.screen.text)
            self.assertIn("Query · NORMAL", s.screen.text)
            s.send(b"\x1b[27u", "❯")
        s.send(b"imissing", "No matching results")
        s.send(b"\x1b[27ue", "Select an extractable archive first")
        self.assertNotIn("Extract archive", s.screen.text)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()),
                         ["corrupt.zip", "folder", "not-an-archive.zip"])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_single_line_naming_and_extraction_keys(self):
        import zipfile
        archive = self.root / "input.zip"
        with zipfile.ZipFile(archive, "w") as z:
            z.writestr("data", "payload")
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"nf", "New file · INSERT")
        s.send(b"name\r", "name")  # Insert Enter must not add a row.
        s.send(b"\x1b[27u\r", "New file · NORMAL")
        s.send(b"O", "New file · INSERT")
        s.send(b"\x1b[27uo", "New file · INSERT")
        s.send(b"\x1b[27u:wq\r", "Operation complete")
        s.wait_absent("New file")
        self.assertTrue((self.root / "name").is_file())
        s.send(b"iinput.zip", "❯ input.zip")
        s.send(b"\x1b[27ue", "Extract archive · NORMAL")
        s.send(b"\r", "Extract archive · NORMAL")
        s.send(b"o", "Extract archive · INSERT")
        s.send(b"output\r", "output")
        s.send(b"\x1b[27uO", "Extract archive · INSERT")
        s.send(b"\x1b[27u:wq\r", "Operation complete")
        s.wait_absent("Extract archive")
        self.assertEqual((self.root / "output/data").read_text(), "payload")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_query_o_and_O_enter_insert_without_a_newline(self):
        (self.root / "file").touch()
        s = self.start(editor="nvim", implicit_root=True)
        s.send(b"o", "Query · INSERT")
        s.send(b"file", "❯ file")
        s.send(b"\x1b[27uO", "Query · INSERT")
        s.send(b"\x1b[27u\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    def test_missing_nvim_falls_back_to_basic_input(self):
        (self.root / "file").touch()
        s = self.start("--editor", "nvim", "--no-config", environment={"PATH": ""})
        self.assertIn("Using basic editing", s.screen.text)
        s.send(b"file", "❯ file")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_default_editor_is_neovim(self):
        (self.root / "world.txt").touch()
        s = self.start(editor=None)
        self.assertIn("NORMAL", s.screen.text)
        s.send(b"ihello world", "❯ hello world")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0dw", "❯ world")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "world.txt") + b"\n"))

    def test_default_editor_without_nvim_falls_back_quietly(self):
        (self.root / "file").touch()
        s = self.start("--no-config", environment={"PATH": ""}, editor=None)
        self.assertNotIn("Using basic editing", s.screen.text)
        self.assertNotIn("INSERT", s.screen.text)
        s.send(b"file", "❯ file")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    def test_remapped_numbered_choice(self):
        (self.root / "file").touch()
        s = self.start("--map-key", "ctrl+n=ctrl+1")
        s.send(b"\x0e")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    def test_palette_chooser_previews_reverts_applies_and_reloads(self):
        (self.root / "file").touch()
        s = self.start("--editor", "basic")
        s.send(b"\x1bp", "Palettes · Enter applies")
        mark = len(s.ui)
        s.send(b"nord", "Find: nord")
        s.wait_bytes(b"48;2;46;52;64", after=mark)  # delayed nord preview
        mark = len(s.ui)
        s.send(b"\x1b[27u")  # Escape reverts to midnight
        s.wait_absent("Palettes ·")
        self.assertIn(b"48;2;15;23;36", s.ui[mark:])
        s.send(b"\x1bpforest", "Find: forest")
        s.send(b"\r", "Palette forest applied for this session")
        mark = len(s.ui)
        s.send(b"\x1b[112;4u", "Palette midnight reloaded")  # CSI-u Alt+Shift+p
        self.assertIn(b"48;2;15;23;36", s.ui[mark:])
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    def test_palette_session_choice_survives_watcher_until_config_reload(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-palette-keys-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\npalette='nord'\n")
            s = self.start("--editor", "basic", "--config", str(config))
            s.send(b"\x1bpmocha\r", "Palette catppuccin-mocha applied")
            mark = len(s.ui)
            config.write_text("[ui]\npalette='tokyonight-day'\n")
            time.sleep(1.2)  # two watcher polls
            s.send(b"x", "❯ x")
            self.assertNotIn(b"48;2;225;226;231", s.ui[mark:])
            # Only changed cells are redrawn; the query panel uses mocha's text color.
            self.assertIn(b"38;2;205;214;244", s.ui[mark:])
            s.send(b"\x1bP", "Palette tokyonight-day reloaded")  # legacy Alt+P
            self.assertIn(b"48;2;225;226;231", s.ui[mark:])
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_failed_palette_reload_in_chooser_drops_the_preview(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-palette-fail-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\npalette='nord'\n")
            s = self.start("--editor", "basic", "--config", str(config))
            s.send(b"\x1bp", "Palettes · Enter applies")
            mark = len(s.ui)
            s.send(b"light", "Find: light")
            s.wait_bytes(b"48;2;255;255;255", after=mark)
            config.write_text("[ui\n")
            mark = len(s.ui)
            s.send(b"\x1b[112;4u", "Palette reload failed")  # CSI-u Alt+Shift+p
            self.assertNotIn("Palettes ·", s.screen.text)
            self.assertIn(b"48;2;46;52;64", s.ui[mark:])  # nord again, not light
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_palette_reload_honors_cli_pin_and_remapped_keys(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-palette-pin-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\npalette='nord'\n")
            s = self.start(
                "--editor", "basic", "--config", str(config), "--palette", "forest",
                "--map-key", "ctrl+t=alt+p", "--map-key", "ctrl+r=alt+P",
            )
            s.send(b"\x14", "Palettes · Enter applies")
            s.send(b"light\r", "Palette light applied")
            s.send(b"\x12", "Palette forest reloaded")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_palette_filter_escape_returns_to_normal_before_reverting(self):
        (self.root / "file").touch()
        s = self.start("--editor", "nvim")
        s.send(b"\x1bp", "Palettes · Enter applies · Esc reverts · NORMAL")
        s.send(b"ijunk nord", "Find: junk nord")
        s.send(b"\x1b[27u", "NORMAL")
        self.assertIn("Palettes ·", s.screen.text)
        mark = len(s.ui)
        s.send(b"0dw", "Find: nord")
        s.wait_bytes(b"48;2;46;52;64", after=mark)
        s.send(b"\x1b[27u")
        s.wait_absent("Palettes ·")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))


if __name__ == "__main__":
    unittest.main()
