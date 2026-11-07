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
import sys
import termios
import tempfile
import time
import unittest
import tomllib
import shutil
from terminal_screen import Screen

# Canonical, as the paths fff prints are, and short as Linux's /tmp, so that
# paths fit on the tests' screens: macOS's own temporary directory is long
# and below the /var symlink.
tempfile.tempdir = os.path.realpath("/tmp" if sys.platform == "darwin" else tempfile.gettempdir())

# Shift+Space in the kitty keyboard protocol: marks, also while typing.
SHIFT_SPACE = b"\x1b[32;2u"
BINARY = str(Path(__file__).resolve().parents[1] / "target/debug/fff")


class Session:
    # Every text field is an embedded Neovim (fff requires it). Tests pin
    # fff pick (the picker mode) unless they pass output_paths=False to exercise the default directory-changing mode.
    # With stdin (bytes), fff runs without a command and reads them from a
    # pipe, the terminal still its controlling one.
    def __init__(self, root, *args, size=(30, 160), implicit_root=False, environment=None, output_paths=True, ready=True, cwd=None, hangup=True, typing=False, stdin=None):
        if stdin is not None:
            implicit_root, output_paths = True, False
            listed, feed = os.pipe()
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
            if stdin is not None:
                os.close(feed)
                os.dup2(listed, 0)
                os.close(listed)
            else:
                os.dup2(slave, 0)
            os.dup2(writer, 1)
            os.dup2(slave, 2)
            os.close(slave)
            os.close(writer)
            if implicit_root:
                os.chdir(cwd or root)
            if not hangup:
                # As under nohup: closing the terminal sends nothing fatal.
                signal.signal(signal.SIGHUP, signal.SIG_IGN)
            os.environ["TERM"] = "xterm-256color"
            os.environ["COLORTERM"] = "truecolor"
            os.environ.pop("NO_COLOR", None)
            # Remembered locations live beside the test directory, never in
            # the tester's own state.
            os.environ["XDG_STATE_HOME"] = str(root) + "-state"
            # So does the trash.
            os.environ["XDG_DATA_HOME"] = str(root) + "-data"
            # And the contents query's extracted texts.
            os.environ["XDG_CACHE_HOME"] = str(root) + "-cache"
            # Nor is the tester's own configuration read.
            os.environ["XDG_CONFIG_HOME"] = str(root) + "-config"
            # A stray l on a file must not open the tester's editor in a new
            # terminal window; tests that open files name their own programs.
            os.environ["EDITOR"] = "fff-test-no-editor"
            if environment: os.environ.update(environment)
            if "--config" not in args and "--no-config" not in args and not environment:
                args=(*args,"--no-config")
            # A command (fff seek, fff explorer open) comes before the root;
            # the picker is fff pick, the only command that prints paths.
            command = [args[0]] if args and args[0] in ("seek", "explorer", "pick", "window") else []
            if command == ["explorer"]: command = list(args[:2])
            args = args[len(command):]
            if output_paths and not command:
                command = ["pick"]
            os.execv(BINARY, [BINARY, *command, *([] if implicit_root else [str(root)]), *args])
        os.close(slave)
        os.close(writer)
        if stdin is not None:
            os.close(listed)
            os.write(feed, stdin)
            os.close(feed)
        self.ui = b""
        self.screen = Screen(*size)
        self.status = None
        if not ready:
            return
        self.wait_ui("✓ complete" if size[0] >= 12 and size[1] >= 36 else " complete")
        # fff pick starts typing the query; most tests browse in Normal mode.
        picking = "pick" in args[:1] or (output_paths and not args[:1] in (("seek",), ("explorer",)))
        if picking and not typing and size[0] >= 12 and size[1] >= 36:
            self.wait_ui("INSERT")
            self.send(b"\x1b[27u", "NORMAL")

    def wait_ui(self, token, after=None, timeout=5):
        deadline = time.monotonic() + timeout
        location = token[1] if isinstance(token, tuple) else None
        token = token.decode("utf-8") if isinstance(token, bytes) else token
        def matches():
            if location is not None:
                # The integrated "@" line sits between the filter and results.
                at = [l for l in self.screen.text.splitlines() if l.lstrip().startswith("@ ")]
                return any(str(location) in l for l in at) and "✓ complete" in self.screen.text
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
                self.take(chunk)
                if matches() and (after is None or self.screen.frames > after):
                    return
        raise AssertionError(f"UI did not show {token!r}:\n{self.screen.text}")

    def wait_absent(self, token, timeout=5):
        deadline = time.monotonic() + timeout
        while token in self.screen.text and time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.05)[0]:
                chunk = os.read(self.master, 65536)
                self.take(chunk)
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
                self.take(chunk)
        if not committed():
            raise AssertionError(f"UI did not emit {token!r} in a completed frame")

    def wait_raw(self, token, timeout=5):
        # Output outside fff's frames (e.g. after it hands over the terminal).
        deadline = time.monotonic() + timeout
        while token not in self.ui and time.monotonic() < deadline:
            if select.select([self.master], [], [], 0.05)[0]:
                chunk = os.read(self.master, 65536)
                self.take(chunk)
        if token not in self.ui:
            raise AssertionError(f"terminal did not show {token!r}")

    def kill(self):
        """SIGKILL, reading the terminal until fff has ended: on macOS a
        session leader cannot finish exiting while a child (Neovim) holds the
        terminal and its output is left unread."""
        os.kill(self.pid, signal.SIGKILL)
        while not os.waitpid(self.pid, os.WNOHANG)[0]:
            self.drain(0.02)
        self.status = -signal.SIGKILL

    def drain(self, seconds):
        """Reads what fff draws for `seconds`: a frame left unread blocks it
        (macOS's terminals buffer much less than Linux's)."""
        if select.select([self.master], [], [], seconds)[0]:
            try:
                self.take(os.read(self.master, 65536))
            except OSError as e:
                # Linux: the terminal's other end is closed once fff has ended.
                if e.errno != errno.EIO:
                    raise
                time.sleep(seconds)

    def take(self, chunk):
        self.ui += chunk
        self.screen.feed(chunk)
        # fff asks what the terminal supports, ending with a device attributes
        # request: answer as one with the kitty keyboard protocol and
        # synchronized output, which the frame tracking relies on.
        if b"\x1b[c" in chunk:
            os.write(self.master, b"\x1b[?0u\x1b[?2026;2$y\x1b[?62;22c")

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
                    self.take(chunk)
                except OSError:
                    pass
        if self.status is None:
            raise AssertionError("fff did not exit")
        output = b""
        while chunk := os.read(self.output, 65536):
            output += chunk
        return self.status, output

    def close(self):
        # The terminal is closed before waiting: on macOS a process exiting
        # with output the terminal has not read waits for it, killed or not.
        os.close(self.master)
        if self.status is None:
            try:
                os.kill(self.pid, signal.SIGKILL)
                os.waitpid(self.pid, 0)
            except ProcessLookupError:
                pass
        os.close(self.output)


class TerminalTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(prefix="fff-pty-")
        self.root = Path(self.tmp.name)
        self.session = None

    def tearDown(self):
        if self.session:
            self.session.close()
        shutil.rmtree(self.tmp.name + "-state", ignore_errors=True)
        shutil.rmtree(self.tmp.name + "-data", ignore_errors=True)
        shutil.rmtree(self.tmp.name + "-cache", ignore_errors=True)
        self.tmp.cleanup()

    def start(self, *args, **kwargs):
        self.session = Session(self.root, *args, **kwargs)
        return self.session

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_normal_quit_prints_nothing_and_ignores_marks(self):
        (self.root / "child").mkdir()
        (self.root / "child" / "file").touch()
        # fff pick prints only what was chosen; fff roam stays where it started.
        for output_paths in (True, False):
            s = self.start("--depth", "1", implicit_root=True, output_paths=output_paths)
            s.send(SHIFT_SPACE, "Marks (1)")
            s.send(b"l", ("cwd", self.root / "child"))
            s.send(b"q")
            self.assertEqual(s.finish(), (0, b""))
            self.session.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_folder_sizes_are_kept_for_the_next_session(self):
        (self.root / "big" / "in").mkdir(parents=True)
        (self.root / "big" / "in" / "file").write_bytes(b"\0" * 3 * 1024 * 1024)
        kept = Path(str(self.root) + "-cache") / "fff" / "sizes"
        for _ in range(2):
            s = self.start("--depth", "1", implicit_root=True)
            s.send(b"", "3.0M")
            s.send(b"q")
            self.assertEqual(s.finish(), (0, b""))
            self.session.close()
            self.assertTrue(kept.is_file())
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_normal_quit_current_uses_relative_nul_and_insert_keeps_text(self):
        (self.root / "child").mkdir()
        (self.root / "child" / "file").touch()
        s = self.start("--relative", "--null", "--depth", "1", implicit_root=True)
        s.send(b"iq!", "❯ q!")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "❯ type to filter")
        s.send(b"l", ("cwd", self.root / "child"))
        s.send(b"!")
        self.assertEqual(s.finish(), (0, b"child\0"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_directories_hide_and_show_without_stopping_navigation(self):
        (self.root / "dir").mkdir()
        (self.root / "dir" / "deep.txt").touch()
        (self.root / "top.txt").touch()
        s = self.start("--depth", "2", implicit_root=True, output_paths=False)
        s.wait_ui("Files · 3")
        s.send(b"d", "files only")
        s.wait_ui("Files · 2")
        self.assertIn("dir/deep.txt", s.screen.text)
        self.assertNotIn("dir/ ", s.screen.text)
        # Navigation still works, and the toggle stays on in the parent.
        s.send(b"\x1bh", ("cwd", self.root.parent))
        self.assertIn("files only", s.screen.text)
        # Alt+d shows them again, also while typing.
        s.send(b"i", "INSERT")
        s.send(b"\x1bd")
        s.wait_absent("files only")
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_files_hide_and_show_except_in_seek(self):
        (self.root / "dir").mkdir()
        (self.root / "dir" / "deep.txt").touch()
        (self.root / "top.txt").touch()
        s = self.start("--depth", "2", implicit_root=True, output_paths=False)
        s.wait_ui("Files · 3")
        s.send(b"D", "folders only")
        s.wait_ui("Files · 1")
        self.assertNotIn("top.txt", s.screen.text)
        # d swaps to files only; Alt+D, also while typing, to folders again.
        s.send(b"d", "files only")
        s.wait_ui("Files · 2")
        s.send(b"i", "INSERT")
        s.send(b"\x1bD", "folders only")
        s.send(b"\x1bD")
        s.wait_absent("folders only")
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)
        self.session.close()
        s = self.start("--depth", "2", implicit_root=True)
        s.send(b"D", "folders only")
        s.send(b"\x03")
        s.finish()
        self.session.close()
        # fff seek cannot navigate: D is Neovim's, deleting to the line's end.
        s = self.start("seek", "--depth", "2", implicit_root=True)
        s.send(b"top", "❯ top")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0D", "❯ type to filter")
        self.assertNotIn("folders only", s.screen.text)
        s.send(b"\x1bD", "does not work in fff seek")
        s.send(b"\x03")
        s.finish()

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_seek_searches_six_deep_by_score_and_starts_typing(self):
        deep = self.root / "a" / "b" / "c" / "d" / "e"
        deep.mkdir(parents=True)
        (deep / "report.txt").touch()  # depth 6
        (self.root / "a" / "repository-ported.txt").touch()  # depth 2, weaker
        s = self.start("seek", implicit_root=True, output_paths=False)
        s.wait_ui("╰ depth 6 ─")
        s.wait_ui("Files · 2")
        self.assertIn("INSERT", s.screen.text)
        # The best score comes first, however deep.
        s.send(b"report", "❯ report")
        s.wait_ui("Files · 2")
        rows = [l for l in s.screen.text.splitlines() if ".txt" in l]
        self.assertIn("report.txt", rows[0], s.screen.text)
        s.send(b"\x03")
        s.finish()

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_contents_query_lists_a_row_per_match_previews_it_and_prints_the_file(self):
        (self.root / "notes.txt").write_text("intro\nfirst canard\nmiddle\nsecond canard here\n")
        (self.root / "other.txt").write_text("nothing to see\n")
        s = self.start(implicit_root=True, typing=True)
        s.wait_ui("Query  INSERT")
        self.assertNotIn("Contents", s.screen.text)
        # Tab goes to the contents query, beside the names query, in the
        # mode it was pressed in.
        s.send(b"\t", "Contents  INSERT")
        self.assertIn("Names", s.screen.text)
        s.send(b"canard", "❯ canard")
        s.wait_ui("2 matches in 1 file")
        rows = [l for l in s.screen.text.splitlines() if "notes.txt" in l and "l. " in l]
        self.assertEqual(len(rows), 2, s.screen.text)
        self.assertIn("l. 2", rows[0])
        self.assertIn("first canard", rows[0])
        self.assertIn("l. 4", rows[1])
        # The preview shows the match among its lines.
        s.wait_ui("Preview · l. 2 · match 1 of 2")
        self.assertIn("intro", s.screen.text)
        s.send(b"\x1b[B", "Preview · l. 4 · match 2 of 2")
        # Tab goes back to the names query, which chooses the files read,
        # still typing.
        s.send(b"\t", "Names  INSERT")
        s.send(b"zzz", "No matches")
        s.send(b"\x7f\x7f\x7f", "Matches · 2")
        # Enter prints the file.
        s.send(b"\r")
        status, out = s.finish()
        self.assertEqual((status, out), (0, str(self.root / "notes.txt").encode() + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_a_file_with_more_matches_than_are_kept_says_so(self):
        (self.root / "many.txt").write_text("canard\n" * 250)
        s = self.start(implicit_root=True, typing=True)
        s.send(b"\t", "Contents  INSERT")
        s.send(b"canard", "200+ matches in 1 file · first 200 per file")

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_space_on_a_match_marks_its_file_and_moves_to_the_next_file(self):
        for name in ("one", "two"):
            (self.root / name).write_text("hello world\nhello again\n")
        s = self.start(implicit_root=True, typing=True)
        s.send(b"\t", "Contents  INSERT")
        s.send(b"hello", "4 matches in 2 files")
        s.send(b"\x1b[27u", "Contents  NORMAL")
        # Each file's two matches share its mark: Space goes on to the next
        # file, so pressing it again marks that one instead of unmarking.
        s.send(b" ", "Marks (1)")
        s.send(b" ", "Marks (2)")
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_match_preview_scrolls_past_its_context_from_the_match(self):
        lines = [f"line {n}" for n in range(1, 61)]
        lines[29] = "line 30 canard"
        lines[49] = "line 50 canard"
        (self.root / "notes.txt").write_text("\n".join(lines) + "\n")
        s = self.start(implicit_root=True, typing=True)
        s.send(b"\t", "Contents  INSERT")
        s.send(b"canard", "❯ canard")
        s.wait_ui("Preview · l. 30 · match 1 of 2")
        s.send(b"\x1b[27u", "NORMAL")
        self.assertNotIn("line 45", s.screen.text)
        # Ctrl+j scrolls on from the match, past the lines kept around it.
        for _ in range(4):
            s.send(b"\x1b[106;5u")  # CSI-u Ctrl+j
        s.wait_ui("line 45")
        # Another match starts at its own match again.
        s.send(b"j", "Preview · l. 50 · match 2 of 2")
        s.wait_ui("line 50 canard")
        s.send(b"\x03")
        s.finish()

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_switch_program_goes_on_as_pick_at_its_depth_and_prints_the_file(self):
        (self.root / "sub").mkdir()
        (self.root / "sub" / "deep.txt").touch()
        (self.root / "top.txt").touch()
        s = self.start(implicit_root=True, output_paths=False)
        s.wait_ui("Files · 2")
        s.send(b"gm", "Switch program")
        self.assertIn("pick", s.screen.text)
        # fff pick searches to its own depth, since roam's was not changed.
        s.send(b"p", "Now fff pick")
        s.wait_ui("deep.txt")
        s.send(b"ideep", "❯ deep")
        # It prints the file, as fff pick does.
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, str(self.root / "sub" / "deep.txt").encode() + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_gd_labels_rows_by_name_or_path_and_keeps_the_choice_across_gm(self):
        (self.root / "a" / "b").mkdir(parents=True)
        (self.root / "a" / "b" / "note.txt").touch()
        s = self.start("--depth", "3", implicit_root=True)
        s.wait_ui("note.txt")
        # fff pick names rows; the highlighted folder fills the footer.
        self.assertNotIn("a/b/note.txt", s.screen.text)
        s.send(b"gd", "Rows show their paths")
        s.wait_ui("a/b/note.txt")
        # fff seek starts with paths, but gm keeps what gd chose.
        s.send(b"gd", "Rows show their names")
        s.send(b"gm", "Switch program")
        s.send(b"s", "Now fff seek")
        s.wait_ui("Files · 1")
        row = next(line for line in s.screen.text.splitlines() if "› 1/0" in line)
        self.assertIn(" note.txt", row)
        self.assertNotIn("a/b/", row)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_switch_program_keeps_the_contents_query(self):
        (self.root / "notes.txt").write_text("a canard here\n")
        (self.root / "other.txt").write_text("nothing\n")
        s = self.start(implicit_root=True, typing=True)
        s.send(b"\t", "Contents  INSERT")
        s.send(b"canard", "1 match in 1 file")
        s.send(b"\x1b[27u", "Contents  NORMAL")
        s.send(b"gm", "Switch program")
        self.assertNotIn("grep", s.screen.text)
        # fff seek keeps the contents query and its matches, still in
        # Normal mode.
        s.send(b"s", "Now fff seek")
        s.wait_ui("Contents  NORMAL")
        s.wait_ui("1 match in 1 file")
        # Tab goes to the names query, in the mode it was pressed in; Alt+/
        # comes back,
        # and clearing the contents query lists the files.
        s.send(b"\t", "Names  NORMAL")
        s.send(b"\x1b/", "Contents  NORMAL")
        s.send(b"\x1b[27u", "Files · 2")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_roam_searches_contents_after_alt_slash(self):
        (self.root / "notes.txt").write_text("intro\na canard here\n")
        (self.root / "other.txt").write_text("nothing\n")
        s = self.start(implicit_root=True, output_paths=False)
        s.wait_ui("Query  NORMAL")
        self.assertNotIn("Contents", s.screen.text)
        # Alt+/ opens the contents query beside the names query.
        s.send(b"\x1b/", "Contents  NORMAL")
        self.assertIn("Names", s.screen.text)
        s.send(b"i", "Contents  INSERT")
        s.send(b"canard", "1 match in 1 file")
        s.wait_ui("Matches · 1")
        row = next(l for l in s.screen.text.splitlines() if "notes.txt" in l)
        self.assertIn("l. 2", row)
        # Back to the names query, still typing; the contents bar stays
        # while it is set.
        s.send(b"\x1b/", "Names  INSERT")
        self.assertIn("Contents", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_pick_starts_typing_the_query(self):
        (self.root / "one").touch()
        s = self.start("--depth", "1", implicit_root=True, typing=True)
        s.wait_ui("Query  INSERT")
        s.send(b"one", "❯ one")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_space_marks_tab_switches_queries_and_escape_twice_clears_marks(self):
        for name in ("one", "two", "three"):
            (self.root / name).touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.wait_ui("Files · 3")
        s.send(b"i" + SHIFT_SPACE, "Marks (1)")  # also while typing
        s.send(b"\x1b[27u", "NORMAL")
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b" ", "Marks (2)")
        # Tab never marks: it switches to the contents query and back.
        s.send(b"\t", "Contents  NORMAL")
        s.send(b"\t", "Query  NORMAL")
        # From Insert mode, both are typed into in Insert mode.
        s.send(b"i\t", "Contents  INSERT")
        s.send(b"\t", "Query  INSERT")
        self.assertIn("Marks (2)", s.screen.text)
        # With a query, Escape clears it first and keeps the marks.
        s.send(b"t", "❯ t")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "❯ type to filter")
        self.assertIn("Marks (2)", s.screen.text)
        # An Escape followed by another key does not clear them either.
        s.send(b"j")
        s.send(b"\x1b[27u")
        s.send(b"j")
        s.send(b"\x1b[27u")
        self.assertIn("Marks (2)", s.screen.text)
        s.send(b"\x1b[27u", "Cleared 2 marks")
        self.assertIn("Marks (0)", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_progress_marker_goes_a_while_after_the_scan(self):
        (self.root / "x.txt").touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.wait_ui("✓ complete")
        s.wait_absent("✓ complete")
        s.send(b"\x03")
        s.finish()

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_seek_never_falls_back_to_its_folder_or_hands_ctrl_o_to_neovim(self):
        (self.root / "only.txt").touch()
        s = self.start("seek", implicit_root=True, output_paths=False)
        # Ctrl+o says why it does nothing; the next letter is still text.
        s.send(b"\x1b[111;5u", "does not work in fff seek")
        s.send(b"zzz", "❯ zzz")
        # Normal-mode Enter with nothing listed stays instead of leaving.
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\r", "Nothing matches")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_marked_list_enter_refuses_a_mark_that_no_longer_exists(self):
        gone = self.root / "gone.txt"
        gone.touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.send(SHIFT_SPACE, "Marks (1)")
        gone.unlink()
        s.send(b"\x1b ", "› ●")
        s.send(b"\r", "This mark no longer exists")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_preview_pane_follows_the_highlight(self):
        (self.root / "a.txt").write_text("first\tline\nsecond line\n")
        (self.root / "b").mkdir()
        (self.root / "b" / "inside.md").touch()
        (self.root / "c.bin").write_bytes(b"\x00\x01\x02")
        s = self.start("--depth", "1", implicit_root=True, size=(40, 120))
        # Shown from the start. Folders come first: b's entries show once the
        # highlight rests.
        s.wait_ui("Preview · directory · 1 entry")
        self.assertIn("inside.md", s.screen.text)
        s.send(b"j", "Preview · text · 2 lines")
        self.assertIn("first   line", s.screen.text)
        s.send(b"j", "Preview · binary data")
        s.send(b"P")
        s.wait_absent("Preview")
        # Alt+v toggles it too, also while typing.
        s.send(b"i", "INSERT")
        s.send(b"\x1bv", "Preview · binary data")
        s.send(b"\x1bv")
        s.wait_absent("Preview")
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_preview_colors_a_text_files_syntax(self):
        (self.root / "main.rs").write_text("const ANSWER: u32 = 42;\n")
        s = self.start("--depth", "1", implicit_root=True, size=(40, 120))
        s.wait_ui("Preview · text · 1 line")
        # The number in the error color of the default palette (midnight).
        s.wait_bytes(b"38;2;250;145;145;48;2;20;30;46m42")
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_default_depth_by_mode_and_feature_chords_only_while_typing(self):
        s = self.start(implicit_root=True, output_paths=False)
        s.wait_ui(" depth 1 ")
        # In Normal mode, Alt+w names the key that works there instead.
        s.send(b"\x1bw", "Alt+w works in Insert mode; in Normal mode, gw")
        self.assertNotIn("Workspaces", s.screen.text)
        s.send(b"i", "INSERT")
        s.send(b"\x1bw", "Workspaces")
        s.send(b"\x03")
        s.finish()
        s.close()
        s = self.session = Session(self.root, implicit_root=True)
        s.wait_ui(" depth 3 ")
        s.send(b"\x03")
        s.finish()

    def test_normal_plain_keys_change_depth_hidden_and_open_prompts(self):
        (self.root / "dir").mkdir()
        (self.root / "dir" / "deep.txt").touch()
        (self.root / ".secret").touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"2", "deep.txt")
        s.send(b"1")
        s.wait_absent("deep.txt")
        s.send(b"+", "deep.txt")
        s.send(b"-")
        s.wait_absent("deep.txt")
        s.send(b".", ".secret")
        s.send(b"g/", "Go to directory · INSERT")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u")
        s.wait_absent("Go to directory")
        s.send(b"gw", "Workspaces")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_quit_from_pinned_root_prints_nothing(self):
        (self.root / "file").touch()
        s = self.start()
        s.send(b"q")
        self.assertEqual(s.finish(), (0, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_no_dir_starts_in_insert_and_quit_returns_no_directory(self):
        (self.root / "file").touch()
        s = self.start("seek")
        # A file choice starts ready to type: "!" is query text here.
        s.send(b"", "Query  INSERT")
        s.send(b"!", "❯ !")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "❯ type to filter")
        s.send(b"!")
        self.assertEqual(s.finish(), (0, b""))

    def test_workspace_alt_digits_use_visible_filtered_rows_and_clamp(self):
        names = [f"place{i:02}" for i in range(12)]
        for name in names:
            (self.root / name).mkdir()
            (self.root / name / "file").touch()
        config = self.root / "config.toml"
        config.write_text("".join(f"[[workspaces]]\nname='{n}'\npath='{n}'\n" for n in names))
        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"gw", "Workspaces")
        s.send(b"\x1b2", ("cwd", self.root / names[1]))
        s.send(b"gw", "Workspaces")
        s.send(b"iplace1", "place11")
        s.send(b"\x1b9", ("cwd", self.root / names[11]))
        s.send(b"\x1bw", "Workspaces")
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
        s.send(SHIFT_SPACE, b"Marks (1)")
        s.send(b"\x1b[50;5u")  # CSI-u: Ctrl+2
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "b") + b"\0"))

    def test_plain_digit_escape_then_cancel(self):
        (self.root / "2.txt").touch()
        (self.root / "other").touch()
        s = self.start()
        s.send(b"i2", "❯ 2")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", b"other")  # Normal-mode Escape clears the query
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_empty_directory_enter(self):
        s = self.start("--null")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root) + b"\0"))

    def test_no_matches_numbered_acts_like_enter(self):
        (self.root / "file").touch()
        s = self.start("--null")
        s.send(b"izzzz", b"No matching results")
        # While typing, Enter with no matches stays open; so does Ctrl+9.
        s.send(b"\x1b[57;5u")
        s.send(b"\x1b[27u", "Query  NORMAL")
        # In Normal mode, Enter with nothing listed takes the current directory.
        s.send(b"\x1b[57;5u")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root) + b"\0"))

    def test_depth_and_exclusion_editor(self):
        (self.root / "build").mkdir()
        (self.root / "build" / "log").touch()
        (self.root / "main.tex").touch()
        s = self.start("--depth", "1", "--exclude", "build")
        s.send(b"\x1b[1;3C", " depth 2 ")  # Alt+Right
        self.assertIn("exclusions on", s.screen.text)
        s.send(b"ge")
        s.wait_absent("exclusions on")
        s.send(b"gE", b"one glob per line")
        s.send(b"o*.tex", "*.tex")
        before_apply = len(s.ui)
        s.send(b"\x1b[27uZZ")
        s.wait_absent("Edit exclusions")
        self.assertNotIn(b"No matching results", s.ui[before_apply:])
        s.send(b"ge", b"No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_repeated_globs_and_filter_editor(self):
        for name in ("a.pdf", "b.epub", "c.log"):
            (self.root / name).touch()
        s = self.start("--depth", "1", "--glob", "*.pdf", "--glob", "*.epub")
        s.send(b"", "filters on")
        s.wait_ui("b.epub")
        self.assertIn("a.pdf", s.screen.text)
        self.assertNotIn("c.log", s.screen.text)
        # gf turns the filters off and on again, keeping them.
        s.send(b"gf", "c.log")
        self.assertNotIn("filters on", s.screen.text)
        s.send(b"gf", "filters on")
        s.wait_absent("c.log")
        s.send(b"gF", "Edit filters")
        self.assertIn("*.epub", s.screen.text)
        s.send(b"o*.log\x1b[27uZZ", "c.log")  # ZZ applies
        s.send(b"gF", "Edit filters")
        s.send(b"ggdGZZ")  # clear every line: no filters left
        s.wait_absent("filters on")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_mark_list_unmark_then_confirm_file(self):
        (self.root / "a").touch()
        (self.root / "b").touch()
        s = self.start()
        s.send(SHIFT_SPACE, b"Marks (1)")
        s.send(b"\x1b ", "› ●")
        s.send(SHIFT_SPACE, "› ○")
        s.send(b"\x1b", b"Marks (0)")
        s.send(b"\x1bk", "›   1    a")
        # Enter while typing returns the highlighted file (Normal-mode Enter
        # would exit into its folder instead).
        s.send(b"i", "Query  INSERT")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "a") + b"\n"))

    def test_filetype_icons_default_cli_and_config_disable(self):
        (self.root / "docs").mkdir()
        (self.root / "main.rs").touch()
        s = self.start("--depth", "1")
        s.wait_ui(" docs/")
        s.wait_ui(" main.rs")
        s.send(SHIFT_SPACE, "●  docs")  # marked rows carry the same icon
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        with tempfile.TemporaryDirectory(prefix="fff-icons-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\nicons=false\n")
            for args in (("--no-icons",), ("--config", str(config))):
                s = self.start("--depth", "1", *args)
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
        s.send(SHIFT_SPACE, b"Marks (1)")
        marked.unlink()
        s.send(b"izzzz", b"No matching results")
        s.send(b"\r\x17", b"Missing marked paths")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_tiny_terminal_has_no_numbered_rows(self):
        (self.root / "file").touch()
        s = self.start(size=(5, 160))
        # fff pick starts typing; Normal-mode Ctrl+1 with no rows leaves.
        s.send(b"\x1b[27u")
        time.sleep(0.3)
        s.send(b"\x1b[49;5u")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root) + b"\n"))

    def test_location_navigation_preserves_marks_and_fixed_root(self):
        (self.root / "a").touch()
        with tempfile.TemporaryDirectory(prefix="fff-location-") as other:
            target = Path(other) / "b"
            target.touch()
            s = self.start()
            s.send(SHIFT_SPACE, b"Marks (1)")
            s.send(b"g/", b"Go to directory")
            s.send(os.fsencode(other) + b"\r", ("cwd", other))
            self.assertIn(other, s.screen.text)
            s.send(b"b", "❯ b")  # the go-to prompt leaves the query in Insert
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
                # Typing: Enter enters directories and returns a file.
                s.send(b"i", "Query  INSERT")
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
        s.send(b"ileaf-only", "❯ leaf-only")
        s.send(b"\x1bh", ("cwd", parent))
        s.wait_ui(b"No matching results")
        self.assertIn(str(parent), s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_alt_number_enters_directory_like_l_and_keeps_marks(self):
        (self.root / "a").mkdir()
        directory = self.root / "b"
        directory.mkdir()
        marked = self.root / "c.txt"
        marked.touch()
        for alt_two, alt_zero in [(b"\x1b2", b"\x1b0"), (b"\x1b[50;3u", b"\x1b[48;3u")]:
            with self.subTest(alt_two=alt_two):
                s = self.start("--null", "--depth", "1", implicit_root=True)
                s.send(b"ic.txt", "❯ c.txt")
                s.send(SHIFT_SPACE, b"Marks (1)")
                s.send(b"\x1b[27u", "NORMAL")
                s.send(b"\x1b[27u", b"Files \xc2\xb7 3")
                s.send(alt_two, ("cwd", directory))
                self.assertIn("Marks (1)", s.screen.text)
                # Nothing is listed in b: Alt+0 does nothing and fff stays.
                s.send(alt_zero)
                s.send(b"\x17")  # Ctrl+w returns the marks
                self.assertEqual(s.finish(), (0, os.fsencode(marked) + b"\0"))
                s.close()
                self.session = None

    def test_alt_number_opens_a_file_in_the_background_and_clamps_to_last(self):
        for name in ("a", "b", "c"):
            (self.root / name).touch()
        env, config, log = self.opener_setup()
        for chord in (b"\x1b0", b"\x1b9"):
            with self.subTest(chord=chord):
                # Each run starts fresh, without the previous run's cursor.
                shutil.rmtree(str(self.root) + "-state", ignore_errors=True)
                log.unlink(missing_ok=True)
                s = self.start("--null", "--config", str(config), environment=env)
                s.send(SHIFT_SPACE, b"Marks (1)")
                s.send(chord, "Opened c with viewer")
                self.wait_file(log, [str(self.root / "c")])
                s.send(b"\x17")  # fff kept running; the marks are untouched
                self.assertEqual(s.finish(), (0, os.fsencode(self.root / "a") + b"\0"))
                s.close()
                self.session = None

    def test_alt_no_matches_stays_open(self):
        (self.root / "a").touch()
        s = self.start()
        s.send(SHIFT_SPACE, b"Marks (1)")
        s.send(b"izzzz", b"No matching results")
        s.send(b"\x1b9", b"No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_alt_number_after_paging_uses_visible_labels(self):
        for i in range(30):
            (self.root / f"file{i:02}").touch()
        env, config, log = self.opener_setup()
        s = self.start("--config", str(config), size=(16, 160), environment=env)
        s.send(b"\x1b[6~", b"file17")
        s.send(b"\x1b2", "Opened file10 with viewer")
        self.wait_file(log, [str(self.root / "file10")])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_right_hand_marks_focus_and_resize(self):
        (self.root / "file").touch()
        s = self.start()
        title = next(line for line in s.screen.text.splitlines() if "Files ·" in line)
        self.assertGreater(title.index("Marks (0)"), title.index("Files ·"))
        self.assertIn(b"38;2;", s.ui)
        s.send(SHIFT_SPACE, b"Marks (1)")
        s.send(b"\x1b ", "› ●")
        # Resize to a compact stacked layout, then exercise the same marked row.
        fcntl.ioctl(s.master, termios.TIOCSWINSZ, struct.pack("HHHH", 24, 60, 0, 0))
        s.screen = Screen(24, 60)
        os.kill(s.pid, signal.SIGWINCH)
        s.wait_ui("Marks (1)")
        s.send(SHIFT_SPACE, "› ○")
        s.send(b"\x1b", b"Marks (0)")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_alt_l_navigates_directories_and_opens_files_without_confirming(self):
        directory = self.root / "folder"
        directory.mkdir()
        target = directory / "file"
        target.touch()
        env, config, log = self.opener_setup()
        s = self.start("--depth", "1", "--config", str(config), implicit_root=True, environment=env)
        s.send(b"\x1bl", ("cwd", directory))
        s.send(SHIFT_SPACE, b"Marks (1)")
        s.send(b"\x1bl", "Opened file with viewer")
        self.wait_file(log, [str(target)])
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(target) + b"\n"))

    def test_f1_opens_the_keys_screen_and_f2_the_session(self):
        (self.root / "file").touch()
        s = self.start()
        self.assertIn("F1 keys", s.screen.text)
        s.send(b"\x1bOP", "showing what works in fff pick · results")  # F1
        self.assertIn("Keys · INSERT", s.screen.text)
        # Typing searches, and your own keys move: Alt+j while typing.
        s.send(b"mark", "❯ mark")
        s.send(b"\x1bj", "mark_all")
        # Escape leaves Insert, then clears the query; it never closes.
        s.send(b"\x1b[27u", "Keys · NORMAL")
        s.send(b"\x1b[27u", "type a function's name")
        # In Normal mode, r and a key writes the key's name: what it does.
        s.send(b"r", "press the key to search")
        s.send(b"\x1b ", "toggle_marked_list")  # Alt+Space: the Keys view
        self.assertIn("Keys · ", s.screen.text)
        # Tab goes back to the Functions view.
        s.send(b"\t", "Functions · ")
        s.send(b"\x1b[27u", "type a function's name")
        s.send(b"\x1bOQ", "CURRENT DIRECTORY")  # F2
        s.send(b"\x1bOP", "showing what works")
        s.send(b"\x1bOP", "F1 keys")  # F1 again closes
        self.assertNotIn("showing what works", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_go_into_folder_enters_a_directory_or_a_files_folder(self):
        (self.root / "sub" / "deep").mkdir(parents=True)
        (self.root / "sub" / "note.txt").touch()
        config = self.config("", keys="[normal]\ngo_to_highlighted = 'gd'\n")
        s = self.start("--config", config, "--depth", "2", implicit_root=True, output_paths=False)
        s.send(b"inote", "❯ note")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"gd", ("cwd", self.root / "sub"))
        self.assertIn("note.txt", s.screen.text)
        s.send(b"ideep", "❯ deep")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"gd", ("cwd", self.root / "sub" / "deep"))
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_colon_picks_a_function_that_works_here_by_name(self):
        (self.root / "file").touch()
        (self.root / ".secret").touch()
        s = self.start(implicit_root=True, output_paths=False)
        s.send(b":", "Commands · INSERT")
        # Only what works here: fff roam answers no dialog.
        self.assertNotIn("save_here", s.screen.text)
        s.send(b"toggle_hid", "toggle_hidden")
        s.send(b"\r", "hidden shown")
        self.assertNotIn("Commands ·", s.screen.text)
        self.assertIn("Query  NORMAL", s.screen.text)
        # Escape leaves typing for Normal mode, where j, k, gg and G move,
        # and a second Escape goes back without running anything.
        s.send(b":", "Commands · INSERT")
        s.send(b"toggle", "toggle_hidden")
        s.send(b"\x1b[27u", "Commands · NORMAL")
        first = next(l for l in s.screen.text.splitlines() if "› " in l and "toggle" in l)
        s.send(b"j")
        s.wait_absent(first)
        s.send(b"gg", first)
        s.send(b"\x1b[27u")
        s.wait_absent("Commands ·")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_gs_sorts_the_matches_and_tab_switches_the_direction(self):
        for name, size in [("file10", 1), ("file2", 300), ("file1", 20)]:
            (self.root / name).write_bytes(b"x" * size)
        s = self.start(implicit_root=True, output_paths=False)
        s.wait_ui("file10")

        def order():
            # Each name where its row first shows it (the footer repeats one).
            names = ["file10", "file2", "file1"]
            found = []
            for line in s.screen.text.splitlines():
                words = line.replace("/", " ").split()
                found += [n for n in names if n in words and n not in found]
            return found

        # Descending to start: each criterion's own order, names from A.
        s.send(b"gs", "Sort descending · INSERT")
        self.assertIn("Tab: Toggle ordering ─── Enter: Sort", s.screen.text)
        s.send(b"name", "Names alphabetically")
        s.send(b"\r", "sorted by name ↓")
        self.assertNotIn("Sort descending", s.screen.text)
        self.assertEqual(order(), ["file1", "file2", "file10"])
        # Tab reverses; the picker opens at the current order.
        s.send(b"gs", "Sort descending")
        s.send(b"\t", "Sort ascending · INSERT")
        s.send(b"\r", "sorted by name ↑")
        self.assertEqual(order(), ["file10", "file2", "file1"])
        # In Normal mode j and k move: the next order after name is size.
        s.send(b"gs", "Sort ascending · INSERT")
        s.send(b"\t", "Sort descending · INSERT")
        s.send(b"\x1b[27uj")
        s.send(b"\r", "sorted by size ↓")
        s.send(b"gs", "Sort descending")
        s.send(b"size", "Sizes")
        s.send(b"\r", "sorted by size ↓")
        self.assertEqual(order(), ["file2", "file1", "file10"])
        # Escape leaves the order as it is; program goes back to the
        # program's order, named nowhere.
        s.send(b"gs", "Sort descending")
        s.send(b"\x1b[27u", "Sort descending · NORMAL")
        self.assertIn("11 orders", s.screen.text)
        s.send(b"\x1b[27u")
        s.wait_absent("Sort descending")
        self.assertIn("sorted by size ↓", s.screen.text)
        s.send(b"gs", "Sort descending")
        s.send(b"program", "As this program ranks")
        s.send(b"\r")
        s.wait_absent("sorted by")
        # Rows are numbered as the results are: Alt+2 is Enter on the
        # second, name.
        s.send(b"gs", "Sort descending")
        self.assertIn("  2 name", s.screen.text)
        s.send(b"\x1b2", "sorted by name ↓")
        self.assertNotIn("Sort descending", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_keys_screen_saves_keys_and_refuses_a_lockout(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-keys-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("")
            keys = Path(folder) / "keys.toml"
            s = self.start("--config", str(config))
            s.send(b"\x1bOP", "showing what works")
            s.send(b"hidden", "❯ hidden")
            s.send(b"\r", "Keys of toggle_hidden")
            # n and a key: where it applies opens, Normal mode only for a
            # letter; q is quit's there, so saying where it applies clashes.
            s.send(b"nq\r", "Where q applies")
            self.assertIn("q CLASHES WITH ANOTHER KEY", s.screen.text)
            self.assertIn("quit ", s.screen.text)
            s.send(b"t", "taken from the others")
            self.assertFalse(keys.exists())  # staged until Enter
            s.send(b"\r", "Keys saved to")
            self.assertIn('toggle_hidden = [".", "q"]', keys.read_text())
            # Removing every key of guide would leave nothing to open this.
            s.send(b"\x1b[27u", "Keys · NORMAL")
            s.send(b"\x1b[27u", "type a function's name")
            s.send(b"ishow_keys", "❯ show_keys")
            s.send(b"\r", "Keys of show_keys")
            s.send(b"dd")
            s.send(b"\r", "Not saved: nothing would open the Keys screen")
            # F1 closes the screen and drops what was not saved.
            s.send(b"\x1bOP", "Changes to keys not saved")
            s.send(b"\x1b[27u")
            s.send(b"q", "hidden shown")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_keys_editor_replaces_a_key_restores_defaults_on_confirmation_and_wraps(self):
        (self.root / "file").touch()
        s = self.start("--no-config")
        s.send(b"\x1bOP", "showing what works")
        # Typing, r is text: its hint shows in Normal mode only.
        self.assertNotIn("r, then a key", s.screen.text)
        s.send(b"\x1b[27u", "r, then a key")
        s.send(b"ihidden", "❯ hidden")
        s.send(b"\r", "Keys of toggle_hidden")
        # A key that cannot be bound where the highlighted one applies
        # says so; R and a free key otherwise take its place.
        s.send(b"RZ\r", "Z cannot be bound where Alt+. applies")
        s.send(b"j")
        s.send(b"RZ\r", ". replaced by Z; Enter saves")
        self.assertIn("› Z ", s.screen.text)
        # A key that clashes opens where it applies, with the clash.
        s.send(b"Rq\r", "Where q applies")
        self.assertIn("q CLASHES WITH ANOTHER KEY", s.screen.text)
        s.send(b"\x1b[27u", "R: Replace")
        # D asks before putting the defaults back.
        s.send(b"D", "Put back the default keys of toggle_hidden?")
        s.send(b"\r", "Default keys back; Enter saves")
        self.assertIn("› Alt+.", s.screen.text)
        # Moving up from the first key goes round to the last.
        s.send(b"k", "WHERE . APPLIES")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_status_messages_fade_after_configured_seconds(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-status-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\nmessage_seconds=1\n")
            s = self.start("seek", "--config", str(config))
            s.send(b"\x1bh", "does not work in fff seek")
            started = time.monotonic()
            s.wait_absent("does not work in fff seek", timeout=4)
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
        s = self.start("seek", "--depth", "2", implicit_root=True)
        s.send(b"", b"nested.txt")
        self.assertNotIn("folder/ ", s.screen.text)
        # Parent and location stay put with a notice. (Every row is a file,
        # so Alt+l would open it rather than enter anything.)
        for key in (b"\x1bh", b"\x1bg"):
            s.send(key, "does not work in fff seek")
            self.assertTrue(any(" ".join(l.split()).startswith(f"@ {self.root}") for l in s.screen.text.splitlines()))
            self.assertNotIn("Go to directory", s.screen.text)
            self.assertNotIn("Workspaces ·", s.screen.text)
        # A no-match query: Ctrl+number does not fall back to the directory.
        s.send(b"zzzz", b"No matching results")
        s.send(b"\x1b[49;5u")  # CSI-u: Ctrl+1
        s.send(b"\x7f\x7f\x7f\x7f", b"top.txt")
        s.send(SHIFT_SPACE, b"Marks (1)")
        # Ctrl+w returns marks only in fff pick; seek stays.
        s.send(b"\x17", b"Ctrl+w returns the marks only where")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_seek_enter_while_typing_leaves_into_the_files_folder(self):
        deep = self.root / "work" / "notes"
        deep.mkdir(parents=True)
        (deep / "plan.docx").touch()
        (self.root / "other.txt").touch()
        # Straight away, in Insert mode, as when typing a filter and then
        # without one: Enter chooses, as in Normal mode, instead of trying to
        # reveal the file in its folder (seek never navigates).
        for typed in [b"", b"plan"]:
            s = self.session = Session(self.root, "seek", "--glob", "*.docx", "--depth", "4",
                                       implicit_root=True)
            s.wait_ui("plan.docx")
            self.assertIn("INSERT", s.screen.text)
            if typed:
                s.send(typed, "❯ plan")
            s.send(b"\r")
            self.assertEqual(s.finish(), (0, os.fsencode(deep) + b"\n"))
            s.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_paths_on_stdin_are_listed_in_order_and_printed_as_spelled(self):
        (self.root / "src").mkdir()
        (self.root / "src" / "main.rs").write_text("fn main() {}\n// a canard\n")
        (self.root / "notes.txt").write_text("nothing\n")
        elsewhere = Path(self.tmp.name + "-state")
        elsewhere.mkdir()
        (elsewhere / "far.txt").touch()
        listed = b"notes.txt\n./src/main.rs\nmissing\nsrc/\nnotes.txt\n" + os.fsencode(elsewhere / "far.txt") + b"\n"
        s = self.start(stdin=listed, ready=False)
        s.wait_ui("Skipped 1 line that names no file or folder")
        s.wait_ui("paths from stdin")
        # In the order read, each once; one from elsewhere is shown whole.
        rows = [l for l in s.screen.text.splitlines() if "│" in l and any(n in l for n in ("notes.txt", "main.rs", "src/", "far.txt"))]
        names = [next(n for n in ("notes.txt", "src/main.rs", "src/", str(elsewhere / "far.txt")) if n in r) for r in rows]
        self.assertEqual(names, ["notes.txt", "src/main.rs", "src/", str(elsewhere / "far.txt")], s.screen.text)
        self.assertIn("INSERT", s.screen.text)
        # Nothing navigates, changes or opens files.
        for key, function in ((b"\x1bh", "go_to_parent"), (b"\x1b[15~", "rescan"), (b"\x1bw", "show_workspaces")):
            s.send(key, f"({function}) does not work in fff reading paths from stdin")
        # The contents query searches the listed files; Enter prints the
        # chosen one as the list spelled it.
        s.send(b"\t", "Contents  INSERT")
        s.send(b"canard", "1 match in 1 file")
        s.send(b"\x1b[27u", "Contents  NORMAL")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, b"./src/main.rs\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nul_separated_paths_on_stdin_return_the_marks_nul_terminated(self):
        for name in ("b.txt", "a txt\nnewline"):
            (self.root / name).touch()
        s = self.start("-0", stdin=b"b.txt\0a txt\nnewline\0", ready=False)
        s.wait_ui("✓ complete")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"m", "Marks (2)")
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, b"b.txt\0a txt\nnewline\0"))

    def test_no_dir_has_no_tabs_so_normal_arrows_move_the_highlight(self):
        (self.root / "folder").mkdir()
        (self.root / "folder" / "nested.txt").touch()
        (self.root / "top.txt").touch()
        s = self.start("seek", "--depth", "2", implicit_root=True)
        s.send(b"", b"nested.txt")
        def highlight(name):
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline:
                if any("›" in l and name in l for l in s.screen.text.splitlines()):
                    return
                if select.select([s.master], [], [], 0.05)[0]:
                    s.take(os.read(s.master, 65536))
            raise AssertionError(f"{name} is not highlighted:\n{s.screen.text}")
        highlight("top.txt")
        # Normal-mode Up and Down switch tabs elsewhere; here they move rows.
        s.send(b"\x1b", "NORMAL")
        s.send(b"\x1b[B")
        highlight("nested.txt")
        s.send(b"\x1b[A")
        highlight("top.txt")
        self.assertNotIn("disabled in fff seek", s.screen.text)

    def test_live_workspace_save_edit_icon_cancel_and_delete(self):
        config = self.root / "config.toml"
        config.write_text("# Keep my comment\n[ui]\npalette='nord'\n")
        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"gw", "Workspaces")
        s.send(b"\x1bn", "New workspace")
        s.send(b"\x1b[27uccNotes", "Notes")
        s.send(b"\x1b[27u\t\t", "Space: Choose icon")
        s.send(b" ", "Workspace icons")
        s.send(b"ihome", "Home house")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\r", "New workspace")
        s.send(b"\r", "Workspace saved")
        saved = tomllib.loads(config.read_text())["workspaces"][0]
        self.assertEqual(saved, dict(name="Notes", path=str(self.root), icon="\uf015"))
        self.assertIn("# Keep my comment", config.read_text())
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27uccRenamed", "Renamed")
        s.send(b"\x1b[27u\t\t", "Space: Choose icon")
        s.send(b" ", "Workspace icons")
        s.send(b"iautomatic", "Automatic")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\r", "Edit workspace")
        s.send(b"\r")
        s.wait_absent("Edit workspace")
        s.wait_absent("New workspace")
        s.wait_ui("Workspace saved")
        saved = tomllib.loads(config.read_text())["workspaces"][0]
        self.assertEqual(saved, dict(name="Renamed", path=str(self.root)))
        before = config.read_text()
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27uccchanged", "changed")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u")
        s.wait_absent("Edit workspace")
        self.assertEqual(config.read_text(), before)
        s.send(b"\x1bd", "Delete workspace")
        s.send(b"\x1b[27u")
        s.wait_absent("Delete workspace")
        self.assertEqual(config.read_text(), before)
        s.send(b"\x1bd", "Delete workspace")
        s.send(b"\r", "Workspace deleted")
        self.assertEqual(tomllib.loads(config.read_text()).get("workspaces", []), [])
        self.assertTrue(self.root.is_dir())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_workspaces_keep_ticked_settings_and_start_sessions(self):
        for name in ["phd/a.pdf", "phd/x.txt", "books/deep/b.pdf"]:
            (self.root / name).parent.mkdir(parents=True, exist_ok=True)
            (self.root / name).touch()
        config = self.root / "config.toml"
        config.write_text(
            "[[workspaces]]\nname='pickpdf'\nprogram='pick'\nfilters=['*.pdf']\n"
            f"roots=['{self.root / 'phd'}', '{self.root / 'gone'}']\n")
        s = self.start("--config", str(config), "--depth", "3", "--glob", "*.pdf",
                       implicit_root=True, output_paths=False)
        s.send(b"gw", "Workspaces")
        self.assertIn("pick · 2 roots · 1 filter", s.screen.text)
        s.send(b"\x1bn", "New workspace")
        self.assertIn("KEEPS", s.screen.text)
        s.send(b"\x1b[27uccnotes", "notes")
        # Past the path, the icon, the program, the roots and the queries.
        s.send(b"\x1b[27u" + b"\t" * 6, "› ○ depth")
        s.send(b" ", "● depth")
        s.send(b"\t ", "● filters")
        s.send(b"\r", "Workspace saved")
        saved = tomllib.loads(config.read_text())["workspaces"][1]
        self.assertEqual(saved, dict(name="notes", path=str(self.root), depth=3, filters=["*.pdf"]))
        s.send(b"\x03")
        s.finish()
        s.close()
        # The workspace's settings start the session, unless given here.
        for args, depth in ((), 3), (("--depth", "1"), 1):
            s = self.session = Session(self.root, "--config", str(config), "--workspace", "notes",
                                       *args, implicit_root=True, output_paths=False,
                                       cwd=self.root / "books")
            s.wait_ui(("cwd", self.root))
            s.wait_ui(f" depth {depth} ")
            if depth == 3:
                s.wait_ui("a.pdf")
                self.assertNotIn("x.txt", s.screen.text)
            s.send(b"\x03")
            s.finish()
            s.close()
        # Its program too, without the roots that are gone.
        s = self.session = Session(self.root, "--config", str(config), "--workspace", "pickpdf",
                                   implicit_root=True, output_paths=False, ready=False)
        s.wait_ui("left out missing roots")
        s.wait_ui("INSERT")
        s.send(b"a.pdf", "a.pdf")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "phd" / "a.pdf") + b"\n"))
        s.close()
        s = self.session = Session(self.root, "--config", str(config), "--workspace", "nothing",
                                   implicit_root=True, output_paths=False, ready=False)
        self.assertEqual(s.finish()[0], 1)

    def test_workspace_save_creates_missing_default_config_and_survives_restart(self):
        config_home = self.root / "settings"
        config = config_home / "fff" / "config.toml"
        environment = {"XDG_CONFIG_HOME": str(config_home)}
        s = self.start(implicit_root=True, environment=environment)
        self.assertFalse(config.exists())
        s.send(b"gw", "Workspaces")
        s.send(b"\x1bn", "New workspace")
        s.send(b"\r", "Workspace saved")
        self.assertEqual(tomllib.loads(config.read_text())["workspaces"][0]["path"], str(self.root))
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        self.session = None
        s = self.start(implicit_root=True, environment=environment)
        s.send(b"gw", "Workspaces")
        self.assertIn(self.root.name, s.screen.text)
        s.send(b"\r", ("cwd", self.root))
        s.wait_absent("Workspaces ·")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_live_workspaces_session_only_and_external_reload(self):
        s = self.start(implicit_root=True)
        s.send(b"gw", "Workspaces")
        s.send(b"\x1bn", "New workspace")
        s.send(b"\r", "for this session")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        self.session = None
        config = self.root / "config.toml"
        config.write_text("[[workspaces]]\nname='Before'\npath='.'\n")
        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"gw", "Before")
        s.send(b"\x1be", "Edit workspace")
        s.send(b"!", "Before!")  # the form starts typing at the end
        config.write_text("[[workspaces]]\nname='External'\npath='.'\n")
        s.send(b"\r", "changed on disk")
        self.assertIn("External", config.read_text())
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u")
        s.wait_absent("Edit workspace")
        s.send(b"\x1br", "Workspaces reloaded")
        self.assertIn("External", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is unavailable")
    def test_live_workspace_nvim_form_and_icon_picker(self):
        s = self.start(implicit_root=True)
        s.send(b"gw", "Workspaces")
        # Normal-mode filter: plain letters act and the border says so.
        self.assertIn("n: New ─── e: Edit ─── d: Delete ─── r: Reload ─── ↑↓: Reorder", s.screen.text)
        self.assertIn("n adds one: a folder, settings to apply, or both", s.screen.text)
        s.send(b"n", "New workspace")
        s.wait_ui("New workspace · INSERT")  # the form is for typing
        s.send(b"\x1b[27uccNotes", "Notes")
        # The path is optional, as it says under it.
        self.assertIn("Optional: left empty", s.screen.text)
        # Normal-mode j and k move through the rows, as Tab does; the path
        # is typed into from the start.
        s.send(b"\x1b[27uj", "› Path   ")
        s.send(b"\x1b[27uj", "Space: Choose icon")
        s.send(b"j")
        s.wait_absent("Space: Choose icon")
        s.send(b"jjkk")
        s.send(b"k", "Space: Choose icon")
        # On the icon row Space opens the chooser, Alt+i does nothing.
        s.send(b"\x1bi")
        self.assertNotIn("Workspace icons", s.screen.text)
        s.send(b" ", "Workspace icons")
        self.assertIn("NORMAL", s.screen.text)
        s.send(b"ihome", "Home house")
        s.send(b"\x1b", "NORMAL")
        s.send(b"\r", "New workspace")
        s.send(b"\r")
        s.wait_absent("New workspace")
        s.wait_ui("Workspace saved")
        self.assertIn("Notes", s.screen.text)
        s.send(b"e", "Edit workspace")
        s.send(b"\x1b[27u", "NORMAL")  # leave Insert, then the form
        s.send(b"\x1b[27u", "Workspaces ·")
        s.send(b"d", "Delete workspace")
        s.send(b"\x1b[27u", "Workspaces ·")
        s.wait_absent("Delete workspace")
        # While typing a filter the letters are text and Alt is needed.
        s.send(b"inot", "Alt+n: New")
        self.assertNotIn("Edit workspace", s.screen.text)
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27u", "Edit workspace · NORMAL")
        s.send(b"\x1b[27u", "Workspaces ·")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is unavailable")
    def test_workspace_neovim_enter_saves_and_escape_cancels(self):
        config = self.root / "config.toml"
        config.write_text("[[workspaces]]\nname='Before'\npath='.'\n")
        name = lambda: tomllib.loads(config.read_text())["workspaces"][0]["name"]
        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"gw", "Before")
        # Escape leaves Insert first; a second Escape cancels the form.
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27uccDiscarded", "Discarded")
        s.send(b"\x1b[27u", "NORMAL")
        self.assertIn("Edit workspace", s.screen.text)
        s.send(b"\x1b[27u", "Workspaces ·")
        s.wait_absent("Edit workspace")
        self.assertEqual(name(), "Before")
        # Enter saves straight from Insert mode.
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27uccWritten\r", "Workspace saved")
        s.wait_absent("Edit workspace")
        self.assertEqual(name(), "Written")
        # Validation errors keep the form open; nothing is written.
        before = config.read_text()
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27udd\r", "Workspace name must not be empty")
        self.assertIn("Edit workspace", s.screen.text)
        s.send(b"\x1b[27u", "Workspaces ·")
        self.assertEqual(config.read_text(), before)
        # An unchanged workspace is a no-op.
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\r", "Workspaces ·")
        s.wait_absent("Edit workspace")
        self.assertEqual(config.read_text(), before)
        # Command-line Enter stays in Neovim; the next Enter saves.
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27u:s/Written/Command/\r", "Command")
        self.assertIn("Edit workspace", s.screen.text)
        self.assertEqual(name(), "Written")
        s.send(b"\r", "Workspace saved")
        self.assertEqual(name(), "Command")
        # Neovim's own ZZ still saves.
        s.send(b"\x1be", "Edit workspace")
        s.send(b"\x1b[27uccZed\x1b[27uZZ", "Workspace saved")
        s.wait_absent("Edit workspace")
        self.assertEqual(name(), "Zed")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_alt_arrows_reorder_workspaces_among_visible_rows(self):
        config = self.root / "config.toml"
        config.write_text(
            "# mine\n[ui]\npreview=false\n\n[[workspaces]]\nname='red1'\npath='.'\n\n"
            "[[workspaces]]\nname='blue'\npath='.'\n\n"
            "[[workspaces]]\nname='red2'\npath='.'\n"
        )
        order = lambda: [b["name"] for b in tomllib.loads(config.read_text())["workspaces"]]

        def wait_order(expected):
            deadline = time.monotonic() + 5
            while order() != expected and time.monotonic() < deadline:
                s.drain(0.02)
            self.assertEqual(order(), expected)

        s = self.start("--config", str(config), implicit_root=True)
        s.send(b"gw", "red2")
        s.send(b"\x1b[B")
        wait_order(["blue", "red1", "red2"])
        s.send(b"\x1b[B")
        wait_order(["blue", "red2", "red1"])
        s.send(b"\x1b[B")  # already last: nothing moves
        s.send(b"\x1b[A")
        wait_order(["blue", "red1", "red2"])
        self.assertIn("# mine", config.read_text())
        # Filtered rows move past their visible neighbour.
        s.send(b"ired", "red2")
        s.wait_absent("blue")
        s.send(b"\x1b[B")
        wait_order(["blue", "red2", "red1"])
        # The highlight follows the moved workspace: Enter opens red1.
        s.send(b"\x1b[A")
        wait_order(["blue", "red1", "red2"])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_configured_workspaces_filter_missing_target_and_preserve_marks(self):
        marked = self.root / "marked"
        marked.touch()
        with tempfile.TemporaryDirectory(prefix="fff-workspaces-") as config_dir:
            base = Path(config_dir)
            target = base / "notes folder"
            target.mkdir()
            (target / "report.tex").touch()
            config = base / "config.toml"
            config.write_text("[[workspaces]]\nname='My notes'\npath='notes folder'\n[[workspaces]]\nname='Missing'\npath='missing'\n")
            s = self.start("--config", str(config), "--null", implicit_root=True)
            s.send(SHIFT_SPACE, b"Marks (1)")
            s.send(b"gw", b"Workspaces")
            s.send(b"iMissing\r", b"not an accessible directory")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"\x1b[27u", b"Marks (1)")
            s.wait_absent("Workspaces ·")
            s.send(b"gw", b"Workspaces")
            s.send(b"imnt\r", ("cwd", target))
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
        s = Session(self.root / "notes", str(self.root / "papers"), "--depth", "1")
        self.session = s
        lines = s.screen.text.splitlines()
        self.assertIn("Query", lines[0])
        self.assertIn("✓ complete", lines[2])
        self.assertIn("╰ depth 1 ─", s.screen.text)
        self.assertNotIn("hidden", lines[2])
        self.assertTrue(" ".join(lines[3].split()).startswith(f"@ {self.root / 'notes'} "), s.screen.text)
        self.assertTrue(lines[3].rstrip().endswith("pinned  notes  papers"), s.screen.text)
        self.assertIn("Files ·", lines[4])
        s.send(b"itopic", "❯ topic")
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
        s.send(b"\x1b[15~", " new ")  # F5 forces freshness
        self.assertIn("✓ complete", s.screen.text)
        s.send(b"i", "Query  INSERT")  # Enter while typing returns the file
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

    @unittest.skipIf(sys.platform == "darwin", "no live updates on macOS (no inotify)")
    def test_watched_directory_updates_without_waiting_for_freshness(self):
        (self.root / "old").touch()
        with tempfile.TemporaryDirectory(prefix="fff-watch-") as folder:
            config = Path(folder) / "config.toml"
            # Freshness alone would not rescan for a minute.
            config.write_text("[cache]\nfresh_seconds=60\nttl_seconds=3600\nprefetch=false\n")
            s = self.start("--depth", "1", "--config", str(config), implicit_root=True)
            (self.root / "appeared").touch()
            s.wait_ui("appeared")
            (self.root / "old").rename(self.root / "renamed")
            s.wait_ui("renamed")
            s.wait_absent(" old")
            # Writes update the size column in place.
            (self.root / "renamed").write_bytes(b"x" * 4300)
            s.wait_ui("4.2K")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipIf(sys.platform == "darwin", "no live updates on macOS (no inotify)")
    def test_deeper_folders_and_pinned_roots_update_live(self):
        (self.root / "sub" / "inner").mkdir(parents=True)
        with tempfile.TemporaryDirectory(prefix="fff-watch-") as folder:
            pinned = Path(folder) / "pinned"
            (pinned / "shelf").mkdir(parents=True)
            config = Path(folder) / "config.toml"
            config.write_text("[cache]\nfresh_seconds=60\nttl_seconds=3600\nprefetch=false\n")
            s = self.start(str(pinned), "--depth", "3", "--config", str(config))
            s.wait_ui("inner/")
            (self.root / "sub" / "inner" / "deep-arrival").touch()
            s.wait_ui("deep-arrival")
            (pinned / "shelf" / "pinned-arrival").touch()
            s.wait_ui("pinned-arrival")
            # A folder that appears is watched once the rescan lists it.
            (self.root / "sub" / "fresh").mkdir()
            s.wait_ui("fresh/")
            (self.root / "sub" / "fresh" / "inside-fresh").touch()
            s.wait_ui("inside-fresh")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_unwatched_directory_waits_for_a_rescan(self):
        with tempfile.TemporaryDirectory(prefix="fff-watch-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\nwatch=false\n[cache]\nfresh_seconds=60\nttl_seconds=3600\nprefetch=false\n")
            s = self.start("--depth", "1", "--config", str(config), implicit_root=True)
            (self.root / "appeared").touch()
            time.sleep(1)
            s.send(b"j")
            self.assertNotIn("appeared", s.screen.text)
            s.send(b"\x1b[15~", "appeared")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_mouse_drag_marks_rows_and_double_click_enters(self):
        (self.root / "alpha").mkdir()
        (self.root / "alpha" / "inside").touch()
        (self.root / "beta").touch()
        (self.root / "gamma").touch()
        s = self.start("--depth", "1", implicit_root=True)
        lines = s.screen.text.splitlines()
        def at(name):
            row = next(i for i, l in enumerate(lines) if f" {name}" in l and "@" not in l)
            return lines[row].index(name) + 1, row + 1
        (x, y), (_, y2) = at("alpha/"), at("gamma")
        s.send(f"\x1b[<0;{x};{y}M\x1b[<32;{x};{y2}M\x1b[<0;{x};{y2}m".encode(), "Marks (3)")
        click = f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode()
        s.send(click, "Marks (0)")
        s.send(click, ("cwd", self.root / "alpha"))
        s.wait_ui("inside")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_explorer_l_on_a_file_says_how_to_choose_it(self):
        (self.root / "page.html").touch()
        s = self.start("explorer", "save", "page.html", "--depth", "1",
                       implicit_root=True, output_paths=False)
        s.send(b"\x1b[27u", "NORMAL")  # save dialogs start typing a name
        s.send(b"l", "Files do not open here. To save over this one, press Ctrl+number")
        s.send(b"\x1b2", "press Ctrl+number on its row")  # Alt+2: the same
        s.send(b"q")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        # An open (attach) dialog says the same, with "choose".
        s = self.session = Session(self.root, "explorer", "open", "--depth", "1",
                                   implicit_root=True, output_paths=False)
        s.send(b"l", "Files do not open here. To choose this one, press Enter on it")
        s.send(b"q")
        self.assertEqual(s.finish(), (130, b""))

    def test_explorer_double_click_is_enter_on_the_row(self):
        (self.root / "alpha").mkdir()
        (self.root / "alpha" / "inside").touch()
        s = self.start("explorer", "open", "--depth", "1", implicit_root=True, output_paths=False)
        def double_click(name):
            lines = s.screen.text.splitlines()
            row = next(i for i, l in enumerate(lines) if f" {name}" in l and "@" not in l)
            x, y = lines[row].index(name) + 1, row + 1
            click = f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode()
            s.send(click)
            s.send(click)
        # A folder is entered (in a normal session too), a file chosen.
        double_click("alpha/")
        s.wait_ui(("cwd", self.root / "alpha"))
        s.wait_ui("inside")
        double_click("inside")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "alpha" / "inside") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_while_typing_a_name_to_save_ctrl_number_and_double_click_act_on_their_row(self):
        docs = self.root / "docs"
        (docs / "sub").mkdir(parents=True)
        (docs / "a.pdf").write_text("a")
        (docs / "other.txt").write_text("o")
        def save():
            s = self.session = Session(self.root, "explorer", "save", str(docs / "a.pdf"),
                                       implicit_root=True, output_paths=False)
            s.wait_ui("Save as a.pdf  INSERT")
            s.send(b"new.pdf", "Save as new.pdf  INSERT")
            return s
        # Ctrl+3 is Enter on other.txt: it offers to replace that file, not
        # to save under the typed name.
        s = save()
        s.send(b"\x1b[51;5u", "other.txt already exists")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(docs / "other.txt") + b"\n"))
        s.close()
        # Ctrl+1 is Enter on the folder: it goes into it.
        s = save()
        s.send(b"\x1b[49;5u", ("cwd", docs / "sub"))
        s.send(b"\x13")
        status, output = s.finish()
        self.assertEqual(status, 0)
        self.assertEqual(os.path.dirname(output.strip()), os.fsencode(docs / "sub"))
        s.close()
        # So is a double click.
        s = save()
        lines = s.screen.text.splitlines()
        row = next(i for i, l in enumerate(lines) if " other.txt" in l and "@" not in l)
        x, y = lines[row].index("other.txt") + 1, row + 1
        click = f"\x1b[<0;{x};{y}M\x1b[<0;{x};{y}m".encode()
        s.send(click)
        s.send(click, "other.txt already exists")
        s.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_enter_with_no_matches_says_so(self):
        (self.root / "file").touch()
        s = self.start()
        s.send(b"izzzz", b"No matching results")
        s.send(b"\r", 'Nothing matches "zzzz"; Esc clears the query')
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.send(b"i\x1b[57;5u", 'Nothing matches "zzzz"')  # Ctrl+9 while typing
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_buttons_appear_once_the_mouse_is_used_and_select_exits(self):
        (self.root / "only").touch()
        with tempfile.TemporaryDirectory(prefix="fff-buttons-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text('[ui]\nbuttons = "mouse"\n')
            s = self.start("--depth", "1", "--config", str(config), implicit_root=True)
            self.assertNotIn("Select & Exit", s.screen.text)
            s.send(b"\x1b[<65;5;5M", "Select & Exit")  # a wheel step
            lines = s.screen.text.splitlines()
            row = next(i for i, l in enumerate(lines) if "Select & Exit" in l)
            column = lines[row].index("Select & Exit") + 1
            s.send(f"\x1b[<0;{column};{row + 1}M\x1b[<0;{column};{row + 1}m".encode())
            self.assertEqual(s.finish(), (0, os.fsencode(self.root / "only") + b"\n"))

    def test_buttons_hide_once_the_mouse_rests(self):
        with tempfile.TemporaryDirectory(prefix="fff-buttons-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text('[ui]\nbuttons = "mouse"\nbuttons_seconds = 1\n')
            s = self.start("--depth", "1", "--config", str(config), implicit_root=True)
            s.send(b"\x1b[<35;5;5M", "Select & Exit")  # the pointer moves
            s.wait_absent("Select & Exit")
            s.send(b"\x1b[<35;6;5M", "Select & Exit")

    def test_buttons_stay_hidden_by_default(self):
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"\x1b[<65;5;5M")
        s.send(b"j")
        self.assertNotIn("Select & Exit", s.screen.text)

    def test_dropped_files_are_copied_here_after_confirmation(self):
        (self.root / "here").mkdir()
        with tempfile.TemporaryDirectory(prefix="fff-drop-") as folder:
            dropped = Path(folder) / "dropped file"
            dropped.write_text("content")
            (self.root / "here" / "mark").touch()
            s = self.start("--depth", "1", implicit_root=True)
            s.send(b"l", ("cwd", self.root / "here"))
            s.send(SHIFT_SPACE, "Marks (1)")
            s.send(b"\x1b[200~'" + os.fsencode(dropped) + b"'\x1b[201~", "Copy here")
            s.send(b"\r", "Copied")
            self.assertEqual((self.root / "here" / "dropped file").read_text(), "content")
            self.assertTrue(dropped.exists())
            self.assertIn("Marks (1)", s.screen.text)
            # While typing, the same paste is only text in the query.
            s.send(b"i", "Query  INSERT")
            s.send(b"\x1b[200~" + os.fsencode(dropped) + b"\x1b[201~", "fff-drop-")
            self.assertNotIn("Copy here", s.screen.text)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_a_drop_is_confirmed_with_the_mouse_alone(self):
        with tempfile.TemporaryDirectory(prefix="fff-drop-") as folder:
            dropped = Path(folder) / "picture.png"
            dropped.write_text("pixels")
            config = Path(folder) / "config.toml"
            config.write_text('[ui]\nbuttons = "always"\n')
            s = self.start("--depth", "1", "--config", str(config), implicit_root=True)
            s.send(b"\x1b[200~file://" + os.fsencode(dropped) + b"\r\n\x1b[201~", "Confirm")
            lines = s.screen.text.splitlines()
            row = next(i for i, l in enumerate(lines) if "Confirm" in l)
            column = lines[row].index("Confirm") + 1
            s.send(f"\x1b[<0;{column};{row + 1}M\x1b[<0;{column};{row + 1}m".encode(), "Copied")
            self.assertEqual((self.root / "picture.png").read_text(), "pixels")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    def test_clicking_a_tab_number_switches_to_it_keeping_marks(self):
        (self.root / "alpha").mkdir()
        (self.root / "alpha" / "inside").touch()
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"l", ("cwd", self.root / "alpha"))
        s.send(SHIFT_SPACE, "Marks (1)")
        lines = s.screen.text.splitlines()
        row = next(i for i, l in enumerate(lines) if "Files" in l)
        column = lines[row].index(" 2 ") + 2
        s.send(f"\x1b[<0;{column};{row + 1}M\x1b[<0;{column};{row + 1}m".encode(), ("cwd", self.root))
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_mouse_capture_can_be_turned_off(self):
        with tempfile.TemporaryDirectory(prefix="fff-mouse-") as folder:
            config = Path(folder) / "config.toml"
            config.write_text("[ui]\nmouse=false\n")
            s = self.start("--depth", "1", "--config", str(config), implicit_root=True)
            self.assertNotIn(b"\x1b[?1000h", s.ui)
            s.send(b"\x03")
            s.finish()
        s = self.start("--depth", "1", implicit_root=True)
        self.assertIn(b"\x1b[?1000h", s.ui)

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
        s = self.start()
        s.send(b"ihello world", "❯ hello world")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0ctw\x1b[27u", "❯ world")  # d hides folders in Normal mode
        # u undoes file operations; : opens the command picker, not
        # Neovim's command line.
        s.send(b"u", "Nothing to undo")
        s.send(b":", "Commands · INSERT")
        s.send(b"\x1b[27u", "Commands · NORMAL")
        s.send(b"\x1b[27u")
        s.wait_absent("Commands ·")
        self.assertIn("❯ world", s.screen.text)
        s.send(b"i", "Query  INSERT")  # Enter while typing returns the file
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "world.txt") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_location_and_workspace_filter(self):
        (self.root / "file").touch()
        with tempfile.TemporaryDirectory(prefix="fff-vim-fields-") as folder:
            target = Path(folder) / "notes"
            target.mkdir()
            (target / "note").touch()
            config = Path(folder) / "config.toml"
            config.write_text("[[workspaces]]\nname='Notes'\npath='notes'\n")
            s = self.start("--config", str(config), implicit_root=True)
            # The go-to prompt starts in Insert mode, ready for typing.
            s.send(b"g/", "Go to directory · INSERT")
            s.send(b"junk " + os.fsencode(target), b"junk")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"0dw\r", ("cwd", target))
            s.send(b"gw", "Workspaces")
            s.send(b"iJunk Notes", "❯ Junk Notes")
            s.send(b"\x1b[27u", "NORMAL")
            # n/e/d/r are chooser commands in Normal mode; other Vim keys edit.
            s.send(b"05x", "❯ Notes")
            s.send(b"\r", ("cwd", target))
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_buffer_closes_keep_query_and_validate_exclusions(self):
        (self.root / "file").touch()
        s = self.start("--exclude", "build")
        s.send(b"ifile", "file")
        # : opens the command picker, so :q! only types there.
        s.send(b"\x1b[27u:q!\r", "Commands · INSERT")
        self.assertNotIn("Using basic editing", s.screen.text)
        s.send(b"\x1b[27u", "Commands · NORMAL")
        s.send(b"\x1b[27u")
        s.wait_absent("Commands ·")
        s.send(b"gE", "Edit exclusions")
        s.send(b"ggdGifile", "file")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "Edit exclusions")
        self.assertIn("Edit exclusions", s.screen.text)
        s.send(b":q!\r", "Query  NORMAL")
        s.wait_absent("Edit exclusions")
        s.send(b"gE", "Edit exclusions")
        self.assertIn("build", s.screen.text)
        s.send(b"ggdGifile", "file")
        s.send(b"\x1b[27u:wq\r", "No matching results")
        s.wait_absent("Edit exclusions")
        s.send(b"gE", "Edit exclusions")
        s.send(b"ggdGi[", "[")
        s.send(b"\x1b[27u:wq\r", "Exclusion line 1:")
        s.wait_absent("Edit exclusions")
        self.assertIn("No matching results", s.screen.text)
        s.send(b"gE", "Edit exclusions")
        self.assertIn("file", s.screen.text)
        s.send(b":q!\r", "No matching results")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_multiline_exclusions_and_pending_find(self):
        (self.root / "main.tex").touch()
        (self.root / "build").mkdir()
        s = self.start("--exclude", "build")
        s.send(b"gE", "NORMAL")
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
            s = self.start("--no-config", environment={"HOME": home})
            self.assertIn("NORMAL", s.screen.text)
            self.assertFalse(marker.exists())
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_list_keys_and_g_passthrough(self):
        for name in ("alpha", "beta", "gamma"):
            (self.root / name).touch()
        s = self.start("--no-config")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"g", "PENDING")
        s.send(b"g", "NORMAL")
        # G = last, k = Alt+k, gg = first, j = Alt+j; the query is untouched.
        s.send(b"Gkggjj")
        s.send(b"k")
        s.send(b"i", "Query  INSERT")  # Enter while typing returns the file
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "beta") + b"\n"))
        s.close()
        s = self.session = Session(self.root, "--no-config")
        s.send(b"igamma", "❯ gamma")
        s.send(b"\x1b[27u", "NORMAL")
        # Unbound continuations reach Neovim intact: gU, g~, ge.
        s.send(b"0gUiw", "❯ GAMMA")
        s.send(b"0g~iw", "❯ gamma")
        s.send(b"$g0", "NORMAL")  # g0 reaches Neovim: the line's first column
        s.send(b"ix", "❯ xgamma")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"ctg\x1b[27u", "❯ gamma")
        s.send(b"i", "Query  INSERT")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "gamma") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_h_and_l_navigate_like_alt(self):
        directory = self.root / "folder"
        directory.mkdir()
        (directory / "file").touch()
        s = self.start("--no-config", "--depth", "1", implicit_root=True)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"l", ("cwd", directory))
        # Normal-mode navigation stays Normal, so h needs no Escape first.
        self.assertIn("Query  NORMAL", s.screen.text)
        s.send(b"h", ("cwd", self.root))
        self.assertIn("Query  NORMAL", s.screen.text)
        # i returns to Insert; typing filters as usual.
        s.send(b"ifol", "❯ fol")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_keys_in_marks_workspaces_and_config(self):
        for name in ("alpha", "beta", "gamma"):
            (self.root / name).touch()
        with tempfile.TemporaryDirectory(prefix="fff-normal-keys-") as folder:
            for name in ("one", "two", "three"):
                (Path(folder) / name).mkdir()
            config = Path(folder) / "config.toml"
            (Path(folder) / "keys.toml").write_text(
                "[normal]\ntoggle_mark='<S-Space>'\nhighlight_first='<Space>t'\nhighlight_last=['G','<Space>b']\nhighlight_next='none'\n"
            )
            config.write_text(
                "".join(f"[[workspaces]]\nname='{n}'\npath='{n}'\n" for n in ("one", "two", "three"))
            )
            s = self.start("--config", str(config), implicit_root=True)
            s.send(SHIFT_SPACE * 3, b"Marks (3)")
            s.send(b"\x1b ", "Marks (3)")
            # Marks list: last, then first via the remapped sequence;
            # Shift+Space unmarks.
            s.send(b"G" + SHIFT_SPACE, b"Marks (2)")
            s.send(b" t" + SHIFT_SPACE, b"Marks (1)")
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"gw", "Workspaces")
            # The chooser's keys are fixed: G, gg and j move there.
            s.send(b"G")
            s.send(b"gg")
            s.send(b"j\r", ("cwd", Path(folder) / "two"))
            s.send(b"\x17")
            self.assertEqual(s.finish(), (0, os.fsencode(self.root / "beta") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_normal_mode_keys_in_palette_chooser(self):
        (self.root / "file").touch()
        s = self.start("--no-config")
        s.send(b"gp", "Palettes · Enter applies · Esc reverts · NORMAL")
        # G previews and applies the last palette; gg then j select the second.
        mark = len(s.ui)
        s.send(b"G")
        s.send(b"\r", "Palette tomorrow-night applied")
        self.assertIn(b"48;2;29;31;33", s.ui[mark:])
        s.send(b"gp", "Palettes ·")
        s.send(b"ggj\r", "Palette light applied")
        # A g not followed by a binding still edits the filter inside Neovim.
        s.send(b"gp", "Palettes ·")
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
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"ifol", "❯ fol")
        s.send(b"\x1bl", ("cwd", directory))
        self.assertIn("Query  INSERT", s.screen.text)
        s.send(b"file", "❯ file")
        s.send(b"\x1bh", ("cwd", self.root))
        self.assertIn("Query  INSERT", s.screen.text)
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "❯")
        s.send(b"l", ("cwd", directory))
        self.assertIn("Query  NORMAL", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_space_marks_in_normal_and_types_in_insert(self):
        for name in ("a", "b"):
            (self.root / name).touch()
        s = self.start()
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
        s = self.start("--exclude", "build")
        s.send(b"gE", "Exclusions")
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
            config.write_text("")
            (Path(folder) / "keys.toml").write_text("[normal]\ntoggle_mark='x'\ntrash='none'\nquit='none'\n")
            s = self.start("--config", str(config))
            s.send(b"x", "Marks (1)")
            s.send(b"gE", "Edit exclusions")
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
        s = self.start("--palette", "gruvbox")
        s.send(b"gp", "Palettes ·")
        s.send(b"\r", "Palette gruvbox-dark applied")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_normal_only_and_empty_selection(self):
        (self.root / "file").write_text("original")
        s = self.start(implicit_root=True)
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
            s = self.start(implicit_root=True)
            s.send(b"i" + prefix.encode(), "❯ " + prefix)
            s.send(b"\x1b[27u" + SHIFT_SPACE * 2, "Marks (2)")
            s.send(b"\x1b[27ui" + target.name.encode(), "❯ " + target.name)
            s.send(b"\x1b[27ul", ("cwd", target))
            # With nothing to overwrite, they act at once: u undoes them.
            done = {"p": "Copied", "v": "Moved", "s": "Linked"}[key]
            s.send(key.encode(), done + " 2 entries; u undoes")
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
    def test_move_and_copy_conflicts_ask_before_overwriting(self):
        for key, verb, label in [(b"v", "Moved", "move"), (b"p", "Copied", "copy")]:
            root = self.root / label
            target = root / "target"
            target.mkdir(parents=True)
            (root / "file-a").write_text("new")
            (root / "file-b").write_text("other")
            (target / "file-a").write_text("old contents")
            s = self.session = Session(root, implicit_root=True)
            s.send(b"ifile-", "❯ file-")
            s.send(b"\x1b[27u" + SHIFT_SPACE * 2, "Marks (2)")
            s.send(b"\x1b[27uitarget", "❯ target")
            s.send(b"\x1b[27ul", ("cwd", target))
            s.send(key, "Overwrite on " + label)
            self.assertIn("file-a", s.screen.text)
            self.assertIn("12 B", s.screen.text)
            self.assertIn("Enter: Overwrite ─── Esc: Cancel", s.screen.text)
            s.send(b"\x1b[27u", label.capitalize() + " cancelled")
            self.assertEqual((target / "file-a").read_text(), "old contents")
            self.assertFalse((target / "file-b").exists())
            self.assertIn("Marks (2)", s.screen.text)
            s.send(key, "Overwrite on " + label)
            s.send(b"\r", verb + " 2 entries, overwriting file-a")
            self.assertEqual((target / "file-a").read_text(), "new")
            self.assertEqual((target / "file-b").read_text(), "other")
            self.assertEqual((root / "file-a").exists(), key == b"p")
            self.assertIn("Marks (0)", s.screen.text)
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))
            s.close()
            self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_failed_file_operations_preserve_all_selected(self):
        source = self.root / "source"
        source.write_text("keep")
        s = self.start(implicit_root=True)
        s.send(SHIFT_SPACE, "Marks (1)")
        for key, message in [
            (b"v", "already in this directory"),
            (b"p", "already in this directory"),
            (b"s", "already in this directory"),
        ]:
            # The destination is the source's current directory: collision.
            s.send(key, message)
            self.assertIn("Marks (1)", s.screen.text)
            self.assertEqual(source.read_text(), "keep")
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(source) + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_new_file_directory_and_discard(self):
        s = self.start(implicit_root=True)
        s.send(b"n", "Press f to create a file")
        s.send(b"f", "New file · INSERT")
        s.send(b"discarded", "discarded")
        s.send(b"\x1b[27u:q!\r", "Query  NORMAL")
        self.assertFalse((self.root / "discarded").exists())
        for choice, name in [(b"f", "new-file"), (b"d", "new-folder")]:
            s.send(b"n", "Press f to create a file")
            s.send(choice, "New file · INSERT" if choice == b"f" else "New directory · INSERT")
            s.send(name.encode(), name)
            s.send(b"\x1b[27u:wq\r", "Created file" if choice == b"f" else "Created directory")
            s.wait_absent("New file" if choice == b"f" else "New directory")
            self.assertTrue((self.root / name).exists())
            self.assertEqual((self.root / name).is_dir(), choice == b"d")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_rename_buffer_validation_and_mark_updates(self):
        (self.root / "alpha").write_text("a")
        (self.root / "beta").write_text("b")
        s = self.start(implicit_root=True)
        s.send(SHIFT_SPACE, "Marks (1)")
        s.send(b"r", "Rename files")
        self.assertIn("alpha", s.screen.text)
        self.assertIn("beta", s.screen.text)
        s.send(b"ggdd:wq\r", "Rename row count changed")
        self.assertTrue((self.root / "alpha").exists())
        self.assertTrue((self.root / "beta").exists())
        s.send(b"r", "Rename files")
        s.send(b"ggccrenamed", "renamed")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\x1b[27u", "Rename files")
        s.send(b":wq\r", "Renamed alpha")
        self.assertFalse((self.root / "alpha").exists())
        self.assertEqual((self.root / "renamed").read_text(), "a")
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "renamed") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_delete_current_or_selected_and_cancel(self):
        (self.root / "alpha").write_text("aaa")
        (self.root / "beta").write_bytes(b"b" * 20000)
        trash = Path(self.tmp.name + "-data") / "fff/trash"
        s = self.start(implicit_root=True)
        s.send(SHIFT_SPACE, "Marks (1)")  # alpha marked, beta highlighted
        s.send(b"x", "c: Current ─── s: Selection ─── a: All")
        self.assertIn("Move to the trash", s.screen.text)
        # x moves to the trash at once; u brings it back.
        s.send(b"c", "Moved beta to the trash; u undoes")
        self.assertFalse((self.root / "beta").exists())
        self.assertEqual((trash / "files/beta").read_bytes(), b"b" * 20000)
        info = (trash / "info/beta.trashinfo").read_text()
        self.assertIn("Path=" + str(self.root / "beta") + "\n", info)
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"u", "Undid: Moved beta to the trash")
        self.assertEqual((self.root / "beta").read_bytes(), b"b" * 20000)
        self.assertEqual(os.listdir(trash / "files") + os.listdir(trash / "info"), [])
        s.send(b"u", "Nothing to undo")
        s.send(b"xs", "Moved alpha to the trash")
        self.assertFalse((self.root / "alpha").exists())
        self.assertTrue((self.root / "beta").exists())
        self.assertIn("Marks (0)", s.screen.text)
        # X deletes permanently, after confirmation.
        s.send(b"X", "permanently delete")
        self.assertIn("19.5 KiB", s.screen.text)
        s.send(b"\x1b[27u", "Query  NORMAL")
        self.assertTrue((self.root / "beta").exists())
        s.send(b"X", "permanently delete")
        s.send(b"\r", "Deleted beta permanently")
        self.assertFalse((self.root / "beta").exists())
        self.assertEqual(os.listdir(trash / "files"), ["alpha"])
        # Nothing to bring back from a permanent deletion: u undoes the trashing before it.
        s.send(b"u", "Undid: Moved alpha to the trash")
        self.assertEqual((self.root / "alpha").read_text(), "aaa")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_deleting_unselected_current_file_keeps_other_selected_files(self):
        selected = self.root / "alpha"
        current = self.root / "beta"
        selected.write_text("selected")
        current.write_text("current")
        s = self.start(implicit_root=True)
        s.send(SHIFT_SPACE, "Marks (1)")
        s.send(b"xc", "Moved beta to the trash")
        self.assertFalse(current.exists())
        self.assertTrue(selected.exists())
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"\x17")
        self.assertEqual(s.finish(), (0, os.fsencode(selected) + b"\n"))

    def config(self, text, keys=None):
        # A folder of its own: keys.toml goes beside the configuration.
        folder = Path(self.tmp.name + "-config")
        folder.mkdir(exist_ok=True)
        self.addCleanup(shutil.rmtree, folder, True)
        config = folder / "config.toml"
        config.write_text(text)
        if keys is not None:
            (folder / "keys.toml").write_text(keys)
        return str(config)

    def trash(self):
        return Path(self.tmp.name + "-data") / "fff/trash"

    def tree(self, name, count):
        tree = self.root / name
        tree.mkdir()
        for n in range(count):
            (tree / f"f{n}").write_bytes(b"x" * 64)
        return tree

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_large_entries_ask_before_going_to_the_trash(self):
        small = self.root / "small"
        big = self.root / "big"
        small.write_text("small")
        big.write_bytes(b"b" * 5000)
        config = self.config("[trash]\nask_above = '1 KiB'\ndays = 7\n")
        s = self.start("--config", config, implicit_root=True)
        s.send(b"m", "Marks (2)")
        s.send(b"x", "Too large for the trash")
        self.assertIn("This entry is larger than 1.0 KiB", s.screen.text)
        self.assertIn("for 7 days", s.screen.text)
        self.assertIn("the other entry goes to the trash", s.screen.text)
        self.assertIn("d: Delete permanently ─── t: Trash ─── Esc: Cancel", s.screen.text)
        self.assertIn("4.9 KiB", s.screen.text)
        s.send(b"\x1b[27u", "Deletion cancelled; nothing changed")
        self.assertTrue(small.exists() and big.exists())
        self.assertIn("Marks (2)", s.screen.text)
        # t: everything goes to the trash, and comes back together.
        s.send(b"x", "Too large for the trash")
        s.send(b"t", "Moved 2 entries to the trash; u undoes")
        self.assertEqual(sorted(os.listdir(self.trash() / "files")), ["big", "small"])
        self.assertIn("Marks (0)", s.screen.text)
        s.send(b"u", "Undid: Moved 2 entries to the trash")
        self.assertEqual(big.read_bytes(), b"b" * 5000)
        self.assertEqual(small.read_text(), "small")
        # d: the large entry is deleted for good, the small one trashed.
        s.send(b"m", "Marks (2)")
        s.send(b"x", "Too large for the trash")
        s.send(b"d", "Moved 1 entry to the trash and deleted 1 entry permanently")
        self.assertFalse(big.exists() or small.exists())
        self.assertEqual(os.listdir(self.trash() / "files"), ["small"])
        s.send(b"u", "1 entry deleted permanently cannot come back")
        self.assertIn("Undid: Moved 1 entry to the trash and deleted 1 entry permanently", s.screen.text)
        self.assertEqual(small.read_text(), "small")
        self.assertFalse(big.exists())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_the_trash_is_a_folder_where_r_restores_entries(self):
        notes = self.root / "notes.md"
        notes.write_text("old")
        s = self.start(implicit_root=True)
        s.send(b"x", "Moved notes.md to the trash; u undoes")
        # Restoring is for the trash alone.
        s.send(b"R", "R restores entries in the trash; gt opens it")
        # Renamed in the trash, it still knows where it came from; a new
        # notes.md has taken its place meanwhile.
        files = self.trash() / "files"
        os.rename(files / "notes.md", files / "renamed.md")
        notes.write_text("new")
        s.send(b"gt", ("cwd", files))
        self.assertIn("trash · R restores", s.screen.text)
        s.send(b"R", "already exists in")
        self.assertIn("Enter: Overwrite ─── k: Keep both ─── Esc: Cancel", s.screen.text)
        s.send(b"\x1b[27u", "Restore cancelled; nothing changed")
        s.send(b"R", "already exists in")
        s.send(b"k", "Restored notes (1).md to")
        self.assertEqual((self.root / "notes (1).md").read_text(), "old")
        self.assertEqual(notes.read_text(), "new")
        self.assertEqual(os.listdir(files), [])
        # Undo puts it back in the trash, still restorable.
        s.send(b"u", "Undid: Restored notes (1).md")
        self.assertFalse((self.root / "notes (1).md").exists())
        s.send(b"R", "already exists in")
        s.send(b"\r", "Restored notes.md to")
        self.assertEqual(notes.read_text(), "old")
        self.assertEqual(os.listdir(files), ["notes.md"])
        self.assertEqual((files / "notes.md").read_text(), "new")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_preview_scrolling_needs_the_pane(self):
        (self.root / "long.txt").write_text("".join(f"line {n:03}\n" for n in range(1, 201)))
        s = self.start(implicit_root=True)
        ctrl_j, ctrl_k = b"\x1b[106;5u", b"\x1b[107;5u"
        s.send(b"", "line 001")
        s.send(ctrl_j)
        s.wait_absent("line 001")
        self.assertIn("long.txt", s.screen.text)
        s.send(ctrl_k, "line 001")
        # Without the pane, it says how to open it.
        s.send(b"P")
        s.wait_absent("line 001")
        s.send(ctrl_k, "No preview pane to scroll; P opens it")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_without_the_trash_x_confirms_a_permanent_deletion(self):
        (self.root / "alpha").write_text("a")
        config = self.config("[trash]\nenabled = false\n")
        s = self.start("--config", config, implicit_root=True)
        s.send(b"x", "Delete permanently")
        s.send(b"\r", "Deleted alpha permanently")
        self.assertFalse((self.root / "alpha").exists())
        self.assertFalse(self.trash().exists())
        s.send(b"u", "Nothing to undo")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_u_undoes_moves_renames_copies_and_new_entries_newest_first(self):
        (self.root / "alpha").write_text("a")
        (self.root / "folder").mkdir()
        target = self.root / "target"
        target.mkdir()
        s = self.start(implicit_root=True)
        s.send(b"ialpha", "❯ alpha")
        s.send(b"\x1b[27u" + SHIFT_SPACE, "Marks (1)")
        s.send(b"\x1b[27uitarget", "❯ target")
        s.send(b"\x1b[27ul", ("cwd", target))
        s.send(b"p", "Copied alpha; u undoes")
        s.send(b"h", ("cwd", self.root))
        s.send(b"ialpha", "❯ alpha")
        s.send(b"\x1b[27ur", "Rename files")
        s.send(b"ccrenamed\x1b[27u:wq\r", "Renamed alpha")
        s.send(b"n", "Press f to create a file")
        s.send(b"f", "New file · INSERT")
        s.send(b"fresh\x1b[27u:wq\r", "Created file fresh")
        self.assertEqual(sorted(os.listdir(self.root)), ["folder", "fresh", "renamed", "target"])
        s.send(b"u", "Undid: Created file fresh")
        self.assertFalse((self.root / "fresh").exists())
        s.send(b"u", "Undid: Renamed alpha")
        self.assertEqual((self.root / "alpha").read_text(), "a")
        self.assertFalse((self.root / "renamed").exists())
        s.send(b"u", "Undid: Copied alpha")
        self.assertEqual(os.listdir(target), [])
        self.assertEqual(sorted(os.listdir(self.trash() / "files")), ["alpha", "fresh"])
        # The copy took alpha's mark as it started; undoing it gives it back.
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"u", "Nothing to undo")
        s.send(b"\x1b[27u\x1b[27u\x1b[27u", "Marks (0)")
        # A move is undone too, but never out from under the browsing directory.
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"ifolder", "❯ folder")
        s.send(b"\x1b[27u" + SHIFT_SPACE, "Marks (1)")
        s.send(b"\x1b[27uitarget", "❯ target")
        s.send(b"\x1b[27ul", ("cwd", target))
        s.send(b"v", "Moved folder")
        s.send(b"ifolder", "❯ folder")
        s.send(b"\x1b[27ul", ("cwd", target / "folder"))
        s.send(b"u", "Cannot undo: it would move the current directory")
        s.send(b"h", ("cwd", target))
        s.send(b"u", "Undid: Moved folder")
        self.assertTrue((self.root / "folder").is_dir())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_a_rebound_undo_key_leaves_u_to_neovim(self):
        (self.root / "alpha").write_text("a")
        config = self.config("", keys="[normal]\nundo = 'U'\n")
        s = self.start("--config", config, implicit_root=True)
        s.send(b"x", "Moved alpha to the trash; U undoes")
        s.send(b"ihello", "❯ hello")
        s.send(b"\x1b[27uu")
        s.wait_absent("❯ hello")
        self.assertFalse((self.root / "alpha").exists())
        s.send(b"U", "Undid: Moved alpha to the trash")
        self.assertTrue((self.root / "alpha").exists())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_old_trash_entries_are_purged_at_startup(self):
        trash = self.trash()
        (trash / "files/old").mkdir(parents=True)
        (trash / "files/old/inner").write_text("old")
        (trash / "files/recent").write_text("recent")
        (trash / "files/undated").write_text("?")
        (trash / "info").mkdir()
        (trash / "info/old.trashinfo").write_text("[Trash Info]\nPath=/x/old\nDeletionDate=2000-01-01T00:00:00\n")
        recent = time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(time.time() - 3 * 86400))
        (trash / "info/recent.trashinfo").write_text(f"[Trash Info]\nPath=/x/recent\nDeletionDate={recent}\n")
        (trash / "info/undated.trashinfo").write_text("[Trash Info]\nPath=/x/undated\n")
        s = self.start(implicit_root=True)
        deadline = time.monotonic() + 5
        while (trash / "files/old").exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertEqual(sorted(os.listdir(trash / "files")), ["recent", "undated"])
        self.assertEqual(sorted(os.listdir(trash / "info")), ["recent.trashinfo", "undated.trashinfo"])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))
        # A shorter setting purges sooner.
        s = self.session = Session(self.root, "--config", self.config("[trash]\ndays = 2\n"), implicit_root=True)
        deadline = time.monotonic() + 5
        while (trash / "files/recent").exists() and time.monotonic() < deadline:
            time.sleep(0.02)
        self.assertEqual(os.listdir(trash / "files"), ["undated"])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_long_operations_run_in_the_jobs_list_and_stop_unchanged(self):
        # Large enough that checking it takes a while even on tmpfs.
        tree = self.tree("tree", 80000)
        target = self.root / "target"
        target.mkdir()
        s = self.start(implicit_root=True)

        def copy_tree():
            s.send(b"itree", "❯ tree")
            s.send(b"\x1b[27u" + SHIFT_SPACE, "Marks (1)")
            s.send(b"\x1b[27uitarget", "❯ target")
            s.send(b"\x1b[27ul", ("cwd", target))
            s.send(b"p", "Copying tree")
            s.wait_ui(" of 4.9 MiB · ", timeout=10)
            # It takes its marks as it starts, and runs in the jobs list.
            self.assertIn("Jobs (1)", s.screen.text)
            self.assertIn("Marks (0)", s.screen.text)

        # A copy stopped while it is staged leaves nothing behind; browsing
        # goes on meanwhile.
        copy_tree()
        self.assertRegex(s.screen.text, r"[\d.]+ (B|KiB|MiB) of 4\.9 MiB · [\d.]+ (B|KiB|MiB)/s · \d+ s")
        # x in the jobs list stops it and undoes it, leaving the marks alone.
        s.send(b"J", "x: Stop and undo")
        s.send(b"x", "Stopped copying tree; nothing changed")
        s.wait_absent("Jobs (")
        self.assertEqual(os.listdir(target), [])
        self.assertIn("Marks (0)", s.screen.text)
        self.assertIsNone(s.status)
        # u does the same to the latest operation, and marks its entries again.
        s.send(b"h", ("cwd", self.root))
        copy_tree()
        s.send(b"u", "Stopped copying tree; nothing changed")
        s.wait_absent("Jobs (")
        self.assertEqual(os.listdir(target), [])
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"\x1b[27u\x1b[27u\x1b[27u", "Marks (0)")
        # Checking a large selection before a confirmation shows its sheet,
        # and Ctrl+c stops it at once.
        s.send(b"h", ("cwd", self.root))
        s.send(b"itree", "❯ tree")
        s.send(b"\x1b[27ux", "Moving tree to the trash")
        s.wait_ui("checking the entries")
        self.assertIn("Ctrl+c: Stop", s.screen.text)
        s.send(b"\x03", "Stopped moving tree to the trash; nothing changed")
        self.assertEqual(len(os.listdir(tree)), 80000)
        self.assertEqual(os.listdir(self.trash() / "files") if (self.trash() / "files").exists() else [], [])
        # A confirmed deletion runs in the jobs list: leaving asks first (Esc
        # stays), and it stops before anything changed.
        s.send(b"X", "Delete permanently")
        s.send(b"\r", "Deleting tree")
        s.wait_ui("checking the entries")
        s.send(b"q", "Quit stops a running operation")
        s.send(b"\x1b[27u")
        s.wait_absent("Quit stops")
        s.send(b"J", "x: Stop and undo")
        s.send(b"x", "Stopped deleting tree; nothing changed")
        self.assertEqual(len(os.listdir(tree)), 80000)
        self.assertIsNone(s.status)
        # Left alone, it finishes, counting entries. Meanwhile what collides
        # with it waits, and the folder it deletes cannot be entered.
        s.send(b"X", "Delete permanently")
        s.send(b"\r", "Deleting tree")
        s.wait_ui(" of 80001 entries · ", timeout=15)
        s.send(b"x", "Deleting tree uses tree; wait for it to finish")
        s.send(b"l", "tree is being moved or deleted")
        s.wait_ui("Deleted tree permanently", timeout=30)
        self.assertFalse(tree.exists())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_closing_the_terminal_lets_a_running_copy_finish(self):
        tree = self.tree("tree", 80000)
        target = self.root / "target"
        target.mkdir()
        s = self.start(implicit_root=True)
        s.send(b"itree", "❯ tree")
        s.send(b"\x1b[27u" + SHIFT_SPACE, "Marks (1)")
        s.send(b"\x1b[27uitarget", "❯ target")
        s.send(b"\x1b[27ul", ("cwd", target))
        s.send(b"p", "Copying tree")
        s.wait_ui(" of 4.9 MiB · ", timeout=10)
        # The terminal goes away mid-copy: fff gets SIGHUP and its screen is
        # gone, yet the copy is published whole before fff ends.
        self.assertEqual(self.hang_up(s, timeout=60), -signal.SIGHUP)
        self.assertEqual(os.listdir(target), ["tree"])
        self.assertEqual(len(os.listdir(target / "tree")), 80000)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_killed_mid_operation_nothing_is_half_there(self):
        # SIGKILL cannot be held: whenever it lands, every entry is either
        # whole under its name or not there, and what was left in a staging
        # folder is pointed out by the next session there.
        tree = self.tree("tree", 20000)
        target = self.root / "target"
        target.mkdir()
        for delay in (0.05, 0.4, 1.0, 2.5):
            s = self.start(implicit_root=True)
            s.send(b"itree", "❯ tree")
            s.send(b"\x1b[27u" + SHIFT_SPACE, "Marks (1)")
            s.send(b"\x1b[27uitarget", "❯ target")
            s.send(b"\x1b[27ul", ("cwd", target))
            s.send(b"p")
            time.sleep(delay)
            s.kill()
            self.assertEqual(len(os.listdir(tree)), 20000)
            names = os.listdir(target)
            if "tree" in names:
                self.assertEqual(len(os.listdir(target / "tree")), 20000, delay)
                self.assertEqual((target / "tree" / "f19999").read_bytes(), b"x" * 64)
            left = [n for n in names if n.startswith(".fff-stage-")]
            self.assertEqual(sorted(set(names) - set(left)), ["tree"] if "tree" in names else [], delay)
            if left:
                s = self.start(implicit_root=True, cwd=target)
                s.wait_ui("An interrupted operation left data in .fff-stage-")
                s.send(b"\x03")
                s.finish()
                for name in left:
                    shutil.rmtree(target / name)
            if "tree" in names:
                shutil.rmtree(target / "tree")
        # A permanent deletion killed partway leaves the tree whole under its
        # name or gone from it, never partly deleted there.
        for delay in (0.3, 1.5):
            victim = self.tree(f"victim{delay}", 20000)
            s = self.start(implicit_root=True)
            s.send(f"ivictim{delay}".encode(), f"❯ victim{delay}")
            s.send(b"\x1b[27uX", "Delete permanently")
            s.send(b"\r")
            time.sleep(delay)
            s.kill()
            if victim.exists():
                self.assertEqual(len(os.listdir(victim)), 20000, delay)

    def hang_up(self, s, timeout=5):
        """Closes the terminal under `s` and returns how fff ended."""
        os.close(s.master)
        s.master = os.open(os.devnull, os.O_RDWR)
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            pid, status = os.waitpid(s.pid, os.WNOHANG)
            if pid:
                s.status = os.waitstatus_to_exitcode(status)
                return s.status
            time.sleep(0.02)
        raise AssertionError("fff kept running after its terminal closed")

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_fff_ends_when_its_terminal_closes_even_under_nohup(self):
        s = self.start(hangup=False)
        # The session fails on the lost terminal; reading it never spins.
        self.assertEqual(self.hang_up(s), 1)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_file_operations_extract_empty_and_explicit_destination(self):
        import zipfile
        archive = self.root / "input.zip"
        with zipfile.ZipFile(archive, "w") as z:
            z.writestr("unpacked", "payload")
        s = self.start(implicit_root=True)
        s.send(b"E", "empty to extract beside the archive")
        s.send(b":wq\r", "Extracted input.zip beside it")
        self.assertEqual((self.root / "unpacked").read_text(), "payload")
        # Filter ensures extraction continues to target the archive alone.
        s.send(b"iinput.zip", "❯ input.zip")
        s.send(b"\x1b[27uE", "Extract archive")
        s.send(b"ioutput", "output")
        s.send(b"\x1b[27u:wq\r", "Extracted input.zip into " + str(self.root / "output"))
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
        s = self.start(implicit_root=True)
        for name in [directory.name, plain.name, corrupt.name]:
            s.send(b"i" + name.encode(), "❯ " + name)
            s.send(b"\x1b[27uE", "Cannot extract ")
            self.assertNotIn("Extract archive", s.screen.text)
            self.assertIn("Query  NORMAL", s.screen.text)
            s.send(b"\x1b[27u", "❯")
        s.send(b"imissing", "No matching results")
        s.send(b"\x1b[27uE", "Select an extractable archive first")
        self.assertNotIn("Extract archive", s.screen.text)
        self.assertEqual(sorted(p.name for p in self.root.iterdir()),
                         ["corrupt.zip", "folder", "not-an-archive.zip"])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_list_steps_wrap_and_rename_starts_on_the_highlighted_row(self):
        for name in ["alpha", "beta", "gamma"]:
            (self.root / name).write_text(name)
        s = self.start(implicit_root=True)
        s.send(b"k", "› 3/0")  # up from the first row wraps to the last
        s.send(b"j", "›   1")  # and down from the last wraps to the first
        s.send(b"j", "›   2")
        s.send(b"r", "Rename files · NORMAL")
        # The cursor starts at the end of the highlighted row, ready to append.
        s.send(b"a2", "beta2")
        s.send(b"\x1b[27u", "Rename files · NORMAL")
        s.send(b"ZZ", "Renamed beta")
        self.assertEqual((self.root / "beta2").read_text(), "beta")
        self.assertTrue((self.root / "alpha").exists() and (self.root / "gamma").exists())
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_zz_applies_an_unchanged_buffer_and_zq_discards(self):
        import zipfile
        with zipfile.ZipFile(self.root / "input.zip", "w") as z:
            z.writestr("unpacked", "payload")
        s = self.start(implicit_root=True)
        s.send(b"E", "Extract archive · NORMAL")
        s.send(b"ZQ", "Query  NORMAL")
        s.wait_absent("Extract archive")
        self.assertFalse((self.root / "unpacked").exists())
        s.send(b"E", "Extract archive · NORMAL")
        s.send(b"ZZ", "Extracted input.zip beside it")
        self.assertEqual((self.root / "unpacked").read_text(), "payload")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_extract_asks_for_scope_only_when_ambiguous_and_refuses_non_archives(self):
        import zipfile
        for name in ["a.zip", "b.zip"]:
            with zipfile.ZipFile(self.root / name, "w") as z:
                z.writestr(name + ".txt", name)
        (self.root / "notes.txt").write_text("not an archive")
        s = self.start(implicit_root=True)
        s.send(b"ia.zip", "❯ a.zip")
        s.send(b"\x1b[27u" + SHIFT_SPACE, "Marks (1)")
        s.send(b"\x1b[27uccb.zip", "❯ b.zip")
        s.send(b"\x1b[27u" + SHIFT_SPACE, "Marks (2)")
        s.send(b"\x1b[27uccnotes", "❯ notes")
        # notes.txt is current but unmarked while archives are marked: ask.
        s.send(b"\x1b[27uE", "c: Current ─── s: Selection ─── a: All")
        s.send(b"a", "Nothing extracted; 1 of 3 cannot be extracted: notes.txt")
        self.assertFalse((self.root / "a.zip.txt").exists())
        s.send(b"E", "c: Current")
        s.send(b"s", "Extract archives · NORMAL")
        s.send(b"ZZ", "Extracted 2 entries beside each archive")
        self.assertEqual((self.root / "a.zip.txt").read_text(), "a.zip")
        self.assertEqual((self.root / "b.zip.txt").read_text(), "b.zip")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_default_mode_chooses_a_directory_instead_of_printing_paths(self):
        sub = self.root / "sub"
        sub.mkdir()
        (sub / "file").write_text("x")
        cd_file = self.root / "cd"
        for keys, expected in [(b"q", None), (b"\x1bl!", sub)]:
            cd_file.write_bytes(b"")
            s = self.start("--cd-file", str(cd_file), implicit_root=True, output_paths=False)
            s.send(keys)
            self.assertEqual(s.finish(), (0, b""))
            self.assertEqual(cd_file.read_bytes(), os.fsencode(expected) if expected else b"")
            s.close()
            self.session = None
        # Normal-mode Enter on a directory leaves into it; without --cd-file
        # it is printed.
        s = self.start(implicit_root=True, output_paths=False)
        s.send(b"isub", "❯ sub")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(sub) + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_directories_remember_their_cursor_across_sessions(self):
        for folder in ["one", "two"]:
            (self.root / folder).mkdir()
            for name in ["a", "b", "c"]:
                (self.root / folder / name).write_text(name)
        s = self.start("--depth", "1", implicit_root=True, output_paths=False)
        def highlighted():
            return next(l for l in s.screen.text.splitlines() if "›" in l and "❯" not in l)
        s.send(b"j", "› 2/0")
        self.assertIn("two/", highlighted())
        s.send(b"l", ("cwd", self.root / "two"))
        s.send(b"jj", "› 3/0")
        # Going up highlights the folder we came from...
        s.send(b"h", ("cwd", self.root))
        s.wait_ui("› 2/0")
        self.assertIn("two/", highlighted())
        # ...and coming back restores the cursor left there.
        s.send(b"l", ("cwd", self.root / "two"))
        s.wait_ui("› 3/0")
        s.send(b"q")
        self.assertEqual(s.finish(), (0, b""))
        s.close()
        # A new session starts where it is launched, not where the last ended.
        s = self.session = Session(self.root, "--depth", "1",
                                   implicit_root=True, output_paths=False)
        s.wait_ui(("cwd", self.root))
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)
        s.close()
        # --browse starts beside a file, highlighted.
        s = self.session = Session(self.root, "--depth", "1", "--browse",
                                   str(self.root / "one" / "b"),
                                   implicit_root=True, output_paths=False)
        s.wait_ui(("cwd", self.root / "one"))
        s.wait_ui("Files · 3")
        self.assertRegex(highlighted(), r"›\s+2\s.*\sb\s")
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)
        s.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_explorer_answers_open_and_save_dialogs(self):
        (self.root / "docs").mkdir()
        (self.root / "docs" / "a.pdf").write_text("a")
        (self.root / "b.txt").write_text("b")
        # Normal-mode Enter enters a folder, then chooses a file.
        s = self.start("explorer", "open", implicit_root=True, output_paths=False)
        s.wait_ui("Press Enter on a file to choose it.")
        s.wait_ui("F1 keys  q quit")
        s.send(b"\r", ("cwd", self.root / "docs"))
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "docs" / "a.pdf") + b"\n"))
        s.close()
        # One answer: Enter takes that file even when other files are marked
        # after it.
        s = self.session = Session(self.root, "explorer", "open", "--depth", "1",
                                   implicit_root=True, output_paths=False)
        s.send(b"j", "› 2/0")
        s.send(SHIFT_SPACE, "Marks (1)")
        s.send(b"k")
        s.send(b"\r", ("cwd", self.root / "docs"))
        s.send(SHIFT_SPACE, "Marks (2)")
        s.send(b"h", ("cwd", self.root))
        s.send(b"j", "  b.txt  1 B")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "b.txt") + b"\n"))
        s.close()
        # q cancels instead of returning the launch directory.
        s = self.session = Session(self.root, "explorer", "open", implicit_root=True,
                                   output_paths=False)
        s.send(b"q")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        # Saving starts beside the suggestion, whose name the query box title
        # shows; a taken name asks first: k keeps both, Enter replaces.
        docs = self.root / "docs"
        def save(*args):
            s = self.session = Session(self.root, "explorer", "save", str(docs / "a.pdf"),
                                       *args, implicit_root=True, output_paths=False)
            s.wait_ui(("cwd", docs))
            # Save dialogs start ready to type a name.
            s.wait_ui("Save as a.pdf  INSERT")
            s.wait_ui("Type a custom name, then press Enter or Ctrl+s to save it here.")
            s.send(b"\x1b[27u", "Save as a.pdf  NORMAL")
            s.wait_ui("Press ! or Ctrl+s to save a.pdf here. Press i to type a custom name.")
            return s
        s = save()
        s.send(b"!", "Keep both")
        s.send(b"k")
        self.assertEqual(s.finish(), (0, os.fsencode(docs / "a (1).pdf") + b"\n"))
        s.close()
        s = save()
        s.send(b"\r", "Replace")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(docs / "a.pdf") + b"\n"))
        s.close()
        # Enter saves under the name in the query box whatever is
        # highlighted: on a folder it does not go into it.
        (docs / "sub").mkdir()
        s = save()
        s.send(b"gg", "sub  0 B  0 entries")  # the folder is highlighted
        s.send(b"\r", "Replace")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(docs / "a.pdf") + b"\n"))
        s.close()
        (docs / "sub").rmdir()
        # A name changing the suggested extension is warned about first;
        # saving again takes it.
        s = save()
        s.send(b"inew.txt", "Save as new.txt  INSERT")
        s.send(b"\r", "new.txt ends in .txt, unlike the suggested a.pdf")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(docs / "new.txt") + b"\n"))
        s.close()
        # Escape goes back to choosing.
        s = save()
        s.send(b"!", "Keep both")
        s.send(b"\x1b[27u")
        s.wait_absent("Keep both")
        s.send(b"q")
        self.assertEqual(s.finish(), (130, b""))
        s.close()
        # A typed name: Ctrl+s saves it from Insert mode, and so does Enter
        # when the name matches nothing.
        for key in [b"\x13", b"\r"]:
            s = save()
            s.send(b"inew.pdf", "Save as new.pdf  INSERT")
            # The name does not filter the folder: a.pdf stays listed.
            self.assertIn("Files · 1", s.screen.text)
            self.assertIn("F1 keys  Ctrl+c quit", s.screen.text)
            s.send(key)
            self.assertEqual(s.finish(), (0, os.fsencode(docs / "new.pdf") + b"\n"))
            s.close()
        # Dialogs search contents too; the name saved is the names query's,
        # never the contents query's.
        (docs / "notes.txt").write_text("a canard here\n")
        s = save()
        s.send(b"inew.pdf", "Save as new.pdf  INSERT")
        s.send(b"\t", "Contents  INSERT")
        self.assertIn("Save as new.pdf", s.screen.text)
        s.send(b"canard", "❯ canard")
        s.wait_ui("Matches · 1")
        s.send(b"\x13")
        self.assertEqual(s.finish(), (0, os.fsencode(docs / "new.pdf") + b"\n"))
        s.close()
        # Enter on a match chooses its file.
        s = self.session = Session(self.root, "explorer", "open", "docs",
                                   implicit_root=True, output_paths=False)
        s.send(b"\t", "Contents  NORMAL")
        s.send(b"icanard", "❯ canard")
        s.wait_ui("Matches · 1")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(docs / "notes.txt") + b"\n"))
        s.close()
        (docs / "notes.txt").unlink()
        # A folder dialog takes the folder being browsed.
        s = self.session = Session(self.root, "explorer", "folder", "docs",
                                   implicit_root=True, output_paths=False)
        s.wait_ui("Press ! or Ctrl+s to choose the folder you are in.")
        s.send(b"!")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "docs") + b"\n"))
        s.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_e_opens_editor_without_review_and_explains_a_bad_editor(self):
        (self.root / "alpha").write_text("a")
        log = self.root / "log"
        env = self.run_environment("myeditor", f'printf "%s\\n" "$@" > "{log}"\n')
        s = self.start("--no-config", implicit_root=True,
                       environment={**env, "EDITOR": "myeditor -p"})
        s.send(b"e", "myeditor finished")
        self.assertEqual(log.read_text().splitlines(), ["-p", str(self.root / "alpha")])
        s.send(b"\x03")
        s.finish()
        s.close()
        for value, message in [("", "$EDITOR is empty"), ("myeditr", "$EDITOR: myeditr: command not found; did you mean myeditor?")]:
            s = self.session = Session(self.root, "--no-config", implicit_root=True,
                                       environment={**env, "EDITOR": value})
            s.send(b"e", message)
            s.send(b"\x03")
            s.finish()
            s.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_gc_edits_the_configuration_and_falls_back_to_its_backup(self):
        (self.root / "alpha").write_text("a")
        config = self.root / "config.toml"
        config.write_text("[ui]\npalette='nord'\n")
        backup = self.root / "config.toml.bak"
        edited = self.root / "edited.toml"
        # The editor replaces the configuration with the prepared edit.
        env = self.run_environment("myeditor", f'cp "{edited}" "$1"\n')
        s = self.start("--config", str(config), implicit_root=True,
                       environment={**env, "EDITOR": "myeditor"})
        edited.write_text("[ui]\npalette = \n")
        s.send(b"gc", "config.toml has errors, so fff reads config.toml.bak; F2 shows them")
        s.wait_ui("config errors")
        # F2 opens the Session tab on the errors, F1 the key guide; each
        # closes its own tab.
        s.send(b"\x1bOQ", "TOML parse error at line 2")
        s.send(b"\x1bOP", "showing what works in fff pick")
        s.send(b"\x1bOP", "config errors")
        s.wait_absent("showing what works")
        self.assertEqual(backup.read_text(), "[ui]\npalette='nord'\n")
        # Editing again keeps the backup that loads, and a fixed file
        # replaces it, with its keys in effect at once.
        edited.write_text("[ui]\npalette='nord'\n")
        (self.root / "keys.toml").write_text("[normal]\nquit = 'Q'\n")
        s.send(b"gc", "Configuration reloaded")
        s.wait_absent("config errors")
        self.assertFalse(backup.exists())
        s.send(b"Q")
        self.assertEqual(s.finish(), (0, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_go_to_suggests_zoxide_folders_and_ctrl_o_i_walk_history(self):
        far = Path(self.tmp.name + "-far") / "projects" / "fff"
        far.mkdir(parents=True)
        self.addCleanup(shutil.rmtree, self.tmp.name + "-far", True)
        (self.root / "sub").mkdir()
        env = self.run_environment("zoxide", f'echo "{far}"\necho "{far.parent}"\n')
        s = self.start("--no-config", implicit_root=True,
                       environment=env, output_paths=False)
        s.send(b"g/", "Go to directory")
        self.assertIn("Enter: Go", s.screen.text)
        s.wait_ui(str(far.parent))
        # zoxide-style words in order pick a frequent folder.
        s.send(b"proj ff", "❯ proj ff")
        s.send(b"\r", ("cwd", far))
        s.send(b"\x1bg", "Go to directory")
        s.send(b"\x1b[B\x1b[B\r", ("cwd", far.parent))
        # Ctrl+o goes back through the visited folders; Ctrl+i forward again.
        s.send(b"\x1b[111;5u", ("cwd", far))
        s.send(b"\x1b[111;5u", ("cwd", self.root))
        s.send(b"\x1b[111;5u", "No earlier directory")
        s.send(b"\x1b[105;5u", ("cwd", far))
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_tabs_keep_directories_and_history_and_share_marks(self):
        (self.root / "left").mkdir()
        (self.root / "left" / "file").touch()
        (self.root / "right").mkdir()
        s = self.start("--depth", "1", implicit_root=True)
        s.send(b"ileft", "❯ left")
        s.send(b"\r", ("cwd", self.root / "left"))
        s.send(b"\x1b[27u", "NORMAL")
        s.send(SHIFT_SPACE, "Marks (1)")
        # Up in Normal mode: tab 2, still in the launch directory.
        s.send(b"\x1b[A", ("cwd", self.root))
        s.send(b"\x1b[111;5u", "No earlier directory")
        s.send(b"iright\r", ("cwd", self.root / "right"))
        # Alt+Down works in Insert mode: back to tab 1 where it was left.
        s.send(b"\x1b[1;3B", ("cwd", self.root / "left"))
        self.assertIn("Marks (1)", s.screen.text)
        s.send(b"\x1b[111;5u", ("cwd", self.root))
        s.send(b"\x1b[1;3B", ("cwd", self.root))
        s.send(b"\x1b[1;3B", ("cwd", self.root / "right"))
        s.send(b"\x1b[27u")
        s.send(b"!")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "right") + b"\n"))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_rebound_operations_and_flag_completion(self):
        (self.root / "alpha").write_text("a")
        log = self.root / "log"
        env = self.run_environment(
            "tool", f'case "$1" in --help) echo "  -x, --and-exit  stop";; *) printf "%s\\n" "$@" > "{log}";; esac\n')
        config = self.config("", keys="[normal]\ntrash = 'D'\nedit_files = 'none'\n")
        s = self.start("--config", config, implicit_root=True, environment=env)
        s.send(SHIFT_SPACE, "Marks (1)")
        s.send(b"x")  # no longer delete: Neovim gets it
        s.send(b"D", "Moved alpha to the trash")
        s.send(b"u", "Undid: Moved alpha to the trash")
        s.send(b"gx", "Run · INSERT")
        s.send(b"tool --and", "❯ tool --and")
        s.send(b"\t", "❯ tool --and-exit ")
        s.send(b"\r", "Run here")
        s.send(b"\r", "tool finished")
        self.assertEqual(log.read_text().splitlines(), ["--and-exit", str(self.root / "alpha")])
        s.send(b"\x03")
        self.assertEqual(s.finish()[0], 130)

    def deep_tree(self):
        # top.txt, folder/, and a deep result sub/more/a.pdf beside 0first.txt.
        more = self.root / "sub" / "more"
        more.mkdir(parents=True)
        (more / "0first.txt").touch()
        (more / "a.pdf").touch()
        (self.root / "folder").mkdir()
        (self.root / "top.txt").touch()
        return more

    def highlighted(self, s):
        return next((l for l in s.screen.text.splitlines() if "›" in l and "❯" not in l), "")

    def wait_highlight(self, s, name):
        deadline = time.monotonic() + 3
        while name not in self.highlighted(s) and time.monotonic() < deadline:
            if select.select([s.master], [], [], 0.05)[0]:
                chunk = os.read(s.master, 65536)
                s.take(chunk)
        self.assertIn(name, self.highlighted(s), s.screen.text)

    def test_normal_enter_prints_a_file_or_directory_and_ctrl_digit_matches_it(self):
        more = self.deep_tree()
        for query, args, expected in [
            (b"top.txt", (), os.fsencode(self.root / "top.txt") + b"\n"),
            (b"a.pdf", (), os.fsencode(more / "a.pdf") + b"\n"),
            (b"a.pdf", ("--relative", "--null"), b"sub/more/a.pdf\0"),
            (b"folder", (), os.fsencode(self.root / "folder") + b"\n"),
            (b"zzzz", (), os.fsencode(self.root) + b"\n"),  # nothing listed
        ]:
            # Ctrl+1 is Enter on the first row.
            for key in [b"\r", b"\x1b[49;5u"]:
                with self.subTest(query=query, args=args, key=key):
                    s = self.start("--depth", "3", *args, implicit_root=True)
                    s.send(SHIFT_SPACE, "Marks (1)")  # marks are ignored
                    s.send(b"i" + query, "❯ " + query.decode())
                    s.send(b"\x1b[27u", "Query  NORMAL")
                    s.send(key)
                    self.assertEqual(s.finish(), (0, expected))
                    s.close()
                    self.session = None

    def test_normal_enter_in_default_mode_opens_a_file_and_leaves(self):
        more = self.deep_tree()
        cd_file = self.root / "cd"
        edited = Path(self.tmp.name + "-edited.log")
        self.addCleanup(lambda: edited.unlink(missing_ok=True))
        env, config, log = self.opener_setup(rules=(
            "[[open.rules]]\ncommand = 'viewer'\nextensions = ['pdf']\n"
            "[[open.rules]]\ncommand = 'texted'\nextensions = ['txt']\nterminal = true\n"))
        env = {**env, **self.run_environment(
            "texted", f'{{ pwd; printf "%s\\n" "$@"; }} > "{edited}"; exit 7\n')}
        def enter(query):
            cd_file.write_bytes(b"")
            s = self.start("--depth", "3", "--cd-file", str(cd_file), "--config",
                           str(config), implicit_root=True, output_paths=False, environment=env)
            s.send(b"i" + query, "❯ " + query.decode())
            s.send(b"\x1b[27u", "Query  NORMAL")
            s.send(b"\r")
            return s
        # A directory: fff leaves into it.
        s = enter(b"folder")
        self.assertEqual(s.finish(), (0, b""))
        self.assertEqual(cd_file.read_bytes(), os.fsencode(self.root / "folder"))
        s.close()
        # A file with a GUI opener: it starts detached, fff leaves, and the
        # shell changes to the file's folder.
        s = enter(b"a.pdf")
        self.assertEqual(s.finish(), (0, b""))
        self.wait_file(log, [str(more / "a.pdf")])
        self.assertEqual(cd_file.read_bytes(), os.fsencode(more))
        s.close()
        # A terminal opener replaces fff in this terminal, in the file's folder;
        # its status is fff's. The launch directory itself is not written.
        s = enter(b"top.txt")
        self.assertEqual(s.finish(), (7, b""))
        self.assertEqual(edited.read_text().splitlines(),
                         [str(self.root), str(self.root / "top.txt")])
        self.assertEqual(cd_file.read_bytes(), b"")
        s.close()
        self.session = None

    def test_ctrl_w_returns_marks_only_where_several_paths_can_be_chosen(self):
        (self.root / "a.pdf").write_text("a")
        (self.root / "b.pdf").write_text("b")
        refused = "Ctrl+w returns the marks only where several paths can be chosen"
        # fff roam, fff seek, a one-file dialog and a save dialog: nothing
        # happens but the message, and the marks stay.
        for args, kwargs in [
            ((), {"output_paths": False}),
            (("seek",), {}),
            (("explorer", "open"), {"output_paths": False}),
            (("explorer", "save", str(self.root / "c.pdf")), {"output_paths": False}),
        ]:
            s = self.session = Session(self.root, *args, implicit_root=True, **kwargs)
            s.send(b"\x1b[27u", "NORMAL")
            s.send(b"m", "Marks (2)")
            self.assertNotIn("Ctrl+w returns your marks", s.screen.text)
            s.send(b"\x17", refused)
            self.assertIn("Marks (2)", s.screen.text)
            # From the marked list too.
            s.send(b"\x1b ", "Marks (2)")
            s.send(b"\x17", refused)
            s.send(b"\x03")
            self.assertEqual(s.finish()[0], 130, args)
            s.close()
        self.session = None
        # fff pick returns them, as does a dialog for several files.
        for args in [(), ("explorer", "open-multiple")]:
            s = self.session = Session(self.root, *args, implicit_root=True,
                                       output_paths=not args)
            s.send(b"\x1b[27u", "NORMAL")
            if not args:
                self.assertIn("Ctrl+w returns your marks", s.screen.text)
            s.send(b"m", "Marks (2)")
            s.send(b"\x17")
            self.assertEqual(s.finish(), (0, os.fsencode(self.root / "a.pdf") + b"\n"
                                          + os.fsencode(self.root / "b.pdf") + b"\n"))
            s.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_window_enters_folders_and_bang_starts_a_shell_there(self):
        self.deep_tree()
        here = Path(self.tmp.name + "-shell.log")
        self.addCleanup(lambda: here.unlink(missing_ok=True))
        env = self.run_environment("fakesh", f'pwd > "{here}"; exit 5\n')
        env["SHELL"] = "fakesh"
        def start():
            return self.start("window", str(self.root), "--depth", "1",
                              implicit_root=True, output_paths=False, environment=env)
        # Normal-mode Enter on a folder goes into it, and fff stays.
        s = start()
        s.wait_ui(("cwd", self.root))
        self.wait_highlight(s, "folder")
        s.send(b"\r", ("cwd", self.root / "folder"))
        # ! replaces fff with the shell in the folder being browsed.
        s.send(b"!")
        self.assertEqual(s.finish(), (5, b""))
        self.assertEqual(here.read_text().strip(), str(self.root / "folder"))
        s.close()
        # q prints nothing: there is no shell to change.
        s = self.session = start()
        s.wait_ui(("cwd", self.root))
        s.send(b"q")
        self.assertEqual(s.finish(), (0, b""))
        s.close()
        self.session = None

    def test_insert_enter_reveals_a_file_in_default_mode(self):
        more = self.deep_tree()
        s = self.start("--depth", "3", implicit_root=True, output_paths=False)
        s.send(b"ia.pdf", "❯ a.pdf")
        s.send(b"\r", ("cwd", more))
        # The query is cleared and the file highlighted; fff keeps running.
        s.wait_ui("0first.txt")
        self.assertNotIn("❯ a.pdf", s.screen.text)
        self.wait_highlight(s, "a.pdf")
        # A file in the current folder: the query clears, the folder stays.
        s.send(b"0f", "❯ 0f")
        s.send(b"\r")
        s.wait_absent("❯ 0f")
        self.wait_highlight(s, "0first.txt")
        # Still typing, as after moving to another folder: letters are text.
        s.wait_ui("Query  INSERT")
        s.send(b"zz", "❯ zz")
        self.assertIn(str(more), next(l for l in s.screen.text.splitlines() if l.lstrip().startswith("@ ")))
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_marked_list_enter_prints_the_highlighted_mark(self):
        more = self.deep_tree()
        for steps, expected in [(b"", more / "a.pdf"), (b"\x1b[B", self.root / "folder")]:
            with self.subTest(steps=steps):
                s = self.start("--depth", "3", implicit_root=True)
                s.send(b"ia.pdf", "❯ a.pdf")
                s.send(SHIFT_SPACE, "Marks (1)")
                s.send(b"\x1b[27u", "NORMAL")
                s.send(b"\x1b[27u", "❯ type to filter")
                s.send(b"ifolder", "❯ folder")
                s.send(SHIFT_SPACE, "Marks (2)")
                s.send(b"\x1b ", "› ●")
                s.send(steps + b"\r")
                self.assertEqual(s.finish(), (0, os.fsencode(expected) + b"\n"))
                s.close()
                self.session = None

    def test_l_opens_a_file_with_its_gui_opener_in_the_background(self):
        more = self.deep_tree()
        env, config, log = self.opener_setup(
            rules="[[open.rules]]\ncommand = 'viewer --page 2'\nextensions = ['PDF']\n")
        s = self.start("--depth", "3", "--config", str(config), implicit_root=True, environment=env)
        s.send(b"ia.pdf", "❯ a.pdf")
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.send(b"l", "Opened a.pdf with viewer")
        self.wait_file(log, ["--page", "2", str(more / "a.pdf")])
        # fff keeps the keyboard, still in Normal mode.
        self.assertIn("Query  NORMAL", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_alt_number_opens_a_file_at_the_match_of_its_row(self):
        for name in ("one.txt", "two.txt"):
            (self.root / name).write_text("canard\nmiddle\ncanard\n")
        env, config, log = self.opener_setup(
            rules="[[open.rules]]\ncommand = 'viewer'\npatterns = ['*']\nat_grep_match = '+{line}'\n")
        s = self.start("--config", str(config), implicit_root=True, environment=env, typing=True)
        s.send(b"\t", "Contents  INSERT")
        s.send(b"canard", "❯ canard")
        s.wait_ui("4 matches in 2 files")
        # Each row's own match, not the first of its file's.
        s.send(b"\x1b[50;3u", "Opened one.txt with viewer")  # CSI-u Alt+2
        self.wait_file(log, ["+3", str(self.root / "one.txt")])
        s.send(b"\x1b[52;3u", "Opened two.txt with viewer")  # Alt+4
        self.wait_file(log, ["+3", str(self.root / "two.txt")])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_failing_background_opener_posts_a_notice(self):
        (self.root / "a.pdf").touch()
        env, config, log = self.opener_setup(body="exit 3\n")
        s = self.start("--config", str(config), implicit_root=True, environment=env)
        s.send(b"l", " Notice ")
        s.wait_ui("viewer exited with status 3")
        self.assertIn("open.log", s.screen.text)
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_o_opens_files_in_the_foreground_and_refuses_directories(self):
        (self.root / "a.pdf").touch()
        (self.root / "b.pdf").touch()
        (self.root / "folder").mkdir()
        log = Path(self.tmp.name + "-opened.log")
        env, config, log = self.opener_setup(body=f'printf "%s\\n" "$@" > "{log}"; echo VIEWER-RAN\n')
        s = self.start("--depth", "1", "--config", str(config), implicit_root=True, environment=env)
        s.send(b"ifolder", "❯ folder")
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.send(b"o", "o opens files")
        self.assertFalse(log.exists())
        # Marked files share an opener: one foreground run gets both.
        s.send(b"\x1b[27u", "❯ type to filter")
        s.send(b"i.pdf", "❯ .pdf")
        s.send(SHIFT_SPACE * 2, "Marks (2)")
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.send(b"o")
        s.wait_raw(b"VIEWER-RAN")
        s.wait_ui("viewer finished")
        self.assertEqual(sorted(log.read_text().splitlines()),
                         [str(self.root / "a.pdf"), str(self.root / "b.pdf")])
        # Alt+o does the same while typing the query.
        log.unlink()
        s.send(b"i", "Query  INSERT")
        s.send(b"\x1b[111;3u")
        self.wait_file(log, [str(self.root / "a.pdf"), str(self.root / "b.pdf")])
        s.wait_ui("Query  INSERT")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def test_terminal_openers_run_inside_the_configured_terminal(self):
        (self.root / "notes.txt").touch()
        (self.root / "other.md").touch()
        log = Path(self.tmp.name + "-opened.log")
        env, config, log = self.opener_setup(
            rules="[[open.rules]]\ncommand = 'pager -R'\nextensions = ['txt']\nterminal = true\n",
            terminal="term -e")
        # The stub terminal records the command it was asked to run.
        self.run_environment("term", f'printf "%s\\n" "$@" > "{log}"\n')
        self.run_environment("pager", "exit 0\n")
        self.run_environment("myedit", "exit 0\n")
        bin_dir = self.tmp.name + "-bin"
        s = self.start("--depth", "1", "--config", str(config), implicit_root=True,
                       environment={**env, "EDITOR": "myedit"})
        s.send(b"inotes", "❯ notes")
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.send(b"l", "Opened notes.txt with term")
        self.wait_file(log, ["-e", bin_dir + "/pager", "-R", str(self.root / "notes.txt")])
        # Unmatched files use $EDITOR, also as a terminal program.
        log.unlink()
        s.send(b"\x1b[27u", "❯ type to filter")
        s.send(b"iother", "❯ other")
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.send(b"l", "Opened other.md with term")
        self.wait_file(log, ["-e", bin_dir + "/myedit", str(self.root / "other.md")])
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    def opener_setup(self, body=None, rules="[[open.rules]]\ncommand = 'viewer'\npatterns = ['*']\n", terminal=None):
        # A stub GUI opener "viewer" on PATH that logs its arguments, and a
        # config naming it for every file. Returns (environment, config, log).
        log = Path(self.tmp.name + "-opened.log")
        self.addCleanup(lambda: log.unlink(missing_ok=True))
        env = self.run_environment("viewer", body or f'printf "%s\\n" "$@" > "{log}"\n')
        config = Path(self.tmp.name + "-open.toml")
        self.addCleanup(lambda: config.unlink(missing_ok=True))
        config.write_text((f"[open]\nterminal = {terminal!r}\n" if terminal else "") + rules)
        return env, config, log

    def wait_file(self, path, lines, timeout=5):
        # Background programs write their log on their own schedule.
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if path.exists() and path.read_text().splitlines() == lines:
                return
            time.sleep(0.02)
        self.assertEqual(path.read_text().splitlines() if path.exists() else None, lines)

    def run_environment(self, name, body):
        # A program on PATH, outside the browsed directory.
        bin_dir = Path(self.tmp.name + "-bin")
        bin_dir.mkdir(exist_ok=True)
        self.addCleanup(shutil.rmtree, bin_dir, True)
        program = bin_dir / name
        program.write_text("#!/bin/sh\n" + body)
        program.chmod(0o755)
        return {"PATH": f"{bin_dir}:{os.environ['PATH']}"}

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_gx_runs_a_completed_program_on_the_current_entry_and_comes_back(self):
        (self.root / "alpha").write_text("a")
        log = self.root / "log"
        env = self.run_environment(
            "recordargs", f'pwd > "{log}"; printf "%s\\n" "$@" >> "{log}"\n')
        s = self.start("--no-config", implicit_root=True, environment=env)
        s.send(b"gx", "Run · INSERT")
        self.assertIn("Enter: Review ─── Tab: Complete ─── Esc: Cancel", s.screen.text)
        s.send(b"recorda", "❯ recorda")
        s.send(b"\t", "❯ recordargs ")
        s.wait_ui("✓ " + self.tmp.name + "-bin/recordargs")
        s.send(b"\r", "Run here")
        self.assertIn("Enter: Run ─── Esc: Cancel", s.screen.text)
        s.send(b"\r", "recordargs finished")
        self.assertEqual(log.read_text().splitlines(), [str(self.root), str(self.root / "alpha")])
        # Unknown commands abort before anything runs, with a suggestion.
        s.send(b"gx", "Run · INSERT")
        s.send(b"recordarg\r", "recordarg: command not found; did you mean recordargs?")
        # A failing program keeps its output on screen until Enter.
        s.send(b"gx", "Run · INSERT")
        s.send(b"false\r", "Run here")
        s.send(b"\r")
        s.wait_raw(b"false exited with status 1. Press Enter to return to fff.")
        s.send(b"\r", "false exited with status 1")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_escape_leaves_insert_then_cancels_the_run_prompt(self):
        (self.root / "alpha").write_text("a")
        s = self.start("--no-config", implicit_root=True)
        s.send(b"gx", "Run · INSERT")
        s.send(b"echo\x1b[27u", "Run · NORMAL")
        self.assertIn("❯ echo", s.screen.text)
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.wait_absent("Run · ")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_gX_replaces_fff_with_the_program_and_records_the_directory(self):
        sub = self.root / "sub"
        sub.mkdir()
        (sub / "beta").write_text("b")
        log = self.root / "log"
        cd_file = self.root / "cd"
        cd_file.write_bytes(b"")
        env = self.run_environment("finale", f'printf "%s\\n" "$@" > "{log}"; exit 7\n')
        s = self.start("--no-config", "--cd-file", str(cd_file),
                       implicit_root=True, environment=env, output_paths=False)
        s.send(b"\x1bl", ("cwd", sub))
        s.send(b"gX", "Run and exit · INSERT")
        s.send(b"finale -x\r", "Run and exit")
        s.send(b"\r")
        status, output = s.finish()
        self.assertEqual((status, output), (7, b""))
        self.assertEqual(log.read_text().splitlines(), ["-x", str(sub / "beta")])
        self.assertEqual(cd_file.read_bytes(), os.fsencode(sub))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_gX_that_cannot_start_leaves_the_shell_where_it_was(self):
        sub = self.root / "sub"
        sub.mkdir()
        (sub / "beta").write_text("b")
        # Executable, but its interpreter does not exist.
        broken = self.root / "broken"
        broken.write_bytes(b"#!/nonexistent/interpreter\n")
        broken.chmod(0o755)
        cd_file = self.root / "cd"
        cd_file.write_bytes(b"")
        s = self.start("--no-config", "--cd-file", str(cd_file),
                       implicit_root=True, output_paths=False)
        s.send(b"\x1bl", ("cwd", sub))
        s.send(b"gX", "Run and exit · INSERT")
        s.send(os.fsencode(broken) + b"\r", "Run and exit")
        s.send(b"\r")
        status, output = s.finish()
        self.assertEqual((status, output), (1, b""))
        self.assertEqual(cd_file.read_bytes(), b"")

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_single_line_naming_and_extraction_keys(self):
        import zipfile
        archive = self.root / "input.zip"
        with zipfile.ZipFile(archive, "w") as z:
            z.writestr("data", "payload")
        s = self.start(implicit_root=True)
        # Escape leaves Insert first; Normal-mode Escape cancels.
        s.send(b"nf", "New file · INSERT")
        self.assertIn("Enter: Create ─── Esc: Cancel", s.screen.text)
        s.send(b"dropped\x1b[27u", "New file · NORMAL")
        s.send(b"\x1b[27u", "Query  NORMAL")
        s.wait_absent("New file")
        self.assertFalse((self.root / "dropped").exists())
        # Validation errors keep the buffer open for correction.
        s.send(b"nf", "New file · INSERT")
        s.send(b"a/b\r", "Enter a filename in the current directory")
        self.assertIn("New file · INSERT", s.screen.text)
        # o and O still enter Insert without adding a row.
        s.send(b"\x1b[27u0d$", "New file · NORMAL")
        s.send(b"o", "New file · INSERT")
        s.send(b"name\x1b[27uO", "New file · INSERT")
        # Enter applies from Insert or Normal mode.
        s.send(b"\r", "Created file")
        s.wait_absent("New file")
        self.assertTrue((self.root / "name").is_file())
        s.send(b"iinput.zip", "❯ input.zip")
        s.send(b"\x1b[27uE", "Extract archive · NORMAL")
        self.assertIn("Enter: Extract ─── Esc: Cancel", s.screen.text)
        # Command-line Enter stays in Neovim.
        s.send(b":s/^/output/\r", "❯ output")
        self.assertIn("Extract archive · NORMAL", s.screen.text)
        s.send(b"\r", "Extracted input.zip into")
        s.wait_absent("Extract archive")
        self.assertEqual((self.root / "output/data").read_text(), "payload")
        s.send(b"\x03")
        self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_query_O_enters_insert_without_a_newline(self):
        # (o opens files instead; see the opener tests.)
        (self.root / "file").touch()
        s = self.start(implicit_root=True)
        s.send(b"O", "Query  INSERT")
        s.send(b"file", "❯ file")
        s.send(b"\x1b[27uO", "Query  INSERT")
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    def test_missing_nvim_is_a_startup_error(self):
        (self.root / "file").touch()
        for args in [(), ("--no-config",)]:
            s = self.session = Session(self.root, *args, environment={"PATH": ""}, ready=False)
            s.wait_raw(b"fff: fff needs Neovim")
            self.assertEqual(s.finish(), (1, b""))
            s.close()
        self.session = None

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_default_editor_is_neovim(self):
        (self.root / "world.txt").touch()
        s = self.start()
        self.assertIn("NORMAL", s.screen.text)
        s.send(b"ihello world", "❯ hello world")
        s.send(b"\x1b[27u", "NORMAL")
        s.send(b"0ctw\x1b[27u", "❯ world")  # d hides folders in Normal mode
        # Normal-mode Enter prints the highlighted file.
        s.send(b"\r")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "world.txt") + b"\n"))

    def test_rebound_choice(self):
        (self.root / "file").touch()
        s = self.start("--config", self.config("", keys="[normal]\nprint_highlighted = ['<CR>', '<C-n>']\n"))
        s.send(b"\x0e")
        self.assertEqual(s.finish(), (0, os.fsencode(self.root / "file") + b"\n"))

    def test_palette_chooser_previews_reverts_applies_and_reloads(self):
        (self.root / "file").touch()
        s = self.start()
        s.send(b"gp", "Palettes · Enter applies")
        mark = len(s.ui)
        s.send(b"inord", "Find: nord")
        s.wait_bytes(b"48;2;46;52;64", after=mark)  # delayed nord preview
        s.send(b"\x1b[27u", "NORMAL")  # the first Escape leaves Insert
        mark = len(s.ui)
        s.send(b"\x1b[27u")  # Escape reverts to midnight
        s.wait_absent("Palettes ·")
        self.assertIn(b"48;2;15;23;36", s.ui[mark:])
        s.send(b"gpiforest", "Find: forest")
        s.send(b"\r", "Palette forest applied for this session")
        # gP in Normal mode, Alt+Shift+p while typing.
        s.send(b"gP", "Palette midnight reloaded")
        s.send(b"gpiforest", "Find: forest")
        s.send(b"\r", "Palette forest applied for this session")
        s.send(b"i", "Query  INSERT")
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
            s = self.start("--config", str(config))
            s.send(b"gpimocha\r", "Palette catppuccin-mocha applied")
            mark = len(s.ui)
            config.write_text("[ui]\npalette='tokyonight-day'\n")
            time.sleep(1.2)  # two watcher polls
            s.send(b"ix", "❯ x")
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
            s = self.start("--config", str(config))
            s.send(b"gp", "Palettes · Enter applies")
            mark = len(s.ui)
            s.send(b"ilight", "Find: light")
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
            (Path(folder) / "keys.toml").write_text(
                "[normal]\nshow_palettes = ['gp', '<C-t>']\nreload_palette = ['gP', '<C-r>']\n"
            )
            s = self.start("--config", str(config), "--palette", "forest")
            s.send(b"\x14", "Palettes · Enter applies")
            s.send(b"ilight\r", "Palette light applied")
            s.send(b"\x12", "Palette forest reloaded")
            s.send(b"\x03")
            self.assertEqual(s.finish(), (130, b""))

    @unittest.skipUnless(shutil.which("nvim"), "Neovim is not installed")
    def test_nvim_palette_filter_escape_returns_to_normal_before_reverting(self):
        (self.root / "file").touch()
        s = self.start()
        s.send(b"gp", "Palettes · Enter applies · Esc reverts · NORMAL")
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
