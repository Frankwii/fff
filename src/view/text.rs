//! Text being edited (the query, prompts, multi-line buffers) with Neovim's
//! cursor and selection, and wrapped messages.
use super::*;

pub(super) fn mode_title(app: &App, field: Field, title: &str) -> String {
    app.edit_view
        .as_ref()
        .filter(|v| v.field == field)
        .map(|v| format!(" {title} · {} ", v.label()))
        .unwrap_or_else(|| format!(" {title} "))
}
pub(super) fn editable_line(
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
pub(super) fn text_line(
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
            // The text goes on past the edge: an ellipsis says so.
            while used + 1 > width
                && let Some(last) = spans.pop()
            {
                used -= last.width();
            }
            if used < width {
                spans.push(styled("…", theme.muted));
            }
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
pub(super) fn command_line(frame: &mut Frame, app: &App, field: Field, area: Rect) {
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

/// `text` in rows of at most `width` columns, broken between words where
/// possible; a word longer than a row is broken where the row ends.
pub(super) fn wrap_text(text: &str, width: usize) -> Vec<String> {
    let mut rows = vec![String::new()];
    let mut used = 0;
    for word in text.split_inclusive(' ') {
        if used > 0 && used + word.trim_end_matches(' ').width() > width {
            rows.push(String::new());
            used = 0;
        }
        for c in word.chars() {
            let w = c.width().unwrap_or(0);
            if used + w > width && used > 0 && c != ' ' {
                rows.push(String::new());
                used = 0;
            }
            rows.last_mut().unwrap().push(c);
            used += w;
        }
    }
    // The spaces a row ended with were where it broke.
    for row in &mut rows {
        row.truncate(row.trim_end_matches(' ').len());
    }
    rows
}
pub(super) fn prompt(
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
            format!(
                "{what}; {} saves; {} discards",
                app.key_or_none("ctrl+s"),
                app.key_or_none("esc")
            )
        }
    } else if modal {
        format!(
            "Location · {} navigates · {} normal / cancel",
            app.key_or_none("enter"),
            app.key_or_none("esc")
        )
    } else {
        format!(
            "Location ({} navigates, {} cancels)",
            app.key_or_none("enter"),
            app.key_or_none("esc")
        )
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
pub(super) fn edit_rows(frame: &mut Frame, app: &App, field: Field, text: &str, editor: Rect) {
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
