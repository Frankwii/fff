//! `org.freedesktop.FileManager1` on the session bus, so that "Show in
//! folder" (Firefox's downloads, for one) opens fff with the file highlighted
//! instead of only opening its folder. `fff setup --desktop` installs a D-Bus
//! service file; the bus starts `fff dbus-file-manager LAUNCHER` on the first
//! call, which runs the `open-folder` launcher on each item and exits after a
//! while without calls.
//!
//! The protocol is spoken directly, as far as a service needs it: the
//! EXTERNAL authentication over a Unix socket, then messages in the D-Bus
//! wire format (method calls in, returns and errors out).
use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    os::unix::{net::UnixStream, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

pub const NAME: &str = "org.freedesktop.FileManager1";
const OBJECT: &str = "/org/freedesktop/FileManager1";
/// The service exits after this long without calls; the bus starts it again.
pub const IDLE: Duration = Duration::from_secs(600);

const METHOD_CALL: u8 = 1;
const METHOD_RETURN: u8 = 2;
const ERROR: u8 = 3;
const NO_REPLY_EXPECTED: u8 = 1;

const INTROSPECTION: &str = r#"<!DOCTYPE node PUBLIC "-//freedesktop//DTD D-BUS Object Introspection 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/introspect.dtd">
<node>
  <interface name="org.freedesktop.FileManager1">
    <method name="ShowFolders">
      <arg type="as" name="URIs" direction="in"/>
      <arg type="s" name="StartupId" direction="in"/>
    </method>
    <method name="ShowItems">
      <arg type="as" name="URIs" direction="in"/>
      <arg type="s" name="StartupId" direction="in"/>
    </method>
    <method name="ShowItemProperties">
      <arg type="as" name="URIs" direction="in"/>
      <arg type="s" name="StartupId" direction="in"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Introspectable">
    <method name="Introspect">
      <arg type="s" name="xml_data" direction="out"/>
    </method>
  </interface>
  <interface name="org.freedesktop.DBus.Peer">
    <method name="Ping"/>
  </interface>
</node>
"#;

/// Answers calls until `idle` passes without one. False when another
/// program already owns the name (it answers instead).
pub fn serve(address: Option<&str>, launcher: &Path, idle: Duration) -> io::Result<bool> {
    let mut bus = Bus::connect(address)?;
    bus.call("Hello", "", &[])?;
    let mut body = Writer::default();
    body.string(NAME);
    // DO_NOT_QUEUE: if the name is taken, leave it to its owner.
    body.u32(4);
    let reply = bus.call("RequestName", "su", &body.data)?;
    // 1: the primary owner now; 4: already it.
    if !matches!(Reader::new(&reply.body, reply.big).u32(), Some(1 | 4)) {
        return Ok(false);
    }
    bus.stream.set_read_timeout(Some(idle))?;
    loop {
        let next = match bus.queued.pop_front() {
            Some(message) => Ok(message),
            None => bus.receive(),
        };
        let message = match next {
            Ok(message) => message,
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                return Ok(true);
            }
            Err(e) => return Err(e),
        };
        if message.kind != METHOD_CALL {
            continue;
        }
        let answer = answer(&message);
        if message.flags & NO_REPLY_EXPECTED == 0 {
            match &answer {
                Answer::Show(..) | Answer::Empty => bus.reply(&message, "", &[])?,
                Answer::Text(text) => {
                    let mut body = Writer::default();
                    body.string(text);
                    bus.reply(&message, "s", &body.data)?
                }
                Answer::Error(name, text) => bus.error(&message, name, text)?,
            }
        }
        if let Answer::Show(paths, highlight) = answer {
            for path in places(paths, highlight) {
                launch(launcher, &path);
            }
        }
    }
}

