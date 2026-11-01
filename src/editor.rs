//! Experimental clean Neovim text engine over private stdio MessagePack RPC.
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use rmpv::Value;
use std::{
    collections::{HashMap, HashSet},
    io::{BufReader, BufWriter, Write},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub enum Field {
    Query,
    Location,
    Exclusions,
    Bookmarks,
    Palettes,
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
    view: EditView,
}
pub struct Nvim {
    child: Child,
    input: BufWriter<ChildStdin>,
    responses: Receiver<Result<Value, String>>,
    serial: u64,
    invalid: HashSet<Field>,
    active: Option<Field>,
    buffers: HashMap<Field, Buffer>,
}
fn arr(values: impl IntoIterator<Item = Value>) -> Value {
    Value::Array(values.into_iter().collect())
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
            input,
            responses,
            serial: 0,
            invalid: HashSet::new(),
            active: None,
            buffers: HashMap::new(),
        };
        nvim.lua("vim.o.shadafile='NONE'; vim.o.swapfile=false; vim.o.backup=false; vim.o.writebackup=false; vim.o.undofile=false; vim.o.modeline=false; vim.o.clipboard=''; vim.o.hidden=true; vim.o.history=0; vim.o.timeoutlen=300; for _,server in ipairs(vim.fn.serverlist()) do vim.fn.serverstop(server) end",vec![])?;
        Ok(nvim)
    }
    fn request(&mut self, method: &str, args: Vec<Value>) -> Result<Value, String> {
        self.serial += 1;
        let id = self.serial;
        rmpv::encode::write_value(
            &mut self.input,
            &arr([0.into(), id.into(), method.into(), arr(args)]),
        )
        .map_err(|e| e.to_string())?;
        self.input.flush().map_err(|e| e.to_string())?;
        loop {
            let value = self
                .responses
                .recv_timeout(Duration::from_secs(1))
                .map_err(|_| "Neovim RPC timed out".to_string())??;
            let Some(parts) = value.as_array() else {
                continue;
            };
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
    pub fn reset(&mut self, field: Field) {
        self.invalid.insert(field);
    }
    pub fn activate(&mut self, field: Field, text: &str) -> Result<EditView, String> {
        let reset = self.invalid.remove(&field);
        if !reset
            && self.active == Some(field)
            && self.buffers.get(&field).is_some_and(|b| b.text == text)
        {
            return Ok(self.buffers[&field].view.clone());
        }
        self.cancel_pending()?;
        if !self.buffers.contains_key(&field) {
            let id=self.lua("local b=vim.api.nvim_create_buf(false,true); vim.bo[b].buftype='nofile'; vim.bo[b].bufhidden='hide'; vim.bo[b].swapfile=false; return b",vec![])?;
            self.buffers.insert(
                field,
                Buffer {
                    id,
                    text: String::new(),
                    view: EditView {
                        field,
                        mode: "i".into(),
                        blocking: false,
                        row: 0,
                        column: 0,
                        anchor: None,
                        command: String::new(),
                        command_column: 0,
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
            let lines = text.split('\n').map(Value::from).collect::<Vec<_>>();
            self.lua("local b,lines=...; vim.bo[b].undolevels=-1; vim.api.nvim_buf_set_lines(b,0,-1,false,lines); vim.bo[b].undolevels=1000; vim.api.nvim_win_set_cursor(0,{#lines,#lines[#lines]}); vim.cmd('startinsert!')",vec![id,arr(lines)])?;
        } else {
            let view = &self.buffers[&field].view;
            let row = view.row + 1;
            let col = view.column;
            let insert = view.mode.starts_with('i');
            self.lua("local row,col,insert=...; local line=vim.api.nvim_buf_get_lines(0,row-1,row,false)[1] or ''; vim.api.nvim_win_set_cursor(0,{row,col}); if insert then vim.cmd(col>=#line and 'startinsert!' or 'startinsert') end",vec![row.into(),col.into(),insert.into()])?;
        }
        self.active = Some(field);
        self.snapshot(field).map(|(_, view)| view)
    }
    fn snapshot(&mut self, field: Field) -> Result<(String, EditView), String> {
        let mode = self.request("nvim_get_mode", vec![])?;
        let blocking = map_get(&mode, "blocking")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if blocking {
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
            .map(|v| v.as_str().unwrap_or(""))
            .collect::<Vec<_>>();
        let cursor = values[1].as_array().ok_or("Invalid Neovim cursor")?;
        let mode = values[2].as_str().unwrap_or("n").to_string();
        let anchor = if matches!(mode.as_str(), "v" | "V" | "\u{16}") {
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
        };
        let text = lines.join("\n");
        let buffer = self.buffers.get_mut(&field).unwrap();
        buffer.text = text.clone();
        buffer.view = view.clone();
        Ok((text, view))
    }
    pub fn input(&mut self, field: Field, key: KeyEvent) -> Result<(String, EditView), String> {
        let Some(keys) = key_notation(key) else {
            return self.snapshot(field);
        };
        self.request("nvim_input", vec![keys.into()])?;
        self.snapshot(field)
    }
    pub fn paste(&mut self, field: Field, text: &str) -> Result<(String, EditView), String> {
        self.request("nvim_paste", vec![text.into(), false.into(), (-1).into()])?;
        self.snapshot(field)
    }
}
impl Drop for Nvim {
    fn drop(&mut self) {
        // Close normally when possible; never let a pending command hold the picker open.
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
