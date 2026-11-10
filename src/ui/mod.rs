use crate::{
    App,
    discovery::Discovery,
    editor::{Field, Nvim},
    keymap::{self, Function, Step},
    program::Program,
    rows,
    workspaces::{Change as WorkspaceChange, Draft as WorkspaceDraft, EditField as WorkspaceField},
};
use crossterm::{
    cursor::{Hide, MoveTo, SetCursorStyle, Show},
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode as K, KeyEvent, KeyEventKind, KeyModifiers as M,
        KeyboardEnhancementFlags as Flags, MouseButton, MouseEvent, MouseEventKind,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        self, BeginSynchronizedUpdate, EndSynchronizedUpdate, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{Terminal as TuiTerminal, backend::CrosstermBackend};
use std::{io, path::PathBuf, time::Duration};

mod actions;
mod configure;
mod dispatch;
mod focus;
mod input;
mod jobs;
pub(crate) mod keys;
mod mouse;
mod screen;
#[cfg(test)]
mod tests;
pub use actions::ScopedAction;
use actions::*;
use focus::{Row, scroll_preview};
use input::*;
pub use input::{StatusTimer, alt_numbered_key, start_editor};
pub(crate) use jobs::Job;
use jobs::*;
pub(crate) use keys::{EditStep, Editor, KeysScreen, Listed};
pub(crate) use mouse::Button;
use mouse::*;
use screen::*;

#[derive(Default)]
pub(crate) enum Focus {
    #[default]
    Results,
    Marks {
        rows: Vec<PathBuf>,
        index: usize,
    },
    /// F2: the session details.
    Info {
        offset: usize,
    },
    /// F1: the Keys screen, a picker over every function and its keys.
    Keys(Box<KeysScreen>),
    /// A list of choices filtered by what is typed (`Picker`): `:`'s
    /// functions or the sorting picker's orders; `back` is the focus it was
    /// opened from, for the choice and for going back.
    Picker {
        query: String,
        index: usize,
        kind: Picker,
        back: Box<Focus>,
    },
    Exclusions(String),
    /// Alt+f: the file filters (`--glob`), one per line.
    Filters(String),
    Operation(crate::operations::Draft),
    DeleteConfirm(crate::operations::DeletePlan),
    /// x on entries too large for the trash, or that cannot go there.
    TrashConfirm(crate::operations::DeletePlan),
    /// Checks before a confirmation taking a while: their progress, and
    /// Ctrl+c to stop them.
    Busy(Job),
    /// The jobs list (`toggle_job_list`), beside the marks: x stops the
    /// highlighted operation and undoes what it did.
    Jobs {
        index: usize,
    },
    /// Archives checked for extraction; the session opens the destination
    /// prompt next.
    Extracting(Vec<PathBuf>),
    /// Leaving while operations run: whether to stop them first.
    QuitConfirm(Box<Finish>),
    /// Leaving once the stopped operations have ended.
    Leaving(Box<Finish>),
    /// Which paths an action applies to, asked only when ambiguous: the current
    /// entry is unmarked while other entries are marked.
    ScopeChoice {
        current: PathBuf,
        selected: Vec<PathBuf>,
        action: ScopedAction,
    },
    TransferConfirm(crate::operations::TransferPlan),
    /// gx / gX: the command line, then its review before it runs.
    Command(crate::run::Prompt),
    RunConfirm(crate::run::Plan),
    NewChoice,
    /// `switch_program`: which program to go on as.
    ProgramChoice,
    /// A name is taken (fff explorer save, R): replace it, keep both or
    /// go back.
    Replace(Replace),
    Location(crate::location::Location),
    Workspaces {
        query: String,
        index: usize,
    },
    WorkspaceEdit(WorkspaceDraft),
    WorkspaceIcons {
        draft: WorkspaceDraft,
        query: String,
        index: usize,
    },
    WorkspaceDelete {
        target: usize,
        query: String,
        index: usize,
    },
    /// Alt+p chooser; `original` is restored on Escape after live previews.
    Palettes {
        query: String,
        index: usize,
        original: crate::config::Palette,
    },
}
/// Names that are taken, and what waits for the choice.
pub(crate) struct Replace {
    /// Each taken path with the free name beside it that keeping both uses.
    pub taken: Vec<(PathBuf, PathBuf)>,
    pub then: Replacing,
}
pub(crate) enum Replacing {
    /// fff explorer save: the answer.
    Save,
    /// R: the entries going back from the trash.
    Restore(crate::operations::RestorePlan),
}
/// How a session ends.
pub enum Finish {
    /// Confirmed paths, or the directory of a quit key.
    Paths(Vec<PathBuf>),
    /// Normal-mode Enter: exit into this folder (printed by fff pick).
    Folder(PathBuf),
    /// Ctrl+c.
    Cancelled,
    /// gX: the terminal is restored; the caller replaces fff with the program.
    Exec(crate::run::Plan),
}
/// How often the progress of a running operation is redrawn.
const PROGRESS_FRAME: Duration = Duration::from_millis(100);
/// While a scan runs, what it changes (and the rankings and contents search
/// following it) is drawn at most this often, leaving the time between to
/// the scan; input still draws at once.
const SCAN_FRAME: Duration = Duration::from_millis(33);
/// How long a change to the current directory's listing waits for more
/// changes before the quiet rescan.
const RELIST_DELAY: Duration = Duration::from_millis(150);
/// The same while file operations run, which change folders for long.
const RELIST_DELAY_BUSY: Duration = Duration::from_secs(1);
/// How long a contents match rests, its plain lines drawn, before their
/// syntax colors are asked for.
const COLORS_REST: Duration = Duration::from_millis(30);
/// Lines either side of a contents match whose colors are asked for.
const COLORS_AROUND: usize = 250;
/// A match within this many lines of its file's start has its colors
/// found from the start, where the syntax begins.
const COLORS_FROM_START: usize = 1000;
/// Lines either side of a match that, already colored, need no asking.
const COLORS_NEAR: usize = 50;
/// Bytes of a line, and of all the lines, sent for colors: Neovim's classic
/// syntax looks at each character.
const COLORS_LINE: usize = 400;
const COLORS_BUDGET: usize = 128 << 10;
/// Times a match's colors are asked for when no answer comes.
const COLORS_TRIES: u8 = 2;
/// Whether `event` only edits the query being typed in Insert mode, which
/// needs no current rows.
fn types_query(app: &App, focus: &Focus, event: &Event) -> bool {
    let typing = matches!(focus, Focus::Results)
        && app.edit_view.as_ref().is_some_and(|v| {
            matches!(v.field, Field::Query | Field::Contents) && v.mode.starts_with('i')
        });
    typing
        && match event {
            Event::Paste(_) | Event::Resize(..) => true,
            Event::Key(key) => {
                !key.modifiers.intersects(M::CONTROL | M::ALT)
                    && matches!(
                        key.code,
                        K::Char(_)
                            | K::Backspace
                            | K::Delete
                            | K::Left
                            | K::Right
                            | K::Home
                            | K::End
                    )
            }
            _ => false,
        }
}
/// The directories live updates watch: the current directory and the pinned
/// roots, then the folders whose entries are listed, shallowest first; and
/// how many lead as roots. While a scan runs, those already watched stay if
/// still in reach.
fn watched_dirs(app: &App, watch: &crate::watch::Watch) -> (Vec<PathBuf>, usize) {
    let mut dirs = vec![app.cwd.clone()];
    dirs.extend(app.scope.iter().filter(|r| **r != app.cwd).cloned());
    let roots = dirs.len();
    // Folders below `depth - 1` have their entries beyond the depth.
    let within = |dir: &std::path::Path| {
        app.scope.iter().any(|root| {
            dir.strip_prefix(root)
                .is_ok_and(|rest| (1..app.depth).contains(&rest.components().count()))
        })
    };
    let mut deeper: Vec<(usize, PathBuf)> = if app.scanning {
        watch
            .watched()
            .filter(|dir| within(dir))
            .map(|dir| (dir.components().count(), dir.to_path_buf()))
            .collect()
    } else {
        app.known
            .values()
            .filter(|c| c.directory && c.depth < app.depth && app.eligible(c))
            .map(|c| (c.depth, c.path.clone()))
            .collect()
    };
    deeper.sort_unstable();
    // Copies and extractions being written change too often to follow.
    deeper.retain(|(_, dir)| !crate::operations::staged(dir));
    dirs.extend(
        deeper
            .into_iter()
            .map(|(_, dir)| dir)
            .take(crate::watch::MOST.saturating_sub(dirs.len())),
    );
    (dirs, roots)
}
/// Runs a session, then remembers where it was for the next one.
pub fn run(
    mut app: App,
    cache: crate::config::CacheConfig,
    palettes: crate::reload::PaletteWatcher,
    performance: crate::config::PerformanceConfig,
) -> io::Result<(Finish, crate::program::Program)> {
    let finish = session(&mut app, cache, palettes, performance);
    app.remember();
    if let Some(path) = crate::state::path() {
        let state = crate::state::State {
            positions: std::mem::take(&mut app.positions),
        };
        let _ = crate::state::save(&path, &state);
    }
    // What the session ends as decides what it outputs (`switch_program`).
    Ok((finish?, app.program()))
}
/// The session loop: each pass catches up with background work, draws when
/// something changed, then waits for and handles one event.
fn session(
    app: &mut App,
    cache: crate::config::CacheConfig,
    palettes: crate::reload::PaletteWatcher,
    performance: crate::config::PerformanceConfig,
) -> io::Result<Finish> {
    let mut session = Session::start(app, cache, palettes, performance)?;
    loop {
        session.poll(app)?;
        session.sync_editor(app)?;
        session.follow_preview(app);
        session.set_cursor_shape(app)?;
        if session.dirty || session.drawn_at.elapsed() >= Duration::from_secs(1) {
            session.draw(app)?;
        }
        // Leaving once nothing runs, also when the last operation ended
        // while the question was asked.
        if matches!(session.focus, Focus::Leaving(_) | Focus::QuitConfirm(_)) && app.jobs.is_empty()
        {
            let (Focus::Leaving(finish) | Focus::QuitConfirm(finish)) =
                std::mem::take(&mut session.focus)
            else {
                unreachable!()
            };
            return Ok(*finish);
        }
        let Some(event) = session.next_event(app)? else {
            continue;
        };
        if let Some(finish) = session.handle(app, event)? {
            // Running operations are stopped first, once confirmed.
            if app.jobs.is_empty() {
                return Ok(finish);
            }
            session.focus = Focus::QuitConfirm(Box::new(finish));
        }
    }
}
/// Where the last frame drew what the mouse and digit keys pick.
struct Drawn {
    /// Rows a page moves.
    page: usize,
    results: ratatui::layout::Rect,
    buttons: Vec<(ratatui::layout::Rect, Button)>,
    tabs: Vec<(ratatui::layout::Rect, usize)>,
    lists: ListAreas,
}
impl Default for Drawn {
    fn default() -> Self {
        Self {
            page: 1,
            results: ratatui::layout::Rect::default(),
            buttons: Vec::new(),
            tabs: Vec::new(),
            lists: ListAreas::default(),
        }
    }
}
/// What a session keeps between passes of its loop. Fields drop in order: a
/// running job is waited for first, then Neovim quits, and the terminal is
/// restored last.
struct Session {
    /// An event read early, before this pass's discovery work.
    queued: Option<io::Result<Event>>,
    /// Input was handled and its frame is not drawn yet: discovery waits.
    answering: bool,
    /// Frames are drawn after input or a visible change, and at least once a
    /// second so external changes (e.g. a deleted mark) still appear.
    dirty: bool,
    /// A scan changed what is shown: drawn within `SCAN_FRAME`.
    scanned: bool,
    drawn_at: std::time::Instant,
    /// Whether the last pass showed the progress marker.
    progress_shown: bool,
    /// Whether the last frame drew the button bar.
    buttons_shown: bool,
    drawn: Drawn,
    mouse: Mouse,
    status: StatusTimer,
    palette_preview: PalettePreview,
    focus: Focus,
    cursor_shape: SetCursorStyle,
    /// The directory the editor's fields were last reset in.
    editor_directory: PathBuf,
    editor: Nvim,
    details_generation: u64,
    watch: Option<crate::watch::Watch>,
    /// What the watched folders were chosen for: generation, scanning,
    /// depth and known paths.
    watch_basis: Option<(u64, bool, usize, usize)>,
    /// When a change to the current directory's listing is reconciled.
    relist_at: Option<std::time::Instant>,
    discovery: Discovery,
    input: Input,
    wake: std::sync::mpsc::SyncSender<()>,
    waking: std::sync::mpsc::Receiver<()>,
    terminal: Terminal,
    /// Where fff was started: `q` leaves there.
    invocation: PathBuf,
    palettes: crate::reload::PaletteWatcher,
    /// A contents query: the highlighted match's file being read as lines.
    match_lines: Option<MatchLines>,
    /// When the match's file's lines arrived: its colors wait for them to
    /// be drawn plain.
    match_text_at: std::time::Instant,
    /// The highlighted match, and since when: its colors wait for it to rest.
    shown_match: Option<(u64, std::time::Instant)>,
    /// Its colors being found, and the times they were asked for.
    match_colors: Option<MatchColors>,
    color_tries: u8,
}
/// Syntax colors asked for lines of a contents match's file.
struct MatchColors {
    lines: std::sync::Arc<crate::grep::Lines>,
    /// Where each line starts.
    starts: Vec<usize>,
    answer: std::sync::mpsc::Receiver<Option<crate::highlight::Lines>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}
/// A file being read for a contents match's preview, one at a time: its
/// text is kept only if its file is still the one wanted when it arrives.
struct MatchLines {
    path: PathBuf,
    lines: std::sync::mpsc::Receiver<Option<crate::grep::Lines>>,
    /// The file changed while it was read.
    stale: bool,
}
impl Session {
    fn start(
        app: &mut App,
        cache: crate::config::CacheConfig,
        palettes: crate::reload::PaletteWatcher,
        performance: crate::config::PerformanceConfig,
    ) -> io::Result<Self> {
        let invocation = std::env::current_dir()?.canonicalize()?;
        if let Some(error) = &app.keys_broken {
            app.status = format!("keys.toml has errors, so fff uses the default keys: {error}");
        }
        // Entries that waited in the trash long enough go, without delaying startup.
        if let Some(trash) = &app.trash {
            trash.purge_in_background();
        }
        let terminal = Terminal::enter(app.mouse)?;
        let (wake, waking) = std::sync::mpsc::sync_channel(1);
        crate::notify::set_waker(wake.clone());
        let waiting = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let input = Input::start(wake.clone(), waiting.clone());
        let mut discovery = Discovery::new(cache);
        discovery.set_waker(wake.clone());
        discovery.yield_to(waiting.clone());
        app.yield_to(waiting);
        app.set_waker(wake.clone());
        discovery.refresh_every = Duration::from_millis(performance.refresh_ms);
        discovery.max_paths = performance.max_paths;
        app.cached_queries = performance.cached_queries;
        app.details.set_waker(wake.clone());
        // A list read from stdin is not followed: its rows are the list's.
        let watch = (app.watch && app.program().scans())
            .then(crate::watch::Watch::new)
            .flatten();
        let details_generation = app.generation;
        // Starting a scan clears the status; keep a startup notice (resuming).
        let notice = std::mem::take(&mut app.status);
        discovery.start(app, true);
        app.status = notice;
        let mut editor = start_editor("nvim")?;
        // fff pick and fff seek are file choices, and a save dialog asks for
        // a name: start ready to type the query.
        if app.program().starts_typing() {
            editor.reset_insert(Field::Query);
        }
        let editor_directory = app.cwd.clone();
        warn_stranded(app);
        app.preview.set_waker(wake.clone());
        Ok(Self {
            queued: None,
            answering: false,
            dirty: true,
            scanned: false,
            drawn_at: std::time::Instant::now(),
            progress_shown: true,
            buttons_shown: false,
            drawn: Drawn::default(),
            mouse: Mouse::default(),
            status: StatusTimer::default(),
            palette_preview: PalettePreview::default(),
            focus: Focus::Results,
            cursor_shape: SetCursorStyle::DefaultUserShape,
            editor_directory,
            editor,
            details_generation,
            watch,
            watch_basis: None,
            relist_at: None,
            discovery,
            input,
            wake,
            waking,
            terminal,
            invocation,
            palettes,
            match_lines: None,
            match_text_at: std::time::Instant::now(),
            shown_match: None,
            match_colors: None,
            color_tries: 0,
        })
    }
    /// Catches up with what happened meanwhile: a finished job, notices,
    /// palette reloads, live updates, discovery and details.
    fn poll(&mut self, app: &mut App) -> io::Result<()> {
        if let Focus::Busy(job) = &self.focus
            && let Some(result) = job.poll(Duration::ZERO)
        {
            let Focus::Busy(job) = std::mem::take(&mut self.focus) else {
                unreachable!()
            };
            finish(app, &mut self.discovery, &mut self.focus, job, result);
            self.dirty = true;
        }
        if poll_jobs(app, &mut self.discovery) {
            self.dirty = true;
            if app.jobs.is_empty() && matches!(self.focus, Focus::Jobs { .. }) {
                self.focus = Focus::Results;
            }
        }
        // Progress moves on its own while an operation runs.
        self.dirty |= (matches!(self.focus, Focus::Busy(_)) || !app.jobs.is_empty())
            && self.drawn_at.elapsed() >= PROGRESS_FRAME;
        self.dirty |= self.status.tick(app);
        // Pop-up notifications from background work, shown for a while.
        let now = std::time::Instant::now();
        for notice in crate::notify::drain() {
            app.notices.push((notice, now));
            self.dirty = true;
        }
        let shown = app.notices.len();
        app.notices
            .retain(|(_, at)| now.duration_since(*at) < crate::notify::SHOWN_FOR);
        self.dirty |= app.notices.len() != shown;
        for update in self.palettes.updates() {
            self.dirty = true;
            match update {
                Ok(state) => palette_reloaded(app, &mut self.focus, state),
                Err(error) => {
                    app.status = format!("Palette reload failed; keeping current colors: {error}")
                }
            }
        }
        if let Some(watch) = &mut self.watch {
            // While a scan streams paths in, only the roots change.
            let known = if app.scanning { 0 } else { app.known.len() };
            let basis = (app.generation, app.scanning, app.depth, known);
            if self.watch_basis != Some(basis) {
                self.watch_basis = Some(basis);
                let (dirs, roots) = watched_dirs(app, watch);
                let dirs: Vec<_> = dirs.iter().map(PathBuf::as_path).collect();
                watch.follow(&dirs, roots);
            }
            let changes = watch.poll();
            for path in &changes.touched {
                app.details.touched(path);
            }
            app.grep.touched(&changes.touched);
            // A changed file's lines are read again for its matches.
            if let Some(text) = &app.match_text
                && changes.touched.contains(&text.path)
            {
                app.match_text = None;
            }
            if let Some(reading) = &mut self.match_lines
                && changes.touched.contains(&reading.path)
            {
                reading.stale = true;
            }
            self.dirty |= !changes.touched.is_empty();
            // Coalesce bursts (an extraction, a copy) into one quiet rescan;
            // while operations run, into one a second at most.
            if changes.listing && self.relist_at.is_none() {
                let delay = if app.jobs.is_empty() {
                    RELIST_DELAY
                } else {
                    RELIST_DELAY_BUSY
                };
                self.relist_at = Some(now + delay);
            }
            // A change during a rescan waits for it, so that a busy folder
            // cannot keep restarting it.
            if self.relist_at.is_some_and(|at| now >= at) && !app.scanning {
                self.relist_at = None;
                self.discovery.changed(app);
            }
        }
        // A drag held past the results' edge keeps scrolling and marking.
        if matches!(self.focus, Focus::Results) {
            self.dirty |= drag_at_edge(app, self.drawn.results, &mut self.mouse, now);
        }
        // Input goes first: with a key waiting, discovery waits a pass.
        if self.queued.is_none() {
            self.queued = match self.input.try_take() {
                Ok(event) => Some(event),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    return Err(io::Error::other("terminal input reader stopped"));
                }
            };
        }
        // A key that started a scan is answered with its result when the
        // scan finishes within a frame, instead of an empty or cached frame
        // and then another.
        if self.queued.is_none() && (!self.answering || self.discovery.just_started()) {
            self.scanned |= self.discovery.drain(app);
        }
        self.scanned |= app.poll_ranking();
        self.scanned |= app.poll_grep();
        // Once the scan is done (or with none), at once.
        if self.scanned && (!app.scanning || self.drawn_at.elapsed() >= SCAN_FRAME) {
            self.dirty = true;
        }
        // The progress marker stays a few seconds after work settles.
        let settled = (!app.working()).then(|| app.settled_at.unwrap_or(now));
        if settled != app.settled_at {
            app.settled_at = settled;
            self.dirty = true;
        }
        let progress = app.progress_shown(now);
        self.dirty |= progress != self.progress_shown;
        self.progress_shown = progress;
        // A resting mouse hides the button bar again.
        let buttons = app.buttons_shown_at(now);
        self.dirty |= buttons != self.buttons_shown;
        self.buttons_shown = buttons;
        if app.generation != self.details_generation {
            self.details_generation = app.generation;
            app.details.forget();
        }
        self.dirty |= app.details.drain();
        // Rows sorted by size follow the totals as they are added up.
        self.scanned |= app.follow_sizes(now);
        self.discovery.idle(
            app,
            matches!(self.focus, Focus::Results),
            std::time::Instant::now(),
        );
        Ok(())
    }
    /// Keeps Neovim on the focused text field: applies a buffer it closed,
    /// and loads the field the focus now edits.
    pub(super) fn sync_editor(&mut self, app: &mut App) -> io::Result<()> {
        if matches!(self.focus, Focus::Extracting(_)) {
            let Focus::Extracting(archives) = std::mem::take(&mut self.focus) else {
                unreachable!()
            };
            self.focus = extract_prompt(app, &mut self.editor, archives);
        }
        if self.editor_directory != app.cwd {
            self.editor_directory = app.cwd.clone();
            warn_stranded(app);
            // Carry the initiating field's editing mode into the new location.
            self.editor.reset_preserving_mode(Field::Query);
        }
        if let Some(view) = app.edit_view.as_ref()
            && view.closed
        {
            if let Focus::Operation(draft) = &self.focus {
                let request = view.saved.then(|| draft.request());
                self.focus = Focus::Results;
                if let Some(request) = request {
                    match request {
                        Ok(request) => perform_operation(
                            app,
                            &mut self.discovery,
                            &mut self.focus,
                            request,
                            false,
                        ),
                        Err(error) => app.status = error,
                    }
                }
            } else if matches!(self.focus, Focus::Exclusions(_) | Focus::Filters(_)) {
                if view.saved {
                    apply_globs(app, &mut self.focus, &mut self.discovery);
                } else {
                    self.focus = Focus::Results;
                }
            } else if let Focus::WorkspaceEdit(draft) = &self.focus {
                // Enter and Escape are the form's keys; Neovim's own
                // ZZ/:wq and ZQ/:q! still save or cancel it.
                if view.saved {
                    save_workspace(app, &mut self.focus);
                } else {
                    self.focus = Focus::Workspaces {
                        query: draft.query.clone(),
                        index: draft.index,
                    };
                }
            }
            app.edit_view = None;
            self.dirty = true;
        }
        if let Some(error) = self.editor.broken() {
            return Err(io::Error::other(format!("Neovim stopped: {error}")));
        }
        if let Some((field, text)) = text_field(app, &mut self.focus) {
            match self.editor.activate(field, &text) {
                Ok(view) => app.edit_view = Some(view),
                Err(error) => self.editor.break_with(error),
            }
        } else {
            app.edit_view = None;
        }
        if !app.pending.is_empty()
            && let Some(view) = &mut app.edit_view
        {
            // Show a partial list-key sequence like Neovim's own pending input.
            view.blocking = true;
        }
        Ok(())
    }
    /// The preview follows the highlighted row or mark; sheets and choosers
    /// over the panes leave it where it was.
    fn follow_preview(&mut self, app: &mut App) {
        self.dirty |= self
            .palette_preview
            .tick(app, &self.focus, std::time::Instant::now());
        let followed = match &self.focus {
            Focus::Results => Some(app.highlight.clone()),
            Focus::Marks { rows, index } => Some(rows.get(*index).cloned()),
            _ => None,
        };
        let followed = match followed {
            Some(path) => path.map(|path| {
                let info = app.details.info(&path);
                crate::preview::Key {
                    size: info.as_ref().map_or(0, |i| i.size),
                    modified: info.and_then(|i| i.modified),
                    hidden: app.policy.hidden,
                    path,
                }
            }),
            None => app.preview.followed().cloned(),
        };
        self.dirty |= app.preview.tick(followed, std::time::Instant::now());
        // A contents match: scrolling starts from it again on another one.
        let showing = matches!(self.focus, Focus::Results)
            .then(|| app.highlighted_hit())
            .flatten()
            .map(|hit| {
                use std::hash::{Hash, Hasher};
                let mut id = std::collections::hash_map::DefaultHasher::new();
                (&app.highlight, hit.line, hit.page, &hit.matched).hash(&mut id);
                id.finish()
            });
        app.preview.follow_match(showing);
        let now = std::time::Instant::now();
        if self.shown_match.map(|(id, _)| id) != showing {
            self.shown_match = showing.map(|id| (id, now));
            if let Some(asked) = self.match_colors.take() {
                asked
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.color_tries = 0;
        }
        // Its file's lines, read on another thread, let it scroll past the
        // match's context; a hidden preview needs none.
        if !app.preview.shown {
            app.match_text = None;
        }
        let path = showing
            .and(app.highlight.clone())
            .filter(|_| app.preview.shown);
        if let Some(reading) = &self.match_lines
            && let Ok(lines) = reading.lines.try_recv()
        {
            if !reading.stale && path.as_ref() == Some(&reading.path) {
                // A file with no lines to read (gone, say) is remembered as
                // such, rather than read again on every pass.
                let lines = lines.unwrap_or_default();
                app.match_text = Some(crate::MatchText {
                    path: reading.path.clone(),
                    lines: std::sync::Arc::new(lines),
                    colors: Default::default(),
                });
                self.match_text_at = now;
                app.preview.more_match_lines();
                self.dirty = true;
            }
            self.match_lines = None;
        }
        // Another file waits for the one being read: moving quickly over
        // matches in large files reads one of them at a time.
        if let Some(path) = path
            && app.match_text.as_ref().is_none_or(|text| text.path != path)
            && self.match_lines.is_none()
        {
            let (send, lines) = std::sync::mpsc::channel();
            let wake = self.wake.clone();
            let reading = path.clone();
            std::thread::spawn(move || {
                let _ = send.send(crate::grep::lines(&reading));
                let _ = wake.try_send(());
            });
            self.match_lines = Some(MatchLines {
                path,
                lines,
                stale: false,
            });
        }
        self.follow_match_colors(app, now);
    }
    /// A contents match's syntax colors: taken when they arrive, and asked
    /// for once it has rested with its plain lines drawn.
    fn follow_match_colors(&mut self, app: &mut App, now: std::time::Instant) {
        if let Some(asked) = &self.match_colors
            && let Ok(answer) = asked.answer.try_recv()
        {
            // Lines of the same text, kept where they already have colors:
            // those found from the file's start are the surer.
            if let Some(answer) = answer
                && let Some(text) = &mut app.match_text
                && std::sync::Arc::ptr_eq(&text.lines, &asked.lines)
            {
                for (&start, spans) in asked.starts.iter().zip(answer) {
                    text.colors.entry(start).or_insert(spans);
                }
                self.dirty = true;
            }
            self.match_colors = None;
        }
        if self.colors_due(app).is_some_and(|at| at <= now) && self.drawn_at >= self.match_text_at {
            self.color_tries += 1;
            self.ask_colors(app);
        }
    }
    /// When the highlighted match's colors are to be asked for: once it has
    /// rested, its file's lines have arrived and the search is done, when
    /// lines near it have none yet.
    fn colors_due(&self, app: &App) -> Option<std::time::Instant> {
        let (_, since) = self.shown_match?;
        if !app.preview.highlight
            || self.match_colors.is_some()
            || self.color_tries >= COLORS_TRIES
            || app.grep.progress().is_some()
        {
            return None;
        }
        let text = app
            .match_text
            .as_ref()
            .filter(|text| Some(&text.path) == app.highlight.as_ref() && text.lines.text_file)?;
        let hit = app.highlighted_hit()?;
        let near = text.lines.window(
            hit.start,
            hit.at,
            hit.context.len(),
            COLORS_NEAR,
            COLORS_NEAR,
        )?;
        let missing = (near.before.iter())
            .chain(&near.context)
            .chain(&near.after)
            .any(|start| !text.colors.contains_key(start));
        missing.then_some(since + COLORS_REST)
    }
    /// Asks for the colors of the lines around the highlighted match, from
    /// its file's start when near it, within `COLORS_BUDGET`.
    fn ask_colors(&mut self, app: &App) {
        let (Some(text), Some(hit)) = (&app.match_text, app.highlighted_hit()) else {
            return;
        };
        let Some(window) = text.lines.window(
            hit.start,
            hit.at,
            hit.context.len(),
            COLORS_FROM_START,
            COLORS_AROUND,
        ) else {
            return;
        };
        let from_start = window.before.len() < COLORS_FROM_START;
        let first = window.before.len();
        let mut starts: Vec<usize> = (window.before.into_iter())
            .chain(window.context)
            .chain(window.after)
            .collect();
        let mut lines: Vec<String> = (starts.iter())
            .map(|&start| {
                let mut line = text.lines.line(start);
                let mut end = line.len().min(COLORS_LINE);
                while !line.is_char_boundary(end) {
                    end -= 1;
                }
                line.truncate(end);
                line
            })
            .collect();
        if !from_start || lines.iter().map(String::len).sum::<usize>() > COLORS_BUDGET {
            // The lines nearest the match, as many as fit.
            let (mut low, mut high, mut used) = (first, first, 0);
            let lowest = first.saturating_sub(COLORS_AROUND);
            loop {
                let mut grew = false;
                if high < lines.len() && used + lines[high].len() <= COLORS_BUDGET {
                    used += lines[high].len();
                    high += 1;
                    grew = true;
                }
                if low > lowest && used + lines[low - 1].len() <= COLORS_BUDGET {
                    low -= 1;
                    used += lines[low].len();
                    grew = true;
                }
                if !grew {
                    break;
                }
            }
            starts = starts[low..high].to_vec();
            lines = lines[low..high].to_vec();
        }
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let colors = crate::highlight::request(&text.path, lines, cancel.clone());
        let (send, answer) = std::sync::mpsc::channel();
        let wake = self.wake.clone();
        std::thread::spawn(move || {
            let _ = send.send(colors.recv().ok());
            let _ = wake.try_send(());
        });
        self.match_colors = Some(MatchColors {
            lines: text.lines.clone(),
            starts,
            answer,
            cancel,
        });
    }
    /// A bar while typing, a block in Neovim's other modes.
    fn set_cursor_shape(&mut self, app: &App) -> io::Result<()> {
        let desired = app
            .edit_view
            .as_ref()
            .map(|view| {
                if view.mode.starts_with('i') {
                    SetCursorStyle::SteadyBar
                } else {
                    SetCursorStyle::SteadyBlock
                }
            })
            .unwrap_or(SetCursorStyle::DefaultUserShape);
        if desired != self.cursor_shape {
            execute!(self.terminal.0.backend_mut(), desired)?;
            self.cursor_shape = desired;
        }
        Ok(())
    }
    fn draw(&mut self, app: &mut App) -> io::Result<()> {
        let mut completed = None;
        self.terminal.synchronize(true)?;
        let focus = &mut self.focus;
        let drawn = self
            .terminal
            .0
            .draw(|frame| completed = Some(crate::view::render(frame, app, focus)))
            .map(|_| ());
        let ended = self.terminal.synchronize(false);
        drawn?;
        ended?;
        let completed = completed.expect("terminal draw callback completes");
        app.details.commit_sizes();
        // Only publish targets after Terminal::draw has flushed the frame.
        if matches!(self.focus, Focus::Results) {
            app.rendered = completed.targets;
        }
        self.drawn = Drawn {
            page: completed.page.max(1),
            results: completed.results,
            buttons: completed.buttons,
            tabs: completed.tabs,
            lists: ListAreas {
                marks_pane: completed.marks_pane,
                marks: completed.marks,
                choices: completed.choices,
            },
        };
        self.dirty = false;
        self.scanned = false;
        self.answering = false;
        self.drawn_at = std::time::Instant::now();
        Ok(())
    }
    /// The next event, or `None` after waiting for one (or for background
    /// work) up to when something else falls due.
    fn next_event(&mut self, app: &App) -> io::Result<Option<Event>> {
        // Both input and discovery wake this wait immediately. The timeout only
        // services due re-sorts, status expiry, palette reload and idle prefetch.
        // Rows still catching up with found paths merge on the next pass;
        // input is read between passes, so it is never kept waiting.
        let wait = if self.discovery.catching_up(app) {
            0
        } else if self.discovery.busy(app) || !app.pending.is_empty() {
            30
        } else {
            250
        };
        // A relist due during a scan waits for it to finish, which wakes
        // the loop; counting it here would spin until then.
        let wait = self
            .relist_at
            .filter(|_| !app.scanning)
            .map(|at| at.saturating_duration_since(std::time::Instant::now()))
            .map_or(wait, |delay| wait.min(delay.as_millis() as u64));
        let wait = self
            .palette_preview
            .remaining(std::time::Instant::now())
            .map_or(Duration::from_millis(wait), |delay| {
                delay.min(Duration::from_millis(wait))
            });
        let wait = app
            .preview
            .remaining(std::time::Instant::now())
            .map_or(wait, |delay| delay.min(wait));
        // A due ask for colors passes once more, after the plain lines are drawn.
        let wait = self.colors_due(app).map_or(wait, |at| {
            at.saturating_duration_since(std::time::Instant::now())
                .min(wait)
        });
        let wait = app
            .buttons_hide()
            .filter(|_| self.buttons_shown)
            .map_or(wait, |at| {
                at.saturating_duration_since(std::time::Instant::now())
                    .min(wait)
            });
        let wait = self.mouse.edge_due().map_or(wait, |at| {
            at.saturating_duration_since(std::time::Instant::now())
                .min(wait)
        });
        let wait = if self.scanned {
            wait.min(SCAN_FRAME.saturating_sub(self.drawn_at.elapsed()))
        } else {
            wait
        };
        let wait = app.grep.due().map_or(wait, |at| {
            at.saturating_duration_since(std::time::Instant::now())
                .min(wait)
        });
        let wait = if matches!(self.focus, Focus::Busy(_)) || !app.jobs.is_empty() {
            wait.min(PROGRESS_FRAME)
        } else {
            wait
        };
        match self.queued.take().map_or_else(|| self.input.try_take(), Ok) {
            Ok(event) => event.map(Some),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                let _ = self.waking.recv_timeout(wait);
                Ok(None)
            }
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                Err(io::Error::other("terminal input reader stopped"))
            }
        }
    }
    /// The entry an action without marks applies to: the highlighted mark in
    /// the marked list, else the highlighted row.
    fn current(&self, app: &App) -> Option<PathBuf> {
        match &self.focus {
            Focus::Marks { rows, index } => rows.get(*index).cloned(),
            _ => app.highlight.clone(),
        }
    }
}

