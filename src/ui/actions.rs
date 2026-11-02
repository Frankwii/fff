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
    Delete,
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
/// Normal-mode Enter on a file, or Ctrl+w on the marks, in the default mode:
/// their openers, and fff leaves. Files sharing an opener go to one
/// invocation, as with o. GUI openers start detached, as with l; one terminal
/// opener may replace fff in this terminal (like gX), and more than one is
/// refused, since only one can. Every program is resolved before any starts.
/// The shell then changes to the last file's folder.
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
/// a file opens and fff leaves (default mode), is printed (`--output-paths`)
/// or is left into its folder (`--no-dir`); nothing leaves into the browsing
/// directory. Marks are ignored. `None` (with the reason in the status line)
/// when the file cannot open. With `--window` there is no shell to leave
/// into: a directory is entered instead, and nothing stays.
pub(super) fn finish_with(
    app: &mut App,
    discovery: &mut Discovery,
    target: Option<&crate::Candidate>,
) -> Option<Finish> {
    match target {
        Some(dir) if app.window && dir.directory => {
            if app.enter(Some(crate::Candidate::clone(dir))) == EnterOutcome::Navigated {
                discovery.start(app, true);
            }
            None
        }
        None if app.window => {
            app.nothing_to_enter();
            None
        }
        Some(file) if !file.directory && app.ready_path(&file.path) => {
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
        Some(file) if !file.directory && !app.window => Some(Finish::Folder(
            file.path.parent().unwrap_or(&app.cwd).to_owned(),
        )),
        Some(file) if !file.directory => None,
        Some(dir) => Some(Finish::Folder(dir.path.clone())),
        None => Some(Finish::Folder(app.cwd.clone())),
    }
}
/// Ctrl+w: the marks are returned, except in the default mode, where they open
/// and fff leaves. `None` (with the reason in the status line) when they
/// cannot open.
pub(super) fn leave_with_marks(app: &mut App, marks: Vec<PathBuf>) -> Option<Finish> {
    if app.output_paths || app.no_dir || app.dialog.is_some() {
        return Some(Finish::Paths(marks));
    }
    match open_and_leave(app, &marks) {
        Ok(finish) => Some(finish),
        Err(error) => {
            app.status = error;
            None
        }
    }
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
    drop(std::mem::replace(input, Input::idle()));
    terminal.suspend();
    let status = plan.wait();
    if let Ok(status) = &status
        && !status.success()
    {
        crate::run::pause_after_failure(&plan.line, *status);
    }
    terminal.resume()?;
    *input = Input::start(wake.clone());
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
pub(super) fn delete_confirmation(sources: Vec<PathBuf>, status: &mut String) -> Focus {
    match crate::operations::DeletePlan::new(sources) {
        Ok(plan) => Focus::DeleteConfirm(plan),
        Err(error) => {
            *status = error;
            Focus::Results
        }
    }
}
pub(super) fn perform_operation(
    app: &mut App,
    discovery: &mut Discovery,
    request: crate::operations::Request,
) {
    use crate::operations::Request;
    // Deletion only unmarks what it removed, so deleting the current path
    // keeps the remaining selection.
    let clear_selection = matches!(
        &request,
        Request::Move { .. } | Request::Copy { .. } | Request::Symlink { .. }
    );
    let destructive = match &request {
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
    if destructive.iter().any(|p| {
        let resolved = p
            .parent()
            .and_then(|parent| parent.canonicalize().ok())
            .and_then(|parent| p.file_name().map(|name| parent.join(name)));
        // Compare resolved paths too: the browsing path may run through a symlink.
        let cwd = app.cwd.canonicalize().unwrap_or_else(|_| app.cwd.clone());
        [&app.cwd, &cwd].iter().any(|cwd| {
            cwd.starts_with(p) || resolved.as_ref().is_some_and(|path| cwd.starts_with(path))
        })
    }) {
        app.status = "Cannot change the current directory or its ancestors".into();
        return;
    }
    let summary = operation_summary(&request);
    match crate::operations::Engine::default().execute(request) {
        Ok(outcome) => {
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
            if clear_selection {
                app.marks.clear();
            }
            let complete = outcome.warning.is_none();
            app.status = if let Some(warning) = outcome.warning {
                // It took effect, but not completely: marks above reflect what
                // happened; the message is shown as a problem.
                warning
            } else if !outcome.changed {
                "Nothing to rename; no names changed".into()
            } else {
                // Rename counts only the rows that changed.
                summary.unwrap_or_else(|| {
                    format!("Renamed {}", entries(outcome.moved.iter().map(|(p, _)| p)))
                })
            };
            if complete {
                app.status_success = app.status.clone();
            }
        }
        Err(error) => app.status = error,
    }
    let message = std::mem::take(&mut app.status);
    discovery.force_refresh(app);
    app.status = message;
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
        Request::Delete { sources, .. } => format!("Deleted {}", entries(sources.iter())),
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
/// The mouse in the results, like a graphical file manager.
#[derive(Default)]
pub(super) struct Mouse {
    /// The last click, for double clicks.
    click: Option<(PathBuf, std::time::Instant)>,
    /// Where a held left button went down; dragging marks rows from here.
    anchor: Option<PathBuf>,
}
/// `[ui] mouse` in the results. A click clears the marks and highlights a
/// row; a second click on it acts like `l`, or in a dialog (fff explorer)
/// returns true for the caller to deliver Enter on it; dragging with the
/// button held marks every row from the pressed one to the current one. The
/// wheel only scrolls. `area` holds the rows of the last frame, one per
/// rendered target.
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
            let Some(anchor) = mouse.anchor.as_ref() else {
                return false;
            };
            let Some(from) = app.rendered.iter().position(|p| p == anchor) else {
                return false;
            };
            // Past the top or bottom of the rows, the drag reaches the edge.
            let to = if event.row < area.y {
                0
            } else {
                usize::from(event.row - area.y).min(app.rendered.len().saturating_sub(1))
            };
            if to == from && app.marks.is_empty() {
                return false;
            }
            let rows: Vec<PathBuf> = if from <= to {
                app.rendered[from..=to].to_vec()
            } else {
                app.rendered[to..=from].iter().rev().cloned().collect()
            };
            app.highlight = rows.last().cloned();
            app.marks = rows.into();
            // A drag is not the first half of a double click.
            mouse.click = None;
        }
        MouseEventKind::Up(MouseButton::Left) => mouse.anchor = None,
        _ => {}
    }
    false
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
    /// Ctrl+w with marks, else Normal-mode Enter on the highlighted row.
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
            if !app.marks.is_empty() {
                let marks = app.confirm(None)?;
                return leave_with_marks(app, marks);
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
            Some(uri) => uri_path(&uri[uri.find('/')?..])?,
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
/// A URI's percent-encoded path as raw path bytes.
fn uri_path(encoded: &str) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStringExt;
    let bytes = encoded.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            decoded.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            decoded.push(bytes[i]);
            i += 1;
        }
    }
    Some(std::ffi::OsString::from_vec(decoded).into())
}
