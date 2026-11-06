use super::*;

/// The workspace chooser, drawn as the pickers are: the filter under the
/// title, a row per workspace (its number for Alt+digit, icon, name and what
/// it keeps), the count on the top border and the keys on the bottom one.
pub(super) fn workspaces(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    query: &str,
    index: usize,
) -> (usize, Vec<(Rect, usize)>) {
    let theme = app.palette;
    let matches = app.matching_workspaces(query);
    // Plain letters work while the filter is in Normal mode; otherwise Alt.
    let normal = app.edit_view.as_ref().is_some_and(|v| v.normal());
    let command = |c: char| {
        if normal {
            c.to_string()
        } else {
            format!("alt+{c}")
        }
    };
    let (new, edit, delete, reload) = (command('n'), command('e'), command('d'), command('r'));
    let keys = [
        ("enter", "Open"),
        (new.as_str(), "New"),
        (edit.as_str(), "Edit"),
        (delete.as_str(), "Delete"),
        (reload.as_str(), "Reload"),
        ("up down", "Reorder"),
        ("esc", "Close"),
    ];
    let width = area.width.saturating_sub(4).min(110);
    // Borders, the filter and its command line, the rows and a status row.
    let rows = matches.len().max(1) as u16;
    let height = area.height.saturating_sub(4).min(24).min(rows + 5);
    let dialog = centered(area, width, height);
    let hints = border_hints(app, &keys, theme.marked, width.saturating_sub(4) as usize);
    let count = counted(app.workspaces.len(), "workspace", "workspaces");
    let block = panel(
        theme,
        mode_title(app, Field::Workspaces, "Workspaces"),
        true,
        theme.marked,
    )
    .padding(Padding::horizontal(1))
    .title_top(Line::from(styled(format!(" {count} "), theme.muted)).right_aligned())
    .title_bottom(Line::from(hints).centered());
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    let (mut line, cursor) = editable_line(
        app,
        Field::Workspaces,
        query,
        0,
        inner.width.saturating_sub(2) as usize,
    );
    if query.is_empty() {
        line = Line::from(styled("type a workspace's name…", theme.muted));
    }
    line.spans.insert(0, bold("❯", theme.marked));
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
        Field::Workspaces,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    // Only fully visible entries get labels.
    let page = list.height as usize;
    let start = first_shown(index, page);
    let visible: Vec<_> = matches.iter().skip(start).take(page).copied().collect();
    let mut choices = Vec::new();
    if matches.is_empty() {
        let text = if app.workspaces.is_empty() {
            format!(
                "No workspaces yet. {} adds one: a folder, settings to apply, or both.",
                keys_named(app, &new).unwrap_or_default()
            )
        } else {
            "No workspace matches.".into()
        };
        frame.render_widget(
            Paragraph::new(styled(text, theme.muted)).wrap(Wrap { trim: false }),
            list,
        );
    } else {
        // Configured icons always show; others get the folder glyph when file
        // icons are enabled, or blank padding so names stay aligned.
        let icon_column = app.icons || app.workspaces.iter().any(|b| b.icon.is_some());
        let name_width = visible
            .iter()
            .map(|&i| safe(&app.workspaces[i].name).width())
            .max()
            .unwrap_or(0)
            .min(28)
            + 2;
        let rows: Vec<_> = visible
            .iter()
            .map(|&i| {
                let b = &app.workspaces[i];
                let available = b.path.as_deref().is_none_or(|p| available(app, p));
                let color = if available {
                    theme.function
                } else {
                    theme.error
                };
                let mut spans = Vec::new();
                if icon_column {
                    let icon = match &b.icon {
                        Some(icon) => safe(icon),
                        None if app.icons => folder_glyph(),
                        None => String::new(),
                    };
                    let pad = 3usize.saturating_sub(icon.width());
                    spans.push(styled(icon + &" ".repeat(pad), color));
                }
                let name = truncate(&safe(&b.name), name_width - 2);
                let pad = name_width.saturating_sub(name.width());
                spans.push(bold(name + &" ".repeat(pad), color));
                let used: usize = NUMBERED + spans.iter().map(|s| s.width()).sum::<usize>();
                let mut aside = safe(summary(app, b));
                if !available {
                    aside = format!("unavailable · {aside}");
                }
                spans.push(styled(
                    elide(&aside, (list.width as usize).saturating_sub(used)),
                    if available { theme.muted } else { theme.error },
                ));
                Line::from(spans)
            })
            .collect();
        choices = numbered_rows(frame, list, theme, start, rows, index);
    }
    if !app.status.is_empty() {
        frame.render_widget(
            Paragraph::new(styled(safe(&app.status), theme.error)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
    (visible.len(), choices)
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
/// The workspace form, framed as the chooser it opens from: what a workspace
/// is, a row per field and setting (the cursor's marked as the pickers mark
/// it), the path's explanation under it and a status row. The keys, Space's
/// among them where it does something, are on the bottom border.
pub(super) fn workspace_form(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    draft: &crate::workspaces::Draft,
) {
    use crate::config::Setting;
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
    let width = area.width.saturating_sub(4).min(110);
    // Inside the borders and padding.
    let room = width.saturating_sub(4) as usize;
    // The pointer, then a field's label, then its value.
    let indent: u16 = 9;
    let body = prose(
        theme,
        theme.marked,
        "A workspace opens a folder, applies the settings ticked below, or both.",
        room,
    );
    // The path's explanation, under it in the value's column.
    let here = tilde(&app.cwd, app.home.as_deref());
    let beside = room.saturating_sub(indent as usize);
    // Prose whose plain words take `color`; its *emphasis* stays.
    let tinted = |text: &str, color: Color| {
        prose(theme, theme.text, text, beside)
            .into_iter()
            .map(move |mut line| {
                for span in &mut line.spans {
                    if !span.style.add_modifier.contains(Modifier::BOLD) {
                        span.style = span.style.fg(color);
                    }
                }
                line
            })
    };
    let mut hint = Vec::new();
    if draft
        .workspace(&app.cwd, app.home.as_deref())
        .is_ok_and(|b| b.path.as_deref().is_some_and(|p| !available(app, p)))
    {
        hint.extend(tinted(
            "This directory does not exist; the workspace will show as unavailable.",
            theme.error,
        ));
    }
    hint.extend(tinted(
        &format!(
            "*Optional:* left empty, the workspace stays in whatever folder you \
             are in and applies only the ticked settings. Relative paths start \
             at {here}; *~* is your home directory."
        ),
        theme.muted,
    ));
    let saves = match &app.workspace_store.path {
        Some(path) => format!("saves to {}", tilde(path, app.home.as_deref())),
        None => "this session only".into(),
    };
    // Space chooses the icon and ticks a setting there is something to keep of.
    let space = match draft.field {
        EditField::Icon => Some("Choose icon"),
        EditField::Setting(setting) if setting.is_in(&draft.kept) => Some("Don't keep"),
        EditField::Setting(setting) if draft.offers(setting) => Some("Keep"),
        _ => None,
    };
    let mut keys = vec![("tab", "Next field"), ("enter", "Save")];
    keys.extend(space.map(|label| ("space", label)));
    keys.push(("esc", "Cancel"));
    // Borders, the explanation and a blank line, the name, the path and its
    // explanation, the icon, a blank line, the settings under their heading,
    // a blank line and a row for commands or errors.
    let settings = Setting::ALL.len() as u16;
    let height = body.len() as u16 + hint.len() as u16 + settings + 9;
    let dialog = centered(area, width, height.min(area.height.saturating_sub(2)));
    let hints = border_hints(app, &keys, theme.marked, room);
    let block = panel(theme, title, true, theme.marked)
        .padding(Padding::horizontal(1))
        .title_top(Line::from(styled(format!(" {saves} "), theme.muted)).right_aligned())
        .title_bottom(Line::from(hints).centered());
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    let top = (body.len() as u16 + 1).min(inner.height);
    frame.render_widget(
        Paragraph::new(body),
        Rect::new(inner.x, inner.y, inner.width, top),
    );
    let fits = |y: u16| y < inner.bottom().saturating_sub(1);
    // A row, highlighted with the pointer when the cursor is on it: what is
    // right of the pointer.
    let row = |frame: &mut Frame, y: u16, active: bool| {
        fits(y).then(|| {
            let line = Rect::new(inner.x, y, inner.width, 1);
            if active {
                frame.render_widget(
                    Paragraph::new(bold("›", theme.marked))
                        .style(Style::default().bg(theme.selection)),
                    line,
                );
            }
            Rect::new(inner.x + 2, y, inner.width.saturating_sub(2), 1)
        })
    };
    let label = |text: &str, active: bool| {
        let style = Style::default().add_modifier(Modifier::BOLD);
        Span::styled(
            format!("{text:<width$}", width = indent as usize - 2),
            style.fg(if active { theme.text } else { theme.muted }),
        )
    };
    // The label's column, then what is right of it.
    let value = |area: Rect| {
        let skip = (indent - 2).min(area.width);
        Rect::new(area.x + skip, area.y, area.width - skip, 1)
    };
    let mut y = inner.y + top;
    for (name, text, field, active) in [
        (
            "Name",
            &draft.name,
            Field::WorkspaceName,
            draft.field == EditField::Name,
        ),
        (
            "Path",
            &draft.path,
            Field::WorkspacePath,
            draft.field == EditField::Path,
        ),
    ] {
        if let Some(area) = row(frame, y, active) {
            frame.render_widget(Paragraph::new(label(name, active)), area);
            if active {
                edit_rows(frame, app, field, text, value(area));
            } else {
                let text = elide(&safe(text), value(area).width as usize);
                frame.render_widget(Paragraph::new(styled(text, theme.text)), value(area));
            }
        }
        y += 1;
    }
    for line in hint {
        if fits(y) {
            frame.render_widget(
                Paragraph::new(line),
                Rect::new(inner.x + indent, y, inner.width.saturating_sub(indent), 1),
            );
        }
        y += 1;
    }
    let active = draft.field == EditField::Icon;
    if let Some(area) = row(frame, y, active) {
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
        let mut spans = vec![label("Icon", active)];
        if !glyph.is_empty() {
            spans.push(bold(format!("{glyph}  "), theme.accent));
        }
        spans.push(styled(name, theme.text));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
    y += 2;
    // The settings, ticked as the Keys screen's checklists are, under a
    // heading in the markers' column.
    if fits(y) {
        frame.render_widget(
            Paragraph::new(bold("KEEPS", theme.muted)),
            Rect::new(inner.x + 2, y, inner.width.saturating_sub(2), 1),
        );
    }
    let show = |p: &Path| tilde(p, app.home.as_deref());
    for (i, setting) in Setting::ALL.into_iter().enumerate() {
        let here = draft.field == EditField::Setting(setting);
        let Some(area) = row(frame, y + 1 + i as u16, here) else {
            break;
        };
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
            style = style.add_modifier(Modifier::BOLD);
        }
        let name = format!(" {:<11}", setting.name());
        let used = 1 + name.width();
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(marker, style.fg(color)),
                Span::styled(name, style),
                Span::styled(
                    elide(
                        &safe(setting.describe(shown, show)),
                        (area.width as usize).saturating_sub(used),
                    ),
                    style,
                ),
            ])),
            area,
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
        let start = first_shown(index, list.height as usize);
        let rows = matches
            .iter()
            .skip(start)
            .map(|&i| {
                let (name, glyph) = crate::icons::WORKSPACE_ICONS[i];
                Line::from(vec![
                    bold(format!("{}  ", glyph.unwrap_or('·')), theme.accent),
                    styled(name, theme.text),
                ])
            })
            .collect();
        choices = numbered_rows(frame, list, theme, start, rows, index);
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
    let width = area.width.saturating_sub(4).min(70);
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
        let start = first_shown(index, list.height as usize);
        let rows = matches
            .iter()
            .skip(start)
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
                Line::from(spans)
            })
            .collect();
        choices = numbered_rows(frame, list, theme, start, rows, index);
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
/// `1 one`, or `n many`.
fn counted(n: usize, one: &str, many: &str) -> String {
    match n {
        1 => format!("1 {one}"),
        n => format!("{n} {many}"),
    }
}
fn folder_glyph() -> String {
    crate::icons::icon(Path::new(""), true).glyph.to_string()
}
/// What a workspace keeps, in a few words: its folder, then its other
/// settings (`pick · 3 roots · depth 10`).
fn summary(app: &App, w: &crate::config::Workspace) -> String {
    use crate::config::Setting;
    let count = |n: usize, one: &str, many: &str| match n {
        0 => format!("no {many}"),
        n => counted(n, one, many),
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
    // How many there are, as the workspace chooser counts its own.
    let count = match kind {
        Picker::Commands(_) => counted(every.into(), "command", "commands"),
        Picker::Sort { .. } => counted(every.into(), "order", "orders"),
    };
    let block = panel(
        theme,
        mode_title(app, Field::Picker, title),
        true,
        theme.accent,
    )
    .padding(Padding::horizontal(1))
    .title_top(Line::from(styled(format!(" {count} "), theme.muted)).right_aligned())
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
    let room = (list.width as usize).saturating_sub(NUMBERED);
    let index = index.min(found.len() - 1);
    let start = first_shown(index, list.height as usize);
    let rows = found
        .iter()
        .skip(start)
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
            Line::from(vec![
                bold(format!("{:<name_width$}", choice.name()), theme.function),
                bold(format!("{:<12}", truncate(&aside, 11)), theme.accent),
                styled(
                    truncate(choice.description(), room.saturating_sub(name_width + 12)),
                    theme.muted,
                ),
            ])
        })
        .collect();
    let choices = numbered_rows(frame, list, theme, start, rows, index);
    (list.height as usize, choices)
}
