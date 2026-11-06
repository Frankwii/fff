//! File operations as background jobs: the checks before a confirmation,
//! running a request with its progress, and what the session shows and
//! keeps (marks, undo records, messages) once it ends.
use super::*;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

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
    let claim = Claim {
        writes: resolving(paths.iter().cloned()),
        leaving: resolving(paths.iter().cloned()),
        ..Claim::default()
    };
    if refused(app, &claim) {
        return;
    }
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
    let claim = Claim {
        writes: resolving(paths.iter().cloned()),
        ..Claim::default()
    };
    if refused(app, &claim) {
        return;
    }
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
    let directory = app.cwd.clone();
    let targets = sources
        .iter()
        .filter_map(|p| p.file_name().map(|n| directory.join(n)));
    let mut claim = Claim {
        writes: resolving(targets),
        ..Claim::default()
    };
    if kind == Transfer::Move {
        claim.writes.extend(resolving(sources.iter().cloned()));
    } else {
        claim.reads = resolving(sources.iter().cloned());
    }
    if refused(app, &claim) {
        return;
    }
    let title = format!("{verb} {}", entries(sources.iter()));
    prepare(app, discovery, focus, title, move || {
        let mut plan = crate::operations::TransferPlan::new(kind, sources, directory)?;
        plan.dropped = dropped;
        Ok(Plan::Transfer(plan))
    });
}
/// E: every chosen archive is checked on another thread before the
/// destination prompt opens.
pub(super) fn extract(app: &mut App, archives: Vec<PathBuf>) -> Job {
    let title = format!("Checking {}", entries(archives.iter()));
    Job::start(
        app,
        Work::Plan(Box::new(move || {
            let refused: Vec<String> = archives
                .iter()
                .filter_map(|p| {
                    crate::operations::validate_archive(p).err().map(|e| {
                        let name = p.file_name().unwrap_or(p.as_os_str()).to_string_lossy();
                        format!("{name} ({e})")
                    })
                })
                .collect();
            match refused.len() {
                0 => Ok(Plan::Extract(archives)),
                _ if archives.len() == 1 => Err(format!("Cannot extract {}", refused[0])),
                n => Err(format!(
                    "Nothing extracted; {n} of {} cannot be extracted: {}",
                    archives.len(),
                    refused.join("; ")
                )),
            }
        })),
        Then::Planned,
        title,
        false,
    )
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
    match job.poll(PROGRESS_AFTER) {
        Some(result) => finish(app, discovery, focus, job, result),
        None => *focus = Focus::Busy(job),
    }
}
/// Whether moving `paths` away would move the browsing directory. Other
/// tabs' directories may go: a tab follows its moved directory, and one
/// whose directory is gone falls back to the nearest existing parent.
fn moves_cwd(app: &App, paths: impl IntoIterator<Item = impl AsRef<std::path::Path>>) -> bool {
    // Compare resolved paths too: the browsing path may run through a symlink.
    let dirs = [
        app.cwd.clone(),
        app.cwd.canonicalize().unwrap_or_else(|_| app.cwd.clone()),
    ];
    paths.into_iter().any(|p| {
        let p = p.as_ref();
        let resolved = resolved(p);
        dirs.iter().any(|dir| {
            dir.starts_with(p) || resolved.as_ref().is_some_and(|path| dir.starts_with(path))
        })
    })
}
/// `path` with the symlinks in its folders resolved, the entry itself not
/// followed.
fn resolved(path: &std::path::Path) -> Option<PathBuf> {
    let parent = path.parent()?.canonicalize().ok()?;
    Some(
        path.file_name()
            .map_or(parent.clone(), |name| parent.join(name)),
    )
}
/// Each path as given and, when it differs, resolved.
fn resolving(paths: impl IntoIterator<Item = PathBuf>) -> Vec<PathBuf> {
    let mut all = vec![];
    for path in paths {
        if let Some(other) = resolved(&path).filter(|r| *r != path) {
            all.push(other);
        }
        all.push(path);
    }
    all
}
/// One path is the other or holds it.
fn overlap(a: &std::path::Path, b: &std::path::Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
/// What an operation reads and changes while it runs, to refuse another
/// that would collide with it; each path as given and resolved.
#[derive(Debug, Default)]
pub(crate) struct Claim {
    reads: Vec<PathBuf>,
    writes: Vec<PathBuf>,
    /// The entries it moves away or deletes, which no one may go into.
    leaving: Vec<PathBuf>,
}
impl Claim {
    fn of(request: &crate::operations::Request) -> Self {
        use crate::operations::Request;
        let into = |sources: &[PathBuf], directory: &PathBuf| -> Vec<PathBuf> {
            sources
                .iter()
                .filter_map(|p| p.file_name().map(|n| directory.join(n)))
                .collect()
        };
        let (reads, writes, leaving): (Vec<PathBuf>, Vec<PathBuf>, Vec<PathBuf>) = match request {
            Request::Copy {
                sources,
                directory,
                replace,
                ..
            } => {
                let mut writes = into(sources, directory);
                writes.extend(replace.iter().cloned());
                (sources.clone(), writes, replace.clone())
            }
            Request::Symlink {
                sources, directory, ..
            } => (sources.clone(), into(sources, directory), vec![]),
            Request::Move {
                sources,
                directory,
                replace,
                ..
            } => {
                let mut writes = into(sources, directory);
                writes.extend(sources.iter().chain(replace).cloned());
                let leaving = sources.iter().chain(replace).cloned().collect();
                (vec![], writes, leaving)
            }
            Request::Rename { sources, targets } => (
                vec![],
                sources.iter().chain(targets).cloned().collect(),
                sources.clone(),
            ),
            Request::Delete { sources, .. } => (vec![], sources.clone(), sources.clone()),
            Request::Create { path, .. } => (vec![], vec![path.clone()], vec![]),
            Request::Restore {
                sources,
                targets,
                replace,
                ..
            } => (
                vec![],
                sources
                    .iter()
                    .chain(targets)
                    .chain(replace)
                    .cloned()
                    .collect(),
                sources.iter().chain(replace).cloned().collect(),
            ),
            // Beside each archive, its contents land in its folder itself.
            Request::Extract {
                archives,
                destination,
            } => (
                archives.clone(),
                match destination {
                    Some(destination) => vec![destination.clone()],
                    None => archives
                        .iter()
                        .filter_map(|a| a.parent().map(PathBuf::from))
                        .collect(),
                },
                vec![],
            ),
        };
        Self {
            reads: resolving(reads),
            writes: resolving(writes),
            leaving: resolving(leaving),
        }
    }
    fn of_undo(record: &crate::operations::undo::Record) -> Self {
        Self {
            reads: vec![],
            writes: resolving(record.touching()),
            leaving: resolving(record.moving().map(|p| p.into_owned())),
        }
    }
    /// A path where `self` and `other` would collide: one changes what the
    /// other reads or changes.
    fn collides<'a>(&'a self, other: &'a Claim) -> Option<&'a std::path::Path> {
        let changed = |writes: &'a [PathBuf], used: &[&'a [PathBuf]]| {
            writes
                .iter()
                .find(|w| used.iter().any(|paths| paths.iter().any(|p| overlap(w, p))))
                .map(PathBuf::as_path)
        };
        changed(&self.writes, &[&other.reads, &other.writes])
            .or_else(|| changed(&other.writes, &[&self.reads]))
    }
}
impl App {
    /// The running job that changes `path` or something in it, if any.
    pub(crate) fn changing(&self, path: &std::path::Path) -> Option<&Job> {
        let paths = resolving([path.to_path_buf()]);
        self.jobs.iter().find(|job| {
            job.claim
                .writes
                .iter()
                .any(|w| paths.iter().any(|p| overlap(w, p)))
        })
    }
    /// The running job moving away or deleting `path` or a folder holding it.
    pub(crate) fn leaving_job(&self, path: &std::path::Path) -> Option<&Job> {
        let paths = resolving([path.to_path_buf()]);
        self.jobs.iter().find(|job| {
            job.claim
                .leaving
                .iter()
                .any(|l| paths.iter().any(|p| p.starts_with(l)))
        })
    }
    /// Says so, and true, when `path` is (in) an entry a running job moves
    /// away or deletes: it cannot be gone into.
    pub(crate) fn leaving(&mut self, path: &std::path::Path) -> bool {
        let Some(title) = self
            .leaving_job(path)
            .map(|job| lowercase_first(&job.title))
        else {
            return false;
        };
        self.status = format!(
            "{} is being moved or deleted ({title}); wait for it to finish",
            name(path)
        );
        true
    }
    /// Says so, and true, when a running job changes one of `paths`: they
    /// are not opened, edited or given to a program meanwhile.
    pub(crate) fn in_use(&mut self, paths: &[PathBuf]) -> bool {
        let Some(message) = paths
            .iter()
            .find_map(|p| self.changing(p).map(|job| in_use_message(p, job)))
        else {
            return false;
        };
        self.status = message;
        true
    }
}
/// Why `path` is not opened or given to a program now.
pub(super) fn in_use_message(path: &std::path::Path, job: &Job) -> String {
    format!(
        "{} is in use ({}); wait for it to finish",
        name(path),
        lowercase_first(&job.title)
    )
}
fn name(path: &std::path::Path) -> String {
    path.file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy()
        .into_owned()
}
/// Says so, and true, when `claim` collides with a running job: the second
/// operation waits for the first to finish.
fn refused(app: &mut App, claim: &Claim) -> bool {
    let Some((path, title)) = app.jobs.iter().find_map(|job| {
        claim
            .collides(&job.claim)
            .map(|p| (p.to_path_buf(), job.title.clone()))
    }) else {
        return false;
    };
    app.status = format!("{title} uses {}; wait for it to finish", name(&path));
    true
}
/// How long an operation may take before its progress is shown instead of
/// waiting for it.
const PROGRESS_AFTER: Duration = Duration::from_millis(150);
/// A file operation (or an undo) running on a thread of its own: the jobs
/// list shows its progress, and x stops it and undoes what it did. Checks
/// before a confirmation are jobs too, shown on a sheet of their own.
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
    claim: Claim,
    /// When it started among this session's operations.
    order: u64,
    /// The marks it took on starting.
    marked: Vec<PathBuf>,
    /// Asked to stop and undo what it did (x, u); true marks its entries
    /// again (u).
    undoing: Option<bool>,
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
    /// E: archives that can be extracted; the destination is asked next.
    Extract(Vec<PathBuf>),
}
enum Then {
    /// Only checked: nothing changed, and Ctrl+c abandons it at once.
    Planned,
    Executed {
        /// The message on success; rename is summarized from its outcome.
        summary: Option<String>,
        /// A deletion that moved everything to the trash, a restore, a
        /// move, copy or link: the message says how to undo it.
        trashed: bool,
    },
    /// Undoing `record`; `remark` marks its entries again.
    Undone {
        record: crate::operations::undo::Record,
        remark: bool,
    },
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
        let claim = match &work {
            Work::Execute(request) => Claim::of(request),
            Work::Undo(record) => Claim::of_undo(record),
            Work::Plan(_) => Claim::default(),
        };
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
            claim,
            order: 0,
            marked: vec![],
            undoing: None,
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
            claim: Claim::default(),
            order: 0,
            marked: vec![],
            undoing: None,
        }
    }
    /// A running operation that never ends, changing `writes`.
    #[cfg(test)]
    pub(crate) fn running(title: &str, writes: &[PathBuf], leaving: bool) -> Self {
        let mut job = Self::shown(title, false);
        job.then = Then::Executed {
            summary: None,
            trashed: false,
        };
        job.claim.writes = writes.to_vec();
        if leaving {
            job.claim.leaving = writes.to_vec();
        }
        job
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
    /// An undo, which cannot itself be stopped and undone.
    pub(crate) fn undoes(&self) -> bool {
        matches!(self.then, Then::Undone { .. })
    }
}
/// Runs an operation's `job`, waiting a moment for it: a quick one is done
/// before the next frame, a longer one goes on in the jobs list.
fn launch(app: &mut App, discovery: &mut Discovery, mut job: Job, marked: Vec<PathBuf>) {
    app.started += 1;
    job.order = app.started;
    job.marked = marked;
    match job.poll(PROGRESS_AFTER) {
        Some(result) => ended(app, discovery, job, result),
        None => app.jobs.push(job),
    }
}
/// What an operation is doing, for its progress.
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
/// Starts `request` unless it would move the browsing directory or collide with
/// a running operation. The marked entries a move, copy, link, deletion or
/// extraction acts on are unmarked as it starts, unless `keep_marks` (files
/// dropped onto the terminal).
pub(super) fn perform_operation(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    request: crate::operations::Request,
    keep_marks: bool,
) {
    use crate::operations::Request;
    *focus = Focus::Results;
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
    if refused(app, &Claim::of(&request)) {
        return;
    }
    // Renamed and restored entries keep their marks, which follow them.
    let sources: &[PathBuf] = match &request {
        Request::Copy { sources, .. }
        | Request::Move { sources, .. }
        | Request::Symlink { sources, .. }
        | Request::Delete { sources, .. } => sources,
        Request::Extract { archives, .. } => archives,
        Request::Rename { .. } | Request::Restore { .. } | Request::Create { .. } => &[],
    };
    let marked: Vec<PathBuf> = if keep_marks {
        vec![]
    } else {
        sources
            .iter()
            .filter(|p| app.marks.contains(p))
            .cloned()
            .collect()
    };
    let unmarked: HashSet<&PathBuf> = marked.iter().collect();
    app.marks.retain(|p| !unmarked.contains(p));
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
        trashed,
    };
    let (title, entries) = progress_title(&request);
    let job = Job::start(app, Work::Execute(request), then, title, entries);
    launch(app, discovery, job, marked);
}
/// u: undoes the latest operation, whether it has ended (the last on the
/// undo list) or still runs (stopped, then undone); either way its entries
/// are marked again.
pub(super) fn undo(app: &mut App, discovery: &mut Discovery) {
    let running = app
        .jobs
        .iter()
        .enumerate()
        .filter(|(_, job)| !job.undoes() && job.undoing.is_none())
        .max_by_key(|(_, job)| job.order);
    if let Some((index, job)) = running
        && app.undo.last().is_none_or(|r| r.order < job.order)
    {
        stop_job(app, index, true);
        return;
    }
    let Some(record) = app.undo.last() else {
        app.status = "Nothing to undo".into();
        return;
    };
    if moves_cwd(app, record.moving()) {
        app.status =
            "Cannot undo: it would move the current directory or its ancestors; go up first".into();
        return;
    }
    if refused(app, &Claim::of_undo(record)) {
        return;
    }
    let record = app.undo.pop().expect("checked above");
    start_undo(app, discovery, record, true);
}
fn start_undo(
    app: &mut App,
    discovery: &mut Discovery,
    record: crate::operations::undo::Record,
    remark: bool,
) {
    let title = format!("Undoing: {}", record.summary);
    let job = Job::start(
        app,
        Work::Undo(record.clone()),
        Then::Undone { record, remark },
        title,
        false,
    );
    launch(app, discovery, job, vec![]);
}
/// x in the jobs list (`remark` false) and u on a running operation
/// (true): stop it as soon as that changes nothing, then undo whatever it
/// had done by then.
pub(super) fn stop_job(app: &mut App, index: usize, remark: bool) {
    let Some(job) = app.jobs.get_mut(index) else {
        return;
    };
    if job.undoes() {
        app.status = format!("{} cannot be stopped; it ends on its own", job.title);
        return;
    }
    if job.undoing.is_none() {
        job.undoing = Some(remark);
        job.stop();
    }
    app.status = format!("Stopping {} to undo it…", lowercase_first(&job.title));
}
/// Leaving fff: every running operation stops as soon as that changes
/// nothing, keeping what it finished; nothing is undone.
pub(super) fn stop_all(app: &mut App) {
    for job in &app.jobs {
        job.stop();
    }
}
/// Ends jobs whose operations have finished; true when one did.
pub(super) fn poll_jobs(app: &mut App, discovery: &mut Discovery) -> bool {
    let mut any = false;
    let mut i = 0;
    while i < app.jobs.len() {
        match app.jobs[i].poll(Duration::ZERO) {
            Some(result) => {
                let job = app.jobs.remove(i);
                ended(app, discovery, job, result);
                any = true;
            }
            None => i += 1,
        }
    }
    any
}
/// Most operations `u` can undo, oldest dropped first.
const UNDO_LIMIT: usize = 100;
/// Applies an ended check: the selection is confirmed (or trashed at once),
/// or the error is shown.
pub(super) fn finish(
    app: &mut App,
    discovery: &mut Discovery,
    focus: &mut Focus,
    job: Job,
    result: crate::operations::Result<Done>,
) {
    *focus = Focus::Results;
    let plan = match result {
        Ok(Done::Plan(plan)) => plan,
        Ok(Done::Outcome(_)) => return ended(app, discovery, job, result),
        Err(error) => {
            app.status = error;
            return;
        }
    };
    match plan {
        Plan::Delete(plan) => *focus = Focus::DeleteConfirm(plan),
        Plan::Trash(plan) if plan.asking().is_empty() => {
            perform_operation(app, discovery, focus, plan.trash_request(false), false)
        }
        Plan::Trash(plan) => *focus = Focus::TrashConfirm(plan),
        // `u` undoes a move, copy or link: only overwriting asks
        // first, and files dropped onto the terminal, which may land
        // by accident.
        Plan::Transfer(plan) if plan.overwrites.is_empty() && !plan.dropped => {
            match plan.request() {
                Ok(request) => perform_operation(app, discovery, focus, request, false),
                Err(error) => app.status = error,
            }
        }
        Plan::Transfer(plan) => *focus = Focus::TransferConfirm(plan),
        Plan::Restore(plan) if plan.taken.is_empty() => {
            let request = plan.request(crate::operations::Taken::Replace);
            perform_operation(app, discovery, focus, request, false)
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
        // The session opens the destination prompt, which needs the editor.
        Plan::Extract(archives) => *focus = Focus::Extracting(archives),
    }
}
/// Marks `paths` again, those still there.
fn remark(app: &mut App, paths: &[PathBuf]) {
    for path in paths {
        if !app.marks.contains(path) && path.symlink_metadata().is_ok() {
            app.marks.push(path.clone());
        }
    }
}
/// Applies an ended operation or undo: the marks, message and undo list
/// follow it, and one stopped to be undone is undone now. The focus stays:
/// it ends in the background.
fn ended(
    app: &mut App,
    discovery: &mut Discovery,
    mut job: Job,
    result: crate::operations::Result<Done>,
) {
    let title = job.title.clone();
    // Its thread delivered the result and ends; nothing waits for it.
    let then = std::mem::replace(&mut job.then, Then::Planned);
    let marked = std::mem::take(&mut job.marked);
    let outcome = match result {
        Ok(Done::Outcome(outcome)) => Ok(outcome),
        Ok(Done::Plan(_)) => unreachable!("checks end in finish"),
        Err(error) => Err(error),
    };
    match (outcome, then) {
        (Ok(_), Then::Planned) => unreachable!("checks end in finish"),
        (Ok(outcome), then) => {
            // Looked up by each mark's ancestors, so that many marks and
            // many moved entries do not compare every pair.
            let moved: HashMap<&Path, &PathBuf> = outcome
                .moved
                .iter()
                .map(|(from, to)| (from.as_path(), to))
                .collect();
            // Nested renames report final paths; the deepest match wins.
            let follow = |path: &mut PathBuf| {
                if let Some((from, to)) = path
                    .ancestors()
                    .find_map(|a| moved.get(a).map(|to| (a, *to)))
                {
                    let suffix = path.strip_prefix(from).expect("an ancestor");
                    *path = if suffix.as_os_str().is_empty() {
                        to.clone()
                    } else {
                        to.join(suffix)
                    };
                }
            };
            app.marks.update(&follow);
            // The other tabs follow their moved directories too.
            let active = app.tab;
            for (i, tab) in app.tabs.iter_mut().enumerate() {
                if i != active {
                    follow(&mut tab.cwd);
                }
            }
            let removed: HashSet<&Path> = outcome.removed.iter().map(PathBuf::as_path).collect();
            app.marks
                .retain(|p| !p.ancestors().any(|a| removed.contains(a)));
            match then {
                Then::Executed { summary, trashed } => {
                    let complete = outcome.warning.is_none();
                    // Rename counts only the rows that changed.
                    let done = summary.unwrap_or_else(|| {
                        format!("Renamed {}", entries(outcome.moved.iter().map(|(p, _)| p)))
                    });
                    let record =
                        (!outcome.undo.is_empty()).then(|| crate::operations::undo::Record {
                            summary: done.clone(),
                            steps: outcome.undo,
                            lost: outcome.lost,
                            order: job.order,
                            marked,
                        });
                    if let Some(remark) = job.undoing {
                        // It finished before it could stop: undo it now.
                        match record {
                            Some(record) => start_undo(app, discovery, record, remark),
                            None => {
                                app.status =
                                    format!("{done} before it could stop; that cannot be undone")
                            }
                        }
                    } else {
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
                        if let Some(record) = record {
                            let at = app.undo.partition_point(|r| r.order <= record.order);
                            app.undo.insert(at, record);
                            if app.undo.len() > UNDO_LIMIT {
                                app.undo.remove(0);
                            }
                        }
                    }
                }
                Then::Undone {
                    record,
                    remark: again,
                } => {
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
                    if again {
                        remark(app, &record.marked);
                    }
                }
                Then::Planned => unreachable!(),
            }
        }
        (Err(error), _) if error == crate::operations::CANCELLED => {
            app.status = format!("Stopped {}; nothing changed", lowercase_first(&title));
            if job.undoing == Some(true) {
                remark(app, &marked);
            }
        }
        (Err(error), then) => {
            app.status = error;
            // It rolled back (or stopped partway): its entries are marked
            // again, unless x stopped it.
            if matches!(then, Then::Executed { .. }) && job.undoing != Some(false) {
                remark(app, &marked);
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("fff-jobs-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir.canonicalize().unwrap()
    }
    fn claim(reads: &[&Path], writes: &[&Path]) -> Claim {
        Claim {
            reads: reads.iter().map(|p| p.to_path_buf()).collect(),
            writes: writes.iter().map(|p| p.to_path_buf()).collect(),
            leaving: vec![],
        }
    }

    #[test]
    fn operations_collide_when_one_changes_what_the_other_uses() {
        let (a, b) = (Path::new("/w/a"), Path::new("/w/b"));
        // Reading the same tree twice is fine; so are separate trees.
        assert!(
            claim(&[a], &[b])
                .collides(&claim(&[a], &[Path::new("/w/c")]))
                .is_none()
        );
        // Writing into, above or over what the other reads or writes is not.
        assert!(
            claim(&[], &[Path::new("/w/a/x")])
                .collides(&claim(&[a], &[]))
                .is_some()
        );
        assert!(
            claim(&[a], &[])
                .collides(&claim(&[], &[Path::new("/w")]))
                .is_some()
        );
        assert!(claim(&[], &[b]).collides(&claim(&[], &[b])).is_some());
        // A copy reads its sources and writes their names in the destination.
        let copy = Claim::of(&crate::operations::Request::Copy {
            sources: vec![a.into()],
            directory: b.into(),
            replace: vec![],
            confirmed: vec![],
            into: None,
        });
        assert!(copy.reads.contains(&a.to_path_buf()));
        assert!(copy.writes.contains(&b.join("a")) && !copy.writes.contains(&b.to_path_buf()));
        assert!(copy.leaving.is_empty());
    }

    #[test]
    fn what_a_running_operation_changes_waits_for_it() {
        let dir = scratch("collide");
        let mut app = App::browse(dir.clone(), 1, crate::Policy::default());
        let mut discovery = Discovery::new(Default::default());
        let mut focus = Focus::Results;
        app.jobs.push(Job::running(
            "Copying photos",
            &[dir.join("b/photos")],
            false,
        ));
        delete(
            &mut app,
            &mut discovery,
            &mut focus,
            vec![dir.join("b")],
            true,
        );
        assert_eq!(app.status, "Copying photos uses b; wait for it to finish");
        assert!(matches!(focus, Focus::Results));
        // Reading from it waits too; elsewhere goes on.
        app.status.clear();
        let copy = crate::operations::Transfer::Copy;
        transfer(
            &mut app,
            &mut discovery,
            &mut focus,
            copy,
            vec![dir.join("b/photos/1.jpg")],
            false,
        );
        assert!(
            app.status.ends_with("wait for it to finish"),
            "{}",
            app.status
        );
        app.status.clear();
        let request = crate::operations::Request::Create {
            path: dir.join("b/photos"),
            directory: true,
        };
        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
        assert!(
            app.status.ends_with("wait for it to finish"),
            "{}",
            app.status
        );
        assert!(!dir.join("b").exists());
        let request = crate::operations::Request::Create {
            path: dir.join("notes"),
            directory: true,
        };
        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
        assert!(dir.join("notes").is_dir(), "{}", app.status);
        // Files it changes are not opened or given to programs.
        assert!(app.in_use(&[dir.join("b/photos/1.jpg")]));
        assert!(
            app.status.starts_with("1.jpg is in use (copying photos)"),
            "{}",
            app.status
        );
        assert!(!app.in_use(&[dir.join("notes")]));
        app.jobs.clear();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn folders_being_moved_or_deleted_cannot_be_entered() {
        let dir = scratch("enter");
        std::fs::create_dir_all(dir.join("old/sub")).unwrap();
        std::fs::create_dir_all(dir.join("new")).unwrap();
        let mut app = App::browse(dir.clone(), 1, crate::Policy::default());
        app.jobs
            .push(Job::running("Deleting old", &[dir.join("old")], true));
        assert!(!app.navigate(dir.join("old/sub"), false));
        assert_eq!(app.cwd, dir);
        assert!(
            app.status
                .starts_with("sub is being moved or deleted (deleting old)")
        );
        assert!(app.navigate(dir.join("new"), false));
        // Going back skips it.
        app.back.push(dir.join("old"));
        assert!(app.jump(true));
        assert_eq!(app.cwd, dir);
        app.jobs.clear();
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn marks_leave_with_a_started_operation_and_come_back_with_its_undo() {
        let dir = scratch("marks");
        std::fs::write(dir.join("a"), "a").unwrap();
        std::fs::create_dir(dir.join("into")).unwrap();
        let mut app = App::browse(dir.clone(), 1, crate::Policy::default());
        app.trash = Some(crate::trash::Trash::new(&dir.join("data"), 30, u64::MAX));
        let mut discovery = Discovery::new(Default::default());
        let mut focus = Focus::Results;
        app.marks.push(dir.join("a"));
        app.marks.push(dir.join("into"));
        let request = crate::operations::Request::Copy {
            sources: vec![dir.join("a")],
            directory: dir.join("into"),
            replace: vec![],
            confirmed: vec![],
            into: None,
        };
        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
        assert!(dir.join("into/a").exists(), "{}", app.status);
        assert_eq!(app.marks.to_vec(), [dir.join("into")]);
        assert_eq!(
            app.undo.last().map(|r| r.marked.clone()),
            Some(vec![dir.join("a")])
        );
        undo(&mut app, &mut discovery);
        while let Some(job) = app.jobs.first() {
            let result = job.poll(Duration::from_secs(5)).unwrap();
            let job = app.jobs.remove(0);
            ended(&mut app, &mut discovery, job, result);
        }
        assert!(!dir.join("into/a").exists(), "{}", app.status);
        assert_eq!(app.marks.to_vec(), [dir.join("into"), dir.join("a")]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn other_tabs_follow_their_moved_directory_and_only_the_current_one_is_kept() {
        let dir = scratch("tabs");
        std::fs::create_dir_all(dir.join("folder/inner")).unwrap();
        std::fs::create_dir(dir.join("into")).unwrap();
        // Every tab starts in the folder fff started in; this one goes up.
        let mut app = App::browse(dir.join("folder/inner"), 1, crate::Policy::default());
        app.cwd = dir.clone();
        let mut discovery = Discovery::new(Default::default());
        let mut focus = Focus::Results;
        let request = crate::operations::Request::Move {
            sources: vec![dir.join("folder")],
            directory: dir.join("into"),
            replace: vec![],
            confirmed: vec![],
            into: None,
        };
        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
        while let Some(job) = app.jobs.first() {
            let result = job.poll(Duration::from_secs(5)).unwrap();
            let job = app.jobs.remove(0);
            ended(&mut app, &mut discovery, job, result);
        }
        assert!(dir.join("into/folder/inner").is_dir(), "{}", app.status);
        assert_eq!(app.tabs[1].cwd, dir.join("into/folder/inner"));
        assert!(app.select_tab(1));
        assert_eq!(app.cwd, dir.join("into/folder/inner"));
        // The browsing directory itself is not moved away.
        let request = crate::operations::Request::Delete {
            sources: vec![dir.join("into")],
            confirmed: vec![],
            trash: vec![],
        };
        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
        assert_eq!(
            app.status,
            "Cannot change the current directory or its ancestors; go up first"
        );
        assert!(app.jobs.is_empty() && dir.join("into").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn starting_an_operation_on_many_marks_is_quick() {
        let dir = scratch("many marks");
        let mut app = App::browse(dir.clone(), 1, crate::Policy::default());
        let mut discovery = Discovery::new(Default::default());
        let mut focus = Focus::Results;
        let sources: Vec<PathBuf> = (0..30_000)
            .map(|i| dir.join(format!("missing-{i:05}")))
            .collect();
        app.marks = sources.clone().into();
        app.marks.push(dir.join("kept"));
        let request = crate::operations::Request::Delete {
            sources,
            confirmed: vec![],
            trash: vec![],
        };
        // A regression to comparing every mark with every source takes
        // minutes in an unoptimized build.
        let start = std::time::Instant::now();
        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
        let took = start.elapsed();
        assert!(took < Duration::from_secs(3), "took {took:?}");
        assert_eq!(app.marks.to_vec(), [dir.join("kept")]);
        while let Some(job) = app.jobs.first() {
            let result = job.poll(Duration::from_secs(5)).unwrap();
            let job = app.jobs.remove(0);
            ended(&mut app, &mut discovery, job, result);
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn u_stops_the_latest_operation_when_it_still_runs() {
        let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
        let mut discovery = Discovery::new(Default::default());
        let record = |order| crate::operations::undo::Record {
            summary: "Renamed a".into(),
            steps: vec![],
            lost: 0,
            order,
            marked: vec![],
        };
        app.undo.push(record(1));
        let mut job = Job::running("Copying photos", &[], false);
        job.order = 2;
        app.jobs.push(job);
        undo(&mut app, &mut discovery);
        assert_eq!(app.jobs[0].undoing, Some(true));
        assert!(app.jobs[0].stopping());
        assert_eq!(app.undo.len(), 1);
        // x stops one without marking again; an undo itself cannot be stopped.
        app.jobs[0].undoing = None;
        stop_job(&mut app, 0, false);
        assert_eq!(app.jobs[0].undoing, Some(false));
        // An operation that started before the last one on the list waits.
        app.jobs[0].undoing = None;
        app.jobs[0].order = 0;
        undo(&mut app, &mut discovery);
        assert!(app.undo.is_empty());
        app.jobs.clear();
    }
}
#[cfg(test)]
#[path = "jobs_stress.rs"]
mod stress;
