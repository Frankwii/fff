//! Terminal presentation. Rendering returns the exact numbered targets in a frame.
use crate::{App, Candidate, config::Palette, editor::Field, ui::Focus};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, BorderType, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
    },
};
use std::path::{Component, Path, PathBuf};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub(crate) struct Rendered {
    pub targets: Vec<PathBuf>,
    pub page: usize,
}

fn styled(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(color))
}
fn bold(text: impl Into<String>, color: Color) -> Span<'static> {
    styled(text, color).add_modifier(Modifier::BOLD)
}
pub(crate) fn safe(text: impl AsRef<str>) -> String {
    text.as_ref()
        .chars()
        .flat_map(|c| {
            if c.is_control() {
                c.escape_default().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}
/// Presentation only: never use an abbreviated label for navigation or output.
fn display_path(path: &Path, base: &Path) -> String {
    let relative = crate::relative_to(path, base);
    let parents = relative
        .components()
        .take_while(|part| *part == Component::ParentDir)
        .count();
    if parents >= 2 {
        let rest: PathBuf = relative.components().skip(parents).collect();
        format!(".../{}", safe(rest.display().to_string()))
    } else {
        safe(relative.display().to_string())
    }
}

/// Keep the basename visible in a long path, measuring terminal columns.
fn elide(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.into();
    }
    if width == 0 {
        return String::new();
    }
    let mut suffix = String::new();
    let mut used = 1;
    for c in text.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        suffix.insert(0, c);
        used += w;
    }
    format!("…{suffix}")
}
fn panel(theme: Palette, title: impl Into<String>, focused: bool, accent: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(
            Style::default()
                .fg(if focused { accent } else { theme.border })
                .add_modifier(if focused {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
        )
        .style(Style::default().bg(theme.panel).fg(theme.text))
        .title(Line::from(bold(
            title,
            if focused { accent } else { theme.text },
        )))
}

pub(crate) fn render(frame: &mut Frame, app: &mut App, focus: &mut Focus) -> Rendered {
    let theme = app.palette;
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(theme.background).fg(theme.text)),
        area,
    );
    // No invisible result rows may acquire numbered targets in a tiny terminal.
    if area.width < 36 || area.height < 12 {
        let text = Text::from(vec![
            Line::from(bold(" picker", theme.accent)),
            Line::from(styled(" Resize terminal to at least 36 × 12", theme.muted)),
            Line::from(styled(
                if app.scanning {
                    " scanning…"
                } else {
                    " complete"
                },
                theme.muted,
            )),
        ]);
        frame.render_widget(Paragraph::new(text), area);
        return Rendered {
            targets: Vec::new(),
            page: 0,
        };
    }
    let area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(area);
    header(frame, layout[0], app);
    query(frame, layout[1], app, matches!(focus, Focus::Results));
    let main = layout[2];
    let (results, marks) = if area.width >= 78 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
            .spacing(1)
            .split(main);
        (columns[0], columns[1])
    } else {
        // Compact terminals stack panes; focusing marks gives that pane more room.
        let marks_height = if matches!(focus, Focus::Marks { .. }) {
            main.height.saturating_sub(3)
        } else {
            4.min(main.height.saturating_sub(3))
        };
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(marks_height)])
            .split(main);
        (rows[0], rows[1])
    };
    let result = results_pane(frame, results, app, matches!(focus, Focus::Results));
    marks_pane(frame, marks, app, focus);
    footer(frame, layout[3], app, focus);
    match focus {
        Focus::Info { offset } => details(frame, area, app, offset),
        Focus::Exclusions(text) => prompt(
            frame,
            app,
            area,
            Field::Exclusions,
            "Edit exclusions",
            text,
            true,
        ),
        Focus::Bookmarks { query, index } => bookmarks(frame, area, app, query, *index),
        Focus::Palettes { query, index, .. } => palettes(frame, area, app, query, *index),
        Focus::Location(text) => prompt(
            frame,
            app,
            area,
            Field::Location,
            "Go to directory",
            text,
            false,
        ),
        _ => {}
    }
    result
}

