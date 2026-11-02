use super::*;

/// Single steps wrap around a list's ends; paging still stops at them.
pub(super) fn cycle(index: usize, len: usize, forward: bool) -> usize {
    match (len, forward) {
        (0, _) => 0,
        (_, true) => (index + 1) % len,
        (_, false) => index.checked_sub(1).unwrap_or(len - 1).min(len - 1),
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopedAction {
    Extract,
    /// x (false) or X (true): delete permanently.
    Delete(bool),
    /// gx (false) or gX (true).
    Run(bool),
    /// E: $EDITOR.
    Edit,
    /// o: each file's opener, in this terminal.
    Open,
}
/// l, Alt+l and Alt+number on a file: its opener in the background (a
/// terminal opener in a new terminal window), while fff keeps the keyboard.
pub(super) fn open_in_background(app: &mut App, path: &std::path::Path) {
    // Open and save dialogs choose files rather than opening them.
    if let Some(hint) = app.dialog.as_ref().and_then(|d| d.file_hint()) {
        app.status = hint.into();
        return;
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let result = crate::open::background_plan(&app.openers, path, &app.cwd, app.home.as_deref())
        .and_then(|plan| crate::open::launch(&plan).map(|()| plan));
    match result {
        Ok(plan) => {
            app.status = format!("Opened {name} with {}", plan.name);
            app.status_success = app.status.clone();
        }
        Err(error) => app.status = error,
    }
}
/// Normal-mode Enter on a file in the default mode: its opener, and fff
/// leaves. Files sharing an opener go to one invocation, as with o. GUI
/// openers start detached, as with l; one terminal opener may replace fff in
/// this terminal (like gX), and more than one is refused, since only one can.
/// Every program is resolved before any starts. The shell then changes to the
/// last file's folder.
pub(super) fn open_and_leave(app: &App, paths: &[PathBuf]) -> Result<Finish, String> {
    let last = paths.last().ok_or("Nothing to open")?;
    if let Some(dir) = paths.iter().find(|p| p.is_dir()) {
        return Err(format!(
            "Only files open this way, not directories like {}",
            dir.file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    let folder = last.parent().unwrap_or(&app.cwd).to_owned();
    let home = app.home.as_deref();
    let mut groups: Vec<(crate::open::Opener, Vec<PathBuf>)> = vec![];
    for path in paths {
        let opener = app.openers.opener(path)?;
        match groups.iter_mut().find(|(o, _)| *o == opener) {
            Some((_, files)) => files.push(path.clone()),
            None => groups.push((opener, vec![path.clone()])),
        }
    }
    let mut detached = vec![];
    let mut here: Option<crate::run::Plan> = None;
    for (opener, files) in groups {
        let plan = crate::open::plan(&opener.line, files, &folder, home)?;
        if !opener.terminal {
            detached.push(plan);
        } else if let Some(other) = &here {
            return Err(format!(
                "These files need two terminal programs, {} and {}, and only one can take \
                 over this terminal; o opens them one after another",
                other.name, plan.name
            ));
        } else {
            here = Some(plan);
        }
    }
    for plan in &detached {
        crate::open::launch(plan)?;
    }
    Ok(match here {
        Some(mut plan) => {
            plan.exit = true;
            Finish::Exec(plan)
        }
        None => Finish::Folder(folder),
    })
}
/// Normal-mode Enter on `target` (or on nothing): a directory is left into;
/// a file opens and fff leaves (default mode), is printed (`fff pick`)
/// or is left into its folder (`fff seek`); nothing leaves into the browsing
/// directory, except in `fff seek`. Marks are ignored. `None` (with the
/// reason in the status line) when the entry is a cached one that is gone or
/// the file cannot open. With `--window` there is no shell to leave into: a
/// directory is entered instead, and nothing stays.
pub(super) fn finish_with(
    app: &mut App,
    discovery: &mut Discovery,
    target: Option<&crate::Candidate>,
) -> Option<Finish> {
    match target {
        Some(entry) if !app.ready_path(&entry.path) => None,
        Some(dir) if app.window && dir.directory => {
            if app.enter(Some(crate::Candidate::clone(dir))) == EnterOutcome::Navigated {
                discovery.start(app, true);
            }
            None
        }
        None if app.window || app.no_dir => {
            app.nothing_to_enter();
            None
        }
        Some(file) if !file.directory => {
            if app.output_paths {
                return Some(Finish::Paths(vec![file.path.clone()]));
            }
            if !app.no_dir {
                return match open_and_leave(app, std::slice::from_ref(&file.path)) {
                    Ok(finish) => Some(finish),
                    Err(error) => {
                        app.status = error;
                        None
                    }
                };
            }
            Some(Finish::Folder(
                file.path.parent().unwrap_or(&app.cwd).to_owned(),
            ))
        }
        Some(dir) => Some(Finish::Folder(dir.path.clone())),
        None => Some(Finish::Folder(app.cwd.clone())),
    }
}
/// Ctrl+w: the marks are returned where the answer can be several paths
/// (`App::returns_marks`); elsewhere nothing happens and the status line says
/// why. `None` when fff stays.
pub(super) fn leave_with_marks(app: &mut App) -> Option<Finish> {
    if !app.returns_marks() {
        app.status = "Ctrl+w returns the marks only where several paths can be chosen, as in \
                      fff pick"
            .into();
        return None;
    }
    app.confirm(None).map(Finish::Paths)
}
/// fff explorer --save, Ctrl+s or !: saves under the typed name (else the
/// suggested one) in the folder being browsed.
pub(super) fn save_here(app: &mut App, focus: &mut Focus) -> Option<Finish> {
    let name = app.dialog.as_ref()?.save_name(&app.query)?.to_owned();
    if let Err(error) = crate::explorer::check_name(&name) {
        app.status = error;
        return None;
    }
    let target = app.cwd.join(name);
    save_as(app, focus, target)
}
/// Saving to `target`: at once when nothing is there; a file there is only
/// replaced after the replace choice.
pub(super) fn save_as(app: &mut App, focus: &mut Focus, target: PathBuf) -> Option<Finish> {
    match target.symlink_metadata() {
        Err(_) => Some(Finish::Paths(vec![target])),
        Ok(meta) if meta.is_dir() => {
            app.status = format!(
                "{} is a folder; Enter goes into it",
                target.file_name().unwrap_or_default().to_string_lossy()
            );
            None
        }
        Ok(_) => {
            *focus = Focus::Replace(target);
            None
        }
    }
}
/// o: the openers take over this terminal one after another, grouped so that
/// files sharing an opener go to one invocation. Directories are refused.
pub(super) fn open_here(
    terminal: &mut Terminal,
    input: &mut Input,
    wake: &std::sync::mpsc::SyncSender<()>,
    app: &mut App,
    discovery: &mut Discovery,
    paths: Vec<PathBuf>,
) -> io::Result<()> {
    if let Some(dir) = paths.iter().find(|p| p.is_dir()) {
        app.status = format!(
            "o opens files, not directories like {}; l enters it",
            dir.file_name().unwrap_or_default().to_string_lossy()
        );
        return Ok(());
    }
    let mut groups: Vec<(String, Vec<PathBuf>)> = vec![];
    for path in paths {
        let line = match app.openers.opener(&path) {
            Ok(opener) => opener.line,
            Err(error) => {
                app.status = error;
                return Ok(());
            }
        };
        match groups.iter_mut().find(|(l, _)| *l == line) {
            Some((_, files)) => files.push(path),
            None => groups.push((line, vec![path])),
        }
    }
    for (line, files) in groups {
        match crate::open::plan(&line, files, &app.cwd, app.home.as_deref()) {
            Ok(plan) => run_here(terminal, input, wake, app, discovery, &plan)?,
            Err(error) => {
                app.status = error;
                break;
            }
        }
    }
    Ok(())
}
pub(super) enum Scope {
    Ready(Vec<PathBuf>),
    Ask(PathBuf, Vec<PathBuf>),
    Nothing,
}
/// The paths an action applies to. Unambiguous without marks (the current
/// entry) or when the current entry is marked (the marks); otherwise ask.
pub(super) fn scope(marks: &crate::marks::Marks, current: Option<PathBuf>) -> Scope {
    match current {
        None if marks.is_empty() => Scope::Nothing,
        None => Scope::Ready(marks.to_vec()),
        Some(current) if marks.is_empty() => Scope::Ready(vec![current]),
        Some(current) if marks.contains(&current) => Scope::Ready(marks.to_vec()),
        Some(current) => Scope::Ask(current, marks.to_vec()),
    }
}
/// Opens the destination prompt only when every chosen file is extractable;
/// otherwise nothing happens and each refused file is named with its reason.
pub(super) fn start_extract(
    app: &mut App,
    editor: &mut crate::editor::Nvim,
    archives: Vec<PathBuf>,
) -> Focus {
    let refused = archives
        .iter()
        .filter_map(|p| {
            crate::operations::validate_archive(p).err().map(|e| {
                let name = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
                format!("{name} ({e})")
            })
        })
        .collect::<Vec<_>>();
    if !refused.is_empty() {
        app.status = if archives.len() == 1 {
            format!("Cannot extract {}", refused[0])
        } else {
            format!(
                "Nothing extracted; {} of {} cannot be extracted: {}",
                refused.len(),
                archives.len(),
                refused.join("; ")
            )
        };
        return Focus::Results;
    }
    {
        let e = &mut *editor;
        e.reset(Field::Extraction);
    }
    Focus::Operation(crate::operations::Draft {
        kind: crate::operations::EditKind::Extract(archives),
        cwd: app.cwd.clone(),
        text: String::new(),
    })
}
/// gx and E: the program gets the terminal (fff stops reading input and leaves
/// its screen) and fff comes back when it ends, pausing first after a failure.
pub(super) fn run_here(
    terminal: &mut Terminal,
    input: &mut Input,
    wake: &std::sync::mpsc::SyncSender<()>,
    app: &mut App,
    discovery: &mut Discovery,
    plan: &crate::run::Plan,
) -> io::Result<()> {
    let waiting = input.waiting.clone();
    drop(std::mem::replace(input, Input::idle()));
    terminal.suspend();
    let status = plan.wait();
    if let Ok(status) = &status
        && !status.success()
    {
        crate::run::pause_after_failure(&plan.line, *status);
    }
    terminal.resume()?;
    *input = Input::start(wake.clone(), waiting);
    let message = match status {
        Ok(status) if status.success() => {
            let done = format!("{} finished", plan.name);
            app.status_success = done.clone();
            done
        }
        Ok(status) => format!("{} {}", plan.line, crate::run::describe(status)),
        Err(error) => format!("Could not run {}: {error}", plan.name),
    };
    discovery.force_refresh(app);
    app.status = message;
    Ok(())
}
pub(super) fn edit_files(
    terminal: &mut Terminal,
    input: &mut Input,
    wake: &std::sync::mpsc::SyncSender<()>,
    app: &mut App,
    discovery: &mut Discovery,
    paths: Vec<PathBuf>,
) -> io::Result<()> {
    match crate::run::editor_plan(paths, &app.cwd, app.home.as_deref()) {
        Ok(plan) => run_here(terminal, input, wake, app, discovery, &plan),
        Err(error) => {
            app.status = error;
            Ok(())
        }
    }
}
pub(super) fn start_run(editor: &mut Nvim, paths: Vec<PathBuf>, exit: bool) -> Focus {
    {
        let e = &mut *editor;
        e.reset_insert(Field::Command);
    }
    Focus::Command(crate::run::Prompt::new(paths, exit))
}
/// x: the paths go to the trash at once, unless some are too large for it
/// or cannot go there; then a sheet asks first. Without a trash, and for X,
/// a permanent deletion is confirmed.
pub(super) fn delete(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    paths: Vec<PathBuf>,
    permanently: bool,
) {
    use crate::operations::DeletePlan;
    let what = entries(paths.iter());
    match app.trash.clone().filter(|_| !permanently) {
        Some(trash) => prepare(
            app,
            discovery,
            focus,
            format!("Moving {what} to the trash"),
            move || DeletePlan::trash(paths, &trash).map(Plan::Trash),
        ),
        None => prepare(
            app,
            discovery,
            focus,
            format!("Deleting {what}"),
            move || DeletePlan::new(paths).map(Plan::Delete),
        ),
    }
}
/// v, p, s and files dropped onto the terminal: a move, copy or link of
/// `sources` into the browsing directory, once confirmed.
pub(super) fn transfer(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    kind: crate::operations::Transfer,
    sources: Vec<PathBuf>,
    dropped: bool,
) {
    use crate::operations::Transfer;
    let verb = match kind {
        Transfer::Move => "Moving",
        Transfer::Copy => "Copying",
        Transfer::Link => "Linking",
    };
    let title = format!("{verb} {}", entries(sources.iter()));
    let directory = app.cwd.clone();
    prepare(app, discovery, focus, title, move || {
        let mut plan = crate::operations::TransferPlan::new(kind, sources, directory)?;
        plan.dropped = dropped;
        Ok(Plan::Transfer(plan))
    });
}
/// Checks the chosen trees (sizes, and what a confirmation binds to) on
/// another thread; a large tree shows the progress sheet meanwhile.
fn prepare(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    title: String,
    plan: impl FnOnce() -> crate::operations::Result<Plan> + Send + 'static,
) {
    let job = Job::start(app, Work::Plan(Box::new(plan)), Then::Planned, title, false);
    run(app, discovery, focus, job);
}
/// Whether moving `paths` away would move the browsing directory.
fn moves_cwd(app: &App, paths: impl IntoIterator<Item = impl AsRef<std::path::Path>>) -> bool {
    // Compare resolved paths too: the browsing path may run through a symlink.
    let cwd = app.cwd.canonicalize().unwrap_or_else(|_| app.cwd.clone());
    paths.into_iter().any(|p| {
        let p = p.as_ref();
        let resolved = p
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| p.file_name().map(|name| parent.join(name)));
        [&app.cwd, &cwd].iter().any(|cwd| {
            cwd.starts_with(p) || resolved.as_ref().is_some_and(|path| cwd.starts_with(path))
        })
    })
}
/// How long an operation may take before its progress is shown instead of
/// waiting for it.
const PROGRESS_AFTER: Duration = Duration::from_millis(150);
/// A file operation (or an undo) running on a thread of its own: the screen
/// shows its progress, and Ctrl+c can stop it while that changes nothing.
pub(crate) struct Job {
    pub(crate) progress: std::sync::Arc<crate::operations::Progress>,
    /// What it is doing, e.g. "Copying 3 entries".
    pub(crate) title: String,
    /// Progress counts entries (deletion) rather than bytes.
    pub(crate) entries: bool,
    pub(crate) started: std::time::Instant,
    result: std::sync::mpsc::Receiver<crate::operations::Result<Done>>,
    then: Then,
}
/// What a job produced.
pub(super) enum Done {
    Outcome(crate::operations::Outcome),
    Plan(Plan),
}
/// A checked selection, ready to confirm or carry out.
pub(super) enum Plan {
    /// X, or x without a trash: confirm a permanent deletion.
    Delete(crate::operations::DeletePlan),
    /// x: trash at once, or ask about entries too large for the trash.
    Trash(crate::operations::DeletePlan),
    Transfer(crate::operations::TransferPlan),
}
enum Then {
    /// Only checked: nothing changed, and Ctrl+c abandons it at once.
    Planned,
    Executed {
        /// The message on success; rename is summarized from its outcome.
        summary: Option<String>,
        /// Move, copy and link clear the marks; deletion unmarks only what
        /// it removed.
        clear_selection: bool,
        /// Marks to keep, for files dropped onto the terminal.
        marks: Option<crate::marks::Marks>,
        /// A deletion that moved everything to the trash.
        trashed: bool,
    },
    Undone(crate::operations::undo::Record),
}
enum Work {
    Execute(crate::operations::Request),
    Undo(crate::operations::undo::Record),
    Plan(Box<dyn FnOnce() -> crate::operations::Result<Plan> + Send>),
}
impl Job {
    fn start(app: &App, work: Work, then: Then, title: String, entries: bool) -> Self {
        let progress = std::sync::Arc::<crate::operations::Progress>::default();
        let engine = crate::operations::Engine::new(app.trash.clone(), Some(progress.clone()));
        let (sender, result) = std::sync::mpsc::channel();
        let waker = app.waker();
        // Checking changes nothing, so it can always stop.
        let planning = matches!(work, Work::Plan(_));
        progress
            .stoppable
            .store(planning, std::sync::atomic::Ordering::Relaxed);
        progress
            .checking
            .store(planning, std::sync::atomic::Ordering::Relaxed);
        std::thread::spawn(move || {
            let outcome = match work {
                Work::Execute(request) => engine.execute(request).map(Done::Outcome),
                Work::Undo(record) => engine.undo(&record).map(Done::Outcome),
                Work::Plan(plan) => plan().map(Done::Plan),
            };
            let _ = sender.send(outcome);
            if let Some(waker) = waker {
                let _ = waker.try_send(());
            }
        });
        Self {
            progress,
            title,
            entries,
            started: std::time::Instant::now(),
            result,
            then,
        }
    }
    /// A job that never ends, to draw its progress.
    #[cfg(test)]
    pub(crate) fn shown(title: &str, entries: bool) -> Self {
        Self {
            progress: Default::default(),
            title: title.into(),
            entries,
            started: std::time::Instant::now(),
            result: std::sync::mpsc::channel().1,
            then: Then::Planned,
        }
    }
    /// The result, once the operation has ended.
    pub(super) fn poll(&self, wait: Duration) -> Option<crate::operations::Result<Done>> {
        use std::sync::mpsc::RecvTimeoutError;
        match self.result.recv_timeout(wait) {
            Ok(result) => Some(result),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => {
                Some(Err("The operation stopped unexpectedly".into()))
            }
        }
    }
    /// Ctrl+c: stop as soon as stopping changes nothing. Checking is
    /// abandoned at once (true): its thread only reads.
    pub(super) fn stop(&self) -> bool {
        self.progress
            .cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
        matches!(self.then, Then::Planned)
    }
    pub(crate) fn stopping(&self) -> bool {
        self.progress
            .cancel
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
/// Runs `job`, waiting a moment for it: a quick operation is done before the
/// next frame, a longer one shows its progress (`Focus::Busy`) until `finish`.
fn run(app: &mut App, discovery: &mut Discovery, focus: &mut Focus, job: Job) {
    match job.poll(PROGRESS_AFTER) {
        Some(result) => finish(app, discovery, focus, job, result),
        None => *focus = Focus::Busy(job),
    }
}
/// What an operation is doing, for its progress sheet.
fn progress_title(request: &crate::operations::Request) -> (String, bool) {
    use crate::operations::Request;
    let what = |paths: &[PathBuf]| entries(paths.iter());
    match request {
        Request::Copy { sources, .. } => (format!("Copying {}", what(sources)), false),
        Request::Move { sources, .. } => (format!("Moving {}", what(sources)), false),
        Request::Symlink { sources, .. } => (format!("Linking {}", what(sources)), false),
        Request::Rename { sources, .. } => (format!("Renaming {}", what(sources)), false),
        Request::Delete { sources, .. } if request.trashes_all() => {
            (format!("Moving {} to the trash", what(sources)), false)
        }
        Request::Delete { sources, .. } => (format!("Deleting {}", what(sources)), true),
        Request::Create { path, .. } => (
            format!("Creating {}", what(std::slice::from_ref(path))),
            false,
        ),
        Request::Extract { archives, .. } => (format!("Extracting {}", what(archives)), false),
    }
}
/// Starts `request` unless it would move the browsing directory; `marks`
/// replace the marks afterwards.
pub(super) fn perform_operation(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    request: crate::operations::Request,
    marks: Option<crate::marks::Marks>,
) {
    use crate::operations::Request;
    *focus = Focus::Results;
    let clear_selection = matches!(
        &request,
        Request::Move { .. } | Request::Copy { .. } | Request::Symlink { .. }
    );
    let destructive: Vec<&PathBuf> = match &request {
        Request::Move {
            sources, replace, ..
        } => sources.iter().chain(replace).collect(),
        // Overwritten entries are removed just like deleted ones.
        Request::Copy { replace, .. } => replace.iter().collect(),
        Request::Rename { sources, .. } | Request::Delete { sources, .. } => {
            sources.iter().collect()
        }
        _ => vec![],
    };
    if moves_cwd(app, &destructive) {
        app.status = "Cannot change the current directory or its ancestors".into();
        return;
    }
    let trashed = request.trashes_all();
    let then = Then::Executed {
        summary: operation_summary(&request),
        clear_selection,
        marks,
        trashed,
    };
    let (title, entries) = progress_title(&request);
    let job = Job::start(app, Work::Execute(request), then, title, entries);
    run(app, discovery, focus, job);
}
/// u: undoes the last operation still on the undo list.
pub(super) fn undo(app: &mut App, discovery: &mut Discovery, focus: &mut Focus) {
    let Some(record) = app.undo.last() else {
        app.status = "Nothing to undo".into();
        return;
    };
    if moves_cwd(app, record.moving()) {
        app.status = "Cannot undo: it would move the current directory or its ancestors".into();
        return;
    }
    let record = app.undo.pop().expect("checked above");
    let job = Job::start(
        app,
        Work::Undo(record.clone()),
        Then::Undone(record),
        "Undoing".into(),
        false,
    );
    run(app, discovery, focus, job);
}
/// Most operations `u` can undo, oldest dropped first.
const UNDO_LIMIT: usize = 100;
/// Applies an ended job's result: a checked selection is confirmed (or
/// trashed at once); an operation's marks, message and undo list follow it.
pub(super) fn finish(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    job: Job,
    result: crate::operations::Result<Done>,
) {
    *focus = Focus::Results;
    let title = job.title.clone();
    let outcome = match result {
        Ok(Done::Plan(plan)) => {
            match plan {
                Plan::Delete(plan) => *focus = Focus::DeleteConfirm(plan),
                Plan::Trash(plan) if plan.asking().is_empty() => {
                    perform_operation(app, discovery, focus, plan.trash_request(false), None)
                }
                Plan::Trash(plan) => *focus = Focus::TrashConfirm(plan),
                Plan::Transfer(plan) => *focus = Focus::TransferConfirm(plan),
            }
            return;
        }
        Ok(Done::Outcome(outcome)) => Ok(outcome),
        Err(error) => Err(error),
    };
    match (outcome, job.then) {
        (Ok(_), Then::Planned) => unreachable!("plans end above"),
        (Ok(outcome), then) => {
            app.marks.update(|marked| {
                // Nested renames report final paths; the deepest match wins.
                if let Some((from, to)) = outcome
                    .moved
                    .iter()
                    .filter(|(from, _)| marked.starts_with(from))
                    .max_by_key(|(from, _)| from.components().count())
                {
                    let suffix = marked.strip_prefix(from).expect("prefix checked");
                    *marked = if suffix.as_os_str().is_empty() {
                        to.clone()
                    } else {
                        to.join(suffix)
                    };
                }
            });
            app.marks
                .retain(|p| !outcome.removed.iter().any(|r| p.starts_with(r)));
            match then {
                Then::Executed {
                    summary,
                    clear_selection,
                    marks,
                    trashed,
                } => {
                    if clear_selection {
                        app.marks.clear();
                    }
                    if let Some(marks) = marks {
                        app.marks = marks;
                    }
                    let complete = outcome.warning.is_none();
                    // Rename counts only the rows that changed.
                    let done = summary.unwrap_or_else(|| {
                        format!("Renamed {}", entries(outcome.moved.iter().map(|(p, _)| p)))
                    });
                    app.status = if let Some(warning) = outcome.warning {
                        // It took effect, but not completely: marks above reflect
                        // what happened; the message is shown as a problem.
                        warning
                    } else if !outcome.changed {
                        "Nothing to rename; no names changed".into()
                    } else if trashed {
                        let key = app.normal_keys.describe(ListAction::Undo);
                        if key.is_empty() {
                            done.clone()
                        } else {
                            format!("{done}; {key} undoes")
                        }
                    } else {
                        done.clone()
                    };
                    if complete {
                        app.status_success = app.status.clone();
                    }
                    if !outcome.undo.is_empty() {
                        app.undo.push(crate::operations::undo::Record {
                            summary: done,
                            steps: outcome.undo,
                            lost: outcome.lost,
                        });
                        if app.undo.len() > UNDO_LIMIT {
                            app.undo.remove(0);
                        }
                    }
                }
                Then::Undone(record) => {
                    app.status = format!("Undid: {}", record.summary);
                    if record.lost > 0 {
                        app.status += &format!(
                            "; {} deleted permanently cannot come back",
                            match record.lost {
                                1 => "1 entry".to_string(),
                                n => format!("{n} entries"),
                            }
                        );
                    }
                    app.status_success = app.status.clone();
                }
                Then::Planned => unreachable!(),
            }
        }
        (Err(error), _) if error == crate::operations::CANCELLED => {
            app.status = format!("Stopped {}; nothing changed", lowercase_first(&title));
        }
        (Err(error), _) => app.status = error,
    }
    let message = std::mem::take(&mut app.status);
    discovery.force_refresh(app);
    app.status = message;
}
pub(super) fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    chars
        .next()
        .map(|c| c.to_lowercase().chain(chars).collect())
        .unwrap_or_default()
}
/// An fff killed mid-operation can leave user data in hidden staging folders.
pub(super) fn warn_stranded(app: &mut App) {
    let stranded = crate::operations::stranded(&app.cwd);
    if let Some(first) = stranded.first() {
        app.status = format!(
            "An interrupted operation left data in {}{}; move it back or delete it",
            first.file_name().unwrap_or_default().to_string_lossy(),
            match stranded.len() {
                1 => String::new(),
                n => format!(" and {} more", n - 1),
            }
        );
    }
}
fn sheets_count(n: usize) -> String {
    if n == 1 {
        "1 entry".into()
    } else {
        format!("{n} entries")
    }
}
/// "README.md" for one entry, "3 entries" for several.
pub(super) fn entries<'a>(paths: impl ExactSizeIterator<Item = &'a PathBuf>) -> String {
    let count = paths.len();
    match paths.last() {
        Some(path) if count == 1 => path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        _ => format!("{count} entries"),
    }
}
/// What a successful request did; rename is summarized from its outcome.
pub(super) fn operation_summary(request: &crate::operations::Request) -> Option<String> {
    use crate::operations::Request;
    Some(match request {
        Request::Move {
            sources, replace, ..
        }
        | Request::Copy {
            sources, replace, ..
        } => {
            let verb = if matches!(request, Request::Move { .. }) {
                "Moved"
            } else {
                "Copied"
            };
            let mut done = format!("{verb} {}", entries(sources.iter()));
            if !replace.is_empty() {
                done += &format!(", overwriting {}", entries(replace.iter()));
            }
            done
        }
        Request::Symlink { sources, .. } => format!("Linked {}", entries(sources.iter())),
        Request::Delete { sources, trash, .. } => {
            let trashed = trash.iter().filter(|&&t| t).count();
            match trashed {
                0 => format!("Deleted {} permanently", entries(sources.iter())),
                n if n == sources.len() => {
                    format!("Moved {} to the trash", entries(sources.iter()))
                }
                n => format!(
                    "Moved {} to the trash and deleted {} permanently",
                    sheets_count(n),
                    sheets_count(sources.len() - n)
                ),
            }
        }
        Request::Create { path, directory } => format!(
            "Created {} {}",
            if *directory { "directory" } else { "file" },
            entries(std::iter::once(path))
        ),
        Request::Extract {
            archives,
            destination: Some(destination),
        } => format!(
            "Extracted {} into {}",
            entries(archives.iter()),
            destination.display()
        ),
        Request::Extract { archives, .. } => format!(
            "Extracted {} beside {}",
            entries(archives.iter()),
            if archives.len() == 1 {
                "it"
            } else {
                "each archive"
            }
        ),
        Request::Rename { .. } => return None,
    })
}
pub(super) fn bookmark_saved(app: &App, action: &str) -> String {
    match &app.bookmark_store.path {
        Some(path) => format!("{action} in {}", path.display()),
        None => format!("{action} for this session"),
    }
}
pub(super) fn save_bookmark(app: &mut App, focus: &mut Focus) {
    let Focus::BookmarkEdit(draft) = focus else {
        return;
    };
    let back = Focus::Bookmarks {
        query: draft.query.clone(),
        index: draft.index,
    };
    let bookmark = draft.bookmark(&app.cwd, app.home.as_deref());
    // An unchanged existing bookmark leaves the configuration untouched.
    if let (Some(index), Ok(bookmark)) = (draft.editing, &bookmark)
        && app.bookmarks.get(index) == Some(bookmark)
    {
        *focus = back;
        return;
    }
    let result = bookmark.and_then(|bookmark| {
        app.bookmark_store.commit(
            &app.bookmarks,
            &BookmarkChange::Put {
                index: draft.editing,
                bookmark,
            },
        )
    });
    match result {
        Ok(bookmarks) => {
            let index = draft.editing.unwrap_or(app.bookmarks.len());
            app.bookmarks = bookmarks;
            app.status = bookmark_saved(app, "Bookmark saved");
            *focus = Focus::Bookmarks {
                query: String::new(),
                index,
            };
        }
        Err(error) => app.status = error,
    }
}
/// Apply the exclusion (Alt+e) or file filter (Alt+f) editor: one glob per line.
pub(super) fn apply_globs(app: &mut App, focus: &mut Focus, discovery: &mut Discovery) {
    let (text, filters) = match focus {
        Focus::Exclusions(text) => (text, false),
        Focus::Filters(text) => (text, true),
        _ => return,
    };
    let current = if filters {
        &app.policy.filters
    } else {
        &app.policy.rules
    };
    // Compare final contents rather than Neovim's modified flag: :w clears
    // that flag, and editing then undoing should leave the policy untouched.
    let unchanged = *text == current.join("\n");
    let lines: Vec<_> = text
        .lines()
        .enumerate()
        .filter(|(_, s)| !s.is_empty())
        .collect();
    let rules: Vec<String> = lines.iter().map(|(_, s)| (*s).to_owned()).collect();
    let invalid = lines
        .iter()
        .find_map(|(line, rule)| crate::glob(rule).err().map(|error| (*line + 1, error)));
    let unchanged = unchanged || rules == *current;
    *focus = Focus::Results;
    if unchanged {
        return;
    }
    if let Some((line, error)) = invalid {
        let kind = if filters { "Filter" } else { "Exclusion" };
        app.status = format!("{kind} line {line}: {}", error.kind());
        return;
    }
    // Both setters are atomic: every glob compiles before the list is replaced.
    if filters {
        // Filters only choose among discovered files, so no rescan is needed.
        if app.policy.set_filters(rules).is_ok() {
            app.offset = 0;
            app.refresh();
        }
    } else if app.policy.set_rules(rules).is_ok() && app.policy.ignore {
        discovery.start(app, false);
    }
}
/// Alt+P: drop the session choice and apply the configuration's palette now.
pub(super) fn reload_palette(app: &mut App, palettes: &crate::reload::PaletteWatcher) {
    match palettes.reload_now() {
        Ok(state) => {
            app.status = format!("Palette {} reloaded from configuration", state.name);
            app.apply_palette(state);
        }
        Err(error) => {
            app.status = format!("Palette reload failed; keeping current colors: {error}")
        }
    }
}
/// Background reload; while the chooser is open it updates the revert target.
pub(super) fn palette_reloaded(
    app: &mut App,
    focus: &mut Focus,
    state: crate::reload::PaletteState,
) {
    let current = match focus {
        Focus::Palettes { original, .. } => *original,
        _ => app.palette,
    };
    if state.palette != current {
        app.status = "Palette reloaded".into();
    }
    if let Focus::Palettes {
        query,
        index,
        original,
    } = focus
    {
        *original = state.palette;
        app.palette_name = state.name;
        app.palettes = state.available;
        *index = (*index).min(app.matching_palettes(query).len().saturating_sub(1));
    } else {
        app.apply_palette(state);
    }
}

/// `l`: enter the highlighted directory, or open the highlighted file in the
/// background.
pub(super) fn activate_highlighted(app: &mut App, discovery: &mut Discovery) {
    let file = app
        .highlighted_row()
        .filter(|c| !c.directory)
        .map(|c| c.path.clone());
    match file {
        Some(path) => open_in_background(app, &path),
        None => {
            if app.navigate_highlighted() {
                discovery.start(app, true);
            }
        }
    }
}
/// Two clicks on the same row within this time act like `l`.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Rows the wheel scrolls per step.
const WHEEL_ROWS: usize = 3;
/// While a drag is held past the results' top or bottom edge, they scroll a
/// row this often, marking as they go.
pub(super) const EDGE_STEP: Duration = Duration::from_millis(50);
/// The mouse in the results, like a graphical file manager.
#[derive(Default)]
pub(super) struct Mouse {
    /// The last click, for double clicks.
    click: Option<(PathBuf, std::time::Instant)>,
    /// Where a held left button went down; dragging marks rows from here.
    anchor: Option<PathBuf>,
    /// Where `anchor` was last found in the rows.
    anchor_at: usize,
    /// A drag held past the top or bottom edge, and when it next scrolls.
    pub(super) edge: Option<(MouseEvent, std::time::Instant)>,
    /// The last click on a mark or a chooser row, for double clicks.
    item: Option<(usize, std::time::Instant)>,
}
impl Mouse {
    /// A click on list item `index`: true when it is the second on it.
    fn again(&mut self, index: usize) -> bool {
        let now = std::time::Instant::now();
        let again = self
            .item
            .take()
            .is_some_and(|(i, at)| i == index && now.duration_since(at) < DOUBLE_CLICK);
        if !again {
            self.item = Some((index, now));
        }
        again
    }
    /// When a drag held past an edge scrolls next.
    pub(super) fn edge_due(&self) -> Option<std::time::Instant> {
        self.edge.map(|(_, at)| at)
    }
}
/// `[ui] mouse` in the results. A click clears the marks and highlights a
/// row; a second click on it acts like `l`, or in a dialog (fff explorer)
/// returns true for the caller to deliver Enter on it; dragging with the
/// button held marks every row from the pressed one to the current one, and
/// past the top or bottom edge scrolls on (see `drag_at_edge`). The wheel
/// only scrolls. `area` holds the rows of the last frame, one per rendered
/// target.
pub(super) fn mouse_results(
    app: &mut App,
    discovery: &mut Discovery,
    event: MouseEvent,
    area: ratatui::layout::Rect,
    mouse: &mut Mouse,
) -> bool {
    let inside = event.column >= area.x && event.column < area.x + area.width;
    let row = (inside && event.row >= area.y)
        .then(|| app.rendered.get(usize::from(event.row - area.y)).cloned())
        .flatten();
    match event.kind {
        MouseEventKind::ScrollDown | MouseEventKind::ScrollUp => {
            let last = app
                .rows
                .len()
                .saturating_sub(usize::from(area.height).max(1));
            app.offset = if event.kind == MouseEventKind::ScrollDown {
                (app.offset + WHEEL_ROWS).min(last)
            } else {
                app.offset.saturating_sub(WHEEL_ROWS)
            };
            app.scrolled = Some(app.highlight.clone());
        }
        MouseEventKind::Down(MouseButton::Left) => {
            mouse.edge = None;
            let Some(path) = row else {
                return false;
            };
            app.marks.clear();
            let now = std::time::Instant::now();
            let double = mouse
                .click
                .take()
                .is_some_and(|(p, at)| p == path && now.duration_since(at) < DOUBLE_CLICK);
            app.highlight = Some(path.clone());
            app.highlight_top = false;
            app.wanted = None;
            if double {
                mouse.anchor = None;
                if app.dialog.is_some() {
                    return true;
                }
                discovery.checkpoint(app);
                activate_highlighted(app, discovery);
            } else {
                mouse.click = Some((path.clone(), now));
                mouse.anchor = Some(path);
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let past = event.row < area.y || event.row >= area.y + area.height;
            mouse.edge = (past && mouse.anchor.is_some())
                .then(|| (event, std::time::Instant::now() + EDGE_STEP));
            drag(app, event, area, mouse);
        }
        MouseEventKind::Up(MouseButton::Left) => {
            mouse.anchor = None;
            mouse.edge = None;
        }
        _ => {}
    }
    false
}
/// What the mouse did in the marked list or a chooser.
#[derive(Debug, PartialEq)]
pub(super) enum ListMouse {
    /// Neither was under it: the results take it.
    Pass,
    /// Handled (or ignored) there.
    Done,
    /// A second click on a chooser row: Enter chooses it.
    Enter,
    /// A second click on a mark: `l` on it, or Enter in a dialog.
    Open(PathBuf),
}
/// Where the last frame drew the marked list and the open chooser's rows
/// (see `view::Rendered`).
#[derive(Default)]
pub(super) struct ListAreas {
    pub marks_pane: ratatui::layout::Rect,
    pub marks: Vec<(ratatui::layout::Rect, usize)>,
    pub choices: Vec<(ratatui::layout::Rect, usize)>,
}
/// The row a wheel step moves to from `current` among `count`.
pub(super) fn wheel_step(current: Option<usize>, count: usize, forward: bool) -> Option<usize> {
    let last = count.checked_sub(1)?;
    match current {
        None => forward.then_some(0),
        Some(i) if forward => Some((i + 1).min(last)),
        Some(i) => Some(i.saturating_sub(1).min(last)),
    }
}
/// `[ui] mouse` in the marked list and the choosers (bookmarks, their icons,
/// palettes, go-to suggestions). A click on a row selects it (a mark focuses
/// the marked list) and a second click on it acts on it; the wheel over
/// them moves the selection. A click outside the marked list returns to the
/// results. While a chooser is open, the mouse does nothing else.
pub(super) fn mouse_lists(
    app: &App,
    focus: &mut Focus,
    event: MouseEvent,
    areas: &ListAreas,
    mouse: &mut Mouse,
) -> ListMouse {
    let position = ratatui::layout::Position::new(event.column, event.row);
    let hit = |list: &[(ratatui::layout::Rect, usize)]| {
        list.iter()
            .find(|(area, _)| area.contains(position))
            .map(|&(_, i)| i)
    };
    let down = event.kind == MouseEventKind::Down(MouseButton::Left);
    let wheel = match event.kind {
        MouseEventKind::ScrollDown => Some(true),
        MouseEventKind::ScrollUp => Some(false),
        _ => None,
    };
    let chooser = match &*focus {
        Focus::Bookmarks { query, index } => {
            Some((Some(*index), app.matching_bookmarks(query).len()))
        }
        Focus::BookmarkIcons { query, index, .. } => Some((
            Some(*index),
            crate::icons::matching_bookmark_icons(query).len(),
        )),
        Focus::Palettes { query, index, .. } => {
            Some((Some(*index), app.matching_palettes(query).len()))
        }
        Focus::Location(location) => Some((location.selected, location.matches().len())),
        _ => None,
    };
    if let Some((current, count)) = chooser {
        let target = match wheel {
            Some(forward) => wheel_step(current, count, forward),
            None if down => hit(&areas.choices),
            None => None,
        };
        let Some(target) = target else {
            return ListMouse::Done;
        };
        let again = down && mouse.again(target);
        match focus {
            Focus::Bookmarks { index, .. }
            | Focus::BookmarkIcons { index, .. }
            | Focus::Palettes { index, .. } => *index = target,
            Focus::Location(location) => location.selected = Some(target),
            _ => {}
        }
        return if again {
            ListMouse::Enter
        } else {
            ListMouse::Done
        };
    }
    if !matches!(focus, Focus::Results | Focus::Marks { .. }) {
        return ListMouse::Pass;
    }
    let over_marks = areas.marks_pane.contains(position);
    let select = |focus: &mut Focus, target: usize| match focus {
        Focus::Marks { index, .. } => *index = target,
        _ => {
            *focus = Focus::Marks {
                rows: app.marks.to_vec(),
                index: target,
            }
        }
    };
    if down {
        if let Some(target) = hit(&areas.marks) {
            let again = mouse.again(target);
            select(focus, target);
            if again && let Focus::Marks { rows, index } = focus {
                return rows
                    .get(*index)
                    .cloned()
                    .map_or(ListMouse::Done, ListMouse::Open);
            }
            return ListMouse::Done;
        }
        if over_marks {
            return ListMouse::Done;
        }
        mouse.item = None;
        if matches!(focus, Focus::Marks { .. }) {
            *focus = Focus::Results;
        }
        return ListMouse::Pass;
    }
    if let Some(forward) = wheel
        && over_marks
    {
        let (count, current) = match focus {
            Focus::Marks { rows, index } => (rows.len(), Some(*index)),
            _ => (app.marks.len(), None),
        };
        if let Some(target) = wheel_step(current, count, forward) {
            select(focus, target);
        }
        return ListMouse::Done;
    }
    if matches!(focus, Focus::Marks { .. }) {
        return ListMouse::Done;
    }
    ListMouse::Pass
}
/// A drag held past the results' edge without moving: scroll a row further
/// and mark it, as the drag would. True when it did.
pub(super) fn drag_at_edge(
    app: &mut App,
    area: ratatui::layout::Rect,
    mouse: &mut Mouse,
    now: std::time::Instant,
) -> bool {
    let Some((event, at)) = mouse.edge else {
        return false;
    };
    if now < at {
        return false;
    }
    mouse.edge = Some((event, now + EDGE_STEP));
    drag(app, event, area, mouse)
}
/// Mark the rows from the anchor to the one under `event`; past the top or
/// bottom edge, to the row beyond the visible ones, which the highlight then
/// scrolls into view. True when the marks changed.
fn drag(app: &mut App, event: MouseEvent, area: ratatui::layout::Rect, mouse: &mut Mouse) -> bool {
    let Some(anchor) = mouse.anchor.as_ref() else {
        return false;
    };
    let Some(last) = app.rows.len().checked_sub(1) else {
        return false;
    };
    // The anchor is usually still on screen or where it was last found.
    let from = app
        .rendered
        .iter()
        .position(|p| p == anchor)
        .map(|i| app.offset + i)
        .filter(|&i| app.rows.get(i).is_some_and(|c| c.path == *anchor))
        .or_else(|| {
            let hint = mouse.anchor_at;
            app.rows
                .get(hint)
                .is_some_and(|c| c.path == *anchor)
                .then_some(hint)
        })
        .or_else(|| app.rows.iter().position(|c| c.path == *anchor));
    let Some(from) = from else {
        return false;
    };
    mouse.anchor_at = from;
    let to = if event.row < area.y {
        app.offset.saturating_sub(1)
    } else if event.row >= area.y + area.height {
        app.offset + app.rendered.len()
    } else {
        // Below the last row of a short list, the drag reaches it.
        app.offset + usize::from(event.row - area.y).min(app.rendered.len().saturating_sub(1))
    }
    .min(last);
    if to == from && app.marks.is_empty() {
        return false;
    }
    let rows: Vec<PathBuf> = if from <= to {
        app.rows[from..=to].iter().map(|c| c.path.clone()).collect()
    } else {
        app.rows[to..=from]
            .iter()
            .rev()
            .map(|c| c.path.clone())
            .collect()
    };
    app.highlight = rows.last().cloned();
    app.highlight_top = false;
    app.wanted = None;
    // The view follows the highlight past the edge.
    app.scrolled = None;
    let changed =
        rows.len() != app.marks.len() || rows.iter().zip(&app.marks[..]).any(|(a, b)| a != b);
    app.marks = rows.into();
    // A drag is not the first half of a double click.
    mouse.click = None;
    changed
}
/// The button bar (`[ui] buttons`): each button does what its key does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Button {
    /// Ctrl+o: the previous directory of this tab.
    Back,
    /// Alt+h: the parent directory.
    Up,
    /// l: enter the highlighted directory or open the highlighted file.
    Open,
    /// Tab without moving: mark or unmark the highlighted row.
    Mark,
    /// Alt+.: show or hide hidden entries.
    Hidden,
    /// Ctrl+c.
    Cancel,
    /// Ctrl+w with marks where they are returned, else Normal-mode Enter on
    /// the highlighted row.
    Select,
    /// Esc in a confirmation.
    Dismiss,
    /// Enter in a confirmation.
    Confirm,
}
impl Button {
    /// Left to right; the bar drops buttons from the end of the first group
    /// when it is narrow, never Cancel or Select.
    /// The buttons for `focus` (left and right groups); `None` hides the bar.
    pub(crate) fn sets(focus: &Focus) -> Option<(&'static [Self], &'static [Self])> {
        match focus {
            Focus::Results => Some((
                &[Self::Back, Self::Up, Self::Open, Self::Mark, Self::Hidden],
                &[Self::Cancel, Self::Select],
            )),
            Focus::TransferConfirm(_) | Focus::DeleteConfirm(_) => {
                Some((&[], &[Self::Dismiss, Self::Confirm]))
            }
            _ => None,
        }
    }
    /// The key a button stands for, where the key's handling is all it does.
    pub(crate) fn key(self) -> Option<KeyEvent> {
        match self {
            Self::Dismiss => Some(KeyEvent::new(K::Esc, M::NONE)),
            Self::Confirm => Some(KeyEvent::new(K::Enter, M::NONE)),
            _ => None,
        }
    }
    pub(crate) fn label(self, app: &App) -> &'static str {
        match self {
            Self::Back => "← Back",
            Self::Up => "↑ Up",
            Self::Open => "Open",
            Self::Mark => match &app.highlight {
                Some(path) if app.marks.contains(path) => "Unmark",
                _ => "Mark",
            },
            Self::Hidden if app.policy.hidden => "Hide hidden",
            Self::Hidden => "Show hidden",
            Self::Cancel | Self::Dismiss => "Cancel",
            Self::Select => "Select & Exit",
            Self::Confirm => "Confirm",
        }
    }
    /// Unavailable buttons are drawn dimmed and ignore clicks.
    pub(crate) fn available(self, app: &App) -> bool {
        match self {
            Self::Back => !app.no_dir && !app.back.is_empty(),
            Self::Up => !app.no_dir && app.cwd.parent().is_some(),
            Self::Open | Self::Mark => app.highlight.is_some(),
            Self::Select => !app.marks.is_empty() || app.highlight.is_some(),
            Self::Hidden | Self::Cancel | Self::Dismiss | Self::Confirm => true,
        }
    }
}
/// A click on `button`; `Some` ends the session.
pub(super) fn press(app: &mut App, discovery: &mut Discovery, button: Button) -> Option<Finish> {
    if !button.available(app) {
        return None;
    }
    match button {
        Button::Back => {
            if app.can_navigate() && app.jump(true) {
                discovery.start(app, true);
            }
        }
        Button::Up => {
            if app.can_navigate()
                && let Some(parent) = app.cwd.parent().map(PathBuf::from)
            {
                app.navigate(parent, true);
                discovery.start(app, true);
            }
        }
        Button::Open => {
            discovery.checkpoint(app);
            activate_highlighted(app, discovery);
        }
        Button::Mark => {
            if let Some(path) = app.highlight.clone() {
                app.toggle(path);
            }
        }
        Button::Hidden => {
            app.policy.hidden = !app.policy.hidden;
            discovery.start(app, false);
        }
        Button::Cancel => return Some(Finish::Cancelled),
        // Delivered as their keys by the session.
        Button::Dismiss | Button::Confirm => {}
        Button::Select => {
            if !app.marks.is_empty() && app.returns_marks() {
                return leave_with_marks(app);
            }
            // In a dialog, the answer is taken from the row's path.
            if app.dialog.is_some() {
                let target = app.highlight.clone()?;
                return app.ready_path(&target).then(|| Finish::Paths(vec![target]));
            }
            // Otherwise like Normal-mode Enter, which leaves unless fff has
            // a window of its own.
            let target = app.highlighted_row().map(|c| crate::Candidate::clone(c));
            return finish_with(app, discovery, target.as_ref());
        }
    }
    None
}
/// The files a terminal pasted for a drop: shell-quoted paths separated by
/// spaces, or `file://` URIs one per line. `None` unless every one is an
/// existing absolute path, so ordinary text is still pasted as text.
pub(super) fn dropped_paths(text: &str, home: Option<&std::path::Path>) -> Option<Vec<PathBuf>> {
    let text = text.trim();
    let words = if text.starts_with("file://") {
        text.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
            .map(String::from)
            .collect()
    } else {
        crate::run::split(text, home).ok()?
    };
    let mut paths: Vec<PathBuf> = Vec::new();
    for word in words {
        let path = match word.strip_prefix("file://") {
            // A host, if any, comes before the path: file://host/path.
            Some(uri) => crate::dbus::decode_path(&uri[uri.find('/')?..]),
            None => PathBuf::from(word),
        };
        if !path.is_absolute() || std::fs::symlink_metadata(&path).is_err() {
            return None;
        }
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    (!paths.is_empty()).then_some(paths)
}
