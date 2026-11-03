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
    /// R: back from the trash.
    Restore,
}
/// l, Alt+l and Alt+number on a file: its opener in the background (a
/// terminal opener in a new terminal window), while fff keeps the keyboard.
pub(super) fn open_in_background(app: &mut App, path: &std::path::Path) {
    // Open and save dialogs choose files rather than opening them.
    if let Some(hint) = app.dialog.as_ref().and_then(|d| d.file_hint(app)) {
        app.status = hint;
        return;
    }
    let name = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    // From a fff grep match, at the match, as the rule's at_grep_match says.
    let at = app
        .highlighted()
        .filter(|_| app.highlight.as_deref() == Some(path))
        .and_then(|row| {
            let grep = app.grep.as_ref().filter(|g| g.active())?;
            let (hits, i) = grep.shown_hits.get(row)?;
            let hit = &hits[*i];
            Some(crate::open::GrepMatch {
                page: hit.page,
                line: hit.line,
                percent: hit.position,
                query: grep.text.trim().to_owned(),
                matched: hit.matched_text(),
                nth: i + 1,
                count: hits.len(),
            })
        });
    let result = crate::open::background_plan(
        &app.openers,
        path,
        at.as_ref(),
        &app.cwd,
        app.home.as_deref(),
    )
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
    use Program::*;
    match (target, app.program) {
        (Some(entry), _) if !app.ready_path(&entry.path) => None,
        (Some(dir), Window) if dir.directory => {
            if app.enter(Some(crate::Candidate::clone(dir))) == EnterOutcome::Navigated {
                discovery.start(app, true);
            }
            None
        }
        (None, Window | Seek) => {
            app.nothing_to_enter();
            None
        }
        (Some(file), program) if !file.directory => match program {
            Roam | Window => match open_and_leave(app, std::slice::from_ref(&file.path)) {
                Ok(finish) => Some(finish),
                Err(error) => {
                    app.status = error;
                    None
                }
            },
            Seek => Some(Finish::Folder(
                file.path.parent().unwrap_or(&app.cwd).to_owned(),
            )),
            _ => Some(Finish::Paths(vec![file.path.clone()])),
        },
        (Some(dir), _) => Some(Finish::Folder(dir.path.clone())),
        (None, _) => Some(Finish::Folder(app.cwd.clone())),
    }
}
/// Select returns the marks, like Ctrl+w, when there are some to return.
pub(super) fn select_returns_marks(app: &App) -> bool {
    !app.marks.is_empty() && app.returns_marks()
}
/// Ctrl+w: the marks are returned where the answer can be several paths
/// (`App::returns_marks`); elsewhere nothing happens and the status line says
/// why. `None` when fff stays.
pub(super) fn leave_with_marks(app: &mut App) -> Option<Finish> {
    if !app.returns_marks() {
        app.status = format!(
            "{} returns the marks only where several paths can be chosen, as in fff pick",
            app.bound_or_none(Function::PrintMarks)
        );
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
                "{} is a folder; {} goes into it",
                target.file_name().unwrap_or_default().to_string_lossy(),
                app.bound_or_none(Function::GoToHighlighted)
            );
            None
        }
        Ok(_) => {
            let folder = target.parent().unwrap_or(&app.cwd);
            let both = crate::explorer::free_name(folder, target.file_name().unwrap_or_default());
            *focus = Focus::Replace(Replace {
                taken: vec![(target, both)],
                then: Replacing::Save,
            });
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
        let name = dir.file_name().unwrap_or_default().to_string_lossy();
        app.status = match (
            app.bound(Function::OpenInTerminal),
            app.bound(Function::GoToHighlighted),
        ) {
            (Some(open), Some(enter)) => {
                format!("{open} opens files, not directories like {name}; {enter} enters it")
            }
            (Some(open), None) => format!("{open} opens files, not directories like {name}"),
            (None, Some(enter)) => format!("{name} is a directory; {enter} enters it"),
            (None, None) => format!("{name} is a directory"),
        };
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
        Err(error) => app.status = bookmark_error(app, error),
    }
}
/// A refused bookmark write's message; when the file changed meanwhile, it
/// names the chooser's reload key (r in Normal mode, else its Alt chord).
pub(super) fn bookmark_error(app: &App, error: String) -> String {
    if error != crate::bookmarks::CHANGED {
        return error;
    }
    match app.key("alt+r") {
        Some(alt) => format!("{error} (r, or {alt} in Insert mode)"),
        None => format!("{error} (r in Normal mode)"),
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
