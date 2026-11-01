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
/// Keep the start of a message, measuring terminal columns.
fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.into();
    }
    let mut kept = String::new();
    let mut used = 1;
    for c in text.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > width {
            break;
        }
        kept.push(c);
        used += w;
    }
    if width == 0 { kept } else { kept + "…" }
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
            Line::from(bold(" fff", theme.accent)),
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
    let location = location_lines(app, area.width as usize);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(location.len() as u16),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    query(frame, layout[0], app, matches!(focus, Focus::Results));
    frame.render_widget(Paragraph::new(location), layout[1]);
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
        Focus::Info { offset, keys } => info(frame, area, app, offset, *keys),
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
        Focus::BookmarkEdit(draft) => bookmark_form(frame, area, app, draft),
        Focus::BookmarkIcons { query, index, .. } => {
            bookmark_icons(frame, area, app, query, *index)
        }
        Focus::BookmarkDelete { target, .. } => bookmark_delete(frame, area, app, *target),
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

/// `~` for the home directory, presentation only.
fn tilde(path: &Path, home: Option<&Path>) -> String {
    match home.and_then(|h| path.strip_prefix(h).ok()) {
        Some(rest) if rest.as_os_str().is_empty() => "~".into(),
        Some(rest) => safe(format!("~/{}", rest.display())),
        None => safe(path.display().to_string()),
    }
}
/// A short label drawn on a tinted background, like a status-line segment.
fn pill(text: impl Into<String>, fg: Color, bg: Color) -> Span<'static> {
    Span::styled(format!(" {} ", text.into()), Style::default().fg(fg).bg(bg))
}

/// The integrated "@" prompt line names the single current directory. Explicit
/// startup roots stay separate as compact pinned names, beside the path when both
/// fit and on a second line otherwise.
fn location_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let theme = app.palette;
    let badge = Style::default()
        .fg(theme.current_directory_text)
        .bg(theme.current_directory)
        .add_modifier(Modifier::BOLD);
    let path = tilde(&app.cwd, app.home.as_deref());
    let pinned = pinned_roots(app);
    let pinned_width = pinned.iter().map(|s| s.width()).sum::<usize>();
    // " @ " badge, a space, the path, a space.
    let fixed = 5;
    let inline = fixed + path.width() + 2 + pinned_width <= width;
    let shown = elide(&path, width.saturating_sub(fixed));
    let basename = app
        .cwd
        .file_name()
        .map(|s| safe(s.to_string_lossy()))
        .unwrap_or_else(|| shown.clone());
    let (parent, last) = if shown.ends_with(&basename) {
        shown.split_at(shown.len() - basename.len())
    } else {
        ("", shown.as_str())
    };
    let mut first = vec![
        Span::styled(" @ ", badge),
        Span::raw(" "),
        styled(parent.to_string(), theme.muted),
        bold(last.to_string(), theme.text),
    ];
    if pinned.is_empty() {
        return vec![Line::from(first)];
    }
    if inline {
        let used = fixed - 1 + shown.width();
        first.push(Span::raw(
            " ".repeat(width.saturating_sub(used + pinned_width)),
        ));
        first.extend(pinned);
        return vec![Line::from(first)];
    }
    // Leading spaces align the pinned list with the path rather than "@".
    let mut second = vec![Span::raw("    ")];
    second.extend(fit_spans(pinned, width.saturating_sub(4)));
    vec![Line::from(first), Line::from(second)]
}

/// `pinned  notes  papers`: a pin glyph (or the word) then each root's name; the
/// root that is also the current directory is bold. F1 shows full paths.
fn pinned_roots(app: &App) -> Vec<Span<'static>> {
    let theme = app.palette;
    if app.roots.is_empty() {
        return Vec::new();
    }
    let label = if app.icons {
        "\u{f435} pinned"
    } else {
        "pinned"
    };
    let mut spans = vec![styled(label, theme.muted)];
    for root in &app.roots {
        // The gap is separate so only the current root's name is underlined.
        let name = styled(root_name(root), theme.text);
        spans.push(Span::raw("  "));
        spans.push(if root == &app.cwd {
            name.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            name
        });
    }
    spans
}
fn root_name(root: &Path) -> String {
    root.file_name()
        .map(|s| safe(s.to_string_lossy()))
        .unwrap_or_else(|| safe(root.display().to_string()))
}

