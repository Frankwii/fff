"""Small screen model for the CSI output emitted by Crossterm's test sessions.

Assertions target the rendered screen, so diff rendering and color changes do not
make tests depend on contiguous substrings in a raw ANSI byte stream.
"""
import codecs
import re
import unicodedata


class Screen:
    def __init__(self, rows, columns):
        self.rows, self.columns = rows, columns
        self.cells = [[" "] * columns for _ in range(rows)]
        self.x = self.y = 0
        self.pending = ""
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.frames = 0
        self.committed = ""

    @property
    def text(self):
        return self.committed

    def feed(self, data):
        self.pending += self.decoder.decode(data)
        while self.pending:
            if self.pending.startswith("\x1b["):
                match = re.match(r"\x1b\[([\x30-\x3f]*)([\x20-\x2f]*)([\x40-\x7e])", self.pending)
                if not match:
                    return
                self._csi(match[1], match[3])
                self.pending = self.pending[match.end():]
                continue
            if self.pending == "\x1b":
                return
            c, self.pending = self.pending[0], self.pending[1:]
            if c == "\r":
                self.x = 0
            elif c == "\n":
                self.y = min(self.rows - 1, self.y + 1)
            elif c == "\b":
                self.x = max(0, self.x - 1)
            elif c >= " " and c != "\x7f":
                if unicodedata.combining(c):
                    if self.x > 0:
                        self.cells[self.y][min(self.x - 1, self.columns - 1)] += c
                    continue
                width = 2 if unicodedata.east_asian_width(c) in ("W", "F") else 1
                if self.x >= self.columns:
                    self.x = 0
                    self.y = min(self.rows - 1, self.y + 1)
                self.cells[self.y][self.x] = c
                if width == 2 and self.x + 1 < self.columns:
                    self.cells[self.y][self.x + 1] = ""
                self.x += width

    def _csi(self, params, code):
        if params == "?2026" and code == "l":
            self.frames += 1
            self.committed = "\n".join("".join(row) for row in self.cells)
        if params and params[0] in "?<>=!":
            return
        values = [int(v) if v else 0 for v in params.split(";")]
        n = values[0] or 1
        if code in "Hf":
            self.y = min(self.rows - 1, n - 1)
            self.x = min(self.columns - 1, (values[1] if len(values) > 1 else 1) - 1)
        elif code == "A":
            self.y = max(0, self.y - n)
        elif code == "B":
            self.y = min(self.rows - 1, self.y + n)
        elif code == "C":
            self.x = min(self.columns - 1, self.x + n)
        elif code == "D":
            self.x = max(0, self.x - n)
        elif code == "G":
            self.x = min(self.columns - 1, n - 1)
        elif code == "d":
            self.y = min(self.rows - 1, n - 1)
        elif code == "J":
            mode = values[0]
            for y in range(self.rows):
                for x in range(self.columns):
                    if mode in (2, 3) or (mode == 0 and (y, x) >= (self.y, self.x)) or (mode == 1 and (y, x) <= (self.y, self.x)):
                        self.cells[y][x] = " "
        elif code == "K":
            mode = values[0]
            for x in range(self.columns):
                if mode == 2 or (mode == 0 and x >= self.x) or (mode == 1 and x <= self.x):
                    self.cells[self.y][x] = " "
