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
use std::{
    collections::HashSet,
    path::{Component, Path, PathBuf},
};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod choosers;
mod footer;
mod guide;
mod keys;
mod panes;
mod results;
mod sheets;
#[cfg(test)]
mod tests;
mod text;
use choosers::*;
use footer::*;
use guide::*;
use keys::*;
use panes::*;
use results::*;
use sheets::*;
use text::*;

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
    /// The marked list's pane, and where each of its rows was drawn with
    /// its index in the list shown.
    pub marks_pane: Rect,
    pub marks: Vec<(Rect, usize)>,
    /// The open chooser's rows (bookmarks, icons, palettes, go-to
    /// suggestions) with the index each one selects.
    pub choices: Vec<(Rect, usize)>,
}

/// Where each drawn list item went, for the mouse: items of `heights` rows
/// stacked from the top of `list`, the first one being item `first`.
fn item_areas(
    list: Rect,
    first: usize,
    heights: impl IntoIterator<Item = usize>,
) -> Vec<(Rect, usize)> {
    let mut y = list.y;
    let mut areas = Vec::new();
    for (i, height) in heights.into_iter().enumerate() {
        if y >= list.bottom() {
            break;
        }
        let height = (height as u16).min(list.bottom() - y);
        areas.push((Rect::new(list.x, y, list.width, height), first + i));
        y += height;
    }
    areas
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
/// `fff pick` and `fff seek` result label: the bare name, or the shortest
/// distinguishing trailing path when another listed entry shares it.
/// Presentation only.
fn short_label(app: &App, path: &Path) -> String {
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
    panel_with(theme, title, Vec::new(), focused, accent)
}
/// A panel whose title continues with `extra` spans in their own colors.
fn panel_with(
    theme: Palette,
    title: impl Into<String>,
    extra: Vec<Span<'static>>,
    focused: bool,
    accent: Color,
) -> Block<'static> {
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
        .title(Line::from(
            [bold(title, if focused { accent } else { theme.text })]
                .into_iter()
                .chain(extra)
                .collect::<Vec<_>>(),
        ))
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
            marks_pane: Rect::default(),
            marks: Vec::new(),
            choices: Vec::new(),
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
                if app.edit_view.as_ref().is_some_and(|v| {
                    matches!(v.field, Field::Query | Field::Contents) && v.mode.starts_with('c')
                }) {
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
    let wide = area.width >= 78;
    let marks_focused = matches!(focus, Focus::Marks { .. });
    // The preview takes the bottom half: the full width under the results and
    // marks, or between them when they are stacked. Focused marks in a
    // compact terminal take its room.
    let preview_height = if app.preview.shown && main.height >= 14 && (wide || !marks_focused) {
        main.height / 2
    } else {
        0
    };
    let (results, marks, preview) = if wide {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(3), Constraint::Length(preview_height)])
            .split(main);
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(64), Constraint::Percentage(36)])
            .spacing(1)
            .split(rows[0]);
        (columns[0], columns[1], rows[1])
    } else {
        // Compact terminals stack panes; focusing marks gives that pane more room.
        let marks_height = if marks_focused {
            main.height.saturating_sub(3)
        } else {
            4.min(main.height.saturating_sub(3 + preview_height))
        };
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(preview_height),
                Constraint::Length(marks_height),
            ])
            .split(main);
        (rows[0], rows[2], rows[1])
    };
    let mut result = results_pane(frame, results, app, matches!(focus, Focus::Results));
    result.marks = marks_pane(frame, marks, app, focus);
    result.marks_pane = marks;
    if preview.height > 0 {
        preview_pane(frame, preview, app);
    }
    if bar > 0 {
        result.buttons = button_bar(frame, layout[3], app, focus);
    }
    footer(frame, layout[4], app, focus);
    match focus {
        Focus::Info {
            offset,
            keys,
            screen,
        } => info(frame, area, app, offset, *keys, screen),
        Focus::Operation(draft) => operation_prompt(frame, main, app, draft),
        Focus::NewChoice => new_choice(frame, main, app),
        Focus::Replace(replace) => replace_choice(frame, main, app, replace),
        Focus::ScopeChoice {
            current,
            selected,
            action,
        } => scope_choice(frame, main, app, current, selected.len(), *action),
        Focus::TransferConfirm(plan) => transfer_confirmation(frame, main, app, plan),
        Focus::Command(prompt) => run_prompt(frame, main, app, prompt),
        Focus::RunConfirm(plan) => run_confirmation(frame, main, app, plan),
        Focus::DeleteConfirm(plan) => delete_confirmation(frame, main, app, &plan.rows),
        Focus::TrashConfirm(plan) => trash_confirmation(frame, main, app, plan),
        Focus::Busy(job) => progress(frame, main, app, job),
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
            (result.bookmark_targets, result.choices) = bookmarks(frame, area, app, query, *index);
            result.page = result.bookmark_targets.len();
        }
        Focus::BookmarkEdit(draft) => bookmark_form(frame, main, app, draft),
        Focus::BookmarkIcons { query, index, .. } => {
            result.choices = bookmark_icons(frame, area, app, query, *index)
        }
        Focus::BookmarkDelete { target, .. } => bookmark_delete(frame, main, app, *target),
        Focus::Palettes { query, index, .. } => {
            result.choices = palettes(frame, area, app, query, *index)
        }
        Focus::Location(location) => result.choices = location_sheet(frame, main, app, location),
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

