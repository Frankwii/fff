use super::*;

/// The terminal, and whether fff captures the mouse (`[ui] mouse`).
pub(super) struct Terminal(
    pub(super) TuiTerminal<CrosstermBackend<std::fs::File>>,
    bool,
);
impl Terminal {
    pub(super) fn enter(mouse: bool) -> io::Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
        let tui = TuiTerminal::new(CrosstermBackend::new(file))?;
        terminal::enable_raw_mode()?;
        let mut guard = Self(tui, mouse);
        execute!(
            guard.0.backend_mut(),
            EnterAlternateScreen,
            Hide,
            EnableBracketedPaste,
            PushKeyboardEnhancementFlags(
                Flags::DISAMBIGUATE_ESCAPE_CODES | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            )
        )?;
        guard.capture_mouse(true)?;
        Ok(guard)
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
        let _ = execute!(
            self.0.backend_mut(),
            EndSynchronizedUpdate,
            PopKeyboardEnhancementFlags,
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
            PushKeyboardEnhancementFlags(
                Flags::DISAMBIGUATE_ESCAPE_CODES | Flags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
            ),
            terminal::Clear(terminal::ClearType::All)
        )?;
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
        let _ = execute!(
            self.0.backend_mut(),
            EndSynchronizedUpdate,
            PopKeyboardEnhancementFlags,
            Show,
            DisableBracketedPaste,
            SetCursorStyle::DefaultUserShape,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

/// Crossterm has one reader. Input and discovery notify the same sleeping UI;
/// the bounded input queue preserves order and applies backpressure on pastes.
pub(super) struct Input {
    pub(super) events: std::sync::mpsc::Receiver<io::Result<Event>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Input {
    pub(super) fn start(wake: std::sync::mpsc::SyncSender<()>) -> Self {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        };
        let (tx, events) = mpsc::sync_channel(64);
        let cancel = Arc::new(AtomicBool::new(false));
        let stopped = cancel.clone();
        let thread = std::thread::spawn(move || {
            while !stopped.load(Ordering::Relaxed) {
                let event = match event::poll(Duration::from_millis(20)) {
                    Ok(false) => continue,
                    Ok(true) => event::read(),
                    Err(error) => Err(error),
                };
                let failed = event.is_err();
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
            cancel: Default::default(),
            thread: None,
        }
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        // Disconnect before joining, in case the bounded queue is full.
        let (_, empty) = std::sync::mpsc::channel();
        drop(std::mem::replace(&mut self.events, empty));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
