//! Palette reload polling runs off the input/render thread.
use crate::config::{Palette, Settings};
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, Thread},
    time::Duration,
};
pub struct PaletteWatcher {
    events: Receiver<Result<Palette, String>>,
    stop: Arc<AtomicBool>,
    thread: Thread,
}
impl PaletteWatcher {
    pub fn start(
        path: Option<PathBuf>,
        disabled: bool,
        pinned: Option<String>,
        initial: Palette,
    ) -> Option<Self> {
        if disabled {
            return None;
        }
        let (tx, events) = mpsc::sync_channel(1);
        let stop = Arc::new(AtomicBool::new(false));
        let stopped = stop.clone();
        let worker = thread::spawn(move || {
            let mut previous = Ok(initial);
            while !stopped.load(Ordering::Relaxed) {
                let next =
                    Settings::load(path.as_deref(), false, pinned.as_deref()).map(|s| s.palette);
                if next != previous && tx.try_send(next.clone()).is_ok() {
                    previous = next;
                }
                thread::park_timeout(Duration::from_millis(500));
            }
        });
        Some(Self {
            events,
            stop,
            thread: worker.thread().clone(),
        })
    }
    pub fn updates(&self) -> impl Iterator<Item = Result<Palette, String>> + '_ {
        self.events.try_iter()
    }
}
impl Drop for PaletteWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.unpark();
    }
}
