//! The Keys screen, drawn like the results: the query box, a line of keys,
//! the list of functions (or of a key's places) beside the highlighted
//! function's details, and the footer. A function's keys are a sheet like
//! the bookmark form, and where a key applies another; all three show
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

/// The checklists of a place: a column per dimension with each value
/// ticked (`●`) or not (`○`), or crossed out (`×`) where it cannot apply
/// with the other columns' ticks: the function does not work there, or its
/// keys cannot be bound in that mode. `cursor` is (column, row). Narrow,
/// the programs take one side and the other three the other.
#[allow(clippy::too_many_arguments)]
fn checklists(
    app: &App,
    function: Function,
    keys: Option<&[Key]>,
    modes: u16,
    scope: Scope,
    cursor: Option<(usize, usize)>,
    width: usize,
) -> Vec<Line<'static>> {
    let theme = app.palette;
    let dims = dimensions();
    let groups: Vec<Vec<usize>> = if width >= 4 * 21 {
        vec![vec![0], vec![1], vec![2], vec![3]]
    } else {
        vec![vec![1], vec![0, 2, 3]]
    };
    let column_width = width / groups.len();
    // Each group is a column of lines, each line its spans.
    let columns: Vec<Vec<Vec<Span<'static>>>> = groups
        .iter()
        .map(|group| {
            let mut lines = Vec::new();
            for (i, &c) in group.iter().enumerate() {
                if i > 0 {
                    lines.push(vec![]);
                }
                let (label, values) = &dims[c];
                lines.push(vec![heading(
                    theme,
                    cell(&format!("  {}", label.to_uppercase()), column_width),
                )]);
                let set = ticks(modes, scope, c);
                for (r, (name, bit)) in values.iter().enumerate() {
                    let here = cursor == Some((c, r));
                    let can = workable(function, keys, modes, scope, c, *bit);
                    let (marker, color) = if !can {
                        ("×", theme.error)
                    } else if set & bit != 0 {
                        ("●", theme.accent)
                    } else {
                        ("○", theme.muted)
                    };
                    let mut style = Style::default().fg(if can { theme.text } else { theme.muted });
                    if here {
                        style = style.bg(theme.selection).add_modifier(Modifier::BOLD);
                    }
                    lines.push(vec![
                        Span::styled(if here { "›" } else { " " }, style.fg(theme.marked)),
                        Span::styled(marker, style.fg(color)),
                        Span::styled(
                            cell(&format!(" {name}"), column_width.saturating_sub(2)),
                            style,
                        ),
                    ]);
                }
            }
            lines
        })
        .collect();
    let tallest = columns.iter().map(Vec::len).max().unwrap_or(0);
    (0..tallest)
        .map(|r| {
            let mut spans = Vec::new();
            for column in &columns {
                match column.get(r) {
                    Some(line) if !line.is_empty() => spans.extend(line.iter().cloned()),
                    _ => spans.push(Span::raw(" ".repeat(column_width))),
                }
            }
            Line::from(spans)
        })
        .collect()
}

