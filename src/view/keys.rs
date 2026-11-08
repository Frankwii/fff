//! The Keys screen, drawn like the results: the query box, a line of keys,
//! the list of functions (or of a key's places) beside the highlighted
//! function's details, and the footer. A function's keys are a sheet like
//! the workspace form, and where a key applies another; all three show
//! places with the same checklists: a column each for the mode, the
//! program, the list and the highlight.
use super::*;
use crate::keymap::{FIXED, Function, Key, Mode, Scope};
use crate::ui::keys::{Place, dimensions, ticks, workable};
use crate::ui::{EditStep, Editor, KeysScreen, Listed};

/// The values of `column` set in `bits`, in as few words as fit: `all`,
/// a list, or `all but` a list, whichever is shorter; the four explorer
/// programs are named once when all are there.
pub(super) fn compact(column: usize, bits: u16) -> String {
    let (_, values) = &dimensions()[column];
    let every = values.iter().fold(0, |all, (_, bit)| all | bit);
    if bits & every == every {
        return if values.len() == 2 { "both" } else { "all" }.into();
    }
    if bits & every == 0 {
        return "none".into();
    }
    let names = |on: bool| {
        let mut names: Vec<&str> = values
            .iter()
            .filter(|(_, bit)| (bits & bit != 0) == on)
            .map(|(name, _)| *name)
            .collect();
        if names.iter().filter(|n| n.starts_with("explorer-")).count() == 4 {
            names.retain(|n| !n.starts_with("explorer-"));
            names.push("explorer");
        }
        names.join(", ")
    };
    let listed = names(true);
    let but = format!("all but {}", names(false));
    if but.len() < listed.len() {
        but
    } else {
        listed
    }
}
/// Where a place applies in `column`, as its function allows: its modes,
/// or the values its keys reach.
fn reached(place: &Place, column: usize) -> u16 {
    if column == 0 {
        return place.bindings.iter().fold(0, |bits, b| bits | b.mode.bit());
    }
    place
        .bindings
        .iter()
        .flat_map(|b| b.reach())
        .fold(0, |bits, s| bits | ticks(0, s, column))
}
use crate::keymap::Dimension;
/// `text` cut to `w` columns, then padded to them.
fn cell(text: &str, w: usize) -> String {
    let text = truncate(text, w.saturating_sub(1));
    let pad = w.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(pad))
}
fn heading(theme: Palette, text: impl Into<String>) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    )
}
/// The widths of a table's columns: each as wide as its widest cell or
/// heading and a gap, the last taking what is left of `width`. When they do
/// not fit, column `flex` gives way first, then the last.
fn widths(headings: &[&str], rows: &[Vec<String>], width: usize, flex: usize) -> Vec<usize> {
    let count = headings.len();
    let mut widths: Vec<usize> = (0..count)
        .map(|c| {
            rows.iter()
                .filter_map(|r| r.get(c))
                .map(|t| t.width())
                .chain([headings[c].width()])
                .max()
                .unwrap_or(0)
                + 2
        })
        .collect();
    let last = count - 1;
    let fixed: usize = widths[..last].iter().sum();
    let room = width.saturating_sub(fixed);
    if room < widths[last].min(12) {
        // The flexible column gives up what the last needs, down to a few.
        let short = widths[last].min(12) - room;
        widths[flex] = widths[flex]
            .saturating_sub(short)
            .max(headings[flex].width() + 2);
    }
    widths[last] = width.saturating_sub(widths[..last].iter().sum());
    widths
}
/// `spans` cut to `w` columns (the cut marked `…`), then padded to them.
fn fit(spans: Vec<Span<'static>>, w: usize) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut used = 0;
    for span in spans {
        let room = w.saturating_sub(used);
        if span.width() > room {
            let style = span.style;
            let cut = truncate(&span.content, room.saturating_sub(1));
            used += cut.width();
            out.push(Span::styled(cut, style));
            break;
        }
        used += span.width();
        out.push(span);
    }
    if used < w {
        out.push(Span::raw(" ".repeat(w - used)));
    }
    out
}
/// A table's heading row, after a gutter of `gutter` columns.
fn headings_line(
    theme: Palette,
    headings: &[&str],
    widths: &[usize],
    gutter: usize,
) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(gutter))];
    for (name, w) in headings.iter().zip(widths) {
        spans.push(heading(theme, cell(name, *w)));
    }
    Line::from(spans)
}
/// A place's key cell: the key, or for a key's further places `or` and
/// the key again, so that they read as alternatives.
fn key_cell(theme: Palette, place: &Place, again: bool, dim: bool) -> Vec<Span<'static>> {
    let color = if dim { theme.muted } else { theme.accent };
    if again {
        vec![bold("or ", theme.marked), styled(place.display(), color)]
    } else {
        vec![bold(place.display(), color)]
    }
}
fn key_text(place: &Place, again: bool) -> String {
    if again {
        format!("or {}", place.display())
    } else {
        place.display()
    }
}
/// A place's mode, program, list and highlight, in few words.
fn place_texts(place: &Place) -> [String; 4] {
    [0, 1, 2, 3].map(|c| compact(c, reached(place, c)))
}
/// Lines of prose (or anything) set in by `gutter` columns.
fn indented(lines: Vec<Line<'static>>, gutter: usize) -> Vec<Line<'static>> {
    lines
        .into_iter()
        .map(|mut line| {
            line.spans.insert(0, Span::raw(" ".repeat(gutter)));
            line
        })
        .collect()
}
/// The rows of a table of places: key, (function), mode, program, list,
/// highlight, sized to fit `width` after a gutter of `gutter` columns.
/// `selected` gives a row the selection's marker and color.
#[allow(clippy::too_many_arguments)]
fn places_table(
    theme: Palette,
    screen: &KeysScreen,
    places: &[Place],
    with_function: bool,
    width: usize,
    gutter: usize,
    selected: Option<usize>,
) -> Vec<Line<'static>> {
    let again = |i: usize| i > 0 && places[i - 1].keys == places[i].keys;
    let mut headings = vec!["KEY"];
    if with_function {
        headings.push("FUNCTION");
    }
    headings.extend(["MODE", "PROGRAM", "LIST", "HIGHLIGHT"]);
    let texts: Vec<Vec<String>> = places
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let mut row = vec![key_text(p, again(i))];
            if with_function {
                row.push(p.function.name().into());
            }
            row.extend(place_texts(p));
            row
        })
        .collect();
    let program = headings.len() - 3;
    let widths = widths(&headings, &texts, width.saturating_sub(gutter), program);
    let mut lines = vec![headings_line(theme, &headings, &widths, gutter)];
    for (i, (place, row)) in places.iter().zip(&texts).enumerate() {
        let dim = !screen.applies_here(place);
        let marker = if selected == Some(i) { "› " } else { "  " };
        let mut spans = vec![bold(
            format!("{marker:<gutter$}")
                .chars()
                .take(gutter)
                .collect::<String>(),
            theme.marked,
        )];
        spans.extend(fit(key_cell(theme, place, again(i), dim), widths[0]));
        for (c, text) in row.iter().enumerate().skip(1) {
            let color = match (dim, c == 1 && with_function) {
                (true, _) => theme.muted,
                (false, true) => theme.function,
                (false, false) if c == widths.len() - 4 => theme.text,
                _ => theme.muted,
            };
            spans.extend(fit(vec![styled(text.clone(), color)], widths[c]));
        }
        let mut line = Line::from(spans);
        if selected == Some(i) {
            line = line.style(Style::default().bg(theme.selection));
        }
        lines.push(line);
    }
    lines
}

