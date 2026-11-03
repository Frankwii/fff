//! Syntax colors for text previews (`[ui] highlight`), from a private
//! headless Neovim: the tree-sitter parser and highlight queries for the
//! file's type where Neovim has them (its own, and those installed in its
//! data directory's `site`), else its classic `:syntax` files.
//!
//! It never holds up a preview: one thread owns the engine, started with the
//! first request, and answers the newest request not yet cancelled, skipping
//! those left behind. An engine that stops answering is killed and started
//! again for the next request; one that keeps failing is given up for the
//! session, and previews stay plain.
use rmpv::Value;
use std::{
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
    },
    time::Duration,
};

/// A kind of syntax the pane colors, each drawn in a palette color.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Syntax {
    Comment,
    String,
    Keyword,
    Type,
    Function,
    Constant,
    Special,
    Heading,
}
impl Syntax {
    /// The kind a tree-sitter capture (`keyword.return`) or a classic
    /// highlight group (`Statement`) stands for; `None` is plain text.
    pub fn of(name: &str) -> Option<Self> {
        let name = name.to_ascii_lowercase();
        let mut parts = name.split('.');
        Some(match (parts.next()?, parts.next()) {
            ("markup", Some("heading")) | ("title", _) => Self::Heading,
            ("markup", Some("raw")) => Self::String,
            ("markup", Some("link")) | ("underlined", _) => Self::Special,
            ("comment" | "specialcomment" | "todo", _) => Self::Comment,
            ("string" | "character", _) => Self::String,
            (
                "keyword" | "statement" | "conditional" | "repeat" | "label" | "exception"
                | "include" | "define" | "macro" | "precondit" | "preproc",
                _,
            ) => Self::Keyword,
            ("type" | "storageclass" | "structure" | "typedef" | "module" | "namespace", _) => {
                Self::Type
            }
            ("function" | "method" | "constructor", _) => Self::Function,
            ("constant" | "number" | "boolean" | "float", _) => Self::Constant,
            ("special" | "specialchar" | "tag" | "attribute", _) => Self::Special,
            _ => return None,
        })
    }
}
/// Each line's colored byte ranges, in order and apart.
pub type Lines = Vec<Vec<(usize, usize, Syntax)>>;

/// How long one answer may take before the engine is taken for stuck.
const ANSWER: Duration = Duration::from_secs(2);
/// Engines started before highlighting is given up for the session.
const STARTS: usize = 3;

struct Request {
    path: PathBuf,
    lines: Vec<String>,
    cancel: Arc<AtomicBool>,
    reply: Sender<Lines>,
}

/// Asks for `lines`' colors, read as the start of the file at `path`. The
/// answer arrives on the receiver; it closes without one when there is no
/// engine, the request was cancelled or the file type has no syntax.
pub fn request(path: &Path, lines: Vec<String>, cancel: Arc<AtomicBool>) -> Receiver<Lines> {
    static QUEUE: OnceLock<Mutex<Sender<Request>>> = OnceLock::new();
    let queue = QUEUE.get_or_init(|| {
        let (sender, requests) = mpsc::channel();
        std::thread::spawn(move || serve(requests));
        Mutex::new(sender)
    });
    let (reply, answer) = mpsc::channel();
    let _ = queue.lock().unwrap().send(Request {
        path: path.to_path_buf(),
        lines,
        cancel,
        reply,
    });
    answer
}

fn serve(requests: Receiver<Request>) {
    let mut engine: Option<Engine> = None;
    let mut starts = 0;
    while let Ok(mut request) = requests.recv() {
        // Only the newest request matters: the highlight moved on.
        while let Ok(newer) = requests.try_recv() {
            request = newer;
        }
        if request.cancel.load(Ordering::Relaxed) {
            continue;
        }
        if engine.is_none() {
            if starts == STARTS {
                continue;
            }
            starts += 1;
            engine = Engine::start().ok();
        }
        let Some(running) = &mut engine else {
            continue;
        };
        match running.highlight(&request.path, &request.lines) {
            Ok(lines) => {
                let _ = request.reply.send(lines);
            }
            // Stuck or gone: the next request starts another.
            Err(_) => engine = None,
        }
    }
}

