//! Pop-up notifications posted from any thread (for example when a program
//! opened in the background fails). The UI drains them into `App::notices`
//! and shows each for a few seconds.
use std::sync::{Mutex, mpsc::SyncSender};

struct Queue {
    pending: Vec<String>,
    waker: Option<SyncSender<()>>,
}
static QUEUE: Mutex<Queue> = Mutex::new(Queue {
    pending: Vec::new(),
    waker: None,
});

/// How long a notification stays on screen.
pub const SHOWN_FOR: std::time::Duration = std::time::Duration::from_secs(8);

/// Lets `post` wake the sleeping UI.
pub fn set_waker(waker: SyncSender<()>) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.waker = Some(waker);
    }
}
pub fn post(text: impl Into<String>) {
    if let Ok(mut queue) = QUEUE.lock() {
        queue.pending.push(text.into());
        if let Some(waker) = &queue.waker {
            let _ = waker.try_send(());
        }
    }
}
pub fn drain() -> Vec<String> {
    QUEUE
        .lock()
        .map(|mut queue| std::mem::take(&mut queue.pending))
        .unwrap_or_default()
}
