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
import shutil
from terminal_screen import Screen

BINARY = str(Path(__file__).resolve().parents[1] / "target/debug/picker")


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
        s.send(b"\r*.tex\x17", "❯")
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
        with tempfile.TemporaryDirectory(prefix="picker-icons-") as folder:
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
        with tempfile.TemporaryDirectory(prefix="picker-location-") as other:
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
        with tempfile.TemporaryDirectory(prefix="picker-status-") as folder:
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
        self.assertIn("Filter", lines[0])
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
        self.assertIn("new", s.screen.text)
        s.send(b"\x1b[15~", "● complete")  # F5
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(new) + b"\n"))

    def test_palette_live_reload_survives_invalid_edits(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="picker-reload-") as folder:
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
        s.send(b"hello world", "❯ hello world")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0dw", "❯ world")
        s.send(b"u", "❯ hello world")
        s.send(b"\x12", "❯ world")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "world.txt") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_location_and_bookmark_filter(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="picker-vim-fields-") as folder:
            target = Path(folder) / "notes"
            target.mkdir()
            (target / "note").touch()
            config = Path(folder) / "config.toml"
            config.write_text("[editor]\nbackend='nvim'\n[[bookmarks]]\nname='Notes'\npath='notes'\n")
            s = self.start("--config", str(config), implicit_root=True, editor=None)
            s.send(b"\x1bg", "INSERT")
            s.send(b"junk " + os.fsencode(target), b"junk")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"0dw\r", ("cwd", target))
            s.send(b"\x1bb", "Bookmarks")
            s.send(b"Junk Notes", "Find: Junk Notes")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"0dw", "Find: Notes")
            s.send(b"\r", ("cwd", target))
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_multiline_exclusions_and_pending_find(self):
        (self.root / "main.tex").touch()
        (self.root / "build").mkdir()
        s = self.start("--editor", "nvim", "--exclude", "build")
        s.send(b"\x1be", "INSERT")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"o*.tex", "*.tex")
        s.send(b"\x17", "No matching results")
        # First Escape enters Normal; another clears the main query.
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0f", "PENDING")
        s.send(b"x", "NORMAL")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_ignores_personal_init_files(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="picker-clean-nvim-") as home:
            config = Path(home) / ".config/nvim"
            config.mkdir(parents=True)
            marker = Path(home) / "init-loaded"
            (config / "init.lua").write_text("vim.fn.writefile({'loaded'}, " + repr(str(marker)) + ")")
            s = self.start("--editor", "nvim", "--no-config", environment={"HOME": home})
            self.assertIn("INSERT", s.screen.text)
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
        s.send(b"gamma", "❯ gamma")
        s.send(b"\x1b[27u", "NORMAL")
        # Unbound continuations reach Neovim intact: gU, ge.
        s.send(b"0gUiw", "❯ GAMMA")
        s.send(b"u", "❯ gamma")
        s.send(b"$ge", "NORMAL")  # plain e would stay on the last letter
        s.send(b"ix", "❯ xgamma")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"x", "❯ gamma")
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
        # Entering a location starts in Normal mode, so h needs no Escape first.
        self.assertIn("Filter · NORMAL", s.screen.text)
        s.send(b"h", ("cwd", self.root))
        self.assertIn("Filter · NORMAL", s.screen.text)
        # i returns to Insert; typing filters as usual.
        s.send(b"ifol", "❯ fol")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_keys_in_marks_bookmarks_and_config(self):
        for name in ("alpha", "beta", "gamma"):
            (self.root / name).touch()
        with tempfile.TemporaryDirectory(prefix="picker-normal-keys-") as folder:
            for name in ("one", "two", "three"):
                (Path(folder) / name).mkdir()
            config = Path(folder) / "config.toml"
            config.write_text(
                "[keys.normal]\nfirst='<space>t'\nlast=['G','<space>b']\ndown='none'\n"
                + "".join(f"[[bookmarks]]\nname='{n}'\npath='{n}'\n" for n in ("one", "two", "three"))
            )
            s = self.start("--editor", "nvim", "--config", str(config), implicit_root=True)
            s.send(b"\t\t\t", b"Marks (3)")
            s.send(b"\x1b ", "Marks (3)")
            # Marks list: last, then first via the remapped sequence; Tab unmarks.
            s.send(b"G\t", b"Marks (2)")
            s.send(b" t\t", b"Marks (1)")
            s.send(b"\x1b[27u", "INSERT")
            s.send(b"\x1bb", "Bookmarks")
            s.send(b"\x1b[27u", "NORMAL")
            # j is unbound here, so it reaches Neovim and leaves "one" highlighted.
            s.send(b" b t")
            s.send(b"j\r", ("cwd", Path(folder) / "one"))
            s.send(b"\x17")
            self.assertEqual(s.finish(), (0, os.fsencode(self.root / "beta") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_keys_in_palette_chooser(self):
        (self.root / "file").touch()
        s = self.start("--editor", "nvim", "--no-config")
        s.send(b"\x1bp", "Palettes · Enter applies · Esc reverts · INSERT")
        s.send(b"\x1b[27u", "NORMAL")
        # G previews and applies the last palette; gg then j select the second.
        mark = len(s.ui)
        s.send(b"G")
        s.send(b"\r", "Palette solarized-light applied")
        self.assertIn(b"48;2;253;246;227", s.ui[mark:])
        s.send(b"\x1bp", "Palettes ·")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"ggj\r", "Palette light applied")
        # A g not followed by a binding still edits the filter inside Neovim.
        s.send(b"\x1bp", "Palettes ·")
        s.send(b"nord", "Find: nord")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0gUiw", "Find: NORD")
        s.send(b"\r", "Palette nord applied")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

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
        self.assertIn("INSERT", s.screen.text)
        s.send(b"hello world", "❯ hello world")
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
        self.assertIn(b"48;2;46;52;64", s.ui[mark:])  # live nord preview
        mark = len(s.ui)
        s.send(b"\x1b[27u", "❯")  # Escape reverts to midnight
        self.assertNotIn("Palettes ·", s.screen.text)
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
        with tempfile.TemporaryDirectory(prefix="picker-palette-keys-") as folder:
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
        with tempfile.TemporaryDirectory(prefix="picker-palette-fail-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\npalette='nord'\n")
            s = self.start("--editor", "basic", "--config", str(config))
            s.send(b"\x1bp", "Palettes · Enter applies")
            s.send(b"light", "Find: light")
            config.write_text("[ui\n")
            mark = len(s.ui)
            s.send(b"\x1b[112;4u", "Palette reload failed")  # CSI-u Alt+Shift+p
            self.assertNotIn("Palettes ·", s.screen.text)
            self.assertIn(b"48;2;46;52;64", s.ui[mark:])  # nord again, not light
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_palette_reload_honors_cli_pin_and_remapped_keys(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="picker-palette-pin-") as folder:
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
        s.send(b"\x1bp", "Palettes · Enter applies · Esc reverts · INSERT")
        s.send(b"junk nord", "Find: junk nord")
        s.send(b"\x1b[27u", "NORMAL")
        self.assertIn("Palettes ·", s.screen.text)
        mark = len(s.ui)
        s.send(b"0dw", "Find: nord")
        self.assertIn(b"48;2;46;52;64", s.ui[mark:])
        s.send(b"\x1b[27u", "❯")
        self.assertNotIn("Palettes ·", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))


if __name__ == "__main__":
    unittest.main()
