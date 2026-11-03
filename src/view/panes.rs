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
                Line::from(styled("Space marks the highlighted entry.", theme.muted)),
                Line::from(""),
                Line::from(styled(
                    if app.returns_marks() {
                        "Ctrl+w returns your marks."
                    } else {
                        "File operations act on them."
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
            if !preview.facts.is_empty() && !preview.lines.is_empty() {
                lines.push(Line::from(""));
            }
            let room = (inner.height as usize).saturating_sub(lines.len());
            for (text, tone) in preview.lines.iter().take(room) {
                let color = match tone {
                    Tone::Text => theme.text,
                    Tone::Muted => theme.muted,
                    Tone::Directory => theme.accent,
                };
                lines.push(Line::from(styled(truncate(text, width), color)));
            }
        }
    }
    frame.render_widget(Paragraph::new(lines), inner);
}
