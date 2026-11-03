//! File operations as background jobs: the checks before a confirmation,
//! running a request with its progress, and what the session shows and
//! keeps (marks, undo records, messages) once it ends.
use super::*;

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
/// R in the trash: the entries go back where they came from at once,
/// unless a place is taken; then a sheet asks first.
pub(super) fn restore(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    paths: Vec<PathBuf>,
) {
    let Some(trash) = app.trash.clone() else {
        app.status = "The trash is off ([trash] enabled = false)".into();
        return;
    };
    let title = format!("Restoring {}", entries(paths.iter()));
    prepare(app, discovery, focus, title, move || {
        crate::operations::RestorePlan::new(paths, &trash).map(Plan::Restore)
    });
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
    thread: Option<std::thread::JoinHandle<()>>,
}
/// An operation runs to its end even when the session ends abruptly (the
/// terminal hung up and drawing failed): leaving would end the process and
/// kill its thread partway. Checking only reads, so it is left behind.
impl Drop for Job {
    fn drop(&mut self) {
        if !matches!(self.then, Then::Planned)
            && let Some(thread) = self.thread.take()
        {
            let _ = thread.join();
        }
    }
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
    /// R: restore at once, or ask about places that are taken.
    Restore(crate::operations::RestorePlan),
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
        /// A deletion that moved everything to the trash, a restore, a
        /// move, copy or link: the message says how to undo it.
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
        let thread = std::thread::spawn(move || {
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
            thread: Some(thread),
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
            thread: None,
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
        Request::Restore { sources, .. } => (format!("Restoring {}", what(sources)), false),
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
        Request::Restore {
            sources, replace, ..
        } => sources.iter().chain(replace).collect(),
        _ => vec![],
    };
    if moves_cwd(app, &destructive) {
        app.status = "Cannot change the current directory or its ancestors; go up first".into();
        return;
    }
    let trashed = request.trashes_all()
        || matches!(
            request,
            Request::Restore { .. }
                | Request::Move { .. }
                | Request::Copy { .. }
                | Request::Symlink { .. }
        );
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
        app.status =
            "Cannot undo: it would move the current directory or its ancestors; go up first".into();
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
    mut job: Job,
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
                // `u` undoes a move, copy or link: only overwriting asks
                // first, and files dropped onto the terminal, which may land
                // by accident.
                Plan::Transfer(plan) if plan.overwrites.is_empty() && !plan.dropped => {
                    match plan.request() {
                        Ok(request) => perform_operation(app, discovery, focus, request, None),
                        Err(error) => app.status = error,
                    }
                }
                Plan::Transfer(plan) => *focus = Focus::TransferConfirm(plan),
                Plan::Restore(plan) if plan.taken.is_empty() => {
                    let request = plan.request(crate::operations::Taken::Replace);
                    perform_operation(app, discovery, focus, request, None)
                }
                Plan::Restore(plan) => {
                    *focus = Focus::Replace(Replace {
                        taken: plan
                            .taken
                            .iter()
                            .map(|(i, both)| (plan.entries[*i].1.clone(), both.clone()))
                            .collect(),
                        then: Replacing::Restore(plan),
                    })
                }
            }
            return;
        }
        Ok(Done::Outcome(outcome)) => Ok(outcome),
        Err(error) => Err(error),
    };
    // Its thread delivered the result and ends; nothing waits for it.
    let then = std::mem::replace(&mut job.then, Then::Planned);
    match (outcome, then) {
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
                        match app.bound(Function::Undo) {
                            Some(key) => format!("{done}; {key} undoes"),
                            None => done.clone(),
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
    app.refresh_trash();
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
        Request::Restore {
            sources,
            targets,
            replace,
            ..
        } => {
            let mut done = match (&sources[..], &targets[..]) {
                ([_], [target]) => format!(
                    "Restored {} to {}",
                    entries(std::iter::once(target)),
                    target.parent().unwrap_or(target).display()
                ),
                _ => format!("Restored {}", entries(sources.iter())),
            };
            if !replace.is_empty() {
                done += &format!(", overwriting {}", entries(replace.iter()));
            }
            done
        }
        Request::Rename { .. } => return None,
    })
}