/// "`key` does this" for the key that does it now; nothing when none does.
fn does(key: Option<String>, what: &str) -> Option<String> {
    key.map(|key| format!("{key} {what}"))
}
/// Clauses that name keys, joined into one sentence; those of keys that do
/// nothing are left out.
fn sentence(start: &str, clauses: impl IntoIterator<Item = Option<String>>) -> String {
    let clauses = clauses.into_iter().flatten().collect::<Vec<_>>();
    match (start.is_empty(), clauses.is_empty()) {
        (_, true) => format!("{start}."),
        (true, false) => format!("{}.", clauses.join(", ")),
        (false, false) => format!("{start}: {}.", clauses.join(", ")),
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
/// `1 2 3` in the results' top-right corner. The active tab wears the "@"
/// badge's colors: it is the tab showing the current directory. None
/// in `fff seek`, which cannot change directory.
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
    // In a trash, a pill after the path says so, and how to restore.
    let trash = app.in_trash().then(|| {
        let text = match app.bound(crate::keymap::Function::Restore) {
            Some(key) => format!("trash · {key} restores"),
            None => "trash".to_string(),
        };
        pill(text, theme.background, theme.marked)
    });
    // " @ " badge, a space, the path, a space (and the pill, with a space).
    let fixed = 5 + trash.as_ref().map_or(0, |p| p.width() + 1);
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
    if let Some(trash) = trash {
        first.push(Span::raw(" "));
        first.push(trash);
    }
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
    second.extend(fit_spans(
        pinned,
        width.saturating_sub(4),
        app.bound(crate::keymap::Function::Session).as_deref(),
    ));
    vec![Line::from(first), Line::from(second)]
}

/// `pinned  notes  papers`: a pin glyph (or the word) then each root's name; the
/// root that is also the current directory is bold. The Session tab shows full paths.
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

/// Keep whole root names that fit, then summarize the rest as `  +N (F1)`,
/// naming `guide`, the key that shows them all.
fn fit_spans(spans: Vec<Span<'static>>, width: usize, guide: Option<&str>) -> Vec<Span<'static>> {
    let summary = |n: usize| match guide {
        Some(key) => format!("  +{n} ({key})"),
        None => format!("  +{n}"),
    };
    let total = spans.iter().map(|s| s.width()).sum::<usize>();
    if total <= width {
        return spans;
    }
    // spans[0] is the label; each root is a (gap, name) pair.
    let roots = (spans.len() - 1) / 2;
    let mut kept = vec![spans[0].clone()];
    let mut used = spans[0].width();
    for (shown, pair) in spans[1..].chunks(2).enumerate() {
        let more = summary(roots - shown);
        let pair_width = pair.iter().map(|s| s.width()).sum::<usize>();
        if used + pair_width + summary(roots - shown - 1).width() > width {
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

/// Session state on the filter's bottom border, written like a title
/// (`✓ complete · hidden shown`): discovery progress while it runs and for a
/// moment after, then only settings that differ from the defaults. Narrow
/// frames drop trailing items and shorten the progress label; F1 lists
/// everything.
fn status_line(app: &App, width: usize) -> Line<'static> {
    let theme = app.palette;
    let (scan, color) = if app.cache_pending && app.scanning && !app.quiet_scan {
        (["◌ cached, refreshing", "◌ incomplete"], theme.marked)
    } else if app.scanning && !app.quiet_scan {
        (["◌ scanning, incomplete", "◌ incomplete"], theme.marked)
    } else if app.ranking_pending() {
        (["◌ sorting", "◌ sorting"], theme.marked)
    } else if app.truncated {
        (["◌ partial: max paths", "◌ partial"], theme.marked)
    } else {
        (["✓ complete"; 2], theme.success)
    };
    let progress = app.progress_shown(std::time::Instant::now());
    let mut optional = Vec::new();
    // Kept longest: until fixed, the configuration's backup is in use (F1).
    if app.config_broken.is_some() {
        optional.push(styled("config errors", theme.error));
    }
    // Filters and exclusions are only named while they hide something; their
    // globs are listed in the editors (Alt+f, Alt+e) and in F1's details.
    if app.policy.filtering() {
        optional.push(styled("filters on", theme.accent));
    }
    if app.policy.hidden {
        optional.push(styled("hidden shown", theme.accent));
    }
    if app.policy.ignore && !app.policy.rules.is_empty() {
        optional.push(styled("exclusions on", theme.accent));
    }
    let mut keep = optional.len();
    let mut label = 0;
    loop {
        let items: Vec<_> = progress
            .then(|| styled(scan[label], color))
            .into_iter()
            .chain(optional[..keep].iter().cloned())
            .collect();
        if items.is_empty() {
            return Line::default();
        }
        let mut spans = vec![Span::raw(" ")];
        for (i, item) in items.into_iter().enumerate() {
            if i > 0 {
                spans.push(styled(" · ", theme.muted));
            }
            spans.push(item);
        }
        spans.push(Span::raw(" "));
        let used = spans.iter().map(|s| s.width()).sum::<usize>();
        if used <= width || (keep == 0 && (label == 1 || !progress)) {
            return Line::from(spans);
        }
        if keep > 0 {
            keep -= 1;
        } else {
            label += 1;
        }
    }
}
fn query(frame: &mut Frame, area: Rect, app: &App, focused: bool) {
    let theme = app.palette;
    let status = status_line(app, area.width.saturating_sub(4) as usize).right_aligned();
    // fff grep: the contents query, wider, beside the names query; the one
    // typed into has the accent border and the cursor.
    if let Some(grep) = &app.grep {
        let halves = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(area);
        let mut summary = vec![Span::raw(" ")];
        if let Some((done, total)) = grep.progress() {
            summary.push(styled(format!("searching {done}/{total}"), theme.marked));
        } else if grep.active() {
            let (matches, files) = grep.summary();
            summary.push(styled(
                format!(
                    "{matches} match{} in {files} file{}",
                    if matches == 1 { "" } else { "es" },
                    if files == 1 { "" } else { "s" }
                ),
                theme.muted,
            ));
        }
        for tool in &grep.missing {
            summary.push(styled(" · ", theme.muted));
            summary.push(styled(format!("{tool} not installed"), theme.error));
        }
        summary.push(Span::raw(" "));
        let summary = Line::from(summary);
        field_box(
            frame,
            halves[0],
            app,
            (Field::Contents, "Contents", &grep.text),
            "type to search contents…".into(),
            focused && grep.editing,
            vec![summary],
        );
        field_box(
            frame,
            halves[1],
            app,
            (Field::Query, "Names", &app.query),
            "type to filter names…".into(),
            focused && !grep.editing,
            vec![status],
        );
        return;
    }
    let title = app
        .dialog
        .as_ref()
        .map_or_else(|| "Query".into(), |d| d.title(&app.query));
    let hint = match &app.dialog {
        Some(dialog) => dialog.hint(app.edit_view.as_ref().is_some_and(|v| !v.normal()), app),
        None => "type to filter names…".into(),
    };
    field_box(
        frame,
        area,
        app,
        (Field::Query, &title, &app.query),
        hint,
        focused,
        vec![status],
    );
}
/// A query's box: its title and mode, its text (or `hint` while empty),
/// the cursor when `focused`, and `bottom` in the lower border.
fn field_box(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    (field, title, text): (Field, &str, &str),
    hint: String,
    focused: bool,
    bottom: Vec<Line<'static>>,
) {
    let theme = app.palette;
    let mut block = panel(
        app.palette,
        mode_title(app, field, title),
        focused,
        theme.accent,
    )
    .padding(Padding::horizontal(1));
    for line in bottom {
        block = block.title_bottom(line);
    }
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let prefix = "❯ ";
    let available = inner.width.saturating_sub(prefix.width() as u16) as usize;
    let (mut line, cursor) = editable_line(app, field, text, 0, available);
    if text.is_empty() {
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
            field,
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
}
