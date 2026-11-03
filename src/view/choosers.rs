use super::*;

pub(super) fn bookmarks(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    query: &str,
    index: usize,
) -> (Vec<usize>, Vec<(Rect, usize)>) {
    let theme = app.palette;
    let width = area.width.saturating_sub(4).min(90);
    let height = area.height.saturating_sub(4).min(22);
    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = panel(
        theme,
        mode_title(
            app,
            Field::Bookmarks,
            &format!(
                "Bookmarks · {} opens · {} returns",
                [
                    app.key("enter"),
                    app.digits_pick("alt").then(|| "Alt+digit".to_string())
                ]
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
        Field::Bookmarks,
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
        Field::Bookmarks,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = app.matching_bookmarks(query);
    // Plain letters work while the filter is in Normal mode; otherwise Alt.
    let normal = app.edit_view.as_ref().is_some_and(|v| v.normal());
    let command = |c: char| {
        if normal {
            Some(c.to_string())
        } else {
            app.key(&format!("alt+{c}"))
        }
    };
    // Each bookmark occupies three lines; only fully visible entries get labels.
    let page = list.height as usize / 3;
    let start = index.saturating_sub(page.saturating_sub(1));
    let visible: Vec<_> = matches.iter().skip(start).take(page).copied().collect();
    let mut choices = Vec::new();
    if matches.is_empty() {
        let text = if app.bookmarks.is_empty() {
            match command('n') {
                Some(key) => format!("No bookmarks yet. {key} adds the current directory."),
                None => "No bookmarks yet.".into(),
            }
        } else {
            "No matching bookmarks".into()
        };
        frame.render_widget(
            Paragraph::new(styled(text, theme.muted)).wrap(Wrap { trim: false }),
            list,
        );
    } else {
        // Configured icons always show; others get the folder glyph when file
        // icons are enabled, or blank padding so names stay aligned.
        let icon_column = app.icons || app.bookmarks.iter().any(|b| b.icon.is_some());
        let items: Vec<_> = visible
            .iter()
            .enumerate()
            .map(|(position, &i)| {
                let b = &app.bookmarks[i];
                let available = available(app, &b.path);
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
                        None if app.icons => crate::icons::icon(&b.path, true).glyph.to_string(),
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
                                &display_path(&b.path, &app.cwd),
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
/// Modal frame for the bookmark icon chooser.
pub(super) fn bookmark_dialog(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    title: String,
    height: u16,
) -> Rect {
    let width = area.width.saturating_sub(2).min(90);
    let height = area.height.saturating_sub(2).min(height);
    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = panel(app.palette, title, true, app.palette.marked).padding(Padding::horizontal(1));
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    inner
}
/// The bookmark form shares the operation sheet: an explanation, three
/// labelled rows (the active one carries the prompt glyph) and a status row.
pub(super) fn bookmark_form(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    draft: &crate::bookmarks::Draft,
) {
    use crate::bookmarks::EditField;
    let theme = app.palette;
    let title = if draft.editing.is_some() {
        "Edit bookmark"
    } else {
        "New bookmark"
    };
    let title = draft
        .field
        .editor()
        .map_or_else(|| format!(" {title} "), |f| mode_title(app, f, title));
    let here = tilde(&app.cwd, app.home.as_deref());
    let body = prose(
        theme,
        theme.marked,
        &format!("Relative paths start at {here}; *~* is your home directory."),
        main.width.saturating_sub(4) as usize,
    );
    let summary = match &app.bookmark_store.path {
        Some(path) => format!("saves to {}", tilde(path, app.home.as_deref())),
        None => "this session only".into(),
    };
    // Borders, prose, a blank line, three rows, and a row for commands or errors.
    let inner = sheet(
        frame,
        main,
        app,
        body.len() as u16 + 7,
        Sheet {
            title,
            accent: theme.marked,
            keys: &[("enter", "Save"), ("esc", "Cancel"), ("tab", "Next field")],
            summary: Some(summary),
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
            Field::BookmarkName,
            draft.field == EditField::Name,
        ),
        (
            1,
            "Path",
            &draft.path,
            Field::BookmarkPath,
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
        .bookmark(&app.cwd, app.home.as_deref())
        .is_ok_and(|b| !available(app, &b.path))
    {
        frame.render_widget(
            Paragraph::new(styled(
                "This directory does not exist; the bookmark will show as unavailable.",
                theme.muted,
            )),
            last,
        );
    }
}
pub(super) fn bookmark_icons(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    query: &str,
    index: usize,
) -> Vec<(Rect, usize)> {
    let inner = bookmark_dialog(
        frame,
        area,
        app,
        mode_title(
            app,
            Field::BookmarkIcons,
            &format!(
                "Bookmark icons · {} chooses · {} returns",
                app.key_or_none("enter"),
                app.key_or_none("esc")
            ),
        ),
        22,
    );
    let theme = app.palette;
    let (mut line, cursor) = editable_line(
        app,
        Field::BookmarkIcons,
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
        Field::BookmarkIcons,
        Rect::new(inner.x, inner.y + 1, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = crate::icons::matching_bookmark_icons(query);
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
                let (name, glyph) = crate::icons::BOOKMARK_ICONS[i];
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
/// Bookmark deletion uses the operation sheet in the deletion accent, listing
/// the one entry like a chooser row.
pub(super) fn bookmark_delete(frame: &mut Frame, main: Rect, app: &App, index: usize) {
    let theme = app.palette;
    let Some(bookmark) = app.bookmarks.get(index) else {
        return;
    };
    let body = prose(
        theme,
        theme.error,
        &format!(
            "Press *{}* to remove this bookmark, or *{}* to keep it. \
             The directory itself is not touched.",
            app.key_or_none("enter"),
            app.key_or_none("esc")
        ),
        main.width.saturating_sub(4) as usize,
    );
    let summary = match &app.bookmark_store.path {
        Some(path) => format!("saves to {}", tilde(path, app.home.as_deref())),
        None => "this session only".into(),
    };
    // Borders, prose, a blank line, the bookmark, and a row for errors.
    let inner = sheet(
        frame,
        main,
        app,
        body.len() as u16 + 5,
        Sheet {
            title: " Delete bookmark ".into(),
            accent: theme.error,
            keys: &[("enter", "Delete"), ("esc", "Cancel")],
            summary: Some(summary),
        },
    );
    let available = available(app, &bookmark.path);
    let icon = match &bookmark.icon {
        Some(icon) => safe(icon),
        None if app.icons => crate::icons::icon(&bookmark.path, true).glyph.to_string(),
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
    let name = safe(&bookmark.name);
    row.push(bold(
        name.clone(),
        if available { theme.marked } else { theme.error },
    ));
    let used = row.iter().map(|s| s.width()).sum::<usize>() + 3;
    row.push(styled(
        format!(
            "   {}",
            elide(
                &tilde(&bookmark.path, app.home.as_deref()),
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
    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
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
/// A bookmark's folder exists, from the details cache: drawing never waits
/// for the filesystem, and one not read yet counts as available.
fn available(app: &App, path: &Path) -> bool {
    match app.details.lookup(path) {
        Some(found) => found.is_some_and(|i| i.directory),
        None => true,
    }
}