/// What a method call asks for.
#[derive(Debug, PartialEq)]
enum Answer {
    /// Open these paths; true to highlight items rather than open folders.
    Show(Vec<PathBuf>, bool),
    Empty,
    Text(String),
    Error(&'static str, String),
}

fn answer(message: &Message) -> Answer {
    let interface = message.interface.as_deref();
    let member = message.member.as_deref().unwrap_or_default();
    match (interface, member) {
        (Some(NAME) | None, "ShowFolders" | "ShowItems" | "ShowItemProperties") => {
            if message.path.as_deref() != Some(OBJECT) {
                return Answer::Error(
                    "org.freedesktop.DBus.Error.UnknownObject",
                    format!("No object at {}", message.path.as_deref().unwrap_or("")),
                );
            }
            let uris = (message.signature.as_deref() == Some("ass"))
                .then(|| Reader::new(&message.body, message.big).strings())
                .flatten();
            match uris {
                Some(uris) => Answer::Show(
                    uris.iter().filter_map(|uri| file_path(uri)).collect(),
                    member != "ShowFolders",
                ),
                None => Answer::Error(
                    "org.freedesktop.DBus.Error.InvalidArgs",
                    format!("{member} takes (as URIs, s StartupId)"),
                ),
            }
        }
        (Some("org.freedesktop.DBus.Introspectable") | None, "Introspect") => {
            Answer::Text(INTROSPECTION.into())
        }
        (Some("org.freedesktop.DBus.Peer") | None, "Ping") => Answer::Empty,
        _ => Answer::Error(
            "org.freedesktop.DBus.Error.UnknownMethod",
            format!("No method {member}"),
        ),
    }
}

/// The windows to open: each folder, or each item's folder once, the first
/// item there highlighted. A folder shown as an item opens itself.
fn places(paths: Vec<PathBuf>, items: bool) -> Vec<PathBuf> {
    let mut places: Vec<PathBuf> = Vec::new();
    let mut folders: Vec<PathBuf> = Vec::new();
    for path in paths {
        let folder = if items && !path.is_dir() {
            path.parent()
                .map_or_else(|| path.clone(), Path::to_path_buf)
        } else {
            path.clone()
        };
        if !folders.contains(&folder) {
            folders.push(folder);
            places.push(path);
        }
    }
    places
}

/// Runs the launcher on `path` on its own, reaping it when it ends.
fn launch(launcher: &Path, path: &Path) {
    let child = Command::new(launcher)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    if let Ok(mut child) = child {
        std::thread::spawn(move || child.wait());
    }
}

/// The local path of a `file://` URI (percent-decoded), or an absolute path
/// given as is; `None` for anything else.
pub fn file_path(uri: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    if uri.starts_with('/') {
        return Some(PathBuf::from(uri));
    }
    // `file:/path`, without an authority, is the short form (RFC 8089).
    let rest = match uri.strip_prefix("file://") {
        Some(rest) => rest,
        None => uri.strip_prefix("file:").filter(|p| p.starts_with('/'))?,
    };
    // An authority before the path: empty or this machine.
    let path = &rest[rest.find('/')?..];
    let host = &rest[..rest.len() - path.len()];
    if !host.is_empty() && host != "localhost" {
        return None;
    }
    let bytes = path.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(byte) = bytes
                .get(i + 1..i + 3)
                .filter(|hex| hex.iter().all(u8::is_ascii_hexdigit))
                .and_then(|hex| std::str::from_utf8(hex).ok())
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            decoded.push(byte);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    Some(PathBuf::from(std::ffi::OsString::from_vec(decoded)))
}

/// The session bus's socket address: `DBUS_SESSION_BUS_ADDRESS`'s first
/// usable `unix:path=` or `unix:abstract=` entry, else `$XDG_RUNTIME_DIR/bus`.
fn socket(address: Option<&str>) -> io::Result<UnixStream> {
    use std::os::linux::net::SocketAddrExt;
    let address = address
        .map(String::from)
        .or_else(|| std::env::var("DBUS_SESSION_BUS_ADDRESS").ok());
    if let Some(address) = address {
        for entry in address.split(';') {
            let Some(keys) = entry.strip_prefix("unix:") else {
                continue;
            };
            for pair in keys.split(',') {
                let Some((key, value)) = pair.split_once('=') else {
                    continue;
                };
                let value = file_path(&format!("file://{value}"))
                    .or_else(|| Some(PathBuf::from(value)))
                    .expect("set above");
                let connected = match key {
                    "path" => UnixStream::connect(&value),
                    "abstract" => std::os::unix::net::SocketAddr::from_abstract_name(
                        value.as_os_str().as_encoded_bytes(),
                    )
                    .and_then(|a| UnixStream::connect_addr(&a)),
                    _ => continue,
                };
                if let Ok(stream) = connected {
                    return Ok(stream);
                }
            }
        }
    }
    let runtime = std::env::var_os("XDG_RUNTIME_DIR")
        .ok_or_else(|| io::Error::other("no session bus: DBUS_SESSION_BUS_ADDRESS is unset"))?;
    UnixStream::connect(Path::new(&runtime).join("bus"))
}

struct Bus {
    stream: UnixStream,
    serial: u32,
    /// Calls that arrived while waiting for the bus to answer one of ours:
    /// the call that made the bus start fff comes before RequestName's reply.
    queued: VecDeque<Message>,
}

#[derive(Debug, Default)]
struct Message {
    big: bool,
    kind: u8,
    flags: u8,
    serial: u32,
    path: Option<String>,
    interface: Option<String>,
    member: Option<String>,
    reply_serial: Option<u32>,
    sender: Option<String>,
    signature: Option<String>,
    body: Vec<u8>,
}

/// A header field's value.
enum Field<'a> {
    Path(&'a str),
    Text(&'a str),
    Signature(&'a str),
    Number(u32),
}

impl Bus {
    fn connect(address: Option<&str>) -> io::Result<Self> {
        let mut stream = socket(address)?;
        let uid = rustix::process::getuid().as_raw().to_string();
        let hex: String = uid.bytes().map(|b| format!("{b:02x}")).collect();
        stream.write_all(format!("\0AUTH EXTERNAL {hex}\r\n").as_bytes())?;
        let mut line = Vec::new();
        let mut byte = [0];
        while !line.ends_with(b"\r\n") {
            stream.read_exact(&mut byte)?;
            line.push(byte[0]);
            if line.len() > 512 {
                break;
            }
        }
        if !line.starts_with(b"OK ") {
            return Err(io::Error::other(format!(
                "session bus refused authentication: {}",
                String::from_utf8_lossy(&line).trim()
            )));
        }
        stream.write_all(b"BEGIN\r\n")?;
        Ok(Self {
            stream,
            serial: 0,
            queued: VecDeque::new(),
        })
    }
    fn send<'a>(
        &mut self,
        kind: u8,
        mut fields: Vec<(u8, Field<'a>)>,
        signature: &'a str,
        body: &[u8],
    ) -> io::Result<u32> {
        self.serial += 1;
        let mut header = Writer::default();
        header.data.extend([b'l', kind, 0, 1]);
        header.u32(body.len() as u32);
        header.u32(self.serial);
        if !signature.is_empty() {
            fields.push((8, Field::Signature(signature)));
        }
        header.array(8, |w| {
            for (code, field) in &fields {
                w.pad(8);
                w.data.push(*code);
                match field {
                    Field::Path(s) => {
                        w.signature("o");
                        w.string(s)
                    }
                    Field::Text(s) => {
                        w.signature("s");
                        w.string(s)
                    }
                    Field::Signature(s) => {
                        w.signature("g");
                        w.signature(s)
                    }
                    Field::Number(n) => {
                        w.signature("u");
                        w.u32(*n)
                    }
                }
            }
        });
        header.pad(8);
        header.data.extend_from_slice(body);
        self.stream.write_all(&header.data)?;
        Ok(self.serial)
    }
    /// Calls a method of the bus itself and waits for its return.
    fn call(&mut self, member: &str, signature: &str, body: &[u8]) -> io::Result<Message> {
        let serial = self.send(
            METHOD_CALL,
            vec![
                (1, Field::Path("/org/freedesktop/DBus")),
                (2, Field::Text("org.freedesktop.DBus")),
                (3, Field::Text(member)),
                (6, Field::Text("org.freedesktop.DBus")),
            ],
            signature,
            body,
        )?;
        loop {
            let message = self.receive()?;
            if message.kind == METHOD_CALL {
                self.queued.push_back(message);
                continue;
            }
            if message.reply_serial == Some(serial) {
                return match message.kind {
                    METHOD_RETURN => Ok(message),
                    _ => Err(io::Error::other(format!(
                        "the session bus refused {member}"
                    ))),
                };
            }
        }
    }
    fn reply(&mut self, to: &Message, signature: &str, body: &[u8]) -> io::Result<()> {
        let mut fields = vec![(5, Field::Number(to.serial))];
        if let Some(sender) = &to.sender {
            fields.push((6, Field::Text(sender)));
        }
        self.send(METHOD_RETURN, fields, signature, body).map(drop)
    }
    fn error(&mut self, to: &Message, name: &str, text: &str) -> io::Result<()> {
        let mut fields = vec![(4, Field::Text(name)), (5, Field::Number(to.serial))];
        if let Some(sender) = &to.sender {
            fields.push((6, Field::Text(sender)));
        }
        let mut body = Writer::default();
        body.string(text);
        self.send(ERROR, fields, "s", &body.data).map(drop)
    }
    fn receive(&mut self) -> io::Result<Message> {
        let mut fixed = [0; 16];
        self.stream.read_exact(&mut fixed)?;
        let big = match fixed[0] {
            b'l' => false,
            b'B' => true,
            _ => return Err(io::Error::other("malformed D-Bus message")),
        };
        let number = |at: usize| {
            let bytes = [fixed[at], fixed[at + 1], fixed[at + 2], fixed[at + 3]];
            if big {
                u32::from_be_bytes(bytes)
            } else {
                u32::from_le_bytes(bytes)
            }
        };
        let (body_len, fields_len) = (number(4) as usize, number(12) as usize);
        // The bus limits messages to 128 MiB.
        if body_len + fields_len > 1 << 27 {
            return Err(io::Error::other("oversized D-Bus message"));
        }
        let header_len = (16 + fields_len).next_multiple_of(8);
        let mut data = fixed.to_vec();
        data.resize(header_len + body_len, 0);
        self.stream.read_exact(&mut data[16..])?;
        let mut message = Message {
            big,
            kind: fixed[1],
            flags: fixed[2],
            serial: number(8),
            body: data[header_len..].to_vec(),
            ..Message::default()
        };
        let mut reader = Reader::new(&data[..16 + fields_len], big);
        reader.pos = 16;
        while reader.pos < 16 + fields_len {
            reader.align(8);
            let (Some(code), Some(signature)) = (reader.byte(), reader.signature()) else {
                break;
            };
            let value = match signature.as_str() {
                "s" | "o" => reader.string().map(Value::Text),
                "g" => reader.signature().map(Value::Text),
                "u" => reader.u32().map(Value::Number),
                _ => None,
            };
            match (code, value) {
                (1, Some(Value::Text(s))) => message.path = Some(s),
                (2, Some(Value::Text(s))) => message.interface = Some(s),
                (3, Some(Value::Text(s))) => message.member = Some(s),
                (5, Some(Value::Number(n))) => message.reply_serial = Some(n),
                (7, Some(Value::Text(s))) => message.sender = Some(s),
                (8, Some(Value::Text(s))) => message.signature = Some(s),
                (_, None) => break,
                _ => {}
            }
        }
        Ok(message)
    }
}

enum Value {
    Text(String),
    Number(u32),
}

/// Little-endian marshalling, aligned from the start of `data`.
#[derive(Default)]
struct Writer {
    data: Vec<u8>,
}
impl Writer {
    fn pad(&mut self, align: usize) {
        self.data.resize(self.data.len().next_multiple_of(align), 0);
    }
    fn u32(&mut self, value: u32) {
        self.pad(4);
        self.data.extend(value.to_le_bytes());
    }
    fn string(&mut self, text: &str) {
        self.u32(text.len() as u32);
        self.data.extend(text.as_bytes());
        self.data.push(0);
    }
    fn signature(&mut self, text: &str) {
        self.data.push(text.len() as u8);
        self.data.extend(text.as_bytes());
        self.data.push(0);
    }
    /// An array of elements aligned to `align`: its length counts from the
    /// first element, after the padding.
    fn array(&mut self, align: usize, elements: impl FnOnce(&mut Self)) {
        self.u32(0);
        let at = self.data.len() - 4;
        self.pad(align);
        let start = self.data.len();
        elements(self);
        let len = (self.data.len() - start) as u32;
        self.data[at..at + 4].copy_from_slice(&len.to_le_bytes());
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
    big: bool,
}
impl<'a> Reader<'a> {
    fn new(data: &'a [u8], big: bool) -> Self {
        Self { data, pos: 0, big }
    }
    fn align(&mut self, align: usize) {
        self.pos = self.pos.next_multiple_of(align);
    }
    fn byte(&mut self) -> Option<u8> {
        let byte = *self.data.get(self.pos)?;
        self.pos += 1;
        Some(byte)
    }
    fn u32(&mut self) -> Option<u32> {
        self.align(4);
        let bytes: [u8; 4] = self.data.get(self.pos..self.pos + 4)?.try_into().ok()?;
        self.pos += 4;
        Some(if self.big {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        })
    }
    fn text(&mut self, len: usize) -> Option<String> {
        let bytes = self.data.get(self.pos..self.pos + len)?;
        self.pos += len + 1;
        String::from_utf8(bytes.to_vec()).ok()
    }
    fn string(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        self.text(len)
    }
    fn signature(&mut self) -> Option<String> {
        let len = self.byte()? as usize;
        self.text(len)
    }
    /// `as` followed by an `s`, as the FileManager1 methods take; the
    /// strings of the array.
    fn strings(&mut self) -> Option<Vec<String>> {
        let len = self.u32()? as usize;
        let end = self.pos + len;
        let mut strings = Vec::new();
        while self.pos < end {
            strings.push(self.string()?);
        }
        self.string()?;
        Some(strings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris_become_paths() {
        assert_eq!(
            file_path("file:///home/me/My%20Files/a%25b.pdf"),
            Some(PathBuf::from("/home/me/My Files/a%b.pdf"))
        );
        assert_eq!(file_path("file://localhost/tmp/x"), Some("/tmp/x".into()));
        assert_eq!(file_path("/tmp/plain"), Some("/tmp/plain".into()));
        assert_eq!(file_path("file:/tmp/x%20y"), Some("/tmp/x y".into()));
        assert_eq!(file_path("file:tmp/x"), None);
        assert_eq!(file_path("file://elsewhere/tmp/x"), None);
        assert_eq!(file_path("https://example.org/x"), None);
        // Undecodable escapes stay as they are; bytes need not be UTF-8.
        assert_eq!(file_path("file:///a%zz"), Some("/a%zz".into()));
        assert_eq!(file_path("file:///a%+5b"), Some("/a%+5b".into()));
        use std::os::unix::ffi::OsStrExt;
        assert_eq!(
            file_path("file:///caf%E9").unwrap().as_os_str().as_bytes(),
            b"/caf\xe9"
        );
    }

    #[test]
    fn calls_are_answered_by_member() {
        let mut body = Writer::default();
        body.array(4, |w| {
            w.string("file:///tmp/a.pdf");
            w.string("file:///tmp/b.pdf");
        });
        body.string("startup");
        let call = |member: &str| Message {
            path: Some(OBJECT.into()),
            interface: Some(NAME.into()),
            member: Some(member.into()),
            signature: Some("ass".into()),
            body: body.data.clone(),
            ..Message::default()
        };
        let both = vec![PathBuf::from("/tmp/a.pdf"), PathBuf::from("/tmp/b.pdf")];
        assert_eq!(answer(&call("ShowItems")), Answer::Show(both.clone(), true));
        assert_eq!(answer(&call("ShowFolders")), Answer::Show(both, false));
        assert!(matches!(answer(&call("Nope")), Answer::Error(..)));
        let mut wrong = call("ShowItems");
        wrong.signature = Some("s".into());
        assert!(matches!(answer(&wrong), Answer::Error(..)));
        let mut elsewhere = call("ShowItems");
        elsewhere.path = Some("/".into());
        assert!(matches!(answer(&elsewhere), Answer::Error(..)));
        let mut introspect = call("Introspect");
        introspect.interface = None;
        assert!(matches!(answer(&introspect), Answer::Text(xml) if xml.contains("ShowItems")));
        // Items in one folder open it once, highlighting the first.
        assert_eq!(
            places(
                vec!["/no/such/a".into(), "/no/such/b".into(), "/no/c".into()],
                true
            ),
            [PathBuf::from("/no/such/a"), PathBuf::from("/no/c")]
        );
    }

    #[test]
    fn a_private_bus_starts_fff_on_each_item() {
        let tools = ["dbus-daemon", "dbus-send"];
        if tools
            .iter()
            .any(|t| Command::new(t).arg("--version").output().is_err())
        {
            eprintln!("dbus-daemon or dbus-send unavailable; skipping");
            return;
        }
        let dir = std::env::temp_dir().join(format!("fff-dbus-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let socket = dir.join("bus");
        let config = dir.join("bus.conf");
        std::fs::write(
            &config,
            format!(
                "<!DOCTYPE busconfig PUBLIC \"-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN\" \
                 \"http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd\">\n\
                 <busconfig><type>session</type><listen>unix:path={}</listen>\
                 <auth>EXTERNAL</auth><policy context=\"default\">\
                 <allow send_destination=\"*\" eavesdrop=\"true\"/><allow eavesdrop=\"true\"/>\
                 <allow own=\"*\"/></policy></busconfig>\n",
                socket.display()
            ),
        )
        .unwrap();
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .arg("--nofork")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let started = std::time::Instant::now();
        while !socket.exists() && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        let address = format!("unix:path={}", socket.display());
        let log = dir.join("log");
        let launcher = dir.join("open-folder");
        std::fs::write(
            &launcher,
            format!("#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n", log.display()),
        )
        .unwrap();
        std::fs::set_permissions(
            &launcher,
            std::os::unix::fs::PermissionsExt::from_mode(0o755),
        )
        .unwrap();
        let service = {
            let (address, launcher) = (address.clone(), launcher.clone());
            std::thread::spawn(move || serve(Some(&address), &launcher, Duration::from_secs(2)))
        };
        let send = |member: &str, uri: &str| {
            let mut tries = 0;
            loop {
                let output = Command::new("dbus-send")
                    .arg(format!("--bus={address}"))
                    .args(["--print-reply", "--dest=org.freedesktop.FileManager1"])
                    .arg(OBJECT)
                    .arg(format!("org.freedesktop.FileManager1.{member}"))
                    .arg(format!("array:string:{uri}"))
                    .arg("string:")
                    .output()
                    .unwrap();
                // The service may not own the name yet.
                if output.status.success() || tries == 50 {
                    return output.status.success();
                }
                tries += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
        };
        assert!(send("ShowItems", "file:///downloads/My%20paper.pdf"));
        assert!(send("ShowFolders", "file:///downloads"));
        let wanted = "/downloads/My paper.pdf\n/downloads\n";
        let started = std::time::Instant::now();
        while std::fs::read_to_string(&log).unwrap_or_default() != wanted
            && started.elapsed() < Duration::from_secs(5)
        {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(std::fs::read_to_string(&log).unwrap_or_default(), wanted);
        // Idle, it exits, having owned the name.
        assert!(service.join().unwrap().unwrap());
        let _ = daemon.kill();
        let _ = daemon.wait();
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
