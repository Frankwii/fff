//! Palette reload polling runs off the input/render thread.
//!
//! The watcher reloads the palette named by the session chooser (Alt+p) when one
//! was applied, else the CLI pin, else `[ui] palette`. A generation counter
//! discards reloads computed for a name the session has since replaced. A
//! configuration file with errors gives way to the backup `gc` left beside it.
use crate::config::{Palette, Settings};
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    thread::{self, Thread},
    time::Duration,
};

/// A loaded palette together with every palette the chooser may offer.
#[derive(Clone, Debug, PartialEq)]
pub struct PaletteState {
    pub name: String,
    pub palette: Palette,
    pub available: Vec<(String, Palette)>,
}
impl PaletteState {
    pub fn from_settings(settings: &Settings) -> Self {
        Self {
            name: settings.palette_name.clone(),
            palette: settings.palette,
            available: settings.palettes.clone(),
        }
    }
}
struct Source {
    path: Option<PathBuf>,
    disabled: bool,
    pinned: Option<String>,
}
impl Source {
    fn load(&self, name: Option<&str>) -> Result<PaletteState, String> {
        Settings::load_or_backup(
            self.path.as_deref(),
            self.disabled,
            name.or(self.pinned.as_deref()),
        )
        .map(|s| PaletteState::from_settings(&s))
    }
}
/// Session override: (generation, chooser-applied palette name).
type Session = Arc<Mutex<(u64, Option<String>)>>;
type Update = (u64, Result<PaletteState, String>);
pub struct PaletteWatcher {
    source: Arc<Source>,
    session: Session,
    events: Option<Receiver<Update>>,
    stop: Arc<AtomicBool>,
    thread: Option<Thread>,
}
impl PaletteWatcher {
    /// Polling is skipped when `disabled` (`--no-config`); Alt+P still works.
    pub fn start(
        path: Option<PathBuf>,
        disabled: bool,
        pinned: Option<String>,
        initial: PaletteState,
    ) -> Self {
        let source = Arc::new(Source {
            path,
            disabled,
            pinned,
        });
        let session: Session = Arc::new(Mutex::new((0, None)));
        let stop = Arc::new(AtomicBool::new(false));
        let mut watcher = Self {
            source: source.clone(),
            session: session.clone(),
            events: None,
            stop: stop.clone(),
            thread: None,
        };
        if disabled {
            return watcher;
        }
        let (tx, events) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let mut previous = (0, Ok(initial));
            while !stop.load(Ordering::Relaxed) {
                let (generation, name) = session.lock().unwrap().clone();
                let next = (generation, source.load(name.as_deref()));
                if next != previous && tx.try_send(next.clone()).is_ok() {
                    previous = next;
                }
                thread::park_timeout(Duration::from_millis(500));
            }
        });
        watcher.events = Some(events);
        watcher.thread = Some(worker.thread().clone());
        watcher
    }
    /// Pending reloads for the current session choice; stale ones are dropped.
    pub fn updates(&self) -> Vec<Result<PaletteState, String>> {
        let current = self.session.lock().unwrap().0;
        self.events
            .iter()
            .flat_map(|events| events.try_iter())
            .filter(|(generation, _)| *generation == current)
            .map(|(_, update)| update)
            .collect()
    }
    /// Keep reloading `name`'s definition for the rest of the session.
    pub fn choose(&self, name: &str) {
        self.set_session(Some(name.to_string()));
    }
    /// Clear any session choice and re-read the configuration immediately.
    pub fn reload_now(&self) -> Result<PaletteState, String> {
        self.set_session(None);
        self.source.load(None)
    }
    /// Every setting, read again as the session started reading them (`gc`).
    pub fn settings(&self) -> Result<Settings, String> {
        Settings::load_or_backup(
            self.source.path.as_deref(),
            self.source.disabled,
            self.source.pinned.as_deref(),
        )
    }
    /// Read the palette again now rather than at the next poll.
    pub fn refresh(&self) {
        if let Some(thread) = &self.thread {
            thread.unpark();
        }
    }
    fn set_session(&self, name: Option<String>) {
        let mut session = self.session.lock().unwrap();
        *session = (session.0 + 1, name);
        drop(session);
        if let Some(thread) = &self.thread {
            thread.unpark();
        }
    }
}
impl Drop for PaletteWatcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = &self.thread {
            thread.unpark();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn wait(watcher: &PaletteWatcher) -> Result<PaletteState, String> {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            if let Some(update) = watcher.updates().pop() {
                return update;
            }
            assert!(Instant::now() < deadline, "no palette update arrived");
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn session_choice_tracks_its_definition_and_reload_now_returns_to_config() {
        let folder =
            std::env::temp_dir().join(format!("fff-reload-session-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let config = folder.join("config.toml");
        std::fs::write(
            &config,
            "[ui]\npalette='nord'\n[palettes.mine]\nextends='light'\n",
        )
        .unwrap();
        let initial =
            PaletteState::from_settings(&Settings::load(Some(&config), false, None).unwrap());
        assert_eq!(initial.name, "nord");
        assert!(initial.available.iter().any(|(n, _)| n == "mine"));
        let watcher = PaletteWatcher::start(Some(config.clone()), false, None, initial);
        watcher.choose("mine");
        let chosen = wait(&watcher).unwrap();
        assert_eq!(chosen.name, "mine");
        // Editing the chosen definition reloads it; `[ui] palette` stays ignored.
        std::fs::write(
            &config,
            "[ui]\npalette='forest'\n[palettes.mine]\nextends='light'\naccent='#010203'\n",
        )
        .unwrap();
        let edited = wait(&watcher).unwrap();
        assert_eq!(edited.name, "mine");
        assert_eq!(edited.palette.accent, ratatui::style::Color::Rgb(1, 2, 3));
        let reloaded = watcher.reload_now().unwrap();
        assert_eq!(reloaded.name, "forest");
        assert_eq!(reloaded.palette, Palette::named("forest").unwrap());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn reload_now_honors_cli_pin_and_works_without_config() {
        let watcher = PaletteWatcher::start(
            None,
            true,
            Some("gruvbox-dark".into()),
            PaletteState::from_settings(&Settings::default()),
        );
        watcher.choose("nord");
        assert!(watcher.updates().is_empty());
        let state = watcher.reload_now().unwrap();
        assert_eq!(state.name, "gruvbox-dark");
        assert_eq!(state.palette, Palette::named("gruvbox-dark").unwrap());
        assert_eq!(state.available.len(), Palette::names().len());
    }
}
