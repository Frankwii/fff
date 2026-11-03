//! The footer: the selected path's details or a message, the F1 pointer,
//! and the opt-in button bar.
use super::*;

/// One line: the selected path (or a status message) and the F1 pointer.
pub(super) fn footer(frame: &mut Frame, area: Rect, app: &App, focus: &Focus) {
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
pub(super) fn button_bar(
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
pub(super) fn footer_details(app: &App, path: &Path) -> Vec<Span<'static>> {
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
