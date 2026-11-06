use super::*;

pub(super) fn workspaces(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    query: &str,
    index: usize,
) -> (Vec<usize>, Vec<(Rect, usize)>) {
    let theme = app.palette;
    let width = area.width.saturating_sub(4).min(90);
    let height = area.height.saturating_sub(4).min(22);
    let dialog = centered(area, width, height);
    let block = panel(
        theme,
        mode_title(
            app,
            Field::Workspaces,
            &format!(
                "Workspaces · {} opens · {} returns",
                [app.key("enter"), Some("Alt+digit".to_string())]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" / "),
                app.key_or_none("esc")
            ),
        ),
        true,
        theme.marked,
    )
    .padding(Padding::horizontal(1));
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    let (mut line, cursor) = editable_line(
        app,
        Field::Workspaces,
        query,
        0,
        inner.width.saturating_sub(6) as usize,
    );
    line.spans.insert(0, styled("Find: ", theme.marked));
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.width > 6 {
        frame.set_cursor_position((inner.x + 6 + cursor, inner.y));
    }
    command_line(
        frame,
        app,
        Field::Workspaces,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = app.matching_workspaces(query);
    // Plain letters work while the filter is in Normal mode; otherwise Alt.
    let normal = app.edit_view.as_ref().is_some_and(|v| v.normal());
    let command = |c: char| {
        if normal {
            Some(c.to_string())
        } else {
            app.key(&format!("alt+{c}"))
        }
    };
    // Each workspace occupies three lines; only fully visible entries get labels.
    let page = list.height as usize / 3;
    let start = index.saturating_sub(page.saturating_sub(1));
    let visible: Vec<_> = matches.iter().skip(start).take(page).copied().collect();
    let mut choices = Vec::new();
    if matches.is_empty() {
        let text = if app.workspaces.is_empty() {
            match command('n') {
                Some(key) => format!("No workspaces yet. {key} adds the current directory."),
                None => "No workspaces yet.".into(),
            }
        } else {
            "No matching workspaces".into()
        };
        frame.render_widget(
            Paragraph::new(styled(text, theme.muted)).wrap(Wrap { trim: false }),
            list,
        );
    } else {
        // Configured icons always show; others get the folder glyph when file
        // icons are enabled, or blank padding so names stay aligned.
        let icon_column = app.icons || app.workspaces.iter().any(|b| b.icon.is_some());
        let items: Vec<_> = visible
            .iter()
            .enumerate()
            .map(|(position, &i)| {
                let b = &app.workspaces[i];
                let available = b.path.as_deref().is_none_or(|p| available(app, p));
                let color = if available { theme.marked } else { theme.error };
                let label = if position + 1 == visible.len() {
                    if position < 9 {
                        format!("{}/0", position + 1)
                    } else {
                        "0".into()
                    }
                } else if position < 9 {
                    (position + 1).to_string()
                } else {
                    String::new()
                };
                let mut title = vec![styled(format!("{label:>3} "), theme.muted)];
                if icon_column {
                    let icon = match &b.icon {
                        Some(icon) => safe(icon),
                        None if app.icons => folder_glyph(),
                        None => String::new(),
                    };
                    let pad = 3usize.saturating_sub(icon.width());
                    title.push(styled(icon + &" ".repeat(pad), color));
                }
                title.push(bold(
                    format!(
                        "{}{}",
                        safe(&b.name),
                        if available { "" } else { " [unavailable]" }
                    ),
                    color,
                ));
                let indent = if icon_column { "       " } else { "    " };
                ListItem::new(vec![
                    Line::from(title),
                    Line::from(styled(
                        indent.to_string()
                            + &elide(
                                &summary(app, b),
                                list.width.saturating_sub(2 + indent.len() as u16) as usize,
                            ),
                        theme.muted,
                    )),
                    Line::from(""),
                ])
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(index.saturating_sub(start)));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(theme.selection))
                .highlight_symbol("› "),
            list,
            &mut state,
        );
        let shown = state.offset();
        choices = item_areas(
            list,
            start + shown,
            std::iter::repeat_n(3, visible.len().saturating_sub(shown)),
        );
    }
    let hint = if app.status.is_empty() {
        [
            does(command('n'), "new"),
            does(command('e'), "edit"),
            does(command('d'), "delete"),
            does(command('r'), "reload"),
            does(keys_named(app, "up down"), "reorder"),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ")
    } else {
        app.status.clone()
    };
    frame.render_widget(
        Paragraph::new(styled(
            safe(hint),
            if app.status.is_empty() {
                theme.muted
            } else {
                theme.error
            },
        )),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
    (visible, choices)
}
/// Modal frame for the workspace icon chooser.
pub(super) fn workspace_dialog(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    title: String,
    height: u16,
) -> Rect {
    let width = area.width.saturating_sub(2).min(90);
    let height = area.height.saturating_sub(2).min(height);
    let dialog = centered(area, width, height);
    let block = panel(app.palette, title, true, app.palette.marked).padding(Padding::horizontal(1));
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    inner
}
/// The workspace form shares the operation sheet: an explanation, three
/// labelled rows (the active one carries the prompt glyph) and a status row.
pub(super) fn workspace_form(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    draft: &crate::workspaces::Draft,
) {
    use crate::workspaces::EditField;
    let theme = app.palette;
    let title = if draft.editing.is_some() {
        "Edit workspace"
    } else {
        "New workspace"
    };
    let title = draft
        .field
        .editor()
        .map_or_else(|| format!(" {title} "), |f| mode_title(app, f, title));
    let here = tilde(&app.cwd, app.home.as_deref());
    let body = prose(
        theme,
        theme.marked,
        &format!(
            "Relative paths start at {here}; *~* is your home directory. An empty \
             path leaves the folder as it is. *Space* ticks the settings to keep, \
             as they are now."
        ),
        main.width.saturating_sub(4) as usize,
    );
    let saves = match &app.workspace_store.path {
        Some(path) => format!("saves to {}", tilde(path, app.home.as_deref())),
        None => "this session only".into(),
    };
    // Borders, prose, a blank line, three rows, a blank line, the settings
    // with their heading, and a row for commands or errors.
    let settings = crate::config::Setting::ALL.len() as u16;
    let inner = sheet(
        frame,
        main,
        app,
        body.len() as u16 + 9 + settings,
        Sheet {
            title,
            accent: theme.marked,
            keys: &[("enter", "Save"), ("esc", "Cancel"), ("tab", "Next field")],
            summary: Some(saves),
        },
    );
    let top = (body.len() as u16 + 1).min(inner.height);
    frame.render_widget(
        Paragraph::new(body),
        Rect::new(inner.x, inner.y, inner.width, top),
    );
    // Label, then the query box's prompt glyph on the active row.
    let indent = 8;
    let row = |frame: &mut Frame, offset: u16, label: &str, active: bool| {
        let y = inner.y + top + offset;
        (y < inner.bottom().saturating_sub(1)).then(|| {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    bold(
                        format!("{label:<6}"),
                        if active { theme.marked } else { theme.muted },
                    ),
                    bold(if active { "❯ " } else { "  " }, theme.marked),
                ])),
                Rect::new(inner.x, y, indent.min(inner.width), 1),
            );
            Rect::new(inner.x + indent, y, inner.width.saturating_sub(indent), 1)
        })
    };
    for (offset, label, value, field, active) in [
        (
            0,
            "Name",
            &draft.name,
            Field::WorkspaceName,
            draft.field == EditField::Name,
        ),
        (
            1,
            "Path",
            &draft.path,
            Field::WorkspacePath,
            draft.field == EditField::Path,
        ),
    ] {
        let Some(area) = row(frame, offset, label, active) else {
            continue;
        };
        if active {
            edit_rows(frame, app, field, value, area);
        } else {
            frame.render_widget(
                Paragraph::new(styled(elide(&safe(value), area.width as usize), theme.text)),
                area,
            );
        }
    }
    let active = draft.field == EditField::Icon;
    if let Some(area) = row(frame, 2, "Icon", active) {
        let (glyph, name) = match &draft.icon {
            Some(icon) => (safe(icon), "custom"),
            None if app.icons => (
                crate::icons::icon(Path::new(&draft.path), true)
                    .glyph
                    .to_string(),
                "automatic",
            ),
            None => (String::new(), "automatic"),
        };
        let mut spans = vec![];
        if !glyph.is_empty() {
            spans.push(bold(format!("{glyph}  "), theme.accent));
        }
        spans.push(styled(name, theme.text));
        if active {
            spans.push(styled(" · Space chooses", theme.muted));
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
    // The settings, as the Keys screen's checklists tick them.
    let y = inner.y + top + 4;
    if y < inner.bottom().saturating_sub(1) {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                styled("  ", theme.muted),
                bold("KEEPS", theme.muted),
            ])),
            Rect::new(inner.x, y, inner.width, 1),
        );
    }
    let show = |p: &Path| tilde(p, app.home.as_deref());
    for (i, setting) in crate::config::Setting::ALL.into_iter().enumerate() {
        let y = y + 1 + i as u16;
        if y >= inner.bottom().saturating_sub(1) {
            break;
        }
        let here = draft.field == EditField::Setting(setting);
        let kept = setting.is_in(&draft.kept);
        let (marker, color) = if kept {
            ("●", theme.accent)
        } else if draft.offers(setting) {
            ("○", theme.muted)
        } else {
            ("×", theme.error)
        };
        let shown = if kept { &draft.kept } else { &draft.offered };
        let mut style = Style::default().fg(if kept { theme.text } else { theme.muted });
        if here {
            style = style.bg(theme.selection).add_modifier(Modifier::BOLD);
        }
        let name = format!(" {:<11}", setting.name());
        let used = 3 + name.width();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(if here { "› " } else { "  " }, style.fg(theme.marked)),
                Span::styled(marker, style.fg(color)),
                Span::styled(name, style),
                Span::styled(
                    elide(
                        &safe(setting.describe(shown, show)),
                        (inner.width as usize).saturating_sub(used),
                    ),
                    style,
                ),
            ])),
            Rect::new(inner.x, y, inner.width, 1),
        );
    }
    let last = Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1);
    let field = draft.field.editor();
    if app
        .edit_view
        .as_ref()
        .is_some_and(|v| Some(v.field) == field && v.mode.starts_with('c'))
    {
        command_line(frame, app, field.unwrap(), last);
    } else if !app.status.is_empty() && app.status != app.status_success {
        frame.render_widget(Paragraph::new(styled(safe(&app.status), theme.error)), last);
    } else if draft
        .workspace(&app.cwd, app.home.as_deref())
        .is_ok_and(|b| b.path.as_deref().is_some_and(|p| !available(app, p)))
    {
        frame.render_widget(
            Paragraph::new(styled(
                "This directory does not exist; the workspace will show as unavailable.",
                theme.muted,
            )),
            last,
        );
    }
}
pub(super) fn workspace_icons(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    query: &str,
    index: usize,
) -> Vec<(Rect, usize)> {
    let inner = workspace_dialog(
        frame,
        area,
        app,
        mode_title(
            app,
            Field::WorkspaceIcons,
            &format!(
                "Workspace icons · {} chooses · {} returns",
                app.key_or_none("enter"),
                app.key_or_none("esc")
            ),
        ),
        22,
    );
    let theme = app.palette;
    let (mut line, cursor) = editable_line(
        app,
        Field::WorkspaceIcons,
        query,
        0,
        inner.width.saturating_sub(6) as usize,
    );
    line.spans.insert(0, styled("Find: ", theme.marked));
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.width > 6 {
        frame.set_cursor_position((inner.x + 6 + cursor, inner.y));
    }
    command_line(
        frame,
        app,
        Field::WorkspaceIcons,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = crate::icons::matching_workspace_icons(query);
    let mut choices = Vec::new();
    if matches.is_empty() {
        frame.render_widget(
            Paragraph::new(styled("No matching icons", theme.muted)),
            list,
        );
    } else {
        let items: Vec<_> = matches
            .iter()
            .map(|&i| {
                let (name, glyph) = crate::icons::WORKSPACE_ICONS[i];
                ListItem::new(Line::from(vec![
                    bold(format!("{}  ", glyph.unwrap_or('·')), theme.accent),
                    styled(name, theme.text),
                ]))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_symbol("› ")
                .highlight_style(Style::default().bg(theme.selection)),
            list,
            &mut state,
        );
        let shown = state.offset();
        choices = item_areas(list, shown, std::iter::repeat_n(1, matches.len() - shown));
    }
    frame.render_widget(
        Paragraph::new(styled(
            "Nerd Font glyphs · Automatic clears the custom icon",
            theme.muted,
        )),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
    choices
}
/// Workspace deletion uses the operation sheet in the deletion accent, listing
/// the one entry like a chooser row.
pub(super) fn workspace_delete(frame: &mut Frame, main: Rect, app: &App, index: usize) {
    let theme = app.palette;
    let Some(workspace) = app.workspaces.get(index) else {
        return;
    };
    let body = prose(
        theme,
        theme.error,
        &format!(
            "Press *{}* to remove this workspace, or *{}* to keep it. \
             The directory itself is not touched.",
            app.key_or_none("enter"),
            app.key_or_none("esc")
        ),
        main.width.saturating_sub(4) as usize,
    );
    let saves = match &app.workspace_store.path {
        Some(path) => format!("saves to {}", tilde(path, app.home.as_deref())),
        None => "this session only".into(),
    };
    // Borders, prose, a blank line, the workspace, and a row for errors.
    let inner = sheet(
        frame,
        main,
        app,
        body.len() as u16 + 5,
        Sheet {
            title: " Delete workspace ".into(),
            accent: theme.error,
            keys: &[("enter", "Delete"), ("esc", "Cancel")],
            summary: Some(saves),
        },
    );
    let available = workspace.path.as_deref().is_none_or(|p| available(app, p));
    let icon = match &workspace.icon {
        Some(icon) => safe(icon),
        None if app.icons => folder_glyph(),
        None => String::new(),
    };
    let mut row = vec![];
    if !icon.is_empty() {
        let pad = 3usize.saturating_sub(icon.width());
        row.push(styled(
            icon + &" ".repeat(pad),
            if available { theme.marked } else { theme.error },
        ));
    }
    let name = safe(&workspace.name);
    row.push(bold(
        name.clone(),
        if available { theme.marked } else { theme.error },
    ));
    let used = row.iter().map(|s| s.width()).sum::<usize>() + 3;
    row.push(styled(
        format!(
            "   {}",
            elide(
                &summary(app, workspace),
                (inner.width as usize).saturating_sub(used),
            )
        ),
        theme.muted,
    ));
    let mut lines = body;
    lines.push(Line::default());
    lines.push(Line::from(row));
    frame.render_widget(Paragraph::new(lines), inner);
    if !app.status.is_empty() && app.status != app.status_success {
        frame.render_widget(
            Paragraph::new(styled(safe(&app.status), theme.error)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
}
/// Alt+p chooser. The whole frame already uses the highlighted (previewed) palette.
pub(super) fn palettes(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    query: &str,
    index: usize,
) -> Vec<(Rect, usize)> {
    let theme = app.palette;
    let width = area.width.saturating_sub(4).min(64);
    let height = area.height.saturating_sub(4).min(22);
    let dialog = centered(area, width, height);
    let block = panel(
        theme,
        mode_title(
            app,
            Field::Palettes,
            &format!(
                "Palettes · {} applies · {} reverts",
                app.key_or_none("enter"),
                app.key_or_none("esc")
            ),
        ),
        true,
        theme.accent,
    )
    .padding(Padding::horizontal(1));
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    let (mut line, cursor) = editable_line(
        app,
        Field::Palettes,
        query,
        0,
        inner.width.saturating_sub(6) as usize,
    );
    line.spans.insert(0, styled("Find: ", theme.accent));
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.width > 6 {
        frame.set_cursor_position((inner.x + 6 + cursor, inner.y));
    }
    command_line(
        frame,
        app,
        Field::Palettes,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = app.matching_palettes(query);
    let mut choices = Vec::new();
    if matches.is_empty() {
        frame.render_widget(
            Paragraph::new(styled("No matching palettes", theme.muted)),
            list,
        );
    } else {
        let builtin = Palette::names().len();
        let items: Vec<_> = matches
            .iter()
            .map(|&i| {
                let (name, p) = &app.palettes[i];
                let mut spans = vec![bold(format!("{:<22}", safe(name)), theme.text)];
                // A compact swatch: background, panel, text, accent, marked, success, error.
                for color in [
                    p.background,
                    p.panel,
                    p.text,
                    p.accent,
                    p.marked,
                    p.success,
                    p.error,
                ] {
                    spans.push(Span::styled("██", Style::default().fg(color)));
                }
                spans.push(styled(
                    if i < builtin {
                        "  built-in"
                    } else {
                        "  custom"
                    },
                    theme.muted,
                ));
                if *name == app.palette_name {
                    spans.push(styled(" · current", theme.success));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let mut state = ListState::default().with_selected(Some(index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(theme.selection))
                .highlight_symbol("› "),
            list,
            &mut state,
        );
        let shown = state.offset();
        choices = item_areas(list, shown, std::iter::repeat_n(1, matches.len() - shown));
    }
    if !app.status.is_empty() {
        frame.render_widget(
            Paragraph::new(styled(safe(&app.status), theme.error)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
    choices
}
/// A workspace's folder exists, from the details cache: drawing never waits
/// for the filesystem, and one not read yet counts as available.
/// The glyph of a workspace without an icon of its own.
fn folder_glyph() -> String {
    crate::icons::icon(Path::new(""), true).glyph.to_string()
}
/// What a workspace keeps, in a few words: its folder, then its other
/// settings (`pick · 3 roots · depth 10`).
fn summary(app: &App, w: &crate::config::Workspace) -> String {
    use crate::config::Setting;
    let count = |n: usize, one: &str, many: &str| match n {
        0 => format!("no {many}"),
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    };
    let show = |p: &Path| tilde(p, app.home.as_deref());
    w.path
        .as_deref()
        .map(show)
        .into_iter()
        .chain(
            Setting::ALL
                .into_iter()
                .filter(|s| s.is_in(w))
                .map(|s| match s {
                    Setting::Roots => count(w.roots.as_ref().map_or(0, Vec::len), "root", "roots"),
                    Setting::Depth => format!("depth {}", s.describe(w, show)),
                    Setting::Filters => {
                        count(w.filters.as_ref().map_or(0, Vec::len), "filter", "filters")
                    }
                    Setting::Exclusions => count(
                        w.exclusions.as_ref().map_or(0, Vec::len),
                        "exclusion",
                        "exclusions",
                    ),
                    Setting::Sorting => match w.sort {
                        Some(Some(sort)) => format!("sorted by {}", sort.label()),
                        _ => "ranked".into(),
                    },
                    _ => s.describe(w, show),
                }),
        )
        .collect::<Vec<_>>()
        .join(" · ")
}
fn available(app: &App, path: &Path) -> bool {
    match app.details.lookup(path) {
        Some(found) => found.is_some_and(|i| i.directory),
        None => true,
    }
}
/// A picker (`ui::Picker`): its choices matching the query, best match
/// first. `:`'s functions show the key each has where it was pressed in
/// Normal mode; the sorting picker's orders, which is the current one, and
/// its title the direction Tab switches.
pub(super) fn picker(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    query: &str,
    index: usize,
    kind: Picker,
) -> (usize, Vec<(Rect, usize)>) {
    use crate::keymap::Mode;
    let theme = app.palette;
    let every = kind.choices("").len() as u16;
    let width = area.width.saturating_sub(4).min(110);
    let height = area.height.saturating_sub(4).min(24).min(every + 4);
    let dialog = centered(area, width, height);
    let (title, keys, prompt, placeholder, name_width): (_, &[_], _, _, _) = match kind {
        Picker::Commands(_) => (
            "Commands",
            &[("enter", "Run"), ("esc", "Cancel")],
            ":",
            "type a function's name or words of what it does…",
            28,
        ),
        Picker::Sort { descending } => (
            if descending {
                "Sort descending"
            } else {
                "Sort ascending"
            },
            &[
                ("tab", "Toggle ordering"),
                ("enter", "Sort"),
                ("esc", "Cancel"),
            ],
            if descending { "↓" } else { "↑" },
            "type a criterion's name or words of what it does…",
            12,
        ),
    };
    let hints = border_hints(app, keys, theme.accent, width.saturating_sub(4) as usize);
    let block = panel(
        theme,
        mode_title(app, Field::Picker, title),
        true,
        theme.accent,
    )
    .padding(Padding::horizontal(1))
    .title_bottom(Line::from(hints).centered());
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    let (mut line, cursor) = editable_line(
        app,
        Field::Picker,
        query,
        0,
        inner.width.saturating_sub(2) as usize,
    );
    if query.is_empty() {
        line = Line::from(styled(placeholder, theme.muted));
    }
    line.spans.insert(0, bold(prompt, theme.accent));
    line.spans.insert(1, Span::raw(" "));
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if inner.width > 2 {
        frame.set_cursor_position((inner.x + 2 + cursor, inner.y));
    }
    command_line(
        frame,
        app,
        Field::Picker,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(2),
    );
    let found = kind.choices(query);
    if found.is_empty() {
        let none = match kind {
            Picker::Commands(_) => "Nothing that works here matches.",
            Picker::Sort { .. } => "No order matches.",
        };
        frame.render_widget(Paragraph::new(styled(none, theme.muted)), list);
        return (list.height as usize, Vec::new());
    }
    let room = (list.width as usize).saturating_sub(2);
    let items: Vec<ListItem> = found
        .iter()
        .map(|&choice| {
            let aside = match (choice, kind) {
                (Choice::Function(f), Picker::Commands(here)) => app
                    .keymap
                    .of(f, Mode::Normal)
                    .into_iter()
                    .find(|b| b.applies(here))
                    .map(|b| b.display())
                    .unwrap_or_default(),
                (Choice::Sort(by), _) if app.sort.map(|s| s.by) == by => match app.sort {
                    Some(sort) => format!("current {}", sort.arrow()),
                    None => "current".into(),
                },
                _ => String::new(),
            };
            ListItem::new(Line::from(vec![
                bold(format!("{:<name_width$}", choice.name()), theme.function),
                bold(format!("{:<12}", truncate(&aside, 11)), theme.accent),
                styled(
                    truncate(choice.description(), room.saturating_sub(name_width + 12)),
                    theme.muted,
                ),
            ]))
        })
        .collect();
    let mut state = ListState::default().with_selected(Some(index.min(found.len() - 1)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().bg(theme.selection))
            .highlight_symbol("› "),
        list,
        &mut state,
    );
    let shown = state.offset();
    let choices = item_areas(
        list,
        shown,
        std::iter::repeat_n(
            1,
            found.len().saturating_sub(shown).min(list.height as usize),
        ),
    );
    (list.height as usize, choices)
}
