//! The panes beside the results: the marked list and the preview.
use super::*;

pub(super) fn marks_pane(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    focus: &Focus,
) -> Vec<(Rect, usize)> {
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
                Line::from(styled(
                    app.bound(crate::keymap::Function::ToggleMark)
                        .map(|key| format!("{key} marks the highlighted entry."))
                        .unwrap_or_default(),
                    theme.muted,
                )),
                Line::from(""),
                Line::from(styled(
                    match app.bound(crate::keymap::Function::PrintMarks) {
                        Some(key) if app.returns_marks() => format!("{key} returns your marks."),
                        _ => "File operations act on them.".into(),
                    },
                    theme.muted,
                )),
            ])
            .wrap(Wrap { trim: false })
            .alignment(ratatui::layout::Alignment::Center),
            inner,
        );
        return Vec::new();
    }
    let item = |path: &PathBuf| {
        // From the details cache: drawing never waits for the filesystem.
        let found = app.details.lookup(path);
        let missing = matches!(&found, Some(None)) || found.iter().flatten().any(|i| i.broken());
        let icon = if app.icons {
            let directory = found.flatten().is_some_and(|i| i.directory);
            crate::icons::icon(path, directory).cell()
        } else {
            String::new()
        };
        let prefix = format!(
            "{} {} {icon}",
            if index.and_then(|i| rows.get(i)) == Some(path) {
                "›"
            } else {
                " "
            },
            if app.marks.contains(path) {
                "●"
            } else {
                "○"
            }
        );
        let suffix = if missing { " [MISSING]" } else { "" };
        // A long name keeps its end (the extension) in view, as rows do.
        let name = elide(
            &safe(
                path.file_name()
                    .unwrap_or(path.as_os_str())
                    .to_string_lossy(),
            ),
            (inner.width as usize).saturating_sub(prefix.width() + suffix.width()),
        );
        let title = format!("{prefix}{name}{suffix}");
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
        // A blank line between marks, where the pane has room for several:
        // a compact one shows marks rather than gaps.
        if inner.height >= 8 {
            lines.push(Line::from(""));
        }
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
    let heights: Vec<usize> = items.iter().map(ListItem::height).collect();
    let mut state = ListState::default().with_selected(selected.map(|i| i - first));
    frame.render_stateful_widget(
        List::new(items).highlight_style(Style::default().bg(theme.selection)),
        inner,
        &mut state,
    );
    let shown = state.offset();
    item_areas(inner, first + shown, heights.into_iter().skip(shown))
}
/// The preview pane: what the followed entry is in the title, its facts, then
/// its first lines or entries, each cut to the pane's width.
pub(super) fn preview_pane(frame: &mut Frame, area: Rect, app: &App) {
    use crate::preview::Tone;
    let theme = app.palette;
    if let Some(i) = app.highlighted()
        && let Some(hit) = app.hit(i)
    {
        return match_preview(frame, area, app, i, hit);
    }
    let current = app.preview.current();
    let title = match current {
        Some(p) if !p.kind.is_empty() => format!(" Preview · {} ", safe(&p.kind)),
        _ => " Preview ".into(),
    };
    let block = panel(theme, title, false, theme.accent).padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width as usize;
    let mut lines = Vec::new();
    match current {
        None if app.preview.followed().is_none() => {
            lines.push(Line::from(styled("Nothing highlighted.", theme.muted)))
        }
        // Still reading: nothing rather than the previous entry's content.
        None => {}
        Some(preview) => {
            let label_width = preview
                .facts
                .iter()
                .map(|(l, _)| l.width())
                .max()
                .unwrap_or(0);
            for (label, value) in &preview.facts {
                let label = format!("{label:<label_width$}  ");
                let room = width.saturating_sub(label.width());
                lines.push(Line::from(vec![
                    styled(label, theme.muted),
                    styled(truncate(value, room), theme.text),
                ]));
            }
            // A rule sets what the entry is apart from what it holds.
            if !preview.facts.is_empty() && !preview.lines.is_empty() {
                lines.push(Line::from(styled("─".repeat(width), theme.border)));
            }
            // Rows, wrapped or cut: scrolling counts these.
            let rows: Vec<Line> = preview
                .lines
                .iter()
                .enumerate()
                .flat_map(|(i, (text, tone))| {
                    let color = match tone {
                        Tone::Text => theme.text,
                        Tone::Muted => theme.muted,
                        Tone::Directory => theme.accent,
                    };
                    let spans = match preview.syntax.get(i) {
                        Some(spans) if app.preview.highlight && !spans.is_empty() => {
                            colored(text, spans, theme)
                        }
                        _ => vec![styled(text.clone(), color)],
                    };
                    fit(spans, width, app.preview.wrap)
                })
                .collect();
            let room = (inner.height as usize).saturating_sub(lines.len());
            let most = rows.len().saturating_sub(room);
            app.preview.room.set((room, most));
            let skip = app.preview.scrolled().min(most);
            lines.extend(rows.into_iter().skip(skip).take(room));
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
/// A contents query: the lines around the highlighted row's match, the match
/// highlighted on each line it is on, its page or line and which of the
/// file's matches it is in the title.
fn match_preview(frame: &mut Frame, area: Rect, app: &App, row: usize, hit: &crate::grep::Hit) {
    let theme = app.palette;
    let (nth, of) = app
        .grep
        .shown_hits
        .get(row)
        .map_or((1, 1), |(hits, at)| (at + 1, hits.len()));
    let title = format!(" Preview · {} · match {nth} of {of} ", hit.location());
    let block = panel(theme, title, false, theme.accent).padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let (width, room) = (inner.width as usize, inner.height as usize);
    let wrap = app.preview.wrap;
    let mut rows = Vec::new();
    let mut start = 0;
    // The file's lines around the context, once read: up to `AROUND`
    // lines either side of the match, so the preview scrolls past it.
    const AROUND: usize = 500;
    let file = app
        .match_text
        .as_ref()
        .filter(|(path, _)| Some(path) == app.highlight.as_ref())
        .and_then(|(_, lines)| {
            let own = lines.starts.binary_search(&hit.start).ok()?;
            let first = own.checked_sub(hit.at)?;
            Some((lines, first))
        });
    let push_plain = |rows: &mut Vec<Line<'static>>, line: &str| {
        let spans = vec![styled(safe(line), theme.muted)];
        for mut row in fit(spans, width.saturating_sub(2), wrap) {
            row.spans.insert(0, Span::raw("  "));
            rows.push(row);
        }
    };
    if let Some((lines, first)) = file {
        for line in &lines.text[first.saturating_sub(AROUND)..first] {
            push_plain(&mut rows, line);
        }
    }
    for (i, line) in hit.context.iter().enumerate() {
        let parts = hit.parts(i);
        if i == hit.at {
            start = rows.len();
        }
        let room = width.saturating_sub(2);
        let spans = if parts.is_empty() {
            vec![styled(safe(line), theme.muted)]
        } else if wrap {
            marked(line, &parts, theme.text, theme)
        } else {
            // Cut, the line keeps its match in view.
            let (line, parts) = lead_window(line, &parts, room);
            marked(&line, &parts, theme.text, theme)
        };
        // The match's line is marked; wrapped rows line up under the text.
        for (n, mut row) in fit(spans, room, wrap).into_iter().enumerate() {
            let gutter = if i == hit.at && n == 0 {
                styled("› ", theme.accent)
            } else {
                Span::raw("  ")
            };
            row.spans.insert(0, gutter);
            rows.push(row);
        }
    }
    if let Some((lines, first)) = file {
        let after = first + hit.context.len();
        let end = (after + AROUND).min(lines.text.len());
        for line in lines.text.get(after..end).unwrap_or_default() {
            push_plain(&mut rows, line);
        }
    }
    // The match a third of the way down, its lines around it, until the
    // preview is scrolled from there.
    let most = rows.len().saturating_sub(room);
    let first = app
        .preview
        .match_top
        .get()
        .unwrap_or_else(|| start.saturating_sub(room / 3))
        .min(most);
    app.preview.match_top.set(Some(first));
    app.preview.room.set((room, most));
    let lines: Vec<Line> = rows.into_iter().skip(first).take(room).collect();
    frame.render_widget(Paragraph::new(lines), inner);
}
/// `text` with `parts` (sorted byte ranges) highlighted, the rest in
/// `color`; parts off a character boundary are left plain.
pub(super) fn marked(
    text: &str,
    parts: &[std::ops::Range<usize>],
    color: Color,
    theme: Palette,
) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut at = 0;
    for part in parts {
        let (Some(before), Some(found)) = (text.get(at..part.start), text.get(part.clone())) else {
            continue;
        };
        if !before.is_empty() {
            out.push(styled(safe(before), color));
        }
        if !found.is_empty() {
            out.push(styled(safe(found), theme.marked).add_modifier(Modifier::BOLD));
        }
        at = part.end;
    }
    if let Some(rest) = text.get(at..)
        && !rest.is_empty()
    {
        out.push(styled(safe(rest), color));
    }
    out
}
/// `text` cut at the front so that its first part shows within `width`
/// columns, a quarter of them kept before it: `…` and the rest, with the
/// parts moved along.
pub(super) fn lead_window(
    text: &str,
    parts: &[std::ops::Range<usize>],
    width: usize,
) -> (String, Vec<std::ops::Range<usize>>) {
    let Some(first) = parts
        .first()
        .filter(|p| text.is_char_boundary(p.start.min(text.len())))
    else {
        return (text.to_owned(), parts.to_vec());
    };
    let before = &text[..first.start.min(text.len())];
    let lead = width / 4;
    if before.width() <= lead {
        return (text.to_owned(), parts.to_vec());
    }
    // Keep the last `lead - 1` columns before the match, after an ellipsis.
    let mut cut = before.len();
    let mut used = 0;
    for (i, c) in before.char_indices().rev() {
        used += c.width().unwrap_or(0);
        if used + 1 > lead {
            break;
        }
        cut = i;
    }
    // The parts lie past `cut`, which may be nearer the start than the
    // ellipsis is long.
    let moved = |at: usize| at.saturating_sub(cut) + '…'.len_utf8();
    let moved = parts
        .iter()
        .map(|p| moved(p.start)..moved(p.end))
        .collect();
    (format!("…{}", &text[cut..]), moved)
}
/// Spans laid out in `width` columns: word-wrapped over as many lines as
/// they need (breaking within a word only when it fills a line), or with
/// `wrap` off cut to one line with an ellipsis.
pub(super) fn fit(spans: Vec<Span<'static>>, width: usize, wrap: bool) -> Vec<Line<'static>> {
    let total: usize = spans.iter().map(|s| s.width()).sum();
    if total <= width {
        return vec![Line::from(spans)];
    }
    if width == 0 {
        return vec![Line::default()];
    }
    let cells: Vec<(char, Style)> = spans
        .iter()
        .flat_map(|s| s.content.chars().map(move |c| (c, s.style)))
        .collect();
    let line = |cells: &[(char, Style)]| {
        let mut out: Vec<Span<'static>> = Vec::new();
        for &(c, style) in cells {
            match out.last_mut() {
                Some(last) if last.style == style => last.content.to_mut().push(c),
                _ => out.push(Span::styled(c.to_string(), style)),
            }
        }
        Line::from(out)
    };
    if !wrap {
        let mut used = 0;
        let mut kept = Vec::new();
        for &(c, style) in &cells {
            let w = c.width().unwrap_or(0);
            if used + w + 1 > width {
                kept.push(('…', style));
                break;
            }
            used += w;
            kept.push((c, style));
        }
        return vec![line(&kept)];
    }
    let mut rows = Vec::new();
    let mut row: Vec<(char, Style)> = Vec::new();
    let mut used = 0;
    for (c, style) in cells {
        let w = c.width().unwrap_or(0);
        if used + w > width && !row.is_empty() {
            // Break after the row's last space, when it has one.
            let space = row.iter().rposition(|(c, _)| *c == ' ').filter(|&i| i > 0);
            let rest = match space {
                Some(i) => {
                    let rest = row.split_off(i + 1);
                    row.pop();
                    rest
                }
                None => Vec::new(),
            };
            rows.push(line(&row));
            row = rest;
            used = row.iter().map(|(c, _)| c.width().unwrap_or(0)).sum();
            if c == ' ' && row.is_empty() {
                continue;
            }
        }
        row.push((c, style));
        used += w;
    }
    rows.push(line(&row));
    rows
}
/// A line of a text file in its syntax colors.
fn colored(
    text: &str,
    spans: &[(usize, usize, crate::highlight::Syntax)],
    theme: Palette,
) -> Vec<Span<'static>> {
    use crate::highlight::Syntax;
    let style = |syntax| match syntax {
        Syntax::Comment => Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::ITALIC),
        Syntax::String => Style::default().fg(theme.success),
        Syntax::Keyword => Style::default().fg(theme.accent),
        Syntax::Type => Style::default().fg(theme.marked),
        Syntax::Function | Syntax::Special => Style::default().fg(theme.link),
        Syntax::Constant => Style::default().fg(theme.error),
        Syntax::Heading => Style::default()
            .fg(theme.accent)
            .add_modifier(Modifier::BOLD),
    };
    let mut out = Vec::new();
    let mut at = 0;
    for &(start, end, syntax) in spans {
        let (start, end) = (start.min(text.len()), end.min(text.len()));
        let (Some(before), Some(piece)) = (text.get(at..start), text.get(start..end)) else {
            continue;
        };
        if !before.is_empty() {
            out.push(styled(before.to_string(), theme.text));
        }
        if !piece.is_empty() {
            out.push(Span::styled(piece.to_string(), style(syntax)));
            at = end;
        }
    }
    if let Some(rest) = text.get(at..) {
        out.push(styled(rest.to_string(), theme.text));
    }
    out
}
