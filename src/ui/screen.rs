use super::*;

/// The terminal, whether fff captures the mouse (`[ui] mouse`), and what the
/// terminal supports.
pub(super) struct Terminal(
    pub(super) TuiTerminal<CrosstermBackend<Tty>>,
    bool,
    pub(super) Support,
);
impl Terminal {
    pub(super) fn enter(mouse: bool) -> io::Result<Self> {
        let mut file = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        terminal::enable_raw_mode()?;
        let support = Support::probe(&mut file);
        let tui = TuiTerminal::new(CrosstermBackend::new(Tty::new(file)))?;
        let mut guard = Self(tui, mouse, support);
        guard.keyboard(true)?;
        execute!(
            guard.0.backend_mut(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste,
        )?;
        guard.capture_mouse(true)?;
        Ok(guard)
    }
    /// Kitty keyboard flags, where supported: Escape and modified keys arrive
    /// unambiguously, and shifted keys as the character they type.
    fn keyboard(&mut self, on: bool) -> io::Result<()> {
        match (self.2.keyboard, on) {
            (true, true) => execute!(
                self.0.backend_mut(),
                PushKeyboardEnhancementFlags(
                    Flags::DISAMBIGUATE_ESCAPE_CODES
                        | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
                        | Flags::REPORT_ALTERNATE_KEYS
                )
            ),
            (true, false) => execute!(self.0.backend_mut(), PopKeyboardEnhancementFlags),
            _ => Ok(()),
        }
    }
    /// Brackets a frame so the terminal shows it at once, where supported.
    pub(super) fn synchronize(&mut self, begin: bool) -> io::Result<()> {
        match (self.2.synchronized, begin) {
            (true, true) => execute!(self.0.backend_mut(), BeginSynchronizedUpdate),
            (true, false) => execute!(self.0.backend_mut(), EndSynchronizedUpdate),
            _ => Ok(()),
        }
    }
    /// `[ui] mouse` changed: capture the mouse from now on, or stop.
    pub(super) fn set_mouse(&mut self, mouse: bool) -> io::Result<()> {
        if self.1 != mouse {
            self.capture_mouse(false)?;
            self.1 = mouse;
            self.capture_mouse(true)?;
        }
        Ok(())
    }
    fn capture_mouse(&mut self, on: bool) -> io::Result<()> {
        match (self.1, on) {
            (true, true) => execute!(self.0.backend_mut(), EnableMouseCapture),
            (true, false) => execute!(self.0.backend_mut(), DisableMouseCapture),
            _ => Ok(()),
        }
    }
}
impl Terminal {
    /// Hands the terminal to another program: fff's screen and modes are undone.
    pub(super) fn suspend(&mut self) {
        let _ = self.capture_mouse(false);
        let _ = self.synchronize(false);
        let _ = self.keyboard(false);
        let _ = execute!(
            self.0.backend_mut(),
            Show,
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
    /// Takes the terminal back and redraws everything on the next frame.
    pub(super) fn resume(&mut self) -> io::Result<()> {
        terminal::enable_raw_mode()?;
        execute!(
            self.0.backend_mut(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste,
            terminal::Clear(terminal::ClearType::All)
        )?;
        self.keyboard(true)?;
        self.capture_mouse(true)?;
        // Forget both frames so the next draw sends every cell. ratatui's own
        // clear() would query the cursor position, which needs a reply.
        self.0.swap_buffers();
        self.0.swap_buffers();
        Ok(())
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        let _ = self.capture_mouse(false);
        let _ = self.synchronize(false);
        let _ = self.keyboard(false);
        let _ = execute!(
            self.0.backend_mut(),
            Show,
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

/// What the terminal supports beyond the basics, asked once at startup.
/// Terminals may log sequences they do not know (st does, on every frame), so
/// fff sends only those confirmed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Support {
    /// The kitty keyboard protocol.
    pub(super) keyboard: bool,
    /// Synchronized output (mode 2026).
    pub(super) synchronized: bool,
}
impl Support {
    /// Assumed when the terminal answers nothing at all.
    const ASSUMED: Self = Self {
        keyboard: true,
        synchronized: true,
    };
    /// Asks for the keyboard flags (`CSI ? u`) and mode 2026 (DECRQM), then
    /// for the primary device attributes, which every terminal answers: the
    /// replies before that one say what is supported. Raw mode must be on.
    /// Keys typed during the round trip are lost.
    fn probe(tty: &mut std::fs::File) -> Self {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;
        if tty
            .write_all(b"\x1b[?u\x1b[?2026$p\x1b[c")
            .and_then(|()| tty.flush())
            .is_err()
        {
            return Self::ASSUMED;
        }
        let deadline = std::time::Instant::now() + Duration::from_millis(500);
        let mut replies = Vec::new();
        loop {
            if let Some(support) = Self::parse(&replies) {
                return support;
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            let mut fd = libc::pollfd {
                fd: tty.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd for the duration of the call.
            if left.is_zero() || unsafe { libc::poll(&mut fd, 1, left.as_millis() as i32) } <= 0 {
                return Self::ASSUMED;
            }
            let mut chunk = [0; 256];
            match tty.read(&mut chunk) {
                Ok(0) | Err(_) => return Self::ASSUMED,
                Ok(n) => replies.extend_from_slice(&chunk[..n]),
            }
        }
    }
    /// The support the replies show, once the device attributes have arrived.
    pub(super) fn parse(replies: &[u8]) -> Option<Self> {
        let mut support = Self {
            keyboard: false,
            synchronized: false,
        };
        let mut rest = replies;
        while let Some(at) = rest.windows(3).position(|w| w == b"\x1b[?") {
            rest = &rest[at + 3..];
            let end = rest
                .iter()
                .position(|b| !(b.is_ascii_digit() || matches!(b, b';' | b'$')))?;
            let params = &rest[..end];
            match rest[end] {
                b'c' => return Some(support),
                b'u' => support.keyboard = true,
                // Mode 2026 set (1), reset (2) or permanently set (3).
                b'y' => {
                    if let Some(state) = params.strip_prefix(b"2026;") {
                        support.synchronized |= matches!(state, b"1$" | b"2$" | b"3$");
                    }
                }
                _ => {}
            }
            rest = &rest[end + 1..];
        }
        None
    }
}

/// The terminal device, written a frame at a time. Drops the underline color
/// resets ratatui sends after every frame: fff never sets an underline color,
/// and terminals without them log each reset (st).
pub(super) struct Tty {
    file: std::fs::File,
    pending: Vec<u8>,
}
impl Tty {
    pub(super) fn new(file: std::fs::File) -> Self {
        Self {
            file,
            pending: Vec::new(),
        }
    }
}
const UNDERLINE_RESET: &[u8] = b"\x1b[59m";
impl io::Write for Tty {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.pending.extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        let mut out = Vec::with_capacity(self.pending.len());
        let mut rest = &self.pending[..];
        while let Some(at) = rest
            .windows(UNDERLINE_RESET.len())
            .position(|w| w == UNDERLINE_RESET)
        {
            out.extend_from_slice(&rest[..at]);
            rest = &rest[at + UNDERLINE_RESET.len()..];
        }
        out.extend_from_slice(rest);
        self.pending.clear();
        self.file.write_all(&out)?;
        self.file.flush()
    }
}
impl Drop for Tty {
    fn drop(&mut self) {
        let _ = io::Write::flush(self);
    }
}

/// Crossterm has one reader. Input and discovery notify the same sleeping UI;
/// the bounded input queue preserves order and applies backpressure on pastes.
pub(super) struct Input {
    events: std::sync::mpsc::Receiver<io::Result<Event>>,
    /// Events sent and not yet taken, so background work can yield to them.
    pub(super) waiting: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Input {
    /// The next event, if one is waiting.
    pub(super) fn try_take(&self) -> Result<io::Result<Event>, std::sync::mpsc::TryRecvError> {
        let event = self.events.try_recv()?;
        self.waiting
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
        Ok(event)
    }
    /// `waiting` counts the events read and not yet taken.
    pub(super) fn start(
        wake: std::sync::mpsc::SyncSender<()>,
        waiting: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let (tx, events) = mpsc::sync_channel(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let sending = waiting.clone();
        // Waiting is done here rather than in crossterm, which reads a
        // hung-up terminal's end of file forever without returning.
        let tty = std::fs::File::open("/dev/tty").ok();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let wait = Duration::from_millis(20);
                let event = if tty.as_ref().is_some_and(|tty| hung_up(tty, wait)) {
                    Err(io::Error::other("the terminal was closed"))
                } else {
                    let ready = if tty.is_some() {
                        event::poll(Duration::ZERO)
                    } else {
                        event::poll(wait)
                    };
                    match ready {
                        Ok(false) => continue,
                        Ok(true) => event::read().map(shifted),
                        Err(error) => Err(error),
                    }
                };
                let failed = event.is_err();
                // Counted first, so taking it never sees the count below zero.
                sending.fetch_add(1, Ordering::Relaxed);
                if tx.send(event).is_err() {
                    break;
                }
                let _ = wake.try_send(());
                if failed {
                    break;
                }
            }
        });
        Self {
            events,
            waiting,
            cancel,
            thread: Some(thread),
        }
    }
}
impl Input {
    /// A stand-in that reads nothing, while another program owns the terminal.
    pub(super) fn idle() -> Self {
        Self {
            events: std::sync::mpsc::sync_channel(1).1,
            waiting: Default::default(),
            cancel: Default::default(),
            thread: None,
        }
    }
}
/// Waits up to `wait` for input on `tty`; true once the terminal has hung up.
fn hung_up(tty: &std::fs::File, wait: Duration) -> bool {
    use std::os::fd::AsRawFd;
    let mut fd = libc::pollfd {
        fd: tty.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd for the duration of the call.
    let ready = unsafe { libc::poll(&mut fd, 1, wait.as_millis() as i32) };
    ready > 0 && fd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0
}
impl Drop for Input {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // Disconnect before joining, in case the bounded queue is full.
        let (_, empty) = std::sync::mpsc::channel();
        drop(std::mem::replace(&mut self.events, empty));
        if let Some(thread) = self.thread.take() {
            // The reader stops within a poll, so a program run next never
            // loses keys to it. One stuck in crossterm (the terminal went
            // away) is left behind rather than holding fff open.
            let deadline = std::time::Instant::now() + Duration::from_millis(500);
            while !thread.is_finished() && std::time::Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(1));
            }
            if thread.is_finished() {
                let _ = thread.join();
            }
        }
        // The events left unread are gone with the queue.
        self.waiting.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}

/// Shifted letters arrive as legacy terminals send them, an uppercase
/// letter with Shift, so they type and bind alike: without alternate keys the
/// kitty protocol reports Shift+a as `a` with Shift, and with them as `A`
/// without it.
fn shifted(event: Event) -> Event {
    use crossterm::event::{KeyCode, KeyModifiers};
    match event {
        Event::Key(mut key) => {
            if let KeyCode::Char(c) = key.code {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    key.code = KeyCode::Char(c.to_ascii_uppercase());
                } else if c.is_uppercase() {
                    key.modifiers |= KeyModifiers::SHIFT;
                }
            }
            Event::Key(key)
        }
        event => event,
    }
}
#[cfg(test)]
#[test]
fn shifted_letters_arrive_uppercase_with_shift() {
    use crossterm::event::{KeyCode, KeyModifiers};
    let key = |c, m| Event::Key(KeyEvent::new(KeyCode::Char(c), m));
    let shift = KeyModifiers::SHIFT;
    assert_eq!(shifted(key('a', shift)), key('A', shift));
    assert_eq!(shifted(key('A', KeyModifiers::NONE)), key('A', shift));
    assert_eq!(
        shifted(key('P', KeyModifiers::ALT)),
        key('P', KeyModifiers::ALT | shift)
    );
    let plain = KeyModifiers::NONE;
    assert_eq!(shifted(key('a', plain)), key('a', plain));
    assert_eq!(shifted(key('!', plain)), key('!', plain));
}