fn header(frame: &mut Frame, area: Rect, app: &App) {
    let theme = app.palette;
    let path = safe(app.cwd.display().to_string());
    let label = " CURRENT DIRECTORY  ";
    let shown = elide(
        &path,
        area.width.saturating_sub(label.len() as u16 + 1) as usize,
    );
    let basename = app
        .cwd
        .file_name()
        .map(|s| safe(s.to_string_lossy()))
        .unwrap_or_else(|| "/".into());
    let (parent, last) = if shown.ends_with(&basename) {
        shown.split_at(shown.len() - basename.len())
    } else {
        ("", shown.as_str())
    };
    let row = Line::from(vec![
        bold(label, theme.current_directory_text),
        styled(parent, theme.current_directory_text),
        bold(last, theme.current_directory_text).add_modifier(Modifier::UNDERLINED),
    ]);
    frame.render_widget(
        Paragraph::new(row).style(Style::default().bg(theme.current_directory)),
        Rect::new(area.x, area.y, area.width, 1),
    );
    let status = if app.cache_pending {
        styled("◌ cached · refreshing (incomplete)", theme.marked)
    } else if app.scanning {
        styled("◌ scanning (incomplete)", theme.marked)
    } else {
        styled("● complete", theme.success)
    };
    let glob = app
        .policy
        .glob
        .as_ref()
        .map(|g| g.glob().glob())
        .unwrap_or("all files");
    let roots = if app.roots.is_empty() {
        "browsing".into()
    } else {
        format!("{} fixed roots", app.roots.len())
    };
    let policy = Line::from(vec![
        styled(
            format!(" depth {}  ·  {} results  ·  ", app.depth, app.rows.len()),
            theme.muted,
        ),
        status,
        styled(
            format!(
                "  ·  {roots}  ·  hidden: {}  ·  exclusions: {}  ·  {}",
                app.policy.hidden,
                app.policy.ignore,
                safe(glob)
            ),
            theme.muted,
        ),
    ]);
    frame.render_widget(
        Paragraph::new(policy),
        Rect::new(area.x, area.y + 1, area.width, 1),
    );
}
fn mode_title(app: &App, field: Field, title: &str) -> String {
    app.edit_view
        .as_ref()
        .filter(|v| v.field == field)
        .map(|v| format!(" {title} · {} ", v.label()))
        .unwrap_or_else(|| format!(" {title} "))
}
fn editable_line(
    app: &App,
    field: Field,
    text: &str,
    row: usize,
    width: usize,
) -> (Line<'static>, u16) {
    let theme = app.palette;
    let view = app.edit_view.as_ref().filter(|v| v.field == field);
    let (text, column) = if let Some(v) = view.filter(|v| v.mode.starts_with('c')) {
        (v.command.as_str(), v.command_column)
    } else {
        (
            text,
            view.filter(|v| v.row == row)
                .map(|v| v.column)
                .unwrap_or(text.len()),
        )
    };
    let mut column = column.min(text.len());
    while !text.is_char_boundary(column) {
        column -= 1;
    }
    let cursor = safe(&text[..column]).width();
    let start = cursor.saturating_add(1).saturating_sub(width);
    let mut spans = Vec::new();
    let mut used = 0;
    let mut x = 0;
    if start > 0 && width > 0 {
        spans.push(styled("…", theme.muted));
        used = 1;
    }
    for (byte, c) in text.char_indices() {
        let shown = safe(c.to_string());
        let w = shown.width();
        if x < start {
            x += w;
            continue;
        }
        if used + w > width {
            break;
        }
        let selected = view
            .and_then(|v| {
                v.anchor.map(|anchor| {
                    let current = (v.row, v.column);
                    let (begin, end) = if anchor <= current {
                        (anchor, current)
                    } else {
                        (current, anchor)
                    };
                    if v.mode == "V" {
                        row >= begin.0 && row <= end.0
                    } else if v.mode == "\u{16}" {
                        row >= begin.0
                            && row <= end.0
                            && byte >= anchor.1.min(current.1)
                            && byte <= anchor.1.max(current.1)
                    } else {
                        (row, byte) >= begin && (row, byte) <= end
                    }
                })
            })
            .unwrap_or(false);
        let style = Style::default().fg(theme.text).bg(if selected {
            theme.selection
        } else {
            theme.panel
        });
        spans.push(Span::styled(shown, style));
        used += w;
        x += w;
    }
    (
        Line::from(spans),
        (cursor.saturating_sub(start) + usize::from(start > 0)).min(width.saturating_sub(1)) as u16,
    )
}
fn query(frame: &mut Frame, area: Rect, app: &App, focused: bool) {
    let theme = app.palette;
    let block = panel(
        app.palette,
        mode_title(app, Field::Query, "Filter"),
        focused,
        theme.accent,
    )
    .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let prefix = "Query: ";
    let available = inner.width.saturating_sub(prefix.len() as u16) as usize;
    let (mut line, cursor) = editable_line(app, Field::Query, &app.query, 0, available);
    if app.query.is_empty()
        && !app
            .edit_view
            .as_ref()
            .is_some_and(|v| v.mode.starts_with('c'))
    {
        line = Line::from(styled("type to find a file or directory…", theme.muted));
    }
    line.spans.insert(0, styled(prefix, theme.accent));
    frame.render_widget(Paragraph::new(line), inner);
    if focused && inner.width > 0 {
        frame.set_cursor_position((inner.x + prefix.len() as u16 + cursor, inner.y));
    }
}