/// Keep whole root names that fit, then summarize the rest as `  +N (F1)`.
fn fit_spans(spans: Vec<Span<'static>>, width: usize) -> Vec<Span<'static>> {
    let total = spans.iter().map(|s| s.width()).sum::<usize>();
    if total <= width {
        return spans;
    }
    // spans[0] is the label; each root is a (gap, name) pair.
    let roots = (spans.len() - 1) / 2;
    let mut kept = vec![spans[0].clone()];
    let mut used = spans[0].width();
    for (shown, pair) in spans[1..].chunks(2).enumerate() {
        let more = format!("  +{} (F1)", roots - shown);
        let pair_width = pair.iter().map(|s| s.width()).sum::<usize>();
        if used + pair_width + format!("  +{} (F1)", roots - shown - 1).width() > width {
            kept.push(Span::styled(
                elide(&more, width.saturating_sub(used)),
                spans[0].style,
            ));
            break;
        }
        kept.extend(pair.iter().cloned());
        used += pair_width;
    }
    kept
}

/// Session state as pills on the filter's bottom border: discovery progress and
/// depth always, then only settings that differ from the defaults. Narrow frames
/// drop trailing pills and shorten the progress label; F1 lists everything.
fn status_line(app: &App, width: usize) -> Line<'static> {
    let theme = app.palette;
    let bg = theme.selection;
    let (scan, color) = if app.cache_pending && app.scanning {
        (["◌ cached · refreshing", "◌ incomplete"], theme.marked)
    } else if app.scanning {
        (["◌ scanning · incomplete", "◌ incomplete"], theme.marked)
    } else {
        (["● complete"; 2], theme.success)
    };
    let mut optional = Vec::new();
    if let Some(glob) = &app.policy.glob {
        optional.push(pill(safe(glob.glob().glob()), theme.accent, bg));
    }
    if app.no_dir {
        optional.push(pill("files only", theme.accent, bg));
    }
    if app.policy.hidden {
        optional.push(pill("hidden shown", theme.accent, bg));
    }
    if !app.policy.ignore {
        optional.push(pill("exclusions off", theme.marked, bg));
    }
    let depth = pill(format!("depth {}", app.depth), theme.text, bg);
    let mut keep = optional.len();
    let mut label = 0;
    loop {
        let spans: Vec<Span<'static>> = [pill(scan[label], color, bg), depth.clone()]
            .into_iter()
            .chain(optional[..keep].iter().cloned())
            .flat_map(|p| [Span::raw(" "), p])
            .chain([Span::raw(" ")])
            .collect();
        let used = spans.iter().map(|s| s.width()).sum::<usize>();
        if used <= width || (keep == 0 && label == 1) {
            return Line::from(spans);
        }
        if keep > 0 {
            keep -= 1;
        } else {
            label += 1;
        }
    }
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
    .padding(Padding::horizontal(1))
    .title_bottom(status_line(app, area.width.saturating_sub(4) as usize).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let prefix = "❯ ";
    let available = inner.width.saturating_sub(prefix.width() as u16) as usize;
    let (mut line, cursor) = editable_line(app, Field::Query, &app.query, 0, available);
    if app.query.is_empty()
        && !app
            .edit_view
            .as_ref()
            .is_some_and(|v| v.mode.starts_with('c'))
    {
        line = Line::from(styled("type to filter names…", theme.muted));
    }
    line.spans.insert(0, bold(prefix, theme.accent));
    frame.render_widget(Paragraph::new(line), inner);
    if focused && inner.width > 0 {
        frame.set_cursor_position((inner.x + prefix.width() as u16 + cursor, inner.y));
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
        } else if app.no_dir && app.query.is_empty() {
            (
                "No files here",
                "Alt+→ searches deeper. Ctrl+c leaves without a selection.",
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
            // Labels are relative to the current directory, so the scope a row
            // came from is already visible; F1 shows its depth and root.
            let meta = " ";
            let (kind, group) = crate::icons::marker(&c.path, c.directory, app.icons);
            let available = inner.width.saturating_sub(
                (prefix.width() + "● ".width() + kind.width() + meta.width()) as u16,
            ) as usize;
            let name = elide(&name, available);
            let gap = available.saturating_sub(name.width());
            let mut spans = vec![
                styled(prefix, if selected { theme.accent } else { theme.muted }),
                styled(if marked { "● " } else { "  " }, theme.marked),
                styled(kind, icon_color(theme, group)),
            ];
            spans.extend(filename_spans(app.palette, &name, c, &app.query));
            spans.push(styled(" ".repeat(gap) + meta, theme.muted));
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
/// Icons reuse palette roles, so custom palettes and `NO_COLOR` apply unchanged.
fn icon_color(theme: Palette, group: crate::icons::Group) -> Color {
    use crate::icons::Group;
    match group {
        Group::Directory => theme.accent,
        Group::Code => theme.success,
        Group::Media | Group::Archive => theme.marked,
        Group::Document => theme.text,
        Group::Config | Group::Other => theme.muted,
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
            // The parent part of a label is dimmed so the name stands out.
            let span = styled(
                c.to_string(),
                if matched {
                    theme.marked
                } else if i < start {
                    theme.muted
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
    );
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
            let metadata = path.metadata().ok();
            let missing = metadata.is_none();
            let icon = if app.icons {
                crate::icons::icon(path, metadata.is_some_and(|m| m.is_dir())).cell()
            } else {
                String::new()
            };
            let title = format!(
                "{} {} {icon}{}{}",
                if index.and_then(|i| rows.get(i)) == Some(path) {
                    "›"
                } else {
                    " "
                },
                if app.marks.contains(path) {
                    "●"
                } else {
                    "○"
                },
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
            // Compact labels wrap; F1 retains exact absolute paths. A label that
            // is just the name (a mark in the current directory) adds nothing.
            let label = display_path(path, &app.cwd);
            if label.contains('/') {
                lines.extend(
                    wrap_text(&label, inner.width.saturating_sub(2).max(1) as usize)
                        .into_iter()
                        .map(|s| Line::from(styled(format!("  {s}"), theme.muted))),
                );
            }
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
/// One line: the selected path (or a status message) and the F1 pointer.
fn footer(frame: &mut Frame, area: Rect, app: &App, focus: &Focus) {
    let theme = app.palette;
    let selected = if let Focus::Marks { rows, index } = focus {
        rows.get(*index)
    } else {
        app.highlight.as_ref()
    };
    let help = [bold("F1", theme.accent), styled(" keys ", theme.muted)];
    let help_width = help.iter().map(|s| s.width()).sum::<usize>();
    let available = (area.width as usize).saturating_sub(help_width + 2);
    let mut spans = if app.status.is_empty() {
        let path = selected
            .map(|p| display_path(p, &app.cwd))
            .unwrap_or_else(|| ".".into());
        let path = elide(&path, available);
        let split = path.rfind('/').map(|i| i + 1).unwrap_or(0);
        vec![
            Span::raw(" "),
            styled(path[..split].to_string(), theme.muted),
            styled(path[split..].to_string(), theme.text),
        ]
    } else {
        vec![
            Span::raw(" "),
            styled(truncate(&safe(&app.status), available), theme.error),
        ]
    };
    let used = spans.iter().map(|s| s.width()).sum::<usize>();
    spans.push(Span::raw(
        " ".repeat((area.width as usize).saturating_sub(used + help_width)),
    ));
    spans.extend(help);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
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
/// F1 overlay with two tabs: the key guide (default) and session details.
fn info(frame: &mut Frame, area: Rect, app: &App, offset: &mut usize, keys: bool) {
    let theme = app.palette;
    let tab = |name: &'static str, active: bool| {
        if active {
            pill(name, theme.background, theme.accent).add_modifier(Modifier::BOLD)
        } else {
            styled(format!(" {name} "), theme.muted)
        }
    };
    let block = panel(theme, "", true, theme.accent)
        .title(Line::from(vec![
            Span::raw(" "),
            tab("Keys", keys),
            Span::raw(" "),
            tab("Session", !keys),
            Span::raw(" "),
        ]))
        .title_bottom(
            Line::from(vec![
                bold(" Tab", theme.accent),
                styled(" switches  ", theme.muted),
                bold("↑↓", theme.accent),
                styled(" scroll  ", theme.muted),
                bold("Esc", theme.accent),
                styled(" closes ", theme.muted),
            ])
            .right_aligned(),
        )
        .padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let lines = if keys {
        key_guide(app, inner.width as usize)
    } else {
        session_lines(app, inner.width as usize)
    };
    *offset = (*offset).min(lines.len().saturating_sub(inner.height as usize));
    frame.render_widget(
        Paragraph::new(lines).scroll(((*offset).min(u16::MAX as usize) as u16, 0)),
        inner,
    );
}

/// A titled group of (keys, description, needs navigation) rows in the F1 guide.
type GuideSection = (&'static str, Vec<(String, &'static str, bool)>);
/// Every binding, grouped. Navigation keys are dimmed with `--no-dir`, and the
/// Normal-mode section shows the configured `[keys.normal]` sequences.
fn key_guide(app: &App, width: usize) -> Vec<Line<'static>> {
    use crate::normal_keys::ListAction::*;
    let theme = app.palette;
    let normal = |action| {
        let keys = app.normal_keys.describe(action);
        if keys.is_empty() { "—".into() } else { keys }
    };
    // (keys, description, needs navigation)
    let sections: Vec<GuideSection> = vec![
        (
            "Navigate",
            vec![
                ("↑↓  Alt+k Alt+j".into(), "move the highlight", false),
                ("PgUp PgDn".into(), "previous / next page", false),
                ("Enter".into(), "open dir, or return file + marks", false),
                ("Alt+l".into(), "enter the highlighted directory", true),
                ("Alt+h".into(), "parent directory, keeping query", true),
                ("Alt+g".into(), "go to a typed location", true),
                ("Alt+b".into(), "open a bookmark", true),
            ],
        ),
        (
            "Select and return",
            vec![
                ("Tab".into(), "mark / unmark, then move down", false),
                ("Alt+Space".into(), "focus the marked list", false),
                ("Ctrl+w".into(), "return the marked paths", false),
                ("Alt+1…9 Alt+0".into(), "activate row n like Enter", false),
                (
                    "Ctrl+1…9 Ctrl+0".into(),
                    "return row n now, ignoring marks",
                    false,
                ),
                (String::new(), "0 is the last visible row", false),
                ("Ctrl+c".into(), "quit without returning anything", false),
            ],
        ),
        (
            "Search",
            vec![
                ("type".into(), "fuzzy-filter names", false),
                ("Esc".into(), "leave Insert, then clear query", false),
                ("Alt+← Alt+→".into(), "search shallower / deeper", false),
                ("Alt+.".into(), "show / hide hidden entries", false),
                ("Alt+H".into(), "exclusions on / off", false),
                ("Alt+e".into(), "edit exclusions", false),
                (normal(Apply), "apply / close exclusions", false),
                ("F5".into(), "rescan, bypassing the cache", false),
            ],
        ),
        (
            "Normal mode in lists",
            vec![
                (normal(Down) + "  " + &normal(Up), "move down / up", false),
                (normal(Left), "parent directory, like Alt+h", true),
                (normal(Right), "enter directory, like Alt+l", true),
                (
                    normal(First) + "  " + &normal(Last),
                    "first / last row",
                    false,
                ),
                (normal(Mark), "mark / unmark, like Tab", false),
                ("i  a".into(), "enter Insert mode", false),
            ],
        ),
        (
            "Look",
            vec![
                ("Alt+p".into(), "choose palette (100 ms preview)", false),
                ("Alt+P".into(), "reload palette from config", false),
                ("F1".into(), "this guide · Tab: session", false),
            ],
        ),
        (
            "Bookmarks",
            vec![
                ("Alt+n".into(), "add current directory (in chooser)", true),
                ("Alt+e".into(), "edit highlighted bookmark", true),
                ("Alt+d".into(), "delete bookmark, with confirmation", true),
                ("Alt+r".into(), "reload bookmarks from config", true),
                (
                    "Tab / Shift+Tab".into(),
                    "switch bookmark form fields",
                    true,
                ),
                ("Alt+i".into(), "choose bookmark Nerd Font icon", true),
                ("Ctrl+s".into(), "save bookmark", true),
                (normal(Apply), "save bookmark in Normal mode", true),
            ],
        ),
    ];
    let key_width = 17;
    let column = |section: &GuideSection, width: usize| {
        let mut lines = vec![vec![bold(section.0.to_uppercase(), theme.marked)]];
        if app.no_dir && section.1.iter().any(|entry| entry.2) {
            lines.push(vec![styled("crossed out: off with --no-dir", theme.muted)]);
        }
        for (keys, description, navigation) in &section.1 {
            let description = elide(description, width.saturating_sub(key_width));
            if keys.is_empty() {
                lines.push(vec![styled(
                    format!("{:key_width$}{description}", ""),
                    theme.muted,
                )]);
                continue;
            }
            let key = format!("{:<key_width$}", elide(keys, key_width - 1));
            let mut line = vec![bold(key, theme.accent), styled(description, theme.text)];
            if *navigation && app.no_dir {
                line = line
                    .into_iter()
                    .map(|span| {
                        span.fg(theme.muted)
                            .add_modifier(Modifier::CROSSED_OUT)
                            .remove_modifier(Modifier::BOLD)
                    })
                    .collect();
            }
            lines.push(line);
        }
        lines.push(Vec::new());
        lines
    };
    // Two columns side by side when wide enough; otherwise one.
    let gap = 4;
    let half = width.saturating_sub(gap) / 2;
    if half >= key_width + 32 {
        let (left, right) = sections.split_at(2);
        let left: Vec<_> = left.iter().flat_map(|s| column(s, half)).collect();
        let right: Vec<_> = right.iter().flat_map(|s| column(s, half)).collect();
        (0..left.len().max(right.len()))
            .map(|i| {
                let mut spans = left.get(i).cloned().unwrap_or_default();
                let used = spans.iter().map(|s| s.width()).sum::<usize>();
                spans.push(Span::raw(" ".repeat(half + gap - used.min(half))));
                spans.extend(right.get(i).cloned().unwrap_or_default());
                Line::from(spans)
            })
            .collect()
    } else {
        sections
            .iter()
            .flat_map(|s| column(s, width))
            .map(Line::from)
            .collect()
    }
}

/// Exact paths, scopes, policies, palette, and marks.
fn session_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let theme = app.palette;
    let mut sections: Vec<(String, Color)> = vec![
        ("CURRENT DIRECTORY".into(), theme.accent),
        (safe(app.cwd.display().to_string()), theme.text),
        (String::new(), theme.text),
        (
            format!("Depth {} · query “{}”", app.depth, safe(&app.query)),
            theme.muted,
        ),
        (String::new(), theme.text),
        ("DISCOVERY SCOPES".into(), theme.accent),
    ];
    for (i, path) in app.scope.iter().enumerate() {
        sections.push((
            format!(
                "{} · {}",
                if i < app.roots.len() {
                    "pinned"
                } else {
                    "current"
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
                    "depth {} below {} · {}",
                    candidate.depth,
                    root_name(root),
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
            "Hidden: {} · exclusions: {}{}",
            app.policy.hidden,
            app.policy.ignore,
            if app.no_dir {
                " · files only (--no-dir)"
            } else {
                ""
            }
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
    sections
        .into_iter()
        .flat_map(|(text, color)| {
            // Section headings are the only accent- or marked-colored entries.
            let heading = color == theme.accent || color == theme.marked;
            wrap_text(&text, width.max(1)).into_iter().map(move |s| {
                let span = styled(s, color);
                Line::from(if heading {
                    span.add_modifier(Modifier::BOLD)
                } else {
                    span
                })
            })
        })
        .collect()
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
    let apply = app
        .normal_keys
        .describe(crate::normal_keys::ListAction::Apply);
    let apply = if apply.is_empty() { "unbound" } else { &apply };
    let hint = if multiline {
        if modal {
            format!("Exclusions: one glob per line; {apply} in Normal applies; Esc normal / cancel")
        } else {
            format!("Exclusions: one glob per line; {apply} applies, Esc cancels")
        }
    } else if modal {
        "Location · Enter navigates · Esc normal / cancel".into()
    } else {
        "Location (Enter navigates, Esc cancels)".into()
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
            "No bookmarks yet. Alt+n adds the current directory."
        } else {
            "No matching bookmarks"
        };
        frame.render_widget(
            Paragraph::new(styled(text, theme.muted)).wrap(Wrap { trim: false }),
            list,
        );
    } else {
        // Configured icons always show; others get the folder glyph when file
        // icons are enabled, or blank padding so names stay aligned.
        let icon_column = app.icons || app.bookmarks.iter().any(|b| b.icon.is_some());
        let items: Vec<_> = matches
            .iter()
            .map(|&i| {
                let b = &app.bookmarks[i];
                let available = b.path.is_dir();
                let color = if available { theme.marked } else { theme.error };
                let mut title = Vec::new();
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
                let indent = if icon_column { "   " } else { "" };
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
        let mut state = ListState::default().with_selected(Some(index));
        frame.render_stateful_widget(
            List::new(items)
                .highlight_style(Style::default().bg(theme.selection))
                .highlight_symbol("› "),
            list,
            &mut state,
        );
    }
    let hint = if app.status.is_empty() {
        "Alt+n add · Alt+e edit · Alt+d delete · Alt+r reload"
    } else {
        &app.status
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
}
/// Shared modal frame for the bookmark form, icon chooser and deletion prompt.
fn bookmark_dialog(frame: &mut Frame, area: Rect, app: &App, title: String, height: u16) -> Rect {
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
fn bookmark_form(frame: &mut Frame, area: Rect, app: &App, draft: &crate::bookmarks::Draft) {
    use crate::bookmarks::EditField;
    let title = if draft.editing.is_some() {
        "Edit bookmark"
    } else {
        "Add bookmark"
    };
    let title = draft
        .field
        .editor()
        .map_or_else(|| title.to_owned(), |f| mode_title(app, f, title));
    let inner = bookmark_dialog(frame, area, app, title, 14);
    let theme = app.palette;
    let put = |frame: &mut Frame, y: u16, text: String, color| {
        if y < inner.height {
            frame.render_widget(
                Paragraph::new(styled(text, color)),
                Rect::new(inner.x, inner.y + y, inner.width, 1),
            );
        }
    };
    put(
        frame,
        0,
        "Tab fields · Enter next / choose icon · Alt+i icons".into(),
        theme.muted,
    );
    for (row, label, value, active, field) in [
        (
            2,
            "Name: ",
            draft.name.as_str(),
            draft.field == EditField::Name,
            Field::BookmarkName,
        ),
        (
            3,
            "Path: ",
            draft.path.as_str(),
            draft.field == EditField::Path,
            Field::BookmarkPath,
        ),
    ] {
        if row >= inner.height {
            continue;
        }
        let (mut line, cursor) =
            editable_line(app, field, value, 0, inner.width.saturating_sub(6) as usize);
        line.spans.insert(
            0,
            bold(label, if active { theme.accent } else { theme.muted }),
        );
        if active {
            line = line.style(Style::default().bg(theme.selection));
        }
        frame.render_widget(
            Paragraph::new(line),
            Rect::new(inner.x, inner.y + row, inner.width, 1),
        );
        if active && inner.width > 6 {
            frame.set_cursor_position((inner.x + 6 + cursor, inner.y + row));
        }
    }
    let icon = draft.icon.as_deref().unwrap_or("Automatic (folder)");
    put(
        frame,
        4,
        format!(
            "{}Icon: {}  Enter chooses",
            if draft.field == EditField::Icon {
                "› "
            } else {
                "  "
            },
            safe(icon)
        ),
        if draft.field == EditField::Icon {
            theme.accent
        } else {
            theme.text
        },
    );
    if let Some(path) = &app.bookmark_store.path {
        put(
            frame,
            6,
            format!(
                "Save to: {}",
                elide(
                    &safe(path.display().to_string()),
                    inner.width.saturating_sub(9) as usize
                )
            ),
            theme.muted,
        );
    } else {
        put(
            frame,
            6,
            "Session only · configuration is disabled or unavailable".into(),
            theme.muted,
        );
    }
    put(
        frame,
        7,
        "Relative paths start at the current browsing directory.".into(),
        theme.muted,
    );
    if !app.status.is_empty() && inner.height > 2 {
        put(frame, inner.height - 2, safe(&app.status), theme.error);
    }
    let apply = app
        .normal_keys
        .describe(crate::normal_keys::ListAction::Apply);
    put(
        frame,
        inner.height.saturating_sub(1),
        format!("Ctrl+s / {apply} (Normal): save · Esc cancels"),
        theme.marked,
    );
}
fn bookmark_icons(frame: &mut Frame, area: Rect, app: &App, query: &str, index: usize) {
    let inner = bookmark_dialog(
        frame,
        area,
        app,
        mode_title(
            app,
            Field::BookmarkIcons,
            "Bookmark icons · Enter chooses · Esc returns",
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
    let list = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3),
    );
    let matches = crate::icons::matching_bookmark_icons(query);
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
    }
    frame.render_widget(
        Paragraph::new(styled(
            "Nerd Font glyphs · Automatic clears the custom icon",
            theme.muted,
        )),
        Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
    );
}
fn bookmark_delete(frame: &mut Frame, area: Rect, app: &App, index: usize) {
    let inner = bookmark_dialog(frame, area, app, "Delete bookmark?".into(), 10);
    let theme = app.palette;
    let Some(bookmark) = app.bookmarks.get(index) else {
        return;
    };
    let lines = vec![
        Line::from(bold(safe(&bookmark.name), theme.marked)),
        Line::from(styled(
            elide(
                &safe(bookmark.path.display().to_string()),
                inner.width as usize,
            ),
            theme.muted,
        )),
        Line::from(""),
        Line::from(styled(
            "Enter deletes this bookmark · Esc cancels",
            theme.text,
        )),
        Line::from(styled(safe(&app.status), theme.error)),
    ];
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
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
        // Keys live in the F1 guide; the footer only points to it.
        assert!(screen.contains("F1 keys"));
        assert!(!screen.contains("Alt+#"));
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
            // Filter (3), location line (1), footer (1), and pane borders (2).
            assert!(targets.len() <= height.saturating_sub(7) as usize);
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
        assert!(screen.contains("› ○"));
        assert!(
            terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .any(|c| c.fg == theme.marked)
        );
        focus = Focus::Info {
            offset: usize::MAX,
            keys: false,
        };
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        assert!(matches!(focus,Focus::Info { offset, .. } if offset<100));
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
    fn configured_palette_and_location_line_reach_the_frame() {
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
        // The location line follows the three-row filter panel; no top banner.
        assert!(!screen.contains("CURRENT DIRECTORY"));
        assert!(screen.lines().next().unwrap().contains("Filter"));
        assert!(
            screen
                .lines()
                .nth(3)
                .unwrap()
                .starts_with("  @  /home/frank/notes")
        );
        // The "@" badge uses the current-directory roles; the basename is bold.
        assert_eq!(buffer[(1, 3)].bg, Color::Rgb(0x12, 0x34, 0x56));
        assert_eq!(buffer[(2, 3)].fg, Color::Rgb(0xfe, 0xfd, 0xfa));
        assert_eq!(buffer[(17, 3)].symbol(), "n");
        assert!(buffer[(17, 3)].modifier.contains(Modifier::BOLD));
        assert!(!buffer[(16, 3)].modifier.contains(Modifier::BOLD));
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
        focus = Focus::Info {
            offset: 0,
            keys: false,
        };
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("PALETTE"));
        focus = Focus::Info {
            offset: 0,
            keys: true,
        };
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("reload palette from config"));
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
        assert_eq!(buffer[(5, 1)].bg, app.palette.selection);
        assert_eq!(buffer[(6, 1)].bg, app.palette.selection);
        let cursor = terminal.get_cursor_position().unwrap();
        assert_eq!((cursor.x, cursor.y), (6, 1));
    }
    #[test]
    fn bookmark_chooser_shows_configured_and_default_icons() {
        let mut app = fixture();
        let bookmark = |name: &str, icon: Option<&str>| crate::config::Bookmark {
            name: name.into(),
            path: "/tmp".into(),
            icon: icon.map(str::to_string),
        };
        app.bookmarks = vec![
            bookmark("Downloads", Some("\u{f10e9}")),
            bookmark("Temp", None),
        ];
        let mut focus = Focus::Bookmarks {
            query: String::new(),
            index: 0,
        };
        let mut terminal = Terminal::new(TestBackend::new(100, 26)).unwrap();
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("\u{f10e9}  Downloads"), "{screen}");
        assert!(screen.contains("\u{f07b}  Temp"), "{screen}");
        app.icons = false;
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        let screen = text(terminal.backend().buffer());
        // The configured icon stays; the other name keeps its alignment.
        assert!(screen.contains("\u{f10e9}  Downloads"), "{screen}");
        assert!(screen.contains("   Temp"), "{screen}");
        assert!(!screen.contains('\u{f07b}'), "{screen}");
    }
    fn draw(app: &mut App, width: u16, height: u16) -> (String, Vec<PathBuf>) {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut focus = Focus::Results;
        let mut targets = Vec::new();
        terminal
            .draw(|f| targets = render(f, app, &mut focus).targets)
            .unwrap();
        (text(terminal.backend().buffer()), targets)
    }
    #[test]
    fn location_line_sits_between_filter_and_results_with_status_on_border() {
        let mut app = fixture();
        let (screen, _) = draw(&mut app, 120, 26);
        let lines: Vec<_> = screen.lines().collect();
        assert!(lines[0].contains("Filter"));
        assert!(lines[1].contains("❯ type to filter"));
        // Progress and depth always; only non-default settings get a pill.
        assert!(
            lines[2].contains("● complete   depth 3   exclusions off"),
            "{screen}"
        );
        assert!(!lines[2].contains("hidden"));
        assert_eq!(lines[3].trim_end(), "  @  /home/frank/notes");
        assert!(lines[4].contains("Files ·"));
        assert!(!screen.contains("pinned"));
        app.scanning = true;
        app.policy.glob = Some(crate::glob("*.tex").unwrap());
        let (screen, _) = draw(&mut app, 120, 26);
        assert!(
            screen.contains("◌ scanning · incomplete   depth 3   *.tex   exclusions off"),
            "{screen}"
        );
        // The minimum frame keeps depth and an explicit incomplete label.
        let (screen, targets) = draw(&mut app, 36, 12);
        let status = screen.lines().nth(2).unwrap();
        assert!(status.contains("◌ incomplete   depth 3"), "{screen}");
        assert!(
            screen
                .lines()
                .nth(3)
                .unwrap()
                .contains("@  /home/frank/notes")
        );
        assert!(screen.lines().nth(3).unwrap().contains("notes"));
        assert!(!targets.is_empty());
    }
    #[test]
    fn pinned_roots_are_compact_and_separate_from_the_current_directory() {
        let roots: Vec<PathBuf> = vec![
            "/work/notes".into(),
            "/work/papers".into(),
            "/srv/shared/library".into(),
        ];
        let mut app = App::new(roots, 2, crate::Policy::default());
        app.scanning = false;
        // Wide: path and pinned root names share one line.
        let (screen, _) = draw(&mut app, 120, 26);
        let line = screen.lines().nth(3).unwrap();
        assert!(line.starts_with("  @  /work/notes "), "{line}");
        assert!(
            line.trim_end()
                .ends_with("\u{f435} pinned  notes  papers  library"),
            "{line}"
        );
        assert!(screen.lines().nth(4).unwrap().contains("Files ·"));
        // Only the current root's name is underlined, not the gap before it.
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        let mut focus = Focus::Results;
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let underlined: String = (0..buffer.area.width)
            .filter(|&x| buffer[(x, 3)].modifier.contains(Modifier::UNDERLINED))
            .map(|x| buffer[(x, 3)].symbol())
            .collect();
        assert_eq!(underlined, "notes");
        // Away from every root, the pinned list moves to a second line.
        app.navigate(
            "/work/notes/a-rather-long-topic-directory-name/chapter".into(),
            false,
        );
        let (screen, targets) = draw(&mut app, 50, 14);
        let lines: Vec<_> = screen.lines().collect();
        assert!(lines[3].contains("@ "), "{screen}");
        assert!(lines[3].trim_end().ends_with("chapter"), "{screen}");
        assert!(
            lines[4].starts_with("     \u{f435} pinned  notes  papers"),
            "{screen}"
        );
        assert!(lines[5].contains("Files ·"));
        assert!(targets.len() <= 14 - 8);
        // Too narrow for all roots: keep whole entries and point to F1.
        app.icons = false;
        app.roots
            .push("/srv/archive/collected-correspondence".into());
        let (screen, _) = draw(&mut app, 36, 12);
        let pinned = screen.lines().nth(4).unwrap();
        assert!(pinned.contains("pinned  notes  papers"), "{screen}");
        assert!(pinned.contains("+2 (F1)"), "{screen}");
        assert!(!pinned.contains("library") && !pinned.contains("correspondence"));
    }
    #[test]
    fn location_line_stays_distinct_without_color() {
        let mut app = fixture();
        app.palette = crate::config::Settings::parse(
            "[ui]\npalette='plain'\n[palettes.plain]\nbackground='default'\npanel='default'\ntext='default'\nmuted='default'\nborder='default'\naccent='default'\nmarked='default'\nsuccess='default'\nerror='default'\nselection='default'\ncurrent_directory='default'\ncurrent_directory_text='default'",
            Path::new("/cfg"),
            None,
            None,
        )
        .unwrap()
        .palette;
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        let mut focus = Focus::Results;
        terminal
            .draw(|f| {
                render(f, &mut app, &mut focus);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        // Without colors, "@" plus a bold basename still identify the location.
        assert_eq!(buffer[(2, 3)].symbol(), "@");
        assert!(buffer[(2, 3)].modifier.contains(Modifier::BOLD));
        assert!(buffer[(17, 3)].modifier.contains(Modifier::BOLD));
        assert!(text(buffer).contains("● complete"));
    }
    #[test]
    fn icons_keep_columns_elision_and_numbered_targets_aligned() {
        let mut app = fixture();
        for name in ["研究_notes.md", &format!("{}tail.rs", "long_".repeat(30))] {
            app.insert(
                0,
                Candidate {
                    path: app.cwd.join(name),
                    directory: false,
                    depth: 1,
                    root: 0,
                },
            );
        }
        app.marks = vec!["/home/frank/notes/report.tex".into()];
        app.refresh();
        let mut frames = Vec::new();
        for icons in [true, false] {
            app.icons = icons;
            let mut focus = Focus::Results;
            let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
            let mut targets = Vec::new();
            terminal
                .draw(|f| targets = render(f, &mut app, &mut focus).targets)
                .unwrap();
            let buffer = terminal.backend().buffer().clone();
            let screen = text(&buffer);
            // Names start in the same column with or without icons.
            let start = screen
                .lines()
                .find_map(|l| l.find("research/").map(|i| l[..i].chars().count()))
                .unwrap();
            assert_eq!(targets.len(), app.rows.len());
            frames.push((screen, start, targets));
        }
        let (on, on_start, on_targets) = &frames[0];
        let (off, off_start, off_targets) = &frames[1];
        assert_eq!((on_start, on_targets), (off_start, off_targets));
        assert!(on.contains("\u{f07b} research/")); // folder
        assert!(on.contains("\u{e7a8} …")); // rust, elided keeping the basename
        assert!(on.contains("tail.rs"));
        assert!(on.contains("● \u{f15c} report.tex")); // marked row
        assert!(off.contains("▸ research/") && off.contains("· todo.txt"));
        assert!(!off.contains('\u{f07b}') && off.contains("● report.tex"));
    }
    #[test]
    fn key_guide_lists_configured_normal_keys_and_dims_navigation_with_no_dir() {
        let mut app = fixture();
        app.normal_keys = crate::normal_keys::NormalKeysConfig {
            down: crate::normal_keys::Sequences::Many(vec!["j".into(), "<space>j".into()]),
            mark: crate::normal_keys::Sequences::One("none".into()),
            ..Default::default()
        }
        .build()
        .unwrap();
        let mut focus = Focus::Info {
            offset: 0,
            keys: true,
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        let screen = text(terminal.backend().buffer());
        // Two columns at this width, every group present, the footer gone.
        let row = screen.lines().find(|l| l.contains("NAVIGATE")).unwrap();
        assert!(row.contains("SEARCH"), "{screen}");
        for group in ["SELECT AND RETURN", "NORMAL MODE IN LISTS", "LOOK"] {
            assert!(screen.contains(group), "{screen}");
        }
        assert!(screen.contains("j / <space>j  k"), "{screen}");
        assert!(screen.contains("gg  G"), "{screen}");
        assert!(!screen.contains("off with --no-dir"));
        app.no_dir = true;
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert!(text(buffer).contains("crossed out: off with --no-dir"));
        let (x, y) = (0..buffer.area.height)
            .find_map(|y| {
                let line: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                line.find("Alt+l")
                    .map(|i| (line[..i].chars().count() as u16, y))
            })
            .unwrap();
        assert!(buffer[(x, y)].modifier.contains(Modifier::CROSSED_OUT));
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
