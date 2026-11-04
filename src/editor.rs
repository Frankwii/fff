//! Clean Neovim text engine over private stdio MessagePack RPC.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rmpv::Value;
use std::{
    collections::HashMap,
    io::{BufReader, BufWriter, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Field {
    Query,
    /// The contents query.
    Contents,
    Operation,
    FileName,
    Extraction,
    Location,
    Command,
    Exclusions,
    Filters,
    Bookmarks,
    BookmarkName,
    BookmarkPath,
    BookmarkIcons,
    Palettes,
    /// The Keys screen's query.
    Keys,
    /// The command picker's query (`:`).
    Commands,
}
impl Field {
    fn single_line(self) -> bool {
        matches!(
            self,
            Self::Query
                | Self::Contents
                | Self::FileName
                | Self::Extraction
                | Self::Command
                | Self::Keys
                | Self::Commands
        )
    }
}
#[derive(Clone, Debug)]
pub struct EditView {
    pub field: Field,
    pub mode: String,
    pub blocking: bool,
    pub row: usize,
    pub column: usize,
    pub anchor: Option<(usize, usize)>,
    pub command: String,
    pub command_column: usize,
    /// The field closed; returned text is its last written snapshot, if any.
    pub closed: bool,
    /// A write occurred during this input.
    pub written: bool,
    /// The closing field has written contents to apply.
    pub saved: bool,
}
impl EditView {
    pub fn label(&self) -> &str {
        if self.blocking {
            "PENDING"
        } else if self.mode.starts_with('i') {
            "INSERT"
        } else if self.mode.starts_with('R') {
            "REPLACE"
        } else if self.mode.starts_with('c') {
            "COMMAND"
        } else if matches!(self.mode.as_str(), "s" | "S" | "\u{13}") {
            "SELECT"
        } else if self.anchor.is_some() {
            "VISUAL"
        } else if self.mode.starts_with("no") {
            "OPERATOR"
        } else {
            "NORMAL"
        }
    }
    pub fn normal(&self) -> bool {
        self.mode == "n" && !self.blocking
    }
    pub fn command(&self) -> bool {
        self.mode.starts_with('c') || self.mode.starts_with("no") || self.blocking
    }
}
struct Buffer {
    id: Value,
    text: String,
    saved: Option<String>,
    view: EditView,
}
pub struct Nvim {
    child: Child,
    program: String,
    input: BufWriter<ChildStdin>,
    responses: Receiver<Result<Value, String>>,
    serial: u64,
    /// Fields to replace on next activation, with the mode to restore.
    invalid: HashMap<Field, String>,
    /// Row whose end receives the cursor on the next replacement; else the last.
    start_row: HashMap<Field, usize>,
    active: Option<Field>,
    buffers: HashMap<Field, Buffer>,
    /// The first error that left Neovim unusable; fff then stops.
    broken: Option<String>,
}
fn arr(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(values.into_iter().collect())
}
// Invalid UTF-8 stays visible as U+FFFD instead of erasing the whole line;
// operations refuse such names rather than acting on a different text.
fn line(value: &Value) -> String {
    match value {
        Value::String(s) => String::from_utf8_lossy(s.as_bytes()).into_owned(),
        _ => String::new(),
    }
}
fn map_get<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value
        .as_map()?
        .iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}
impl Nvim {
    pub fn start() -> Result<Self, String> {
        Self::start_program("nvim")
    }
    pub fn start_program(program: &str) -> Result<Self, String> {
        let mut child = Command::new(program)
            .args([
                "--embed",
                "--headless",
                "-u",
                "NONE",
                "-i",
                "NONE",
                "--noplugin",
                "-n",
            ])
            .env("NVIM_LOG_FILE", "/dev/null")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Cannot start clean Neovim: {e}"))?;
        let input = BufWriter::new(child.stdin.take().unwrap());
        let mut output = BufReader::new(child.stdout.take().unwrap());
        let (tx, responses) = mpsc::sync_channel(32);
        std::thread::spawn(move || {
            loop {
                let value = rmpv::decode::read_value(&mut output)
                    .map_err(|e| format!("Neovim RPC disconnected: {e}"));
                let failed = value.is_err();
                if tx.send(value).is_err() || failed {
                    break;
                }
            }
        });
        let mut nvim = Self {
            child,
            program: program.to_owned(),
            input,
            responses,
            serial: 0,
            invalid: HashMap::new(),
            start_row: HashMap::new(),
            active: None,
            buffers: HashMap::new(),
            broken: None,
        };
        // Attach a private UI so command errors are delivered as messages
        // instead of blocking a headless engine's RPC loop at a hit-enter prompt.
        nvim.request(
            "nvim_ui_attach",
            vec![
                80.into(),
                24.into(),
                Value::Map(vec![
                    ("rgb".into(), true.into()),
                    ("ext_linegrid".into(), true.into()),
                    ("ext_messages".into(), true.into()),
                ]),
            ],
        )?;
        // Capture contents before a quit/unload can disconnect RPC or destroy
        // the buffer. Intentional closes are handled separately from crashes.
        let info = nvim.request("nvim_get_api_info", vec![])?;
        let channel = info.as_array().ok_or("Invalid Neovim API info")?[0].clone();
        nvim.lua(
            r#"
            local channel=...
            _G.fff_channel=channel
            local function close(b)
                if vim.b[b].fff_field then
                    vim.rpcrequest(channel, 'fff_closed', b,
                        vim.api.nvim_buf_get_lines(b, 0, -1, false))
                end
            end
            -- Hidden fields are owned by fff; their text must not prevent the
            -- active field from quitting. Its own modified flag stays native.
            vim.api.nvim_create_autocmd('QuitPre', {callback=function()
                local current=vim.api.nvim_get_current_buf()
                for _,b in ipairs(vim.api.nvim_list_bufs()) do
                    if b ~= current and vim.b[b].fff_field then
                        vim.bo[b].modified=false
                    end
                end
            end})
            vim.api.nvim_create_autocmd('VimLeavePre', {callback=function()
                close(vim.api.nvim_get_current_buf())
            end})
            vim.api.nvim_create_autocmd('BufUnload', {callback=function(ev) close(ev.buf) end})
        "#,
            vec![channel],
        )?;
        nvim.lua("vim.o.shadafile='NONE'; vim.o.swapfile=false; vim.o.backup=false; vim.o.writebackup=false; vim.o.undofile=false; vim.o.modeline=false; vim.o.clipboard=''; vim.o.hidden=true; vim.o.history=0; vim.o.timeoutlen=300; for _,server in ipairs(vim.fn.serverlist()) do vim.fn.serverstop(server) end",vec![])?;
        Ok(nvim)
    }
    fn request(&mut self, method: &str, args: Vec<Value>) -> Result<Value, String> {
        self.serial += 1;
        let id = self.serial;
        // A quit can close stdin before the next snapshot request is sent.
        // Still drain stdout: its final notification distinguishes a normal
        // field exit from an engine crash, even after a broken write.
        let write_error = rmpv::encode::write_value(
            &mut self.input,
            &arr([0.into(), id.into(), method.into(), arr(args)]),
        )
        .map_err(|e| e.to_string())
        .and_then(|()| self.input.flush().map_err(|e| e.to_string()))
        .err();
        loop {
            let value = match self.responses.recv_timeout(Duration::from_secs(1)) {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => return Err(write_error.unwrap_or(error)),
                Err(_) => {
                    return Err(write_error.unwrap_or_else(|| format!("Neovim {method} timed out")));
                }
            };
            let Some(parts) = value.as_array() else {
                continue;
            };
            // Fields report writes and closes as requests: Neovim waits for
            // the answer, so a quit cannot cut the text off at the pipe's
            // capacity the way an unanswered notification is when it exits.
            if parts.len() == 4
                && parts[0].as_u64() == Some(0)
                && let Some(event) = parts[2]
                    .as_str()
                    .filter(|e| ["fff_closed", "fff_written"].contains(e))
            {
                let _ = rmpv::encode::write_value(
                    &mut self.input,
                    &arr([1.into(), parts[1].clone(), Value::Nil, Value::Nil]),
                );
                let _ = self.input.flush();
                if let Some(args) = parts[3].as_array()
                    && args.len() == 2
                    && let Some(buffer) = self
                        .buffers
                        .values_mut()
                        .find(|b| b.id == args[0] && Some(b.view.field) == self.active)
                    && let Some(lines) = args[1].as_array()
                {
                    buffer.text = lines.iter().map(line).collect::<Vec<_>>().join("\n");
                    if event == "fff_written" {
                        buffer.saved = Some(buffer.text.clone());
                        buffer.view.written = true;
                        continue;
                    }
                    buffer.view.closed = true;
                    return Err("Neovim field closed".into());
                }
                continue;
            }
            if parts.len() == 4 && parts[0].as_u64() == Some(1) && parts[1].as_u64() == Some(id) {
                if parts[2] != Value::Nil {
                    return Err(format!("Neovim: {}", parts[2]));
                }
                return Ok(parts[3].clone());
            }
        }
    }
    fn lua(&mut self, code: &str, args: Vec<Value>) -> Result<Value, String> {
        self.request("nvim_exec_lua", vec![code.into(), arr(args)])
    }
    fn cancel_pending(&mut self) -> Result<(), String> {
        self.request("nvim_input", vec!["<Esc>".into()])?;
        Ok(())
    }
    /// Replace the field's buffer on next activation, starting in Normal mode.
    pub fn reset(&mut self, field: Field) {
        self.invalid.insert(field, "n".into());
    }
    /// Reset in Normal mode with the cursor at the end of `row`.
    pub fn reset_at_row(&mut self, field: Field, row: usize) {
        self.reset(field);
        self.start_row.insert(field, row);
    }
    /// Replace the field's buffer on next activation, ready for typing.
    pub fn reset_insert(&mut self, field: Field) {
        self.invalid.insert(field, "i".into());
    }
    /// Replace text while keeping the active field's stable editing mode.
    /// Operators, command-line input and pending motions are cancelled.
    pub fn reset_preserving_mode(&mut self, field: Field) {
        let mode = self
            .active
            .and_then(|active| self.buffers.get(&active))
            .map(|b| b.view.mode.as_str())
            .unwrap_or("n");
        self.invalid.insert(field, mode.to_owned());
    }
    pub fn activate(&mut self, field: Field, text: &str) -> Result<EditView, String> {
        let requested_mode = self.invalid.remove(&field);
        let reset = requested_mode.is_some();
        if !reset
            && self.active == Some(field)
            && self.buffers.get(&field).is_some_and(|b| b.text == text)
        {
            return Ok(self.buffers[&field].view.clone());
        }
        self.cancel_pending()?;
        if !self.buffers.contains_key(&field) {
            let id = self.lua(
                r#"
                local single_line=...
                local b=vim.api.nvim_create_buf(false,true)
                vim.b[b].fff_field=true
                vim.bo[b].buftype='acwrite'
                vim.bo[b].bufhidden='hide'
                vim.bo[b].swapfile=false
                vim.api.nvim_buf_set_name(b, 'fff://'..b)
                if single_line then
                    for _,key in ipairs({'o','O'}) do
                        vim.keymap.set('n',key,'i',{buffer=b,noremap=true})
                    end
                    vim.keymap.set({'n','i'},'<CR>','<Nop>',{buffer=b,noremap=true})
                end
                -- ZZ applies like :wq even when nothing changed; native ZZ
                -- would close an unmodified buffer without writing (a discard).
                vim.keymap.set('n','ZZ','<Cmd>wq<CR>',{buffer=b,noremap=true})
                vim.api.nvim_create_autocmd('BufWriteCmd', {buffer=b, callback=function()
                    vim.rpcrequest(fff_channel, 'fff_written', b, vim.api.nvim_buf_get_lines(b, 0, -1, false))
                    vim.bo[b].modified=false
                end})
                return b
            "#,
                vec![field.single_line().into()],
            )?;
            self.buffers.insert(
                field,
                Buffer {
                    id,
                    text: String::new(),
                    saved: None,
                    view: EditView {
                        field,
                        mode: "n".into(),
                        blocking: false,
                        row: 0,
                        column: 0,
                        anchor: None,
                        command: String::new(),
                        command_column: 0,
                        closed: false,
                        written: false,
                        saved: false,
                    },
                },
            );
        }
        let id = self.buffers[&field].id.clone();
        self.lua("vim.api.nvim_set_current_buf(...)", vec![id.clone()])?;
        if reset
            || self.buffers[&field].text != text
            || self.active != Some(field) && self.buffers[&field].text.is_empty()
        {
            self.buffers.get_mut(&field).unwrap().saved = None;
            let lines = text.split('\n').map(Value::from).collect::<Vec<_>>();
            let row = self
                .start_row
                .remove(&field)
                .map_or(lines.len(), |row| row.min(lines.len() - 1) + 1);
            self.lua("local b,lines,row=...; vim.bo[b].undolevels=-1; vim.api.nvim_buf_set_lines(b,0,-1,false,lines); vim.bo[b].undolevels=1000; vim.bo[b].modified=false; vim.api.nvim_win_set_cursor(0,{row,#lines[row]}); vim.cmd('stopinsert')",vec![id,arr(lines),row.into()])?;
        } else {
            let view = &self.buffers[&field].view;
            let row = view.row + 1;
            let col = view.column;
            let insert = view.mode.starts_with('i');
            self.lua("local row,col,insert=...; local line=vim.api.nvim_buf_get_lines(0,row-1,row,false)[1] or ''; vim.api.nvim_win_set_cursor(0,{row,col}); if insert then vim.cmd(col>=#line and 'startinsert!' or 'startinsert') end",vec![row.into(),col.into(),insert.into()])?;
        }
        if let Some(mode) = requested_mode {
            let input = match mode.as_str() {
                m if m.starts_with('i') => Some("a"),
                "R" | "Rc" | "Rx" => Some("R"),
                "Rv" | "Rvc" | "Rvx" => Some("gR"),
                "v" => Some("v"),
                "V" => Some("V"),
                "\u{16}" => Some("<C-v>"),
                "s" => Some("gh"),
                "S" => Some("gH"),
                "\u{13}" => Some("g<C-h>"),
                _ => None,
            };
            if let Some(input) = input {
                self.request("nvim_input", vec![input.into()])?;
            }
        }
        self.active = Some(field);
        self.snapshot(field).map(|(_, view)| view)
    }
    fn snapshot(&mut self, field: Field) -> Result<(String, EditView), String> {
        let mode = self.request("nvim_get_mode", vec![])?;
        let blocking = map_get(&mode, "blocking")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        // Hit-enter/error prompts also block ordinary Lua RPC even when the
        // fast mode API reports blocking=false. Let Neovim receive the next key.
        if blocking
            || map_get(&mode, "mode")
                .and_then(Value::as_str)
                .is_some_and(|m| m.starts_with('r'))
        {
            let buffer = self.buffers.get_mut(&field).unwrap();
            buffer.view.blocking = true;
            buffer.view.mode = map_get(&mode, "mode")
                .and_then(Value::as_str)
                .unwrap_or("n")
                .into();
            return Ok((buffer.text.clone(), buffer.view.clone()));
        }
        let data=self.lua("local p=vim.api.nvim_win_get_cursor(0); local m=vim.fn.mode(1); local v=vim.fn.getpos('v'); return {vim.api.nvim_buf_get_lines(0,0,-1,false),p,m,v,vim.fn.getcmdtype()..vim.fn.getcmdline(),vim.fn.getcmdpos()}",vec![])?;
        let values = data.as_array().ok_or("Invalid Neovim snapshot")?;
        let lines = values[0]
            .as_array()
            .ok_or("Invalid Neovim lines")?
            .iter()
            .map(line)
            .collect::<Vec<_>>();
        let cursor = values[1].as_array().ok_or("Invalid Neovim cursor")?;
        let mode = values[2].as_str().unwrap_or("n").to_string();
        let anchor = if matches!(mode.as_str(), "v" | "V" | "\u{16}" | "s" | "S" | "\u{13}") {
            let p = values[3].as_array().ok_or("Invalid visual position")?;
            Some((
                p[1].as_u64().unwrap_or(1) as usize - 1,
                p[2].as_u64().unwrap_or(1) as usize - 1,
            ))
        } else {
            None
        };
        let view = EditView {
            field,
            mode,
            blocking: false,
            row: cursor[0].as_u64().unwrap_or(1) as usize - 1,
            column: cursor[1].as_u64().unwrap_or(0) as usize,
            anchor,
            command: values[4].as_str().unwrap_or("").into(),
            command_column: values[5].as_u64().unwrap_or(1) as usize,
            closed: false,
            written: self.buffers[&field].view.written,
            saved: false,
        };
        let text = lines.join("\n");
        let buffer = self.buffers.get_mut(&field).unwrap();
        buffer.text = text.clone();
        buffer.view = view.clone();
        buffer.view.written = false;
        Ok((text, view))
    }
    pub fn input(&mut self, field: Field, key: KeyEvent) -> Result<(String, EditView), String> {
        let Some(keys) = key_notation(key) else {
            return self.snapshot(field);
        };
        let result = self
            .request("nvim_input", vec![keys.into()])
            .and_then(|_| self.snapshot(field));
        if self.buffers[&field].view.closed {
            let buffer = &self.buffers[&field];
            let text = buffer.saved.clone().unwrap_or_else(|| buffer.text.clone());
            let mut view = buffer.view.clone();
            view.saved = buffer.saved.is_some();
            view.mode = "n".into();
            view.blocking = false;
            // A field quit may also have quit the shared engine. Start a clean
            // engine before the next activation; never write to its closed pipe.
            *self = Self::start_program(&self.program)?;
            return Ok((text, view));
        }
        result
    }
    pub fn paste(&mut self, field: Field, text: &str) -> Result<(String, EditView), String> {
        let text = if field.single_line() {
            text.replace(['\r', '\n'], " ")
        } else {
            text.to_owned()
        };
        self.request("nvim_paste", vec![text.into(), false.into(), (-1).into()])?;
        self.snapshot(field)
    }
}
impl Nvim {
    pub fn break_with(&mut self, error: String) {
        self.broken.get_or_insert(error);
    }
    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
    }
}
impl Drop for Nvim {
    fn drop(&mut self) {
        // Close normally when possible; never let a pending command hold the fff open.
        let _ = rmpv::encode::write_value(
            &mut self.input,
            &arr([2.into(), "nvim_input".into(), arr(["<Esc>".into()])]),
        );
        let _ = rmpv::encode::write_value(
            &mut self.input,
            &arr([2.into(), "nvim_command".into(), arr(["qa!".into()])]),
        );
        let _ = self.input.flush();
        let deadline = std::time::Instant::now() + Duration::from_millis(30);
        while std::time::Instant::now() < deadline {
            if self.child.try_wait().ok().flatten().is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn key_notation(key: KeyEvent) -> Option<String> {
    let base = match key.code {
        KeyCode::Char('<') => "lt".into(),
        KeyCode::Char(' ') => "Space".into(),
        KeyCode::Char(c) => c.to_string(),
        KeyCode::Enter => "CR".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Backspace => "BS".into(),
        KeyCode::Delete => "Del".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "S-Tab".into(),
        _ => return None,
    };
    if let KeyCode::Char(c) = key.code
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        && c != '<'
    {
        return Some(c.to_string());
    }
    let mut modifiers = String::new();
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        modifiers.push_str("C-");
    }
    if key.modifiers.contains(KeyModifiers::ALT) {
        modifiers.push_str("M-");
    }
    if key.modifiers.contains(KeyModifiers::SHIFT)
        && !matches!(key.code,KeyCode::Char(c) if c.is_uppercase())
    {
        modifiers.push_str("S-");
    }
    Some(format!("<{modifiers}{base}>"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn child_crash_is_reported_and_drop_reaps_the_process() {
        if Command::new("nvim")
            .arg("--version")
            .env("NVIM_LOG_FILE", "/dev/null")
            .output()
            .is_err()
        {
            eprintln!("nvim missing; skipping process test");
            return;
        }
        let mut editor = Nvim::start().unwrap();
        let pid = editor.child.id();
        editor.activate(Field::Query, "").unwrap();
        editor.child.kill().unwrap();
        editor.child.wait().unwrap();
        assert!(
            editor
                .input(
                    Field::Query,
                    KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE)
                )
                .is_err()
        );
        drop(editor);
        if std::path::Path::new("/proc").exists() {
            assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
        }
    }
}