/// What a picker chooses among.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picker {
    /// `:`: the functions that work in Normal mode where it was pressed.
    Commands(keymap::Cell),
    /// `show_sorting`: the orders, in the direction Tab switches.
    Sort { descending: bool },
}
/// One of a picker's choices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Choice {
    Function(Function),
    /// Sorted by a criterion, or ranked as the program ranks (`None`).
    Sort(Option<rows::Criterion>),
}
impl Choice {
    pub fn name(self) -> &'static str {
        match self {
            Self::Function(f) => f.name(),
            Self::Sort(Some(by)) => by.name(),
            Self::Sort(None) => "program",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Function(f) => f.description(),
            Self::Sort(Some(by)) => by.description(),
            Self::Sort(None) => {
                "As this program ranks them: the best match minus a penalty for depth, as log sorts; in fff seek the best match first; a list from stdin the best match, then its order."
            }
        }
    }
}
impl Picker {
    /// Its choices in their own order: the functions but for those a digit
    /// goes with, or the program's ranking and then each criterion.
    fn all(self) -> Vec<Choice> {
        match self {
            Self::Commands(here) => Function::ALL
                .iter()
                .filter(|f| {
                    f.works(keymap::Mode::Normal, here)
                        && !matches!(
                            f,
                            Function::SearchToDepth
                                | Function::OpenRow
                                | Function::PrintRowAndMarks
                                | Function::QuitIntoRow
                                | Function::HighlightRow
                                | Function::GoToRow
                        )
                })
                .map(|&f| Choice::Function(f))
                .collect(),
            Self::Sort { .. } => std::iter::once(None)
                .chain(rows::Criterion::ALL.map(Some))
                .map(Choice::Sort)
                .collect(),
        }
    }
    /// Its choices matching `query`, best match first (by name, else by
    /// every word in the description).
    pub(crate) fn choices(self, query: &str) -> Vec<Choice> {
        let query = query.trim();
        let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
        let mut found: Vec<(i64, usize, Choice)> = self
            .all()
            .into_iter()
            .enumerate()
            .filter_map(|(i, choice)| {
                if query.is_empty() {
                    return Some((0, i, choice));
                }
                let named = crate::score(choice.name(), &query.replace(' ', "_"));
                let described = || {
                    let text = choice.description().to_lowercase();
                    words
                        .iter()
                        .all(|w| text.contains(w.as_str()))
                        .then_some(i64::MIN / 2)
                };
                named.or_else(described).map(|score| (score, i, choice))
            })
            .collect();
        found.sort_by_key(|(score, i, _)| (std::cmp::Reverse(*score), *i));
        found.into_iter().map(|(_, _, choice)| choice).collect()
    }
}