/// The checklists of a place: a column per dimension with each value
/// ticked (`●`) or not (`○`), or crossed out (`×`) where it cannot apply
/// with the other columns' ticks: the function does not work there, or its
/// keys cannot be bound in that mode. Ticked values in `clashing` (bits by
/// column) are marked as clashing there. `cursor` is (column, row), shown in
/// the `gutter`. Columns are as wide as their values and share out the rest
/// of `width`; when four do not fit, the programs take one side and the
/// other three the other.
#[allow(clippy::too_many_arguments)]
fn checklists(
    app: &App,
    function: Function,
    keys: Option<&[Key]>,
    modes: u16,
    scope: Scope,
    cursor: Option<(usize, usize)>,
    width: usize,
    gutter: usize,
    clashing: [u16; 4],
) -> Vec<Line<'static>> {
    let theme = app.palette;
    let dims = dimensions();
    // A value takes the cursor's room (where there is a cursor), its
    // marker and a space, then its name.
    let pointer = if cursor.is_some() { 2 } else { 0 };
    let natural = |c: usize| {
        let (label, values) = &dims[c];
        values
            .iter()
            .map(|(name, _)| name.width())
            .chain([label.width()])
            .max()
            .unwrap_or(0)
            + pointer
            + 4
    };
    let room = width.saturating_sub(gutter);
    let groups: Vec<Vec<usize>> = if (0..4).map(natural).sum::<usize>() <= room {
        vec![vec![0], vec![1], vec![2], vec![3]]
    } else {
        vec![vec![1], vec![0, 2, 3]]
    };
    let mut widths: Vec<usize> = groups
        .iter()
        .map(|g| g.iter().map(|&c| natural(c)).max().unwrap_or(0))
        .collect();
    // Spare room spreads the columns, though not so far that a row is hard
    // to follow across.
    let extra = (room.saturating_sub(widths.iter().sum()) / widths.len()).min(10);
    for w in &mut widths {
        *w += extra;
    }
    // Each group is a column of lines, each line its spans.
    let columns: Vec<Vec<Vec<Span<'static>>>> = groups
        .iter()
        .zip(&widths)
        .map(|(group, &column_width)| {
            let mut lines = Vec::new();
            for (i, &c) in group.iter().enumerate() {
                if i > 0 {
                    lines.push(vec![]);
                }
                let (label, values) = &dims[c];
                lines.push(vec![heading(
                    theme,
                    cell(
                        &format!("{}{}", " ".repeat(pointer), label.to_uppercase()),
                        column_width,
                    ),
                )]);
                let set = ticks(modes, scope, c);
                for (r, (name, bit)) in values.iter().enumerate() {
                    let here = cursor == Some((c, r));
                    let can = workable(function, keys, modes, scope, c, *bit);
                    let (marker, color) = if !can {
                        ("×", theme.error)
                    } else if set & clashing[c] & bit != 0 {
                        ("●", theme.error)
                    } else if set & bit != 0 {
                        ("●", theme.accent)
                    } else {
                        ("○", theme.muted)
                    };
                    let mut style = Style::default().fg(if can { theme.text } else { theme.muted });
                    if here {
                        style = style.bg(theme.selection).add_modifier(Modifier::BOLD);
                    }
                    let mut line = Vec::new();
                    if pointer > 0 {
                        line.push(Span::styled(
                            if here { "› " } else { "  " },
                            style.fg(theme.marked),
                        ));
                    }
                    line.push(Span::styled(marker, style.fg(color)));
                    line.push(Span::styled(
                        cell(
                            &format!(" {name}"),
                            column_width.saturating_sub(pointer + 1),
                        ),
                        style,
                    ));
                    lines.push(line);
                }
            }
            lines
        })
        .collect();
    let tallest = columns.iter().map(Vec::len).max().unwrap_or(0);
    (0..tallest)
        .map(|r| {
            let mut spans = vec![Span::raw(" ".repeat(gutter))];
            for (column, w) in columns.iter().zip(&widths) {
                match column.get(r) {
                    Some(line) if !line.is_empty() => spans.extend(line.iter().cloned()),
                    _ => spans.push(Span::raw(" ".repeat(*w))),
                }
            }
            Line::from(spans)
        })
        .collect()
}

