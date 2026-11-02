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
pub(super) fn scope(marks: &[PathBuf], current: Option<PathBuf>) -> Scope {
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
            for marked in &mut app.marks {
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
            }
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
        .rows
        .iter()
        .find(|c| Some(&c.path) == app.highlight.as_ref() && !c.directory)
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
/// row; a second click on it acts like `l`; dragging with the button held
/// marks every row from the pressed one to the current one. The wheel only
/// scrolls. `area` holds the rows of the last frame, one per rendered target.
pub(super) fn mouse_results(
    app: &mut App,
    discovery: &mut Discovery,
    event: MouseEvent,
    area: ratatui::layout::Rect,
    mouse: &mut Mouse,
) {
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
                return;
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
                discovery.checkpoint(app);
                activate_highlighted(app, discovery);
            } else {
                mouse.click = Some((path.clone(), now));
                mouse.anchor = Some(path);
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            let Some(anchor) = mouse.anchor.as_ref() else {
                return;
            };
            let Some(from) = app.rendered.iter().position(|p| p == anchor) else {
                return;
            };
            // Past the top or bottom of the rows, the drag reaches the edge.
            let to = if event.row < area.y {
                0
            } else {
                usize::from(event.row - area.y).min(app.rendered.len().saturating_sub(1))
            };
            if to == from && app.marks.is_empty() {
                return;
            }
            let rows: Vec<PathBuf> = if from <= to {
                app.rendered[from..=to].to_vec()
            } else {
                app.rendered[to..=from].iter().rev().cloned().collect()
            };
            app.highlight = rows.last().cloned();
            app.marks = rows;
            // A drag is not the first half of a double click.
            mouse.click = None;
        }
        MouseEventKind::Up(MouseButton::Left) => mouse.anchor = None,
        _ => {}
    }
}
