//! The footer: the selected path's details or a message, the F1 pointer,
//! and the opt-in button bar.
use super::*;

/// One line: the selected path (or a status message) and the F1 pointer.
/// Its details are colored by kind as `eza -l` colors them, in the
/// palette's roles: sizes and counts `success`, the date `accent`, the
/// owner `marked`, and the permissions letter by letter.
pub(super) fn footer(frame: &mut Frame, area: Rect, app: &App, focus: &Focus) {
    let theme = app.palette;
    let selected = if let Focus::Marks { rows, index } = focus {
        rows.get(*index)
    } else {
        app.highlight.as_ref()
    };
    // How to leave; q is text while typing.
    let typing = app.edit_view.as_ref().is_some_and(|v| !v.normal());
    let quit = if typing {
        app.bound_in(crate::keymap::Function::Cancel, crate::keymap::Mode::Insert)
    } else {
        app.bound_in(crate::keymap::Function::Quit, crate::keymap::Mode::Normal)
            .or_else(|| app.bound(crate::keymap::Function::Cancel))
    };
    let mut help = vec![];
    for (key, what) in [
        (app.bound(crate::keymap::Function::ShowKeys), " keys "),
        (quit, " quit "),
    ] {
        if let Some(key) = key {
            if !help.is_empty() {
                help.push(Span::raw(" "));
            }
            help.push(bold(key, theme.accent));
            help.push(styled(what, theme.muted));
        }
    }
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
        let width = |d: &[Vec<Span>]| {
            d.iter()
                .map(|value| 2 + value.iter().map(Span::width).sum::<usize>())
                .sum::<usize>()
        };
        while !details.is_empty() && available.saturating_sub(width(&details)) < wanted {
            details.pop();
        }
        let room = available.saturating_sub(width(&details));
        let path = elide(&path, room);
        let split = path.rfind('/').map(|i| i + 1).unwrap_or(0);
        // The name in its row's color: a folder, link, executable or empty
        // file.
        let name = selected.map_or(theme.text, |p| {
            let info = app.details.info(p);
            let directory = info.as_ref().is_some_and(|i| i.directory);
            info_color(theme, info.as_ref(), directory)
        });
        let mut spans = vec![
            Span::raw(" "),
            styled(path[..split].to_string(), theme.muted),
            styled(path[split..].to_string(), name),
        ];
        for value in details {
            spans.push(Span::raw("  "));
            spans.extend(value);
        }
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
/// The footer's details of `path` in order of importance, each value as
/// its spans: a link's target, size or entry count, modification time,
/// permissions and owner. The footer leaves out the last that do not fit.
pub(super) fn footer_details(app: &App, path: &Path) -> Vec<Vec<Span<'static>>> {
    use crate::details;
    let theme = app.palette;
    let Some(info) = app.details.info(path) else {
        return Vec::new();
    };
    let mut values: Vec<Vec<Span<'static>>> = Vec::new();
    if let Some((target, resolves)) = &info.link {
        let target = format!("→ {}", safe(target.to_string_lossy()));
        values.push(vec![if *resolves {
            styled(target, theme.link)
        } else {
            styled(target + " (broken)", theme.error)
        }]);
    }
    if info.directory {
        // The size of the files below it, then its own entries.
        if let Some(size) = app.details.size(path).filter(|s| !s.unreadable) {
            let exact = details::exact(size.bytes);
            let mut spans = if size.bytes < 1000 {
                vec![bold(exact, theme.success), styled(" B", theme.success)]
            } else {
                vec![
                    bold(details::size(size.bytes), theme.success),
                    styled(format!(" ({exact} B)"), theme.muted),
                ]
            };
            if size.counting {
                spans.push(styled(" so far", theme.muted));
            } else if size.partial {
                spans.push(styled(" (some unreadable)", theme.muted));
            }
            values.push(spans);
        }
        if let Some(count) = app.details.count(path) {
            let text = match count {
                details::Count::Entries(1) => "1 entry".into(),
                details::Count::Entries(_) => format!("{} entries", details::entries(count)),
                details::Count::Unreadable => "unreadable".into(),
            };
            values.push(vec![styled(text, theme.success)]);
        }
    } else if info.link.is_none() && !app.details.on_kernel(path) {
        // A kernel filesystem's sizes are made up (/proc/kcore's is the
        // address space): none is shown.
        let exact = details::exact(info.size);
        values.push(if info.size < 1000 {
            vec![bold(exact, theme.success), styled(" B", theme.success)]
        } else {
            vec![
                bold(details::size(info.size), theme.success),
                styled(format!(" ({exact} B)"), theme.muted),
            ]
        });
    }
    if let Some(modified) = info.modified {
        values.push(vec![styled(details::timestamp(modified), theme.accent)]);
    }
    values.push(permission_spans(theme, &details::permissions(info.mode)));
    values.push(vec![styled(app.details.owner(&info), theme.marked)]);
    values
}
/// `drwxr-xr-x` letter by letter, as `eza -l` colors it: the kind as a
/// folder, read, write and run in the palette's yellow, red and green
/// (`marked`, `error`, `success`), and what is not set dimmed.
fn permission_spans(theme: Palette, text: &str) -> Vec<Span<'static>> {
    text.chars()
        .enumerate()
        .map(|(i, c)| {
            let color = match c {
                '-' => theme.muted,
                _ if i == 0 => theme.accent,
                'r' => theme.marked,
                'w' => theme.error,
                _ => theme.success,
            };
            styled(c.to_string(), color)
        })
        .collect()
}