/// Fills a scratch buffer with the lines, finds the file type by name and
/// content, and returns `{row, start, end, name}` for each capture or
/// classic group, in drawing order (an `end` of -1 runs to the line's end).
const HIGHLIGHT: &str = r#"
local path, lines = ...
local buf = vim.api.nvim_create_buf(false, true)
local out = {}
local ok, err = pcall(function()
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  local ft = vim.filetype.match({ buf = buf, filename = path })
  if not ft or ft == '' then return end
  local function add(row, s, e, name) out[#out + 1] = { row, s, e, name } end
  local lang = vim.treesitter.language.get_lang(ft) or ft
  local found, parser = pcall(vim.treesitter.get_parser, buf, lang, { error = false })
  local queried, query = pcall(vim.treesitter.query.get, lang, 'highlights')
  if found and parser and queried and query then
    parser:parse(true)
    parser:for_each_tree(function(tree, ltree)
      local q = vim.treesitter.query.get(ltree:lang(), 'highlights')
      if not q then return end
      for id, node in q:iter_captures(tree:root(), buf, 0, #lines) do
        local name = q.captures[id]
        local sr, sc, er, ec = node:range()
        if sr == er then
          add(sr, sc, ec, name)
        else
          add(sr, sc, -1, name)
          for r = sr + 1, er - 1 do add(r, 0, -1, name) end
          add(er, 0, ec, name)
        end
      end
    end)
    return
  end
  vim.api.nvim_buf_call(buf, function()
    vim.bo.syntax = ft
    if (vim.b.current_syntax or '') == '' then return end
    for r = 1, #lines do
      local line, previous, start = lines[r], '', 0
      for c = 1, #line do
        local name = vim.fn.synIDattr(vim.fn.synIDtrans(vim.fn.synID(r, c, 1)), 'name')
        if name ~= previous then
          if previous ~= '' then add(r - 1, start, c - 1, previous) end
          previous, start = name, c - 1
        end
      end
      if previous ~= '' then add(r - 1, start, #line, previous) end
    end
  end)
end)
vim.api.nvim_buf_delete(buf, { force = true })
return out
"#;

struct Engine {
    child: Child,
    input: BufWriter<ChildStdin>,
    output: Receiver<Value>,
    serial: u64,
}
impl Engine {
    fn start() -> Result<Self, String> {
        let mut child = Command::new("nvim")
            .args(["--embed", "--headless", "--clean", "-n", "-i", "NONE"])
            .env("NVIM_LOG_FILE", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        let input = BufWriter::new(child.stdin.take().unwrap());
        let mut stdout = BufReader::new(child.stdout.take().unwrap());
        let (sender, output) = mpsc::channel();
        std::thread::spawn(move || {
            while let Ok(value) = rmpv::decode::read_value(&mut stdout) {
                if sender.send(value).is_err() {
                    break;
                }
            }
        });
        let mut engine = Self {
            child,
            input,
            output,
            serial: 0,
        };
        // Parsers and queries installed for the user's own Neovim count;
        // its configuration and plugins do not.
        engine.request(
            "nvim_exec_lua",
            vec![
                "vim.opt.rtp:append(vim.fn.stdpath('data') .. '/site') vim.cmd('syntax on')".into(),
                Value::Array(vec![]),
            ],
        )?;
        Ok(engine)
    }
    fn request(&mut self, method: &str, args: Vec<Value>) -> Result<Value, String> {
        self.serial += 1;
        let id = self.serial;
        let message = Value::Array(vec![0.into(), id.into(), method.into(), Value::Array(args)]);
        rmpv::encode::write_value(&mut self.input, &message).map_err(|e| e.to_string())?;
        self.input.flush().map_err(|e| e.to_string())?;
        let deadline = std::time::Instant::now() + ANSWER;
        loop {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let value = self
                .output
                .recv_timeout(left)
                .map_err(|_| format!("Neovim {method} did not answer"))?;
            // Notifications and earlier answers are not this one.
            let Some([kind, serial, error, result]) = value.as_array().map(Vec::as_slice) else {
                continue;
            };
            if kind.as_u64() == Some(1) && serial.as_u64() == Some(id) {
                return match error {
                    Value::Nil => Ok(result.clone()),
                    error => Err(error.to_string()),
                };
            }
        }
    }
    fn highlight(&mut self, path: &Path, lines: &[String]) -> Result<Lines, String> {
        let answer = self.request(
            "nvim_exec_lua",
            vec![
                HIGHLIGHT.into(),
                Value::Array(vec![
                    path.to_string_lossy().as_ref().into(),
                    Value::Array(lines.iter().map(|l| l.as_str().into()).collect()),
                ]),
            ],
        )?;
        let mut captures = Vec::new();
        for capture in answer.as_array().into_iter().flatten() {
            let Some([row, start, end, name]) = capture.as_array().map(Vec::as_slice) else {
                continue;
            };
            let (Some(row), Some(start), Some(end), Some(name)) =
                (row.as_u64(), start.as_u64(), end.as_i64(), name.as_str())
            else {
                continue;
            };
            let end = usize::try_from(end).unwrap_or(usize::MAX);
            captures.push((row as usize, start as usize, end, name));
        }
        Ok(paint(lines, captures))
    }
}
impl Drop for Engine {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Colors each line by its captures, in order: a later capture over an
/// earlier one wins, as in Neovim, so `@variable` inside a string's
/// interpolation is plain again. Markers that are not colors (`spell`,
/// `conceal`, private `_names`) are skipped.
pub fn paint<'a>(
    lines: &[String],
    captures: impl IntoIterator<Item = (usize, usize, usize, &'a str)>,
) -> Lines {
    let mut cells: Vec<Vec<Option<Syntax>>> = lines.iter().map(|l| vec![None; l.len()]).collect();
    for (row, start, end, name) in captures {
        if name.starts_with('_') || ["spell", "nospell", "conceal", "conceal_lines"].contains(&name)
        {
            continue;
        }
        let Some(cells) = cells.get_mut(row) else {
            continue;
        };
        let end = end.min(cells.len());
        if start < end {
            cells[start..end].fill(Syntax::of(name));
        }
    }
    cells
        .into_iter()
        .zip(lines)
        .map(|(cells, line)| {
            let mut spans: Vec<(usize, usize, Syntax)> = Vec::new();
            let mut start = 0;
            // Runs end only where characters do, so a span never splits one.
            for end in (1..=line.len()).filter(|&i| line.is_char_boundary(i)) {
                if end == line.len() || cells[end] != cells[start] {
                    if let Some(syntax) = cells[start] {
                        spans.push((start, end, syntax));
                    }
                    start = end;
                }
            }
            spans
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn captures_and_groups_name_their_kind() {
        assert_eq!(Syntax::of("keyword.return"), Some(Syntax::Keyword));
        assert_eq!(Syntax::of("Statement"), Some(Syntax::Keyword));
        assert_eq!(Syntax::of("string.escape"), Some(Syntax::String));
        assert_eq!(Syntax::of("markup.heading.1"), Some(Syntax::Heading));
        assert_eq!(Syntax::of("function.method.call"), Some(Syntax::Function));
        assert_eq!(Syntax::of("number.float"), Some(Syntax::Constant));
        assert_eq!(Syntax::of("variable"), None);
        assert_eq!(Syntax::of("punctuation.bracket"), None);
    }

    #[test]
    fn later_captures_win_and_spans_keep_characters_whole() {
        let lines = vec!["let s = \"é{x}\";".to_string(), "// end".to_string()];
        let painted = paint(
            &lines,
            [
                (0, 0, 3, "keyword"),
                (0, 8, 15, "string"),
                (0, 12, 13, "variable"),
                (0, 4, 5, "spell"),
                (1, 0, usize::MAX, "comment"),
                (7, 0, 1, "comment"),
            ],
        );
        assert_eq!(
            painted,
            vec![
                vec![
                    (0, 3, Syntax::Keyword),
                    (8, 12, Syntax::String),
                    (13, 15, Syntax::String),
                ],
                vec![(0, 6, Syntax::Comment)],
            ]
        );
    }

    #[test]
    fn neovim_colors_a_rust_file() {
        if Command::new("nvim").arg("--version").output().is_err() {
            return;
        }
        let lines = vec!["// note".to_string(), "fn main() {}".to_string()];
        let answer = request(
            Path::new("main.rs"),
            lines,
            Arc::new(AtomicBool::new(false)),
        )
        .recv_timeout(Duration::from_secs(10))
        .unwrap();
        assert_eq!(answer[0], vec![(0, 7, Syntax::Comment)]);
        assert_eq!(answer[1].first(), Some(&(0, 2, Syntax::Keyword)));
    }
}
