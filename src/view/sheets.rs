use super::*;

/// File-operation dialogs share one geometry: a sheet spanning the outer edges
/// of the result and mark panes, resting on their bottom border, as tall as its
/// content. Key hints sit in the bottom border; an optional summary sits top right.
pub(super) struct Sheet<'a> {
    pub(super) title: String,
    pub(super) accent: Color,
    pub(super) keys: &'a [(&'a str, &'a str)],
    pub(super) summary: Option<String>,
}
pub(super) fn sheet(frame: &mut Frame, main: Rect, app: &App, height: u16, sheet: Sheet) -> Rect {
    let Sheet {
        title,
        accent,
        keys,
        summary,
    } = sheet;
    let theme = app.palette;
    let height = height.min(main.height);
    let dialog = Rect::new(
        main.x,
        main.bottom().saturating_sub(height),
        main.width,
        height,
    );
    // Hints must fit inside the corners: tighten separators, then leave out
    // Esc (the prose explains it), then labels, rather than clip a key.
    let rule = Style::default().fg(accent).add_modifier(Modifier::BOLD);
    let room = (main.width as usize).saturating_sub(4);
    let fitting = [
        ("───", true, true),
        ("─", true, true),
        ("─", false, true),
        ("─", false, false),
    ]
    .into_iter()
    .map(|(separator, escape, labels)| {
        let mut hints = vec![];
        for (key, label) in keys.iter().filter(|(k, _)| escape || *k != "Esc") {
            if !hints.is_empty() {
                hints.push(Span::styled(separator, rule));
            }
            hints.push(bold(format!(" {key}"), accent));
            hints.push(styled(
                if labels {
                    format!(": {label} ")
                } else {
                    " ".into()
                },
                theme.text,
            ));
        }
        hints
    })
    .find(|h| h.iter().map(|s| s.width()).sum::<usize>() <= room);
    let hints = fitting.unwrap_or_default();
    let mut block = panel(theme, title, true, accent)
        .padding(Padding::horizontal(1))
        .title_bottom(Line::from(hints).centered());
    if let Some(summary) = summary {
        block = block
            .title_top(Line::from(styled(format!(" {summary} "), theme.muted)).right_aligned());
    }
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    inner
}
/// Word-wraps styled prose; `*key*` segments are drawn as bold keys.
pub(super) fn prose(theme: Palette, accent: Color, text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = vec![Line::default()];
    let mut used = 0;
    for (i, part) in text.split('*').enumerate() {
        let style = if i % 2 == 1 {
            Style::default().fg(accent).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };
        // Keep each word's trailing space with it so lines break between words.
        for word in part.split_inclusive(' ') {
            let w = word.trim_end().width();
            if used > 0 && used + w > width {
                lines.push(Line::default());
                used = 0;
            }
            let word = if used == 0 { word.trim_start() } else { word };
            used += word.width();
            lines
                .last_mut()
                .unwrap()
                .spans
                .push(Span::styled(word.to_string(), style));
        }
    }
    lines
}
pub(super) fn count(n: usize) -> String {
    if n == 1 {
        "1 entry".into()
    } else {
        format!("{n} entries")
    }
}
pub(super) fn quoted(path: &Path, app: &App) -> String {
    let mut name = display_path(path, &app.cwd);
    if std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) && !name.ends_with('/') {
        name.push('/');
    }
    name
}
/// Paths with right-aligned sizes, styled like result rows, heaviest first.
pub(super) fn size_lines(
    app: &App,
    rows: &[(PathBuf, u64)],
    capacity: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let theme = app.palette;
    crate::operations::deletion_rows(rows, capacity)
        .into_iter()
        .map(|(listed, size)| {
            let mut spans = vec![styled(
                format!("{:>10}  ", crate::operations::human_size(size)),
                theme.muted,
            )];
            let room = width.saturating_sub(12);
            match listed {
                crate::operations::Listed::Path(path) => {
                    let directory = std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir());
                    let (glyph, group) = crate::icons::marker(&path, directory, app.icons);
                    let name = elide(&quoted(&path, app), room.saturating_sub(glyph.width()));
                    let split = name.trim_end_matches('/').rfind('/').map_or(0, |i| i + 1);
                    spans.push(styled(glyph, icon_color(theme, group)));
                    spans.push(styled(name[..split].to_string(), theme.muted));
                    spans.push(styled(
                        name[split..].to_string(),
                        path_color(theme, &path, directory),
                    ));
                }
                crate::operations::Listed::More(n) => {
                    // Align with the names, past the icon column.
                    let glyph = crate::icons::marker(Path::new(""), false, app.icons).0;
                    spans.push(Span::raw(" ".repeat(glyph.width())));
                    spans.push(
                        styled(
                            format!("and {n} more {}", if n == 1 { "entry" } else { "entries" }),
                            theme.muted,
                        )
                        .italic(),
                    )
                }
            }
            Line::from(spans)
        })
        .collect()
}
pub(super) fn total(rows: &[(PathBuf, u64)]) -> String {
    let bytes = rows
        .iter()
        .fold(0u64, |n, (_, size)| n.saturating_add(*size));
    format!(
        "{} · {}",
        count(rows.len()),
        crate::operations::human_size(bytes)
    )
}
/// A short explanation above a listing of the affected paths and their sizes.
#[allow(clippy::too_many_arguments)]
pub(super) fn listing(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    title: &str,
    accent: Color,
    text: &str,
    keys: &[(&str, &str)],
    rows: &[(PathBuf, u64)],
) {
    let theme = app.palette;
    let width = main.width.saturating_sub(4) as usize;
    let body = prose(theme, accent, text, width);
    // Borders, prose, a blank line, then as many rows as fit.
    let height = (body.len() + rows.len() + 3) as u16;
    let inner = sheet(
        frame,
        main,
        app,
        height,
        Sheet {
            title: format!(" {title} "),
            accent,
            keys,
            summary: Some(total(rows)),
        },
    );
    let capacity = (inner.height as usize).saturating_sub(body.len() + 1);
    let mut lines = body;
    lines.push(Line::default());
    lines.extend(size_lines(app, rows, capacity, inner.width as usize));
    frame.render_widget(Paragraph::new(lines), inner);
}
pub(super) fn delete_confirmation(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    rows: &[(PathBuf, u64)],
) {
    let (what, them) = if rows.len() == 1 {
        ("this entry", "it")
    } else {
        ("these entries", "them")
    };
    listing(
        frame,
        main,
        app,
        "Delete permanently",
        app.palette.error,
        &format!(
            "Press *Enter* to permanently delete {what}, including everything inside \
             directories. Press *Esc* to keep {them}."
        ),
        &[("Enter", "Delete"), ("Esc", "Cancel")],
        rows,
    );
}
/// Move, copy and link confirmations list the selection; when existing entries
/// would be overwritten, the window lists those instead, as a warning.
pub(super) fn transfer_confirmation(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    plan: &crate::operations::TransferPlan,
) {
    use crate::operations::Transfer;
    let (verb, noun, label) = match plan.kind {
        Transfer::Move => ("Moving", "move", "Move"),
        Transfer::Copy => ("Copying", "copy", "Copy"),
        Transfer::Link => ("Linking", "link", "Link"),
    };
    let here = tilde(&app.cwd, app.home.as_deref());
    let selected = plan.sources.len();
    let these = if selected == 1 {
        "this entry".to_string()
    } else {
        format!("these {selected} entries")
    };
    if plan.overwrites.is_empty() {
        let action = match plan.kind {
            Transfer::Link => format!("create links to {these} in {here}"),
            _ => format!("{noun} {these} into {here}"),
        };
        listing(
            frame,
            main,
            app,
            &format!("{label} here"),
            app.palette.marked,
            &format!("Press *Enter* to {action}, or *Esc* to cancel."),
            &[("Enter", label), ("Esc", "Cancel")],
            &plan.sources,
        );
        return;
    }
    let one = plan.overwrites.len() == 1;
    listing(
        frame,
        main,
        app,
        &format!("Overwrite on {noun}"),
        app.palette.error,
        &format!(
            "{verb} {} into {here} would overwrite {}. Press *Enter* to {noun} everything \
             and overwrite {}, or *Esc* to cancel the whole {noun}.",
            count(selected),
            if one { "this entry" } else { "these entries" },
            if one { "it" } else { "them" },
        ),
        &[("Enter", "Overwrite"), ("Esc", "Cancel")],
        &plan.overwrites,
    );
}
pub(super) fn these(paths: &[PathBuf], app: &App) -> String {
    match paths {
        [] => "no files".into(),
        [one] => quoted(one, app),
        many => format!("these {} entries", many.len()),
    }
}
/// The command line for gx / gX: Tab completes, a live line shows what the
/// command word resolves to, Enter reviews.
pub(super) fn run_prompt(frame: &mut Frame, main: Rect, app: &App, prompt: &crate::run::Prompt) {
    let theme = app.palette;
    let here = tilde(&app.cwd, app.home.as_deref());
    let then = if prompt.exit {
        "fff exits and the program takes its place"
    } else {
        "fff comes back when the program ends"
    };
    let text = format!(
        "Type a program to run on {} in {here}; the paths are added after what you \
         type, and {then}. *Tab* completes commands and paths. There is no shell: \
         quote shell symbols such as | > & ; ? $ to pass them literally.",
        these(&prompt.paths, app)
    );
    let width = main.width.saturating_sub(4) as usize;
    let body = prose(theme, theme.marked, &text, width);
    let shown = prompt.candidates.len().min(4) as u16;
    // Borders, prose, blank, input, the program line, candidates, a status row.
    let height = body.len() as u16 + 6 + shown;
    let title = if prompt.exit { "Run and exit" } else { "Run" };
    let inner = sheet(
        frame,
        main,
        app,
        height,
        Sheet {
            title: mode_title(app, Field::Command, title),
            accent: theme.marked,
            keys: &[("Enter", "Review"), ("Tab", "Complete"), ("Esc", "Cancel")],
            summary: Some(count(prompt.paths.len())),
        },
    );
    let top = (body.len() as u16 + 1).min(inner.height);
    frame.render_widget(
        Paragraph::new(body),
        Rect::new(inner.x, inner.y, inner.width, top),
    );
    let row = inner.y + top;
    if row >= inner.bottom() {
        return;
    }
    frame.render_widget(
        Paragraph::new(bold("❯ ", theme.marked)),
        Rect::new(inner.x, row, 2.min(inner.width), 1),
    );
    edit_rows(
        frame,
        app,
        Field::Command,
        &prompt.text,
        Rect::new(inner.x + 2, row, inner.width.saturating_sub(2), 1),
    );
    let mut lines = vec![match prompt.program(&app.cwd, app.home.as_deref()) {
        Some(Ok(path)) => Line::from(styled(
            format!("✓ {}", safe(path.to_string_lossy())),
            theme.success,
        )),
        Some(Err(error)) => Line::from(styled(format!("✗ {}", safe(error)), theme.error)),
        None => Line::from(styled("Type a command name", theme.muted)),
    }];
    if !prompt.candidates.is_empty() {
        let mut spans = vec![];
        for (i, candidate) in prompt.candidates.iter().enumerate() {
            let style = if prompt.chosen == Some(i) {
                Style::default()
                    .fg(theme.marked)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                Style::default().fg(theme.text)
            };
            spans.push(Span::styled(safe(candidate), style));
            spans.push(Span::raw("  "));
        }
        lines.push(Line::from(spans));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }),
        Rect::new(
            inner.x,
            row + 1,
            inner.width,
            inner.bottom().saturating_sub(row + 2),
        ),
    );
    let last = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    if app
        .edit_view
        .as_ref()
        .is_some_and(|v| v.field == Field::Command && v.mode.starts_with('c'))
    {
        command_line(frame, app, Field::Command, last);
    }
}
/// Alt+g: a typed path, or words picking a bookmark or a zoxide favorite.
pub(super) fn location_sheet(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    location: &crate::location::Location,
) -> Vec<(Rect, usize)> {
    use crate::location::Source;
    let theme = app.palette;
    let width = main.width.saturating_sub(4) as usize;
    let body = prose(
        theme,
        theme.marked,
        "Type a path, or words from a folder's path to pick one of your bookmarks or \
         zoxide's frequent folders. *↑* *↓* choose, *Tab* completes a path, *Enter* goes.",
        width,
    );
    let matches = location.matches();
    let shown = matches.len().min(8);
    // Borders, prose, blank, input, suggestions, and a row for commands/errors.
    let height = (body.len() + shown + 5) as u16;
    let inner = sheet(
        frame,
        main,
        app,
        height,
        Sheet {
            title: mode_title(app, Field::Location, "Go to directory"),
            accent: theme.marked,
            keys: &[
                ("Enter", "Go"),
                ("↑↓", "Choose"),
                ("Tab", "Complete"),
                ("Esc", "Cancel"),
            ],
            summary: (!location.places.is_empty()).then(|| count(matches.len())),
        },
    );
    let top = (body.len() as u16 + 1).min(inner.height);
    frame.render_widget(
        Paragraph::new(body),
        Rect::new(inner.x, inner.y, inner.width, top),
    );
    let row = inner.y + top;
    if row >= inner.bottom() {
        return Vec::new();
    }
    frame.render_widget(
        Paragraph::new(bold("❯ ", theme.marked)),
        Rect::new(inner.x, row, 2.min(inner.width), 1),
    );
    edit_rows(
        frame,
        app,
        Field::Location,
        &location.text,
        Rect::new(inner.x + 2, row, inner.width.saturating_sub(2), 1),
    );
    // Keep the chosen suggestion visible.
    let first = location
        .selected
        .map_or(0, |i| i.saturating_sub(shown.saturating_sub(1)));
    let lines = matches
        .iter()
        .enumerate()
        .skip(first)
        .take(shown)
        .map(|(i, (path, source))| {
            let chosen = location.selected == Some(i);
            let name = match source {
                Source::Bookmark(name) => format!("★ {name}  "),
                Source::Zoxide => "  ".into(),
            };
            let place = elide(
                &tilde(path, app.home.as_deref()),
                width.saturating_sub(name.width() + 2),
            );
            let bg = if chosen { theme.selection } else { theme.panel };
            Line::from(vec![
                styled(if chosen { "› " } else { "  " }, theme.accent),
                bold(safe(name), theme.marked),
                styled(place, if chosen { theme.text } else { theme.muted }),
            ])
            .style(Style::default().bg(bg))
        })
        .collect::<Vec<_>>();
    let list = Rect::new(
        inner.x,
        row + 1,
        inner.width,
        inner.bottom().saturating_sub(row + 2),
    );
    let choices = item_areas(list, first, std::iter::repeat_n(1, lines.len()));
    frame.render_widget(Paragraph::new(lines), list);
    let last = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    if app
        .edit_view
        .as_ref()
        .is_some_and(|v| v.field == Field::Location && v.mode.starts_with('c'))
    {
        command_line(frame, app, Field::Location, last);
    } else if !app.status.is_empty() && app.status != app.status_success {
        frame.render_widget(Paragraph::new(styled(safe(&app.status), theme.error)), last);
    }
    choices
}
/// The paths a program will receive, in argument order.
pub(super) fn path_lines(
    app: &App,
    paths: &[PathBuf],
    capacity: usize,
    width: usize,
) -> Vec<Line<'static>> {
    let theme = app.palette;
    let take = if paths.len() > capacity {
        capacity.saturating_sub(1)
    } else {
        paths.len()
    };
    let mut lines = paths[..take]
        .iter()
        .map(|path| {
            let directory = std::fs::symlink_metadata(path).is_ok_and(|m| m.is_dir());
            let (glyph, group) = crate::icons::marker(path, directory, app.icons);
            let name = elide(&quoted(path, app), width.saturating_sub(glyph.width()));
            let split = name.trim_end_matches('/').rfind('/').map_or(0, |i| i + 1);
            Line::from(vec![
                styled(glyph, icon_color(theme, group)),
                styled(name[..split].to_string(), theme.muted),
                styled(
                    name[split..].to_string(),
                    path_color(theme, path, directory),
                ),
            ])
        })
        .collect::<Vec<_>>();
    if take < paths.len() {
        lines.push(Line::from(
            styled(
                format!("and {} more", count(paths.len() - take)),
                theme.muted,
            )
            .italic(),
        ));
    }
    lines
}
pub(super) fn run_confirmation(frame: &mut Frame, main: Rect, app: &App, plan: &crate::run::Plan) {
    let theme = app.palette;
    let here = tilde(&plan.cwd, app.home.as_deref());
    let then = if plan.exit {
        "fff exits and the program takes its place"
    } else {
        "fff comes back when it ends"
    };
    let text = format!(
        "Press *Enter* to run {} on {} in {here}; {then}. Press *Esc* to cancel.",
        plan.line,
        these(&plan.paths, app)
    );
    let width = main.width.saturating_sub(4) as usize;
    let body = prose(theme, theme.marked, &text, width);
    let program = Line::from(vec![
        styled("❯ ", theme.marked),
        bold(safe(&plan.line), theme.text),
        styled(
            format!("  {}", safe(plan.program.to_string_lossy())),
            theme.muted,
        ),
    ]);
    let height = (body.len() + plan.paths.len() + 5) as u16;
    let title = if plan.exit {
        "Run and exit"
    } else {
        "Run here"
    };
    let inner = sheet(
        frame,
        main,
        app,
        height,
        Sheet {
            title: format!(" {title} "),
            accent: theme.marked,
            keys: &[("Enter", "Run"), ("Esc", "Cancel")],
            summary: Some(count(plan.paths.len())),
        },
    );
    let capacity = (inner.height as usize).saturating_sub(body.len() + 3);
    let mut lines = body;
    lines.push(Line::default());
    lines.push(program);
    lines.push(Line::default());
    lines.extend(path_lines(app, &plan.paths, capacity, inner.width as usize));
    frame.render_widget(Paragraph::new(lines), inner);
}
/// A one-keystroke choice: prose naming each key, keys repeated in the border.
pub(super) fn choice(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    title: &str,
    text: &str,
    keys: &[(&str, &str)],
) {
    let theme = app.palette;
    let body = prose(
        theme,
        theme.marked,
        text,
        main.width.saturating_sub(4) as usize,
    );
    let inner = sheet(
        frame,
        main,
        app,
        body.len() as u16 + 2,
        Sheet {
            title: format!(" {title} "),
            accent: theme.marked,
            keys,
            summary: None,
        },
    );
    frame.render_widget(Paragraph::new(body), inner);
}
pub(super) fn scope_choice(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    current: &Path,
    selected: usize,
    action: crate::ui::ScopedAction,
) {
    let (title, verb) = match action {
        crate::ui::ScopedAction::Extract => ("Extract", "extract"),
        crate::ui::ScopedAction::Delete => ("Delete", "delete"),
        crate::ui::ScopedAction::Run(false) => ("Run", "run a program on"),
        crate::ui::ScopedAction::Run(true) => ("Run and exit", "run a program on"),
        crate::ui::ScopedAction::Edit => ("Edit", "open in $EDITOR"),
        crate::ui::ScopedAction::Open => ("Open", "open"),
    };
    choice(
        frame,
        main,
        app,
        title,
        &format!(
            "The current entry is not selected. Press *c* to {verb} only the current \
             entry, {}; *s* to {verb} the selection ({}); or *a* to {verb} both ({}).",
            quoted(current, app),
            count(selected),
            count(selected + 1),
        ),
        &[
            ("c", "Current"),
            ("s", "Selection"),
            ("a", "All"),
            ("Esc", "Cancel"),
        ],
    );
}
/// fff explorer --save: the chosen name is taken.
pub(super) fn replace_choice(frame: &mut Frame, main: Rect, app: &App, target: &Path) {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let folder = target.parent().unwrap_or(target);
    let both = crate::explorer::free_name(folder, &name);
    choice(
        frame,
        main,
        app,
        "Replace",
        &format!(
            "*{name}* already exists in {}. Press *Enter* to replace it, *k* to keep \
             both (saving as *{}*), or *Esc* to go back and choose another name.",
            tilde(folder, app.home.as_deref()),
            both.file_name().unwrap_or_default().to_string_lossy(),
        ),
        &[("Enter", "Replace"), ("k", "Keep both"), ("Esc", "Back")],
    );
}
pub(super) fn new_choice(frame: &mut Frame, main: Rect, app: &App) {
    choice(
        frame,
        main,
        app,
        "Create",
        &format!(
            "Press *f* to create a file or *d* to create a directory in {}. \
             You will name it next.",
            tilde(&app.cwd, app.home.as_deref()),
        ),
        &[("f", "File"), ("d", "Directory"), ("Esc", "Cancel")],
    );
}
/// Rename, extraction and creation buffers. Rename fills the pane area; the
/// single-line buffers are as tall as their explanation plus one input row.
pub(super) fn operation_prompt(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    draft: &crate::operations::Draft,
) {
    use crate::operations::EditKind;
    let theme = app.palette;
    let field = draft.field();
    let here = tilde(&app.cwd, app.home.as_deref());
    let (title, text) = match &draft.kind {
        EditKind::Rename(_) => (
            "Rename files",
            "Edit names in place: one entry per row, in the same order. Rename a \
             directory on its own row; rows inside it may use its old or new name."
                .to_string(),
        ),
        EditKind::Extract(archives) if archives.len() == 1 => (
            "Extract archive",
            format!(
                "Type the directory to extract {} into, relative to {here}. Leave it \
                 empty to extract beside the archive.",
                quoted(&archives[0], app)
            ),
        ),
        EditKind::Extract(archives) => (
            "Extract archives",
            format!(
                "Type the directory to extract the {} archives into, relative to \
                 {here}. Leave it empty to extract each beside its archive.",
                archives.len()
            ),
        ),
        EditKind::New(true) => (
            "New directory",
            format!("Type a name for the new directory in {here}."),
        ),
        EditKind::New(false) => (
            "New file",
            format!("Type a name for the new file in {here}."),
        ),
    };
    let keys: &[(&str, &str)] = match &draft.kind {
        EditKind::Rename(_) if app.edit_view.is_some() => &[("ZZ", "Apply"), ("ZQ", "Discard")],
        EditKind::Rename(_) => &[("Ctrl+s", "Apply"), ("Esc", "Discard")],
        EditKind::Extract(_) => &[("Enter", "Extract"), ("Esc", "Cancel")],
        EditKind::New(_) => &[("Enter", "Create"), ("Esc", "Cancel")],
    };
    let body = prose(
        theme,
        theme.marked,
        &text,
        main.width.saturating_sub(4) as usize,
    );
    let multiline = draft.multiline();
    // Borders, prose, a blank line, the input, and a row for commands or errors.
    let height = if multiline {
        main.height
    } else {
        body.len() as u16 + 5
    };
    let inner = sheet(
        frame,
        main,
        app,
        height,
        Sheet {
            title: mode_title(app, field, title),
            accent: theme.marked,
            keys,
            summary: None,
        },
    );
    let top = (body.len() as u16 + 1).min(inner.height);
    frame.render_widget(
        Paragraph::new(body),
        Rect::new(inner.x, inner.y, inner.width, top),
    );
    // Single-line inputs carry the query box's prompt glyph.
    let indent = if multiline { 0 } else { 2 };
    if !multiline {
        frame.render_widget(
            Paragraph::new(bold("❯ ", theme.marked)),
            Rect::new(inner.x, inner.y + top, indent.min(inner.width), 1),
        );
    }
    let editor = Rect::new(
        inner.x + indent,
        inner.y + top,
        inner.width.saturating_sub(indent),
        inner.height.saturating_sub(top + 1),
    );
    edit_rows(frame, app, field, &draft.text, editor);
    let last = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    if app
        .edit_view
        .as_ref()
        .is_some_and(|v| v.field == field && v.mode.starts_with('c'))
    {
        command_line(frame, app, field, last);
    } else if !app.status.is_empty() && app.status != app.status_success {
        frame.render_widget(Paragraph::new(styled(safe(&app.status), theme.error)), last);
    }
}
