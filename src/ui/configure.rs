//! gc: the configuration file in `$EDITOR`, then everything it sets applied
//! to the running session.
use super::*;
use crate::config::{Settings, backup_path, brief};

impl Session {
    /// gc: edits the configuration file in this terminal, then reads it
    /// again. Before, a copy that loads is kept beside it as its backup; an
    /// edit with errors leaves the session reading that copy.
    pub(super) fn edit_config(&mut self, app: &mut App) -> io::Result<()> {
        let Some(path) = app.config_path.clone() else {
            app.status = "No configuration file to edit: fff started with --no-config".into();
            return Ok(());
        };
        // The backup is the last configuration that loaded: while the file
        // has errors, it is already the one in use and stays as it is.
        let loads = matches!(self.palettes.settings(), Ok(s) if s.broken.is_none());
        if loads
            && path.exists()
            && let Err(error) = std::fs::copy(&path, backup_path(&path))
        {
            app.status = format!(
                "Could not back up the configuration to {}: {error}",
                backup_path(&path).display()
            );
            return Ok(());
        }
        // A configuration not written yet starts as a new file there.
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match crate::run::editor_plan(vec![path], &app.cwd, app.home.as_deref()) {
            Ok(plan) => run_here(
                &mut self.terminal,
                &mut self.input,
                &self.wake,
                app,
                &mut self.discovery,
                &plan,
            )?,
            Err(error) => {
                app.status = error;
                return Ok(());
            }
        }
        self.reload_config(app)
    }
    /// Reads the configuration again and applies it; its backup goes once
    /// the file loads.
    fn reload_config(&mut self, app: &mut App) -> io::Result<()> {
        let Some(path) = app.config_path.clone() else {
            return Ok(());
        };
        let settings = match self.palettes.settings() {
            Ok(settings) => settings,
            Err(error) => {
                app.status = format!("{}; the settings in use stay", brief(&error));
                return Ok(());
            }
        };
        let broken = settings.broken.clone();
        if let Err(error) = self.apply_settings(app, settings)? {
            app.status = format!("{error}; the settings in use stay");
            return Ok(());
        }
        app.config_broken = broken;
        app.status = match app.config_errors_status() {
            Some(status) => status,
            None => {
                let backup = backup_path(&path);
                match std::fs::remove_file(&backup) {
                    Err(error) if error.kind() != io::ErrorKind::NotFound => format!(
                        "Configuration reloaded; could not remove {}: {error}",
                        backup.display()
                    ),
                    _ => {
                        app.status_success = "Configuration reloaded".into();
                        app.status_success.clone()
                    }
                }
            }
        };
        Ok(())
    }
    /// Everything `settings` sets, as `main` applies it at startup, except
    /// `[ui] preview`, which only says how a session starts. The palette is
    /// the watcher's: it keeps a palette chosen with Alt+p.
    fn apply_settings(
        &mut self,
        app: &mut App,
        settings: Settings,
    ) -> io::Result<Result<(), String>> {
        // keys.toml is read again too; with errors, the keys in use stay.
        app.keys_path = settings.bookmark_path.as_deref().map(keymap::path_beside);
        if let Some(path) = &app.keys_path {
            match keymap::Keymap::load(path) {
                Ok(keys) => {
                    app.keymap = keys;
                    app.keys_broken = None;
                }
                Err(error) => return Ok(Err(format!("keys.toml: {error}"))),
            }
        }
        app.openers = settings.open;
        app.bookmarks = settings.bookmarks;
        app.bookmark_store = crate::bookmarks::Store::new(
            settings.bookmark_path,
            app.bookmarks.clone(),
            app.home.clone(),
        );
        app.icons = settings.icons && !app.no_icons;
        app.status_timeout = settings.message_timeout;
        app.columns = settings.columns;
        self.terminal.set_mouse(settings.mouse)?;
        app.mouse = settings.mouse;
        app.buttons = settings.buttons;
        app.buttons_timeout = settings.buttons_timeout;
        if app.watch != settings.watch {
            app.watch = settings.watch;
            self.watch = app.watch.then(crate::watch::Watch::new).flatten();
            self.watch_basis = None;
        }
        app.preview.highlight = settings.highlight;
        app.trash = settings
            .trash
            .enabled
            .then(|| crate::trash::Trash::from_env(settings.trash.days, settings.trash.ask_above))
            .flatten();
        app.refresh_trash();
        self.discovery.configure(settings.cache);
        self.discovery.refresh_every = Duration::from_millis(settings.performance.refresh_ms);
        self.discovery.max_paths = settings.performance.max_paths;
        app.cached_queries = settings.performance.cached_queries;
        if app.config_rules != settings.exclusions {
            // The new rules lead; the session's own follow, as they were.
            let old = std::mem::replace(&mut app.config_rules, settings.exclusions);
            let rules: Vec<String> = app
                .config_rules
                .iter()
                .cloned()
                .chain(
                    app.policy
                        .rules
                        .iter()
                        .filter(|rule| !old.contains(rule) && !app.config_rules.contains(rule))
                        .cloned(),
                )
                .collect();
            if rules != app.policy.rules && app.policy.set_rules(rules).is_ok() && app.policy.ignore
            {
                self.discovery.start(app, false);
            }
        }
        self.palettes.refresh();
        Ok(Ok(()))
    }
}