/// Hints of keys and what they do, spread over lines `width` wide: as many
/// to a line as fit, with even gaps.
fn hint_lines(theme: Palette, hints: Vec<(String, &str)>, width: usize) -> Vec<Line<'static>> {
    let room = width.saturating_sub(2);
    let size = |(key, what): &(String, &str)| key.width() + 2 + what.width();
    let mut groups: Vec<Vec<(String, &str)>> = vec![Vec::new()];
    let mut used = 0;
    for hint in hints {
        let last = groups.last_mut().unwrap();
        if !last.is_empty() && used + 3 + size(&hint) > room {
            used = size(&hint);
            groups.push(vec![hint]);
        } else {
            used += size(&hint) + if last.is_empty() { 0 } else { 3 };
            last.push(hint);
        }
    }
    groups
        .into_iter()
        .map(|group| {
            let used: usize = group.iter().map(size).sum();
            let gap = (room.saturating_sub(used) / group.len().max(1)).clamp(3, 8);
            let mut spans = vec![Span::raw("  ")];
            for (i, (key, what)) in group.into_iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw(" ".repeat(gap)));
                }
                spans.push(bold(key, theme.accent));
                spans.push(styled(format!(": {what}"), theme.muted));
            }
            Line::from(spans)
        })
        .collect()
}
/// The whole screen; returns the rows a page moves.
pub(super) fn keys_screen(frame: &mut Frame, area: Rect, app: &App, screen: &KeysScreen) -> usize {
    let theme = app.palette;
    // What the screen's own keys do, as the sheets' borders say theirs,
    // spread over the line, or over more lines where one is too short.
    let mut hints: Vec<(String, &str)> = vec![
        ("Space/Enter".into(), "edit"),
        (
            "Tab".into(),
            if screen.keys {
                "Functions view"
            } else {
                "Keys view"
            },
        ),
    ];
    // `r` is typed into the query in Insert mode.
    if app.mode() == Mode::Normal {
        hints.push(("r, then a key".into(), "search for it"));
    }
    hints.push((
        "Ctrl+a".into(),
        if screen.everywhere {
            "only what works here"
        } else {
            "every function"
        },
    ));
    if let (Some(down), Some(up)) = (
        app.bound_in(Function::ScrollPreviewDown, app.mode()),
        app.bound_in(Function::ScrollPreviewUp, app.mode()),
    ) {
        hints.push((format!("{down}/{up}"), "scroll the details"));
    }
    if let Some(close) = app.bound(Function::ShowKeys) {
        hints.push((close, "close"));
    }
    let hint_lines = hint_lines(theme, hints, area.width as usize);
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(hint_lines.len() as u16),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    query_box(frame, layout[0], app, screen);
    frame.render_widget(Paragraph::new(hint_lines), layout[1]);
    let main = layout[2];
    let (list, details) = if main.width >= 100 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
            .spacing(1)
            .split(main);
        (columns[0], columns[1])
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(14.min(main.height / 2)),
            ])
            .split(main);
        (rows[0], rows[1])
    };
    let rows = screen.listed(&app.keymap);
    let page = list_pane(frame, list, app, screen, &rows);
    details_pane(frame, details, app, screen, rows.get(screen.index));
    keys_footer(frame, layout[3], app, rows.len());
    if let Some(editor) = &screen.editor {
        editor_sheet(frame, main, app, screen, editor);
    }
    page
}
fn query_box(frame: &mut Frame, area: Rect, app: &App, screen: &KeysScreen) {
    let theme = app.palette;
    let focused = screen.editor.is_none() && screen.query_in_focus(app);
    let shown = if screen.everywhere {
        "showing every function".to_string()
    } else {
        // The results count with their query.
        let focus = match screen.here.focus {
            crate::keymap::Focus::Marks => "marks",
            _ => "results",
        };
        format!(
            "showing what works in fff {} · {focus}",
            screen.here.program.name()
        )
    };
    let title = if screen.recording {
        " Keys · press the key to search ".to_string()
    } else {
        mode_title(app, Field::Keys, "Keys")
    };
    let block = panel(theme, title, focused, theme.accent)
        .padding(Padding::horizontal(1))
        .title_bottom(Line::from(styled(format!(" {shown} "), theme.muted)).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let available = inner.width.saturating_sub(2) as usize;
    let (mut line, cursor) = editable_line(app, Field::Keys, &screen.query, 0, available);
    if screen.query.is_empty() {
        line = Line::from(styled(
            "type a function's name, words of its description, or a key's name…",
            theme.muted,
        ));
    }
    line.spans.insert(0, bold("❯ ", theme.accent));
    frame.render_widget(Paragraph::new(line), inner);
    if focused && inner.width > 0 {
        frame.set_cursor_position((inner.x + 2 + cursor, inner.y));
    }
    if focused && inner.height > 1 {
        command_line(
            frame,
            app,
            Field::Keys,
            Rect::new(inner.x, inner.y + 1, inner.width, 1),
        );
    }
}
fn list_pane(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    screen: &KeysScreen,
    rows: &[Listed],
) -> usize {
    let theme = app.palette;
    let keys = screen.keys;
    let title = if keys {
        format!(" Keys · {} ", rows.len())
    } else {
        format!(" Functions · {} ", rows.len())
    };
    let focused = screen.editor.is_none() && !screen.query_in_focus(app);
    let block = panel(theme, title, focused, theme.accent).title_bottom(
        Line::from(if keys {
            vec![styled(
                " grey: does not apply where you opened this screen ",
                theme.muted,
            )]
        } else {
            vec![
                styled(" ", theme.muted),
                bold("•", theme.marked),
                styled(" keys changed from the defaults ", theme.muted),
            ]
        })
        .right_aligned(),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let width = inner.width.saturating_sub(2) as usize;
    let list = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
    );
    // Rows after the list's own marker; headings over them.
    let (heading_line, lines): (Line<'static>, Vec<Line<'static>>) = if keys {
        let places: Vec<Place> = rows
            .iter()
            .filter_map(|r| match r {
                Listed::Key(p) => Some(p.clone()),
                _ => None,
            })
            .collect();
        let mut table = places_table(theme, screen, &places, true, width, 0, None);
        let heading_line = table.remove(0);
        (heading_line, table)
    } else {
        functions_table(app, screen, rows, width)
    };
    let mut heading_line = heading_line;
    heading_line.spans.insert(0, Span::raw("  "));
    frame.render_widget(
        Paragraph::new(heading_line),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    if rows.is_empty() {
        frame.render_widget(
            Paragraph::new(styled(
                "  Nothing matches. Ctrl+a searches every function.",
                theme.muted,
            )),
            list,
        );
        return list.height as usize;
    }
    let items: Vec<ListItem> = lines.into_iter().map(ListItem::new).collect();
    let mut state = ListState::default().with_selected(Some(screen.index.min(rows.len() - 1)));
    frame.render_stateful_widget(
        List::new(items)
            .highlight_style(Style::default().bg(theme.selection))
            .highlight_symbol("› "),
        list,
        &mut state,
    );
    list.height as usize
}
/// The Functions view's headings and rows: each function's name (marked
/// `•` when its keys changed), its keys in each mode and its description,
/// the columns as wide as their contents.
fn functions_table(
    app: &App,
    screen: &KeysScreen,
    rows: &[Listed],
    width: usize,
) -> (Line<'static>, Vec<Line<'static>>) {
    let theme = app.palette;
    let headings = ["FUNCTION", "INSERT MODE", "NORMAL MODE", "DESCRIPTION"];
    let texts: Vec<Vec<String>> = rows
        .iter()
        .map(|row| match row {
            Listed::Function(f) => vec![
                format!("{} •", f.name()),
                keys_of(app, screen, *f, Mode::Insert),
                keys_of(app, screen, *f, Mode::Normal),
                f.description().into(),
            ],
            Listed::Fixed(i) => {
                let (keys, place, what) = FIXED[*i];
                vec![
                    "fixed key".into(),
                    keys.into(),
                    String::new(),
                    format!("{place}: {what}"),
                ]
            }
            Listed::Key(_) => vec![],
        })
        .collect();
    let widths = widths(&headings, &texts, width, 2);
    let lines = rows
        .iter()
        .zip(&texts)
        .map(|(row, text)| {
            let mut spans = Vec::new();
            match row {
                Listed::Function(f) => {
                    let changed = Mode::ALL.into_iter().any(|m| app.keymap.is_custom(*f, m));
                    spans.extend(fit(
                        vec![
                            bold(f.name(), theme.function),
                            bold(if changed { " •" } else { "" }, theme.marked),
                        ],
                        widths[0],
                    ));
                    spans.extend(fit(vec![bold(text[1].clone(), theme.accent)], widths[1]));
                    spans.extend(fit(vec![bold(text[2].clone(), theme.accent)], widths[2]));
                }
                _ => {
                    spans.extend(fit(vec![styled(text[0].clone(), theme.muted)], widths[0]));
                    spans.extend(fit(
                        vec![bold(text[1].clone(), theme.accent)],
                        widths[1] + widths[2],
                    ));
                }
            }
            spans.extend(fit(vec![styled(text[3].clone(), theme.muted)], widths[3]));
            Line::from(spans)
        })
        .collect();
    (headings_line(theme, &headings, &widths, 0), lines)
}
/// The keys of `function` in `mode` that apply here (all of them, with
/// everything shown), set apart by two spaces; `—` for none.
fn keys_of(app: &App, screen: &KeysScreen, function: Function, mode: Mode) -> String {
    let mut names: Vec<String> = Vec::new();
    for place in crate::ui::keys::places(&app.keymap, function) {
        if place.modes & mode.bit() != 0
            && (screen.everywhere || screen.applies_here(&place))
            && !names.contains(&place.display())
        {
            names.push(place.display());
        }
    }
    if names.is_empty() && !function.available(mode).is_empty() {
        return "—".into();
    }
    names.join("  ")
}
fn details_pane(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    screen: &KeysScreen,
    row: Option<&Listed>,
) {
    let theme = app.palette;
    let width = area.width.saturating_sub(4) as usize;
    let function = match row {
        Some(Listed::Function(f)) => Some(*f),
        Some(Listed::Key(place)) => Some(place.function),
        _ => None,
    };
    let block = match (function, row) {
        (Some(f), _) => panel_with(
            theme,
            " ",
            vec![bold(f.name(), theme.function), Span::raw(" ")],
            false,
            theme.accent,
        ),
        (None, Some(Listed::Fixed(_))) => panel(theme, " Fixed key ", false, theme.accent),
        _ => panel(theme, " Details ", false, theme.accent),
    }
    .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut lines: Vec<Line<'static>> = Vec::new();
    match (function, row) {
        (Some(f), _) => {
            lines.extend(prose(theme, theme.accent, f.description(), width));
            if !screen.works_here(f) {
                lines.push(Line::from(styled(
                    "It does not work where you opened this screen.",
                    theme.muted,
                )));
            }
            lines.push(Line::default());
            let places = crate::ui::keys::places(&app.keymap, f);
            if places.is_empty() {
                lines.push(Line::from(heading(theme, "KEYS")));
                lines.push(Line::from(styled("No keys. Space adds one.", theme.muted)));
            } else {
                lines.extend(places_table(theme, screen, &places, false, width, 0, None));
            }
            lines.push(Line::default());
            lines.push(Line::from(heading(theme, "WORKS")));
            lines.extend(checklists(
                app,
                f,
                None,
                0b11,
                Scope::ALL,
                None,
                width,
                0,
                [0; 4],
            ));
        }
        (None, Some(Listed::Fixed(i))) => {
            let (keys, place, what) = FIXED[*i];
            lines.push(Line::from(vec![
                styled(format!("{:<8}", "Keys"), theme.muted),
                bold(keys, theme.accent),
            ]));
            lines.push(Line::from(vec![
                styled(format!("{:<8}", "Where"), theme.muted),
                styled(place, theme.text),
            ]));
            lines.push(Line::default());
            lines.extend(prose(theme, theme.accent, what, width));
            lines.push(Line::default());
            lines.push(Line::from(styled(
                "A fixed key: it cannot be rebound.",
                theme.muted,
            )));
        }
        _ => {}
    }
    let scroll = screen
        .details
        .min(lines.len().saturating_sub(inner.height as usize));
    frame.render_widget(
        Paragraph::new(lines).scroll((scroll.min(u16::MAX as usize) as u16, 0)),
        inner,
    );
}
fn keys_footer(frame: &mut Frame, area: Rect, app: &App, rows: usize) {
    let theme = app.palette;
    let left = if app.status.is_empty() {
        styled(format!(" {rows} listed"), theme.muted)
    } else if app.status == app.status_success {
        styled(format!(" ✓ {}", safe(&app.status)), theme.success)
    } else {
        styled(format!(" {}", safe(&app.status)), theme.error)
    };
    let mut help = Vec::new();
    if let Some(key) = app.bound(Function::ShowKeys) {
        help.push(bold(key, theme.accent));
        help.push(styled(" close ", theme.muted));
    }
    let used = left.width() + help.iter().map(|s| s.width()).sum::<usize>();
    let mut spans = vec![left];
    spans.push(Span::raw(
        " ".repeat((area.width as usize).saturating_sub(used)),
    ));
    spans.extend(help);
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
fn saves_to(app: &App) -> String {
    match &app.keys_path {
        Some(path) => format!("saves to {}", tilde(path, app.home.as_deref())),
        None => "this session only".into(),
    }
}

fn editor_sheet(frame: &mut Frame, main: Rect, app: &App, screen: &KeysScreen, editor: &Editor) {
    if let EditStep::Where {
        keys,
        modes,
        scope,
        replaces,
        column,
        row,
    } = &editor.step
    {
        where_sheet(
            frame,
            main,
            app,
            editor,
            keys,
            (*modes, *scope),
            replaces.as_ref(),
            (*column, *row),
        );
        return;
    }
    let theme = app.palette;
    let function = editor.function;
    let width = main.width.saturating_sub(4) as usize;
    let rows = editor.rows();
    let changed = editor.changed(&app.keymap);
    let keys: &[(&str, &str)] = match editor.step {
        EditStep::Capture { .. } => &[
            ("enter", "Next"),
            ("backspace", "Undo key"),
            ("esc", "Back"),
        ],
        EditStep::Defaults => &[("enter", "Restore"), ("esc", "Back")],
        _ => &[
            ("space", "Edit"),
            ("R", "Replace"),
            ("n", "New"),
            ("d", "Delete"),
            ("D", "Defaults"),
            ("enter", "Save"),
            ("esc", "Discard"),
        ],
    };
    // Everything sets in by the rows' marker, so that it lines up. What
    // only explains, or repeats the highlighted row, goes first when the
    // sheet does not fit (`more`).
    let inside = width.saturating_sub(2);
    let mut body: Vec<Line<'static>> = indented(
        prose(theme, theme.accent, function.description(), inside),
        2,
    );
    body.push(Line::default());
    if rows.is_empty() {
        body.push(Line::from(styled("  No keys: n adds one.", theme.muted)));
    } else {
        let browsing = matches!(editor.step, EditStep::Browse);
        let selected = browsing.then_some(editor.index);
        body.extend(places_table(
            theme, screen, &rows, false, width, 2, selected,
        ));
    }
    body.push(Line::default());
    let mut more: Vec<Line<'static>> = Vec::new();
    match &editor.step {
        EditStep::Capture { keys, replaces } => {
            let label = match replaces {
                Some(place) => format!("  Replace {} with:  ", place.display()),
                None => "  New key:  ".into(),
            };
            body.push(Line::from(vec![
                styled(label, theme.text),
                bold(
                    if keys.is_empty() {
                        "press it…".to_string()
                    } else {
                        crate::keymap::display(keys)
                    },
                    theme.accent,
                ),
            ]));
            let help = match replaces {
                Some(place) => format!(
                    "The first key can be any key, and more may follow for a sequence. *Enter* puts it where {} applied; where it clashes with another key, you choose where it applies next. *Backspace* takes one back and *Esc* gives up.",
                    place.display()
                ),
                None => "The first key can be any key, and more may follow for a sequence. *Enter* goes on to where it applies, *Backspace* takes one back and *Esc* gives up.".into(),
            };
            body.extend(indented(prose(theme, theme.accent, &help, inside), 2));
        }
        EditStep::Defaults => {
            body.push(Line::from(vec![
                styled("  Put back the default keys of ", theme.text),
                bold(function.name(), theme.function),
                styled("?", theme.text),
            ]));
            body.extend(indented(
                prose(
                    theme,
                    theme.accent,
                    "Your keys for it go, in both modes. *Enter* puts the defaults back, staged until the sheet is saved; *Esc* keeps your keys.",
                    inside,
                ),
                2,
            ));
        }
        _ => {
            if let Some(place) = rows.get(editor.index) {
                more.push(Line::from(vec![
                    heading(theme, "  WHERE "),
                    bold(place.display(), theme.accent),
                    heading(theme, " APPLIES IN THE HIGHLIGHTED ROW"),
                ]));
                more.extend(checklists(
                    app,
                    function,
                    Some(&place.keys),
                    place.modes,
                    place.scope,
                    None,
                    width,
                    2,
                    [0; 4],
                ));
                more.push(Line::default());
            }
            more.extend(indented(
                prose(
                    theme,
                    theme.muted,
                    "A key applies wherever a tick of each column meets. For places one set of ticks cannot say, such as in fff pick, or in fff roam on a directory, press *n* and give the same key again: its rows read *or Enter*, *or l*.",
                    inside,
                ),
                2,
            ));
        }
    }
    let message = editor.message.as_ref().map(|(text, error)| {
        Line::from(styled(
            format!("  {}", truncate(&safe(text), width.saturating_sub(2))),
            if *error { theme.error } else { theme.success },
        ))
    });
    let needs = (body.len() + usize::from(message.is_some()) + 2) as u16;
    if needs + more.len() as u16 <= main.height {
        body.extend(more);
    }
    body.extend(message);
    let inner = sheet(
        frame,
        main,
        app,
        body.len() as u16 + 2,
        Sheet {
            title: format!(
                " Keys of {}{} ",
                function.name(),
                if changed { " · not saved" } else { "" }
            ),
            accent: theme.marked,
            keys,
            summary: Some(saves_to(app)),
        },
    );
    frame.render_widget(Paragraph::new(body), inner);
}
/// Where one key applies, to change: its checklists with a cursor, and the
/// clashes left to settle, each value they share marked in the checklists.
/// Its size does not change as you move, where there is room.
#[allow(clippy::too_many_arguments)]
fn where_sheet(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    editor: &Editor,
    keys: &[Key],
    (modes, scope): (u16, Scope),
    replaces: Option<&Place>,
    cursor: (usize, usize),
) {
    let theme = app.palette;
    let function = editor.function;
    let width = main.width.saturating_sub(4) as usize;
    let shown = crate::keymap::display(keys);
    let drafts = crate::ui::keys::bindings_of(function, keys, modes, scope);
    let replaced = replaces.map(|p| p.bindings.clone()).unwrap_or_default();
    let clashes = editor.clashes(&drafts, &replaced);
    let mut body: Vec<Line<'static>> = vec![
        Line::from(vec![
            Span::raw("  "),
            bold(shown.clone(), theme.accent),
            styled(" does ", theme.text),
            bold(function.name(), theme.function),
            styled(" wherever a tick of each column meets.", theme.text),
        ]),
        Line::default(),
    ];
    // The values a clash takes in each column: modes, then the box.
    let shared: [u16; 4] = clashes.iter().fold([0; 4], |bits, (other, at)| {
        [
            bits[0] | other.mode.bit(),
            bits[1] | at.programs,
            bits[2] | at.focuses,
            bits[3] | at.highlights,
        ]
    });
    body.extend(checklists(
        app,
        function,
        Some(keys),
        modes,
        scope,
        Some(cursor),
        width,
        0,
        shared,
    ));
    body.push(Line::default());
    let mut legend = vec![
        styled("●", theme.accent),
        styled(" applies there   ", theme.muted),
        styled("○", theme.muted),
        styled(" does not   ", theme.muted),
    ];
    if !clashes.is_empty() {
        legend.extend([
            styled("●", theme.error),
            styled(" clashes there   ", theme.muted),
        ]);
    }
    legend.extend([
        styled("×", theme.error),
        styled(" cannot, with the other columns' ticks", theme.muted),
    ]);
    body.extend(indented(vec![Line::from(legend)], 2));
    if !clashes.is_empty() {
        body.push(Line::default());
        body.push(Line::from(vec![
            Span::raw("  "),
            bold(
                format!(
                    "{shown} CLASHES WITH {}",
                    if clashes.len() == 1 {
                        "ANOTHER KEY".into()
                    } else {
                        format!("{} KEYS", clashes.len())
                    }
                ),
                theme.error,
            ),
        ]));
        body.extend(clash_table(theme, &clashes, width, 3));
        body.extend(indented(
            prose(
                theme,
                theme.accent,
                "Untick a marked value until nothing clashes, or press *u* to keep this key only where nothing else has it, or *t* to take it from the others there.",
                width.saturating_sub(2),
            ),
            2,
        ));
    }
    if let Some((text, error)) = &editor.message {
        body.push(Line::from(styled(
            format!("  {}", truncate(&safe(text), width.saturating_sub(2))),
            if *error { theme.error } else { theme.success },
        )));
    }
    let keys_hints: &[(&str, &str)] = if clashes.is_empty() {
        &[
            ("space", "Tick"),
            ("tab", "Next column"),
            ("enter", "Done"),
            ("esc", "Back"),
        ]
    } else {
        &[
            ("space", "Tick"),
            ("tab", "Next column"),
            ("u", "Only where free"),
            ("t", "Take"),
            ("enter", "Done"),
            ("esc", "Back"),
        ]
    };
    // As tall as with three clashes where there is room, so the sheet stays
    // put as ticks change; never shorter than what it shows.
    let needs = body.len() as u16 + 2;
    let height = needs.max((dimensions()[1].1.len() + 17) as u16);
    let inner = sheet_needing(
        frame,
        main,
        app,
        height.min(main.height).max(needs),
        needs,
        Sheet {
            title: format!(" Where {shown} applies "),
            accent: theme.marked,
            keys: keys_hints,
            summary: Some(saves_to(app)),
        },
    );
    frame.render_widget(Paragraph::new(body), inner);
}
/// The keys a key clashes with: each one's key, function and where they
/// meet, up to `most` rows and a line counting the rest.
fn clash_table(
    theme: Palette,
    clashes: &[(crate::keymap::Binding, Scope)],
    width: usize,
    most: usize,
) -> Vec<Line<'static>> {
    let headings = ["KEY", "FUNCTION", "MODE", "PROGRAM", "LIST", "HIGHLIGHT"];
    let texts: Vec<Vec<String>> = clashes
        .iter()
        .take(most)
        .map(|(other, at)| {
            vec![
                other.display(),
                other.function.name().into(),
                other.mode.title().into(),
                compact(1, at.programs),
                compact(2, at.focuses),
                compact(3, at.highlights),
            ]
        })
        .collect();
    let widths = widths(&headings, &texts, width.saturating_sub(2), 3);
    let mut lines = vec![headings_line(theme, &headings, &widths, 2)];
    for row in &texts {
        let mut spans = vec![Span::raw("  ")];
        for (c, text) in row.iter().enumerate() {
            let span = match c {
                0 => bold(text.clone(), theme.accent),
                1 => bold(text.clone(), theme.function),
                _ => styled(text.clone(), theme.text),
            };
            spans.extend(fit(vec![span], widths[c]));
        }
        lines.push(Line::from(spans));
    }
    if clashes.len() > most {
        lines.push(Line::from(styled(
            format!("  and {} more", clashes.len() - most),
            theme.muted,
        )));
    }
    lines
}
