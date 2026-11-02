//! Terminal presentation. Rendering returns the exact numbered targets in a frame.
use crate::{App, Candidate, config::Palette, details::Column, editor::Field, ui::Focus};
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

mod choosers;
mod guide;
mod sheets;
#[cfg(test)]
mod tests;
use choosers::*;
use guide::*;
use sheets::*;

pub(crate) struct Rendered {
    pub targets: Vec<PathBuf>,
    pub bookmark_targets: Vec<usize>,
    pub page: usize,
    /// Where the result rows were drawn, for mouse clicks; one row per target.
    pub results: Rect,
    /// The button bar's buttons and where they were drawn.
    pub buttons: Vec<(Rect, crate::ui::Button)>,
    /// Where each tab's number was drawn in the tab strip.
    pub tabs: Vec<(Rect, usize)>,
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
/// `fff seek` result label: the bare name, or the shortest distinguishing
/// trailing path when another listed file shares it. Presentation only.
fn no_dir_label(app: &App, path: &Path) -> String {
    match app.labels.get(path) {
        Some(label) => safe(label.display().to_string()),
        None => safe(path.file_name().unwrap_or_default().to_string_lossy()),
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
            bookmark_targets: Vec::new(),
            page: 0,
            results: Rect::default(),
            buttons: Vec::new(),
            tabs: Vec::new(),
        };
    }
    let area = Rect::new(
        area.x + 1,
        area.y,
        area.width.saturating_sub(2),
        area.height,
    );
    let location = location_lines(app, area.width as usize);
    let bar = u16::from(app.buttons_shown() && crate::ui::Button::sets(focus).is_some());
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(
                if app
                    .edit_view
                    .as_ref()
                    .is_some_and(|v| v.field == Field::Query && v.mode.starts_with('c'))
                {
                    4
                } else {
                    3
                },
            ),
            Constraint::Length(location.len() as u16),
            Constraint::Min(3),
            Constraint::Length(bar),
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
    let mut result = results_pane(frame, results, app, matches!(focus, Focus::Results));
    marks_pane(frame, marks, app, focus);
    if bar > 0 {
        result.buttons = button_bar(frame, layout[3], app, focus);
    }
    footer(frame, layout[4], app, focus);
    match focus {
        Focus::Info { offset, keys } => info(frame, area, app, offset, *keys),
        Focus::Operation(draft) => operation_prompt(frame, main, app, draft),
        Focus::NewChoice => new_choice(frame, main, app),
        Focus::Replace(target) => replace_choice(frame, main, app, target),
        Focus::ScopeChoice {
            current,
            selected,
            action,
        } => scope_choice(frame, main, app, current, selected.len(), *action),
        Focus::TransferConfirm(plan) => transfer_confirmation(frame, main, app, plan),
        Focus::Command(prompt) => run_prompt(frame, main, app, prompt),
        Focus::RunConfirm(plan) => run_confirmation(frame, main, app, plan),
        Focus::DeleteConfirm(plan) => delete_confirmation(frame, main, app, &plan.rows),
        Focus::Exclusions(text) => prompt(
            frame,
            app,
            area,
            Field::Exclusions,
            "Edit exclusions",
            text,
            true,
        ),
        Focus::Filters(text) => {
            prompt(frame, app, area, Field::Filters, "Edit filters", text, true)
        }
        Focus::Bookmarks { query, index } => {
            result.bookmark_targets = bookmarks(frame, area, app, query, *index);
            result.page = result.bookmark_targets.len();
        }
        Focus::BookmarkEdit(draft) => bookmark_form(frame, main, app, draft),
        Focus::BookmarkIcons { query, index, .. } => {
            bookmark_icons(frame, area, app, query, *index)
        }
        Focus::BookmarkDelete { target, .. } => bookmark_delete(frame, main, app, *target),
        Focus::Palettes { query, index, .. } => palettes(frame, area, app, query, *index),
        Focus::Location(location) => location_sheet(frame, main, app, location),
        _ => {}
    }
    notices(frame, main, app);
    result
}
/// Pop-up notifications (e.g. a program opened in the background failed),
/// newest first at the top right of the panes, a few at a time.
fn notices(frame: &mut Frame, main: Rect, app: &App) {
    let theme = app.palette;
    let width = main.width.saturating_sub(4).min(60);
    let mut y = main.y;
    for (text, _) in app.notices.iter().rev().take(3) {
        let lines = wrap_text(&safe(text), width.saturating_sub(4) as usize);
        let height = (lines.len() as u16 + 2).min(main.bottom().saturating_sub(y));
        if height < 3 || width < 10 {
            break;
        }
        let area = Rect::new(main.right().saturating_sub(width + 1), y, width, height);
        let block = panel(theme, " Notice ", true, theme.error).padding(Padding::horizontal(1));
        let inner = block.inner(area);
        frame.render_widget(Clear, area);
        frame.render_widget(block, area);
        frame.render_widget(
            Paragraph::new(
                lines
                    .into_iter()
                    .map(|l| Line::from(styled(l, theme.text)))
                    .collect::<Vec<_>>(),
            ),
            inner,
        );
        y += height;
    }
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

/// `1 2 3` in the results' top-right corner. The active tab wears the "@"
/// badge's colors: it is the tab showing the current directory. None
/// in `fff seek`, which cannot change directory.
/// Each tab number's cell in the strip `tab_strip` draws right-aligned in the
/// top border of `area`: a space, three-column numbers, a space, the corner.
fn tab_areas(app: &App, area: Rect) -> Vec<(Rect, usize)> {
    let width = 3 * crate::TABS as u16 + 2;
    if app.no_dir || area.width < width + 2 {
        return Vec::new();
    }
    let start = area.x + area.width - 1 - width + 1;
    (0..crate::TABS)
        .map(|i| (Rect::new(start + 3 * i as u16, area.y, 3, 1), i))
        .collect()
}
fn tab_strip(app: &App) -> Line<'static> {
    let theme = app.palette;
    if app.no_dir {
        return Line::default();
    }
    let active = Style::default()
        .fg(theme.current_directory_text)
        .bg(theme.current_directory)
        .add_modifier(Modifier::BOLD);
    let mut spans = vec![Span::raw(" ")];
    for i in 0..crate::TABS {
        let label = format!(" {} ", i + 1);
        spans.push(if i == app.tab {
            Span::styled(label, active)
        } else {
            styled(label, theme.muted)
        });
    }
    spans.push(Span::raw(" "));
    Line::from(spans).right_aligned()
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
    // Filters and exclusions only get a pill while they hide something; their
    // globs are listed in the editors (Alt+f, Alt+e) and in F1's details.
    if !app.policy.filters.is_empty() {
        optional.push(pill("filters on", theme.accent, bg));
    }
    if app.no_dir || app.files_only {
        optional.push(pill("files only", theme.accent, bg));
    }
    if app.policy.hidden {
        optional.push(pill("hidden shown", theme.accent, bg));
    }
    if app.policy.ignore && !app.policy.rules.is_empty() {
        optional.push(pill("exclusions on", theme.accent, bg));
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
    let column = view
        .filter(|v| v.row == row)
        .map(|v| v.column)
        .unwrap_or(text.len());
    text_line(theme, view, text, row, width, column)
}
fn text_line(
    theme: Palette,
    view: Option<&crate::editor::EditView>,
    text: &str,
    row: usize,
    width: usize,
    column: usize,
) -> (Line<'static>, u16) {
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
/// Render command entry below the field without hiding its buffer contents.
fn command_line(frame: &mut Frame, app: &App, field: Field, area: Rect) {
    let Some(view) = app
        .edit_view
        .as_ref()
        .filter(|v| v.field == field && v.mode.starts_with('c'))
    else {
        return;
    };
    if area.width == 0 || area.height == 0 {
        return;
    }
    let (line, cursor) = text_line(
        app.palette,
        None,
        &view.command,
        0,
        area.width as usize,
        view.command_column,
    );
    frame.render_widget(Paragraph::new(line), area);
    frame.set_cursor_position((area.x + cursor, area.y));
}
fn query(frame: &mut Frame, area: Rect, app: &App, focused: bool) {
    let theme = app.palette;
    let block = panel(
        app.palette,
        mode_title(
            app,
            Field::Query,
            &app.dialog
                .as_ref()
                .map_or_else(|| "Query".into(), |d| d.title(&app.query)),
        ),
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
    if app.query.is_empty() {
        let hint = match &app.dialog {
            Some(dialog) => dialog.hint(app.edit_view.as_ref().is_some_and(|v| !v.normal())),
            None => "type to filter names…".into(),
        };
        line = Line::from(styled(hint, theme.muted));
    }
    line.spans.insert(0, bold(prefix, theme.accent));
    frame.render_widget(Paragraph::new(line), inner);
    if focused && inner.width > 0 {
        frame.set_cursor_position((inner.x + prefix.width() as u16 + cursor, inner.y));
    }
    if focused && inner.height > 1 {
        command_line(
            frame,
            app,
            Field::Query,
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
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
    if app.scrolled.as_ref().is_some_and(|at| *at != app.highlight) {
        app.scrolled = None;
    }
    if page > 0
        && app.scrolled.is_none()
        && let Some(i) = app.highlighted()
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
        block
            .title_top(tab_strip(app))
            .title_bottom(Line::from(styled(range, theme.muted)).right_aligned()),
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
        } else if app.files_only && app.query.is_empty() {
            (
                "No files here",
                "Directories are hidden: Alt+d shows them, Alt+→ searches deeper.",
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
        let columns = fitting_columns(&app.columns, inner.width as usize);
        let now = std::time::SystemTime::now();
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
            let mut name = if app.no_dir {
                no_dir_label(app, &c.path)
            } else {
                display_path(&c.path, &app.cwd)
            };
            if c.directory && !name.ends_with('/') {
                name.push('/');
            }
            let prefix = format!("{} {:>3} ", if selected { "›" } else { " " }, label);
            let marked = app.marks.contains(&c.path);
            let info = app.details.info(&c.path);
            // Labels are relative to the current directory (or, in fff seek,
            // as short as a shared name allows); the footer and F1 show the rest.
            let (kind, group) = crate::icons::marker(&c.path, c.directory, app.icons);
            let fixed = (prefix.width() + "● ".width() + kind.width()) as u16;
            let room = inner.width.saturating_sub(fixed) as usize;
            let meta = row_details(app, c, info.as_ref(), &columns, now);
            let available = room.saturating_sub(meta.width());
            let name = elide(&name, available);
            let mut spans = vec![
                styled(prefix, if selected { theme.accent } else { theme.muted }),
                styled(if marked { "● " } else { "  " }, theme.marked),
                styled(kind, icon_color(theme, group)),
            ];
            let color = info_color(theme, info.as_ref(), c.directory);
            spans.extend(filename_spans(app.palette, &name, c, &app.query, color));
            let mut used = name.width();
            // A link's target follows its name while there is room for it.
            if let Some((target, _)) = info.as_ref().and_then(|i| i.link.as_ref()) {
                let arrow = " → ";
                let space = available.saturating_sub(used + arrow.width() + 1);
                if space >= 3 {
                    let target = elide(&safe(target.to_string_lossy()), space);
                    used += arrow.width() + target.width();
                    spans.push(styled(arrow, color));
                    spans.push(styled(target, theme.muted));
                }
            }
            let gap = available.saturating_sub(used);
            spans.push(styled(" ".repeat(gap) + &meta, theme.muted));
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(bg)),
                row,
            );
        }
    }
    Rendered {
        targets: visible.iter().map(|c| c.path.clone()).collect(),
        bookmark_targets: Vec::new(),
        page,
        results: inner,
        buttons: Vec::new(),
        tabs: tab_areas(app, area),
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
/// Broken links, links, then directories, empty files and executables get
/// their own roles. Only visible rows are inspected, so discovery stays free of
/// extra stats.
fn info_color(theme: Palette, info: Option<&crate::details::Info>, directory: bool) -> Color {
    let file = |i: &crate::details::Info| i.mode & libc::S_IFMT == libc::S_IFREG;
    match info {
        Some(i) if i.broken() => theme.error,
        Some(i) if i.link.is_some() => theme.link,
        _ if directory => theme.accent,
        Some(i) if file(i) && i.size == 0 => theme.empty,
        Some(i) if file(i) && i.mode & 0o111 != 0 => theme.executable,
        _ => theme.text,
    }
}
fn path_color(theme: Palette, path: &Path, directory: bool) -> Color {
    info_color(theme, crate::details::Info::read(path).as_ref(), directory)
}
/// Each column's width, so that columns line up from row to row.
fn column_width(column: Column) -> usize {
    match column {
        Column::Permissions => 10,
        Column::Size => 5,
        Column::Modified => 4,
    }
}
/// Names come first: columns are dropped from the left until a name has at
/// least `NAME_ROOM` columns.
const NAME_ROOM: usize = 20;
fn fitting_columns(columns: &[Column], width: usize) -> Vec<Column> {
    let mut columns = columns.to_vec();
    let total = |c: &[Column]| c.iter().map(|&c| column_width(c) + 2).sum::<usize>();
    // The row prefix, mark and icon take about 10 columns.
    while !columns.is_empty() && width.saturating_sub(10 + total(&columns)) < NAME_ROOM {
        columns.remove(0);
    }
    columns
}
/// The right-hand details of a result row, each column right-aligned, with a
/// trailing space; blank where a value is unknown (yet).
fn row_details(
    app: &App,
    candidate: &Candidate,
    info: Option<&crate::details::Info>,
    columns: &[Column],
    now: std::time::SystemTime,
) -> String {
    use crate::details;
    if columns.is_empty() {
        return " ".into();
    }
    let mut text = String::new();
    for &column in columns {
        let value = match (column, info) {
            (_, None) => String::new(),
            (Column::Permissions, Some(i)) => details::permissions(i.mode),
            (Column::Size, Some(i)) if candidate.directory || i.directory => app
                .details
                .count(&candidate.path)
                .map(details::entries)
                .unwrap_or_default(),
            (Column::Size, Some(i)) if i.broken() => String::new(),
            (Column::Size, Some(i)) => details::size(i.size),
            (Column::Modified, Some(i)) => {
                i.modified.map(|m| details::age(m, now)).unwrap_or_default()
            }
        };
        text.push_str(&format!("  {value:>0$}", column_width(column)));
    }
    text.push(' ');
    text
}
fn filename_spans(
    theme: Palette,
    label: &str,
    candidate: &Candidate,
    query: &str,
    name: Color,
) -> Vec<Span<'static>> {
    let basename = candidate
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let start = label
        .len()
        .saturating_sub(safe(&basename).len() + usize::from(candidate.directory));
    // Only a name the query matches is highlighted (a save dialog lists the
    // others too).
    let query = if crate::score(&basename, query).is_some() {
        query
    } else {
        ""
    };
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
                } else {
                    name
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
        _ => (&app.marks[..], None),
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
    let item = |path: &PathBuf| {
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
    };
    // Build only the items on screen, the window List picks with a fresh
    // state: from the top while the selection fits, otherwise ending at it.
    let height = inner.height as usize;
    let selected = index.filter(|&i| i < rows.len());
    let mut first = selected.unwrap_or(0);
    let mut items = vec![item(&rows[first])];
    let mut used = items[0].height();
    while first > 0 {
        let previous = item(&rows[first - 1]);
        if used + previous.height() > height {
            break;
        }
        used += previous.height();
        items.push(previous);
        first -= 1;
    }
    items.reverse();
    if first == 0 {
        for path in &rows[items.len()..] {
            if used >= height {
                break;
            }
            let next = item(path);
            used += next.height();
            items.push(next);
        }
    }
    let mut state = ListState::default().with_selected(selected.map(|i| i - first));
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
    // How to leave; q is text while typing.
    let typing = app.edit_view.as_ref().is_some_and(|v| !v.normal());
    let quit = if typing { "Ctrl+c" } else { "q" };
    let help = vec![
        bold("F1", theme.accent),
        styled(" keys ", theme.muted),
        Span::raw(" "),
        bold(quit, theme.accent),
        styled(" quit ", theme.muted),
    ];
    let help_width = help.iter().map(|s| s.width()).sum::<usize>();
    let available = (area.width as usize).saturating_sub(help_width + 2);
    let mut spans = if app.status.is_empty() {
        let path = selected
            .map(|p| display_path(p, &app.cwd))
            .unwrap_or_else(|| ".".into());
        // The path keeps the room it needs, up to `NAME_ROOM`; details fill
        // the rest, the least important dropped first.
        let mut details = selected.map(|p| footer_details(app, p)).unwrap_or_default();
        let wanted = path.width().min(NAME_ROOM);
        let width = |d: &[Span]| d.iter().map(Span::width).sum::<usize>();
        while !details.is_empty() && available.saturating_sub(width(&details)) < wanted {
            details.pop();
            details.pop();
        }
        let room = available.saturating_sub(width(&details));
        let path = elide(&path, room);
        let split = path.rfind('/').map(|i| i + 1).unwrap_or(0);
        let mut spans = vec![
            Span::raw(" "),
            styled(path[..split].to_string(), theme.muted),
            styled(path[split..].to_string(), theme.text),
        ];
        spans.extend(details);
        spans
    } else if app.status == app.status_success {
        vec![
            Span::raw(" "),
            styled(
                truncate(&format!("✓ {}", safe(&app.status)), available),
                theme.success,
            ),
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
/// The button bar: in the results, navigation and marking on the left, Cancel
/// and Select & Exit on the right; in a confirmation, Cancel and Confirm. Left
/// buttons that do not fit are left out.
fn button_bar(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    focus: &Focus,
) -> Vec<(Rect, crate::ui::Button)> {
    use crate::ui::Button;
    let Some((left_set, right_set)) = Button::sets(focus) else {
        return Vec::new();
    };
    let theme = app.palette;
    let width = |b: Button| b.label(app).width() as u16 + 2;
    let right_width: u16 = right_set.iter().map(|&b| width(b) + 1).sum();
    let mut left = Vec::new();
    let mut used = 0;
    for &button in left_set {
        if used + width(button) + 1 + right_width > area.width {
            break;
        }
        left.push(button);
        used += width(button) + 1;
    }
    let mut placed = Vec::new();
    let mut x = area.x;
    for button in left {
        placed.push((Rect::new(x, area.y, width(button), 1), button));
        x += width(button) + 1;
    }
    let mut x = (area.x + area.width).saturating_sub(right_width.saturating_sub(1));
    for &button in right_set {
        placed.push((Rect::new(x, area.y, width(button), 1), button));
        x += width(button) + 1;
    }
    for &(rect, button) in &placed {
        let (fg, bg) = if !button.available(app) {
            (theme.muted, theme.panel)
        } else if matches!(button, Button::Select | Button::Confirm) {
            (theme.background, theme.accent)
        } else {
            (theme.text, theme.selection)
        };
        frame.render_widget(
            Paragraph::new(Line::from(pill(button.label(app), fg, bg))),
            rect,
        );
    }
    placed
}
/// The footer's details of `path`, as (gap, value) span pairs in order of
/// importance: a link's target, size or entry count, modification time,
/// permissions and owner.
fn footer_details(app: &App, path: &Path) -> Vec<Span<'static>> {
    use crate::details;
    let theme = app.palette;
    let Some(info) = app.details.info(path) else {
        return Vec::new();
    };
    let mut values = Vec::new();
    if let Some((target, resolves)) = &info.link {
        let target = format!("→ {}", safe(target.to_string_lossy()));
        values.push(if *resolves {
            styled(target, theme.link)
        } else {
            styled(target + " (broken)", theme.error)
        });
    }
    if info.directory {
        if let Some(count) = app.details.count(path) {
            let text = match count {
                details::Count::Entries(1) => "1 entry".into(),
                details::Count::Entries(_) => format!("{} entries", details::entries(count)),
                details::Count::Unreadable => "unreadable".into(),
            };
            values.push(styled(text, theme.muted));
        }
    } else if info.link.is_none() {
        let exact = details::exact(info.size);
        values.push(styled(
            if info.size < 1000 {
                format!("{exact} B")
            } else {
                format!("{} ({exact} B)", details::size(info.size))
            },
            theme.muted,
        ));
    }
    if let Some(modified) = info.modified {
        values.push(styled(details::timestamp(modified), theme.muted));
    }
    values.push(styled(details::permissions(info.mode), theme.muted));
    values.push(styled(app.details.owner(&info), theme.muted));
    values
        .into_iter()
        .flat_map(|value| [Span::raw("  "), value])
        .collect()
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
    let hint: String = if multiline {
        let what = if field == Field::Filters {
            "Filters: one glob per line; files must match one"
        } else {
            "Exclusions: one glob per line"
        };
        if modal {
            format!("{what}; write saves; close discards")
        } else {
            format!("{what}; Ctrl+s saves; Esc discards")
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
    let view = app.edit_view.as_ref().filter(|v| v.field == field);
    let commanding = view.is_some_and(|v| v.mode.starts_with('c'));
    let editor = Rect::new(
        inner.x,
        inner.y + 2,
        inner.width,
        inner.height.saturating_sub(3 + u16::from(commanding)),
    );
    edit_rows(frame, app, field, text, editor);
    if commanding {
        command_line(
            frame,
            app,
            field,
            Rect::new(inner.x, editor.bottom(), inner.width, 1),
        );
    }
    if !status.is_empty() {
        frame.render_widget(
            Paragraph::new(styled(safe(status), theme.error)),
            Rect::new(inner.x, inner.bottom().saturating_sub(1), inner.width, 1),
        );
    }
}
/// Buffer rows scrolled to keep the cursor visible, with the terminal cursor placed.
fn edit_rows(frame: &mut Frame, app: &App, field: Field, text: &str, editor: Rect) {
    let view = app.edit_view.as_ref().filter(|v| v.field == field);
    let rows = text.split('\n').collect::<Vec<_>>();
    let cursor_row = view.map(|v| v.row).unwrap_or(rows.len() - 1);
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
}
