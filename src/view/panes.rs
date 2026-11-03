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
            let room = (inner.height as usize).saturating_sub(lines.len());
            let most = preview.lines.len().saturating_sub(room);
            app.preview.room.set((room, most));
            let skip = app.preview.scrolled().min(most);
            for (i, (text, tone)) in preview.lines.iter().enumerate().skip(skip).take(room) {
                let color = match tone {
                    Tone::Text => theme.text,
                    Tone::Muted => theme.muted,
                    Tone::Directory => theme.accent,
                };
                lines.push(match preview.syntax.get(i) {
                    Some(spans) if app.preview.highlight && !spans.is_empty() => {
                        colored(text, spans, width, theme)
                    }
                    _ => Line::from(styled(truncate(text, width), color)),
                });
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
/// A line of a text file in its syntax colors, cut to `width`.
fn colored(
    text: &str,
    spans: &[(usize, usize, crate::highlight::Syntax)],
    width: usize,
    theme: Palette,
) -> Line<'static> {
    use crate::highlight::Syntax;
    let shown = truncate(text, width);
    // What is left of the text before an ellipsis.
    let kept = if text.starts_with(&shown) {
        shown.len()
    } else {
        shown.len() - '…'.len_utf8()
    };
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
        let (start, end) = (start.min(kept), end.min(kept));
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
    out.push(styled(text[at..kept].to_string(), theme.text));
    if kept < shown.len() {
        out.push(styled(shown[kept..].to_string(), theme.text));
    }
    Line::from(out)
}