/// The whole screen; returns the rows a page moves.
pub(super) fn keys_screen(frame: &mut Frame, area: Rect, app: &App, screen: &KeysScreen) -> usize {
    let theme = app.palette;
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    query_box(frame, layout[0], app, screen);
    // What the screen's own keys do.
    let mut hints = vec![Span::raw("  ")];
    let mut hint = |key: String, what: &str| {
        hints.push(bold(key, theme.accent));
        hints.push(styled(format!(" {what}    "), theme.muted));
    };
    hint("Space".into(), "or Enter: edit");
    hint(
        "Tab".into(),
        if screen.keys {
            "Functions view"
        } else {
            "Keys view"
        },
    );
    hint("r".into(), "then a key: search it (Normal mode)");
    hint(
        "Ctrl+a".into(),
        if screen.everywhere {
            "only what works here"
        } else {
            "every function"
        },
    );
    if let (Some(down), Some(up)) = (
        app.bound_in(Function::ScrollPreviewDown, app.mode()),
        app.bound_in(Function::ScrollPreviewUp, app.mode()),
    ) {
        hint(format!("{down}/{up}"), "scroll the details");
    }
    if let Some(close) = app.bound(Function::ShowKeys) {
        hint(close, "close");
    }
    frame.render_widget(Paragraph::new(Line::from(hints)), layout[1]);
    let main = layout[2];
    let (list, details) = if main.width >= 100 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(if screen.keys {
                [Constraint::Percentage(68), Constraint::Percentage(32)]
            } else {
                [Constraint::Percentage(60), Constraint::Percentage(40)]
            })
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
    let focused = screen.editor.is_none();
    let shown = if screen.everywhere {
        "showing every function".to_string()
    } else {
        format!(
            "showing what works in fff {} · {}",
            screen.here.program.name(),
            screen.here.place.name()
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
/// Column headings, each padded to its width; 0 takes the rest.
fn header(theme: Palette, columns: &[(&str, usize)], width: usize) -> Line<'static> {
    let mut used = 0;
    let mut spans = vec![Span::raw("  ")];
    for (name, w) in columns {
        let w = if *w == 0 {
            width.saturating_sub(used)
        } else {
            *w
        };
        used += w;
        spans.push(heading(theme, cell(name, w)));
    }
    Line::from(spans)
}
const PLACE_COLUMNS: [(&str, usize); 5] = [
    ("KEY", 13),
    ("MODE", 8),
    ("PROGRAM", 21),
    ("LIST", 12),
    ("HIGHLIGHT", 0),
];
/// A place's columns after its key: mode, program, list and highlight in
/// few words.
fn place_spans(theme: Palette, place: &Place, width: usize, dim: bool) -> Vec<Span<'static>> {
    let color = |c: Color| if dim { theme.muted } else { c };
    let mut used = 0;
    let mut spans = Vec::new();
    for (column, (_, w)) in PLACE_COLUMNS[1..].iter().enumerate() {
        let w = if *w == 0 {
            width.saturating_sub(used)
        } else {
            *w
        };
        used += w;
        spans.push(styled(
            cell(&compact(column, reached(place, column)), w),
            color(if column == 0 { theme.text } else { theme.muted }),
        ));
    }
    spans
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
    let block = panel(theme, title, screen.editor.is_none(), theme.accent).title_bottom(
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
    let columns: Vec<(&str, usize)> = if keys {
        vec![
            ("KEY", 12),
            ("FUNCTION", 28),
            ("MODE", 8),
            ("PROGRAM", 21),
            ("LIST", 12),
            ("HIGHLIGHT", 0),
        ]
    } else {
        vec![
            ("FUNCTION", 30),
            ("INSERT MODE", 17),
            ("NORMAL MODE", 17),
            ("DESCRIPTION", 0),
        ]
    };
    frame.render_widget(
        Paragraph::new(header(theme, &columns, width)),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );
    let list = Rect::new(
        inner.x,
        inner.y + 1,
        inner.width,
        inner.height.saturating_sub(1),
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
    let items: Vec<ListItem> = rows
        .iter()
        .map(|row| ListItem::new(row_line(app, screen, row, width)))
        .collect();
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
fn row_line(app: &App, screen: &KeysScreen, row: &Listed, width: usize) -> Line<'static> {
    let theme = app.palette;
    match row {
        Listed::Function(f) => {
            let changed = Mode::ALL.into_iter().any(|m| app.keymap.is_custom(*f, m));
            Line::from(vec![
                bold(cell(f.name(), 28), theme.text),
                bold(if changed { "• " } else { "  " }, theme.marked),
                bold(
                    cell(&keys_of(app, screen, *f, Mode::Insert), 17),
                    theme.accent,
                ),
                bold(
                    cell(&keys_of(app, screen, *f, Mode::Normal), 17),
                    theme.accent,
                ),
                styled(
                    truncate(f.description(), width.saturating_sub(64)),
                    theme.muted,
                ),
            ])
        }
        Listed::Fixed(i) => {
            let (keys, place, what) = FIXED[*i];
            Line::from(vec![
                styled(cell("fixed key", 30), theme.muted),
                bold(cell(keys, 34), theme.accent),
                styled(
                    truncate(&format!("{place}: {what}"), width.saturating_sub(64)),
                    theme.muted,
                ),
            ])
        }
        Listed::Key(place) => {
            let dim = !screen.applies_here(place);
            let mut spans = vec![
                bold(
                    cell(&place.display(), 12),
                    if dim { theme.muted } else { theme.accent },
                ),
                bold(
                    cell(place.function.name(), 28),
                    if dim { theme.muted } else { theme.text },
                ),
            ];
            spans.extend(place_spans(theme, place, width.saturating_sub(40), dim));
            Line::from(spans)
        }
    }
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
    let title = match (function, row) {
        (Some(f), _) => format!(" {} ", f.name()),
        (None, Some(Listed::Fixed(_))) => " Fixed key ".into(),
        _ => " Details ".into(),
    };
    let block = panel(theme, title, false, theme.accent).padding(Padding::horizontal(1));
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
            lines.push(Line::from(heading(theme, "KEYS")));
            let places = crate::ui::keys::places(&app.keymap, f);
            if places.is_empty() {
                lines.push(Line::from(styled("No keys. Space adds one.", theme.muted)));
            }
            for (i, place) in places.iter().enumerate() {
                let again = i > 0 && places[i - 1].keys == place.keys;
                let dim = !screen.applies_here(place);
                lines.push(Line::from(vec![
                    if again {
                        styled("  or ", theme.muted)
                    } else {
                        bold(
                            place.display(),
                            if dim { theme.muted } else { theme.accent },
                        )
                    },
                    styled(
                        format!(
                            "  in {} mode",
                            compact(0, reached(place, 0)).replace("both", "either")
                        ),
                        theme.text,
                    ),
                ]));
                for (column, label) in [(1, "program"), (2, "list"), (3, "highlight")] {
                    lines.push(Line::from(vec![
                        styled(format!("    {label:<11}"), theme.muted),
                        styled(compact(column, reached(place, column)), theme.text),
                    ]));
                }
            }
            lines.push(Line::default());
            lines.push(Line::from(heading(theme, "WORKS")));
            lines.extend(checklists(app, f, None, 0b11, Scope::ALL, None, width));
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
        _ => &[
            ("space", "Edit"),
            ("n", "New"),
            ("d", "Delete"),
            ("R", "Defaults"),
            ("enter", "Save"),
            ("esc", "Discard"),
        ],
    };
    let mut body: Vec<Line<'static>> = prose(theme, theme.accent, function.description(), width);
    body.push(Line::default());
    body.push(header(theme, &PLACE_COLUMNS, width.saturating_sub(2)));
    if rows.is_empty() {
        body.push(Line::from(styled("  No keys: n adds one.", theme.muted)));
    }
    let browsing = matches!(editor.step, EditStep::Browse);
    for (i, place) in rows.iter().enumerate() {
        let selected = browsing && i == editor.index;
        let dim = !screen.applies_here(place);
        // A key's further places read as alternatives: "or".
        let again = i > 0 && rows[i - 1].keys == place.keys;
        let mut spans = vec![
            bold(if selected { "› " } else { "  " }, theme.marked),
            if again {
                styled(cell("  or", 13), theme.muted)
            } else {
                bold(
                    cell(&place.display(), 13),
                    if dim { theme.muted } else { theme.accent },
                )
            },
        ];
        spans.extend(place_spans(theme, place, width.saturating_sub(15), false));
        let mut line = Line::from(spans);
        if selected {
            line = line.style(Style::default().bg(theme.selection));
        }
        body.push(line);
    }
    body.push(Line::default());
    match &editor.step {
        EditStep::Capture { keys } => {
            body.push(Line::from(vec![
                styled("New key:  ", theme.text),
                bold(
                    if keys.is_empty() {
                        "press it…".to_string()
                    } else {
                        crate::keymap::display(keys)
                    },
                    theme.accent,
                ),
            ]));
            body.extend(prose(
                theme,
                theme.accent,
                "The first key can be any key, and more may follow for a sequence. *Enter* goes on to where it applies, *Backspace* takes one back and *Esc* gives up.",
                width,
            ));
        }
        _ => {
            if let Some(place) = rows.get(editor.index) {
                body.push(Line::from(vec![
                    heading(theme, "WHERE "),
                    bold(place.display(), theme.accent),
                    heading(theme, " APPLIES IN THIS ROW"),
                ]));
                body.extend(checklists(
                    app,
                    function,
                    Some(&place.keys),
                    place.modes,
                    place.scope,
                    None,
                    width,
                ));
                body.push(Line::default());
            }
            body.extend(prose(
                theme,
                theme.muted,
                "A key applies wherever a tick of each column meets. For places one set of ticks cannot say, such as in fff pick, or in fff roam on a directory, press *n* and give the same key again: its rows read *or*.",
                width,
            ));
        }
    }
    if let Some((text, error)) = &editor.message {
        body.push(Line::from(styled(
            truncate(&safe(text), width),
            if *error { theme.error } else { theme.success },
        )));
    }
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
    // A tall body keeps its end, where the step's prompt is, in view.
    let scroll = (body.len() as u16).saturating_sub(inner.height);
    frame.render_widget(Paragraph::new(body).scroll((scroll, 0)), inner);
}
/// Where one key applies, to change: its checklists with a cursor, and the
/// clashes left to settle. Its size does not change as you move.
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
    let mut body: Vec<Line<'static>> = vec![
        Line::from(vec![
            bold(shown.clone(), theme.accent),
            styled(
                format!(
                    " does {} wherever a tick of each column meets.",
                    function.name()
                ),
                theme.text,
            ),
        ]),
        Line::default(),
    ];
    body.extend(checklists(
        app,
        function,
        Some(keys),
        modes,
        scope,
        Some(cursor),
        width,
    ));
    body.push(Line::default());
    body.push(Line::from(vec![
        styled("● applies   ○ does not   ", theme.muted),
        styled("×", theme.error),
        styled(
            format!(
                " cannot, with the other columns' ticks: {} does not work there, or {shown} cannot be bound in that mode",
                function.name()
            ),
            theme.muted,
        ),
    ]));
    let drafts = crate::ui::keys::bindings_of(function, keys, modes, scope);
    let replaced = replaces.map(|p| p.bindings.clone()).unwrap_or_default();
    let clashes = editor.clashes(&drafts, &replaced);
    for (other, at) in clashes.iter().take(3) {
        body.push(Line::from(styled(
            truncate(
                &format!(
                    "{} is already {}'s key in {} mode, in {} · {} · {}",
                    other.display(),
                    other.function.name(),
                    other.mode.title(),
                    compact(1, at.programs),
                    compact(2, at.places),
                    compact(3, at.highlights)
                ),
                width,
            ),
            theme.error,
        )));
    }
    if !clashes.is_empty() {
        body.extend(prose(
            theme,
            theme.accent,
            "Untick until nothing clashes, or press *u* to keep this key only where nothing else has it, or *t* to take it from the others there.",
            width,
        ));
    }
    if let Some((text, error)) = &editor.message {
        body.push(Line::from(styled(
            truncate(&safe(text), width),
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
    // As tall as with three clashes, so the sheet stays put.
    let height = (dimensions()[1].1.len() + 15) as u16;
    let inner = sheet(
        frame,
        main,
        app,
        height,
        Sheet {
            title: format!(" Where {shown} applies "),
            accent: theme.marked,
            keys: keys_hints,
            summary: Some(saves_to(app)),
        },
    );
    frame.render_widget(Paragraph::new(body), inner);
}