fn results_pane(frame: &mut Frame, area: Rect, app: &mut App, focused: bool) -> Rendered {
    let theme = app.palette;
    let block = panel(
        app.palette,
        format!(" Files · {} ", app.rows.len()),
        focused,
        theme.accent,
    );
    let inner = block.inner(area);
    let page = inner.height as usize;
    if page > 0
        && let Some(i) = app
            .rows
            .iter()
            .position(|c| Some(&c.path) == app.highlight.as_ref())
    {
        if i < app.offset {
            app.offset = i;
        }
        if i >= app.offset + page {
            app.offset = i + 1 - page;
        }
    }
    app.offset = app.offset.min(app.rows.len().saturating_sub(1));
    let visible: Vec<_> = app.rows.iter().skip(app.offset).take(page).collect();
    let last = (app.offset + visible.len()).min(app.rows.len());
    let range = if visible.is_empty() {
        " 0 results ".into()
    } else {
        format!(" {}–{} of {} ", app.offset + 1, last, app.rows.len())
    };
    frame.render_widget(
        block.title_bottom(Line::from(styled(range, theme.muted)).right_aligned()),
        area,
    );
    if visible.is_empty() {
        let (title, hint) = if app.scanning {
            (
                "Discovering files…",
                "You can keep typing while the scan runs.",
            )
        } else if app.physical_empty {
            (
                "Empty directory",
                "Enter returns this directory. Alt+h goes up.",
            )
        } else {
            (
                "No matching results",
                "Change the query, or press Esc to clear it.",
            )
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(bold(title, theme.text)),
                Line::from(""),
                Line::from(styled(hint, theme.muted)),
            ])
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: false }),
            inner,
        );
    } else {
        for (i, c) in visible.iter().enumerate() {
            let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            let selected = Some(&c.path) == app.highlight.as_ref();
            let bg = if selected && focused {
                theme.selection
            } else {
                theme.panel
            };
            frame.render_widget(Block::default().style(Style::default().bg(bg)), row);
            let label = if i + 1 == visible.len() && i < 9 {
                format!("{}/0", i + 1)
            } else if i + 1 == visible.len() {
                "0".into()
            } else if i < 9 {
                (i + 1).to_string()
            } else {
                String::new()
            };
            let mut name = display_path(&c.path, &app.cwd);
            if c.directory && !name.ends_with('/') {
                name.push('/');
            }
            let prefix = format!("{} {:>3} ", if selected { "›" } else { " " }, label);
            let marked = app.marks.contains(&c.path);
            let meta = format!(" d{} r{} ", c.depth, c.root + 1);
            let available = inner
                .width
                .saturating_sub((prefix.width() + 4 + meta.width()) as u16)
                as usize;
            let name = elide(&name, available);
            let gap = available.saturating_sub(name.width());
            let mut spans = vec![
                styled(prefix, if selected { theme.accent } else { theme.muted }),
                styled(if marked { "● " } else { "  " }, theme.marked),
                styled(
                    if c.directory { "▸ " } else { "· " },
                    if c.directory {
                        theme.accent
                    } else {
                        theme.muted
                    },
                ),
            ];
            spans.extend(filename_spans(app.palette, &name, c, &app.query));
            spans.push(styled(" ".repeat(gap) + &meta, theme.muted));
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(bg)),
                row,
            );
        }
    }
    Rendered {
        targets: visible.iter().map(|c| c.path.clone()).collect(),
        page,
    }
}
fn filename_spans(
    theme: Palette,
    label: &str,
    candidate: &Candidate,
    query: &str,
) -> Vec<Span<'static>> {
    let basename = candidate
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let start = label
        .len()
        .saturating_sub(safe(&basename).len() + usize::from(candidate.directory));
    let mut wanted = query
        .to_lowercase()
        .chars()
        .collect::<Vec<_>>()
        .into_iter()
        .peekable();
    label
        .char_indices()
        .map(|(i, c)| {
            let matched = i >= start
                && wanted
                    .peek()
                    .is_some_and(|q| c.to_lowercase().any(|lower| lower == *q));
            if matched {
                wanted.next();
            }
            let span = styled(
                c.to_string(),
                if matched {
                    theme.marked
                } else if candidate.directory {
                    theme.accent
                } else {
                    theme.text
                },
            );
            if matched {
                span.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                span
            }
        })
        .collect()
}
fn marks_pane(frame: &mut Frame, area: Rect, app: &App, focus: &Focus) {
    let theme = app.palette;
    let focused = matches!(focus, Focus::Marks { .. });
    let block = panel(
        app.palette,
        format!(" Marks ({}) ", app.marks.len()),
        focused,
        theme.marked,
    )
    .title_bottom(Line::from(styled(" Alt+Space · switch pane ", theme.muted)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let (rows, index) = match focus {
        Focus::Marks { rows, index } => (rows.as_slice(), Some(*index)),
        _ => (app.marks.as_slice(), None),
    };
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(styled("Collect paths as you browse.", theme.muted)),
                Line::from(styled("Tab marks the highlighted entry.", theme.muted)),
                Line::from(""),
                Line::from(styled("Ctrl+w returns your marks.", theme.muted)),
            ])
            .wrap(Wrap { trim: false })
            .alignment(ratatui::layout::Alignment::Center),
            inner,
        );
        return;
    }
    let items = rows
        .iter()
        .map(|path| {
            let missing = !path.exists();
            let title = format!(
                "{} [{}] {}{}",
                if index.and_then(|i| rows.get(i)) == Some(path) {
                    "›"
                } else {
                    " "
                },
                if app.marks.contains(path) { "x" } else { " " },
                safe(
                    path.file_name()
                        .unwrap_or(path.as_os_str())
                        .to_string_lossy()
                ),
                if missing { " [MISSING]" } else { "" }
            );
            let mut lines = vec![Line::from(bold(
                title,
                if missing { theme.error } else { theme.marked },
            ))];
            // Compact labels wrap; F1 retains exact absolute paths.
            lines.extend(
                wrap_text(
                    &display_path(path, &app.cwd),
                    inner.width.saturating_sub(2).max(1) as usize,
                )
                .into_iter()
                .map(|s| Line::from(styled(format!("  {s}"), theme.muted))),
            );
            lines.push(Line::from(""));
            ListItem::new(Text::from(lines))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(index);
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().bg(theme.selection)),
        inner,
        &mut state,
    );
}
fn footer(frame: &mut Frame, area: Rect, app: &App, focus: &Focus) {
    let theme = app.palette;
    let selected = if let Focus::Marks { rows, index } = focus {
        rows.get(*index)
    } else {
        app.highlight.as_ref()
    };
    let path = selected
        .map(|p| display_path(p, &app.cwd))
        .unwrap_or_else(|| ".".into());
    let hint = if app.status.is_empty() {
        Line::from(styled(elide(&path, area.width as usize), theme.muted))
    } else {
        Line::from(styled(safe(&app.status), theme.error))
    };
    let keys = Line::from(vec![
        bold(" Enter ", theme.accent),
        styled("open / return  ", theme.muted),
        bold("Alt+# ", theme.accent),
        styled("open row  ", theme.muted),
        bold("Ctrl+# ", theme.accent),
        styled("choose only  ", theme.muted),
        bold("Tab ", theme.marked),
        styled("mark  ", theme.muted),
        bold("Ctrl+w ", theme.marked),
        styled("return marks", theme.muted),
    ]);
    let help = if area.width >= 136 {
        "Alt+h parent · Alt+l enter · Alt+b bookmarks · Alt+g location · Alt+←/→ depth · Alt+p/P palette · F5 refresh · F1 details · Ctrl+c quit"
    } else if area.width >= 115 {
        "Alt+h parent · Alt+l enter · Alt+b bookmarks · Alt+g location · Alt+←/→ depth · F5 refresh · F1 details · Ctrl+c quit"
    } else if area.width >= 94 {
        "Alt+h parent · Alt+l enter · Alt+b bookmarks · Alt+p/P palette · F5 refresh · Ctrl+c quit"
    } else if area.width >= 75 {
        "Alt+h parent · Alt+l enter · Alt+b bookmarks · F5 refresh · Ctrl+c quit"
    } else {
        "Ctrl+c quit · Alt+b bookmarks · F1 details"
    };
    let more = Line::from(styled(help, theme.muted));
    frame.render_widget(Paragraph::new(vec![hint, keys, more]), area);
}
fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    let mut used = 0;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width && used > 0 {
            rows.push(String::new());
            used = 0;
        }
        rows.last_mut().unwrap().push(c);
        used += w;
    }
    rows
}
fn details(frame: &mut Frame, area: Rect, app: &App, offset: &mut usize) {
    let theme = app.palette;
    let block = panel(
        app.palette,
        " Session details · Esc returns ",
        true,
        theme.accent,
    )
    .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let mut sections = vec![
        ("CURRENT DIRECTORY".into(), theme.accent),
        (safe(app.cwd.display().to_string()), theme.text),
        (String::new(), theme.text),
        (
            format!("DEPTH {}  ·  QUERY {}", app.depth, safe(&app.query)),
            theme.muted,
        ),
        (String::new(), theme.text),
        ("DISCOVERY SCOPES".into(), theme.accent),
    ];
    for (i, path) in app.scope.iter().enumerate() {
        sections.push((
            format!(
                "r{} · {} · {}",
                i + 1,
                if i < app.roots.len() {
                    "fixed root"
                } else {
                    "current directory"
                },
                safe(path.display().to_string())
            ),
            theme.text,
        ));
    }
    if let Some(path) = &app.highlight {
        sections.push((String::new(), theme.text));
        sections.push(("HIGHLIGHTED PATH".into(), theme.accent));
        sections.push((safe(path.display().to_string()), theme.text));
        if let Some(candidate) = app.rows.iter().find(|c| &c.path == path) {
            let root = &app.scope[candidate.root];
            sections.push((
                format!(
                    "d{} · r{} · {}",
                    candidate.depth,
                    candidate.root + 1,
                    safe(
                        path.strip_prefix(root)
                            .unwrap_or(path)
                            .display()
                            .to_string()
                    )
                ),
                theme.muted,
            ));
        }
    }
    sections.push((String::new(), theme.text));
    sections.push(("POLICIES".into(), theme.accent));
    sections.push((
        format!(
            "Hidden: {} · exclusions: {}",
            app.policy.hidden, app.policy.ignore
        ),
        theme.text,
    ));
    sections.push((
        format!(
            "File glob: {}",
            safe(
                app.policy
                    .glob
                    .as_ref()
                    .map(|g| g.glob().glob())
                    .unwrap_or("none")
            )
        ),
        theme.text,
    ));
    sections.extend(
        app.policy
            .rules
            .iter()
            .map(|r| (format!("Exclude: {}", safe(r)), theme.muted)),
    );
    sections.push((String::new(), theme.text));
    sections.push(("PALETTE".into(), theme.accent));
    sections.push((safe(&app.palette_name), theme.text));
    sections.push((
        "Alt+p choose (live preview, session only) · Alt+P reload from config".into(),
        theme.muted,
    ));
    sections.push((String::new(), theme.text));
    sections.push(("MARKED PATHS".into(), theme.marked));
    sections.extend(app.marks.iter().map(|p| {
        (
            format!(
                "{}{}",
                safe(p.display().to_string()),
                if p.exists() { "" } else { " [MISSING]" }
            ),
            if p.exists() { theme.text } else { theme.error },
        )
    }));
    let lines: Vec<_> = sections
        .into_iter()
        .flat_map(|(text, color)| {
            wrap_text(&text, inner.width.max(1) as usize)
                .into_iter()
                .map(move |s| Line::from(styled(s, color)))
        })
        .collect();
    *offset = (*offset).min(lines.len().saturating_sub(inner.height as usize));
    frame.render_widget(
        Paragraph::new(lines).scroll(((*offset).min(u16::MAX as usize) as u16, 0)),
        inner,
    );
}
fn prompt(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    field: Field,
    title: &str,
    text: &str,
    multiline: bool,
) {
    let status = &app.status;
    let theme = app.palette;
    let width = area.width.saturating_sub(4).min(84);
    let height = if multiline {
        area.height.saturating_sub(4).min(16)
    } else {
        7
    }
    .min(area.height);
    let dialog = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    let block = panel(theme, mode_title(app, field, title), true, theme.marked)
        .padding(Padding::horizontal(1));
    let inner = block.inner(dialog);
    frame.render_widget(Clear, dialog);
    frame.render_widget(block, dialog);
    let modal = app.edit_view.is_some();
    let hint = if multiline {
        if modal {
            "Exclusions: one glob per line; Ctrl+w applies; Esc normal / cancel"
        } else {
            "Exclusions: one glob per line; Ctrl+w applies, Esc cancels"
        }
    } else if modal {
        "Location · Enter navigates · Esc normal / cancel"
    } else {
        "Location (Enter navigates, Esc cancels)"
    };
    frame.render_widget(
        Paragraph::new(styled(hint, theme.muted)).wrap(Wrap { trim: false }),
        Rect::new(inner.x, inner.y, inner.width, 2.min(inner.height)),
    );
    let editor = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let view = app.edit_view.as_ref().filter(|v| v.field == field);
    let rows = text.split('\n').collect::<Vec<_>>();
    let cursor_row = view
        .map(|v| if v.mode.starts_with('c') { 0 } else { v.row })
        .unwrap_or(rows.len() - 1);
    let start = cursor_row.saturating_sub(editor.height.saturating_sub(1) as usize);
    let mut cursor_x = 0;
    let shown = rows
        .iter()
        .enumerate()
        .skip(start)
        .take(editor.height as usize)
        .map(|(row, text)| {
            let (line, x) = editable_line(app, field, text, row, editor.width as usize);
            if row == cursor_row {
                cursor_x = x;
            }
            line
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(shown), editor);
    if editor.height > 0 && editor.width > 0 {
        frame.set_cursor_position((
            editor.x + cursor_x,
            editor.y
                + cursor_row
                    .saturating_sub(start)
                    .min(editor.height as usize - 1) as u16,
        ));
    }
    if !status.is_empty() {
        frame.render_widget(
            Paragraph::new(styled(safe(status), theme.error)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
}

fn bookmarks(frame: &mut Frame, area: Rect, app: &App, query: &str, index: usize) {
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
            "Bookmarks · Enter opens · Esc returns",
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
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = app.matching_bookmarks(query);
    if matches.is_empty() {
        let text = if app.bookmarks.is_empty() {
            "No bookmarks configured. Add [[bookmarks]] to config.toml."
        } else {
            "No matching bookmarks"
        };
        frame.render_widget(
            Paragraph::new(styled(text, theme.muted)).wrap(Wrap { trim: false }),
            list,
        );
    } else {
        let items: Vec<_> = matches
            .iter()
            .map(|&i| {
                let b = &app.bookmarks[i];
                let available = b.path.is_dir();
                ListItem::new(vec![
                    Line::from(bold(
                        format!(
                            "{}{}",
                            safe(&b.name),
                            if available { "" } else { " [unavailable]" }
                        ),
                        if available { theme.marked } else { theme.error },
                    )),
                    Line::from(styled(
                        elide(
                            &display_path(&b.path, &app.cwd),
                            list.width.saturating_sub(2) as usize,
                        ),
                        theme.muted,
                    )),
                    Line::from(""),
                ])
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
    }
    if !app.status.is_empty() {
        frame.render_widget(
            Paragraph::new(styled(safe(&app.status), theme.error)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
}
/// Alt+p chooser. The whole frame already uses the highlighted (previewed) palette.
fn palettes(frame: &mut Frame, area: Rect, app: &App, query: &str, index: usize) {
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
            "Palettes · Enter applies · Esc reverts",
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
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = app.matching_palettes(query);
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
    }
    if !app.status.is_empty() {
        frame.render_widget(
            Paragraph::new(styled(safe(&app.status), theme.error)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
    fn fixture() -> App {
        let mut app = App::browse("/home/frank/notes".into(), 3, crate::Policy::default());
        for (name, directory) in [
            ("algorithms", true),
            ("research", true),
            ("report.tex", false),
            ("reading-list.md", false),
            ("todo.txt", false),
        ] {
            app.insert(
                0,
                Candidate {
                    path: app.cwd.join(name),
                    directory,
                    depth: 1,
                    root: 0,
                },
            );
        }
        app.refresh();
        app.scanning = false;
        app.marks = vec![
            "/home/frank/papers/Turing 1952.pdf".into(),
            "/home/frank/notes/report.tex".into(),
        ];
        app
    }
    fn text(buffer: &Buffer) -> String {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
    #[test]
    fn split_panes_colors_and_rendered_numbers() {
        let theme = Palette::default();
        let mut app = fixture();
        let mut focus = Focus::Results;
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        let mut targets = Vec::new();
        terminal
            .draw(|frame| targets = render(frame, &mut app, &mut focus).targets)
            .unwrap();
        let buffer = terminal.backend().buffer();
        let screen = text(buffer);
        let files = screen.lines().find(|s| s.contains("Files ·")).unwrap();
        assert!(files.find("Marks (2)").unwrap() > files.find("Files ·").unwrap());
        assert_eq!(
            targets,
            app.rows.iter().map(|c| c.path.clone()).collect::<Vec<_>>()
        );
        assert!(buffer.content().iter().any(|c| c.bg == theme.selection));
        assert!(buffer.content().iter().any(|c| c.fg == theme.accent));
        assert!(screen.contains("Alt+#"));
        assert!(screen.contains("[MISSING]"));
        export_preview(buffer);
    }
    #[test]
    fn narrow_tiny_unicode_and_resize_frames_have_only_visible_targets() {
        let mut app = fixture();
        app.insert(
            0,
            Candidate {
                path: app.cwd.join("研究_notes.md"),
                directory: false,
                depth: 1,
                root: 0,
            },
        );
        app.refresh();
        let mut focus = Focus::Results;
        let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
        let mut targets = Vec::new();
        terminal
            .draw(|f| targets = render(f, &mut app, &mut focus).targets)
            .unwrap();
        assert!(!targets.is_empty());
        let screen = text(terminal.backend().buffer());
        assert!(screen.find("Marks (2)").unwrap() > screen.find("research").unwrap());
        terminal.backend_mut().resize(20, 5);
        terminal
            .draw(|f| targets = render(f, &mut app, &mut focus).targets)
            .unwrap();
        assert!(targets.is_empty());
        for (width, height) in [(36, 12), (80, 12), (80, 20), (160, 35)] {
            terminal.backend_mut().resize(width, height);
            terminal
                .draw(|f| targets = render(f, &mut app, &mut focus).targets)
                .unwrap();
            assert!(targets.len() <= height.saturating_sub(11) as usize);
        }
    }
    #[test]
    fn marks_focus_retains_unmarked_rows_and_details_clamp_scroll() {
        let theme = Palette::default();
        let mut app = fixture();
        let unmarked = app.marks.remove(0);
        let mut focus = Focus::Marks {
            rows: vec![unmarked],
            index: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("› [ ]"));
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|c| c.fg == theme.marked)
        );
        focus = Focus::Info { offset: usize::MAX };
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        assert!(matches!(focus,Focus::Info { offset } if offset<100));
    }
    #[test]
    fn relative_labels_compact_parents_without_changing_targets() {
        let base = Path::new("/work/notes/deep");
        for (path, label) in [
            ("/work/notes/deep/file.tex", "file.tex"),
            ("/work/notes/deep/topic/file.tex", "topic/file.tex"),
            ("/work/notes/file.tex", "../file.tex"),
            ("/work/papers/file.tex", ".../papers/file.tex"),
            ("/other/file.tex", ".../other/file.tex"),
            ("/work/notes/deep", "."),
            ("/work", ".../"),
            ("/work/notes/deep/.../file.tex", ".../file.tex"),
        ] {
            assert_eq!(display_path(Path::new(path), base), label);
        }
        let mut app = App::new(
            vec![base.into(), "/work/papers".into()],
            1,
            crate::Policy::default(),
        );
        let path = PathBuf::from("/work/papers/file.tex");
        app.insert(
            0,
            Candidate {
                path: path.clone(),
                directory: false,
                depth: 1,
                root: 1,
            },
        );
        app.refresh();
        app.marks.push(path.clone());
        app.scanning = false;
        let mut focus = Focus::Results;
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        let mut targets = Vec::new();
        terminal
            .draw(|f| targets = render(f, &mut app, &mut focus).targets)
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.matches(".../papers/file.tex").count() >= 3); // result, mark, footer
        assert!(!screen.contains("/work/papers/file.tex"));
        assert_eq!(targets, vec![path.clone()]);
        app.rendered = targets;
        assert_eq!(app.numbered(1), path);
        let mut bytes = Vec::new();
        crate::write_paths(&mut bytes, &[path], base, false, true).unwrap();
        assert_eq!(bytes, b"/work/papers/file.tex\0");
    }
    #[test]
    fn configured_palette_and_current_directory_banner_reach_the_frame() {
        let mut app = fixture();
        app.palette=crate::config::Settings::parse("[ui]\npalette='custom'\n[palettes.custom]\nextends='light'\ncurrent_directory='#123456'\ncurrent_directory_text='#fefdfa'\naccent='#334455'",Path::new("/cfg"),None,None).unwrap().palette;
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        let mut focus = Focus::Results;
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let screen = text(buffer);
        assert!(screen.lines().next().unwrap().contains("CURRENT DIRECTORY"));
        assert_eq!(buffer[(1, 0)].bg, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(buffer[(2, 0)].fg, Color::Rgb(0xfe, 0xfd, 0xfa));
        assert!(
            buffer
                .content()
                .iter()
                .any(|c| c.fg == Color::Rgb(0x33, 0x44, 0x55))
        );
        assert!(buffer.content().iter().any(|c| c.bg == app.palette.panel));
    }
    #[test]
    fn palette_chooser_filters_marks_current_and_details_list_palette_keys() {
        let mut app = fixture();
        let settings = crate::config::Settings::parse(
            "[ui]\npalette='nord'\n[palettes.paper]\nextends='light'",
            Path::new("/cfg"),
            None,
            None,
        )
        .unwrap();
        app.apply_palette(crate::reload::PaletteState::from_settings(&settings));
        let mut focus = Focus::Palettes {
            query: "paper".into(),
            index: 0,
            original: app.palette,
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("Palettes · Enter applies · Esc reverts"));
        assert!(screen.contains("Find: paper"));
        assert!(screen.contains("› paper"));
        assert!(screen.contains("custom"));
        assert!(!screen.contains("gruvbox-dark"));
        assert_eq!(app.matching_palettes("paper").len(), 1);
        if let Focus::Palettes { query, .. } = &mut focus {
            query.clear();
        }
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("built-in · current"));
        focus = Focus::Info { offset: 0 };
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("PALETTE"));
        assert!(screen.contains("Alt+P reload from config"));
    }
    #[test]
    fn vim_mode_selection_and_cursor_are_rendered_in_the_field() {
        let mut app = fixture();
        app.query = "abcdef".into();
        app.edit_view = Some(crate::editor::EditView {
            field: Field::Query,
            mode: "v".into(),
            blocking: false,
            row: 0,
            column: 1,
            anchor: Some((0, 0)),
            command: String::new(),
            command_column: 0,
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        let mut focus = Focus::Results;
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(text(buffer).contains("Filter · VISUAL"));
        assert_eq!(buffer[(10, 4)].bg, app.palette.selection);
        assert_eq!(buffer[(11, 4)].bg, app.palette.selection);
        let cursor = terminal.get_cursor_position().unwrap();
        assert_eq!((cursor.x, cursor.y), (11, 4));
    }
    fn export_preview(buffer: &Buffer) {
        let Ok(path) = std::env::var("PICKER_PREVIEW") else {
            return;
        };
        use std::io::Write;
        let mut file = std::fs::File::create(path).unwrap();
        writeln!(file, "{} {}", buffer.area.width, buffer.area.height).unwrap();
        for cell in buffer.content() {
            let (r, g, b) = match cell.fg {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (218, 228, 240),
            };
            let (br, bg, bb) = match cell.bg {
                Color::Rgb(r, g, b) => (r, g, b),
                _ => (15, 23, 36),
            };
            writeln!(file, "{r},{g},{b};{br},{bg},{bb};{}", cell.symbol()).unwrap();
        }
    }
}
