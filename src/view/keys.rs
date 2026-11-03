//! The Keys screen, drawn like the results: the query box, a line of keys,
//! the list of functions (or of keys, in a key search) beside the
//! highlighted function's details, and the footer. A function's editor is a
//! sheet like the bookmark form.
use super::*;
use crate::keymap::{Binding, Dimension, FIXED, Function, Mode, Scope};
use crate::ui::{EditStep, Editor, KeysScreen, Listed, Search};

/// Where a box applies, in words: `roam, pick · the results · on a
/// directory`; `everywhere` for the whole product.
fn places(scope: Scope) -> String {
    fn part<D: Dimension>(bits: u16, name: impl Fn(D) -> &'static str) -> Option<String> {
        (bits != D::every()).then(|| {
            Scope::values::<D>(bits)
                .map(name)
                .collect::<Vec<_>>()
                .join(", ")
        })
    }
    let parts: Vec<String> = [
        part::<crate::keymap::Program>(scope.programs, |p| p.name()),
        part::<crate::keymap::Place>(scope.places, |p| match p {
            crate::keymap::Place::Results => "the results",
            crate::keymap::Place::Marks => "the marked list",
        }),
        part::<crate::keymap::Highlight>(scope.highlights, |h| match h {
            crate::keymap::Highlight::Dir => "on a directory",
            crate::keymap::Highlight::File => "on a file",
            crate::keymap::Highlight::None => "with nothing highlighted",
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    if parts.is_empty() {
        "everywhere".into()
    } else {
        parts.join(" · ")
    }
}
/// Where a binding applies, in words.
fn applies(binding: &Binding) -> String {
    binding
        .reach()
        .into_iter()
        .map(places)
        .collect::<Vec<_>>()
        .join("; ")
}
/// Where a function works in `mode`, in words.
fn works(function: Function, mode: Mode) -> Option<String> {
    let boxes = function.available(mode);
    (!boxes.is_empty()).then(|| boxes.into_iter().map(places).collect::<Vec<_>>().join("; "))
}
fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Insert => "Insert mode",
        Mode::Normal => "Normal mode",
    }
}
/// `text` cut to `w` columns, then padded to them.
fn cell(text: &str, w: usize) -> String {
    let text = truncate(text, w.saturating_sub(1));
    let pad = w.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(pad))
}
fn heading(theme: Palette, text: String) -> Span<'static> {
    Span::styled(
        text,
        Style::default()
            .fg(theme.muted)
            .add_modifier(Modifier::BOLD),
    )
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
    hint("Space".into(), "(Normal mode) or Enter: edit its keys");
    match screen.search {
        Search::Text => hint("Tab".into(), "search by pressing keys"),
        Search::Capturing(_) => hint("Tab".into(), "stop recording"),
        Search::Captured(_) => hint("Tab".into(), "search by text"),
    }
    hint(
        "Ctrl+a".into(),
        if screen.everywhere {
            "only what works here"
        } else {
            "every function"
        },
    );
    if let Some(close) = app.bound(Function::ShowKeys) {
        hint(close, "close");
    }
    frame.render_widget(Paragraph::new(Line::from(hints)), layout[1]);
    let main = layout[2];
    let (list, details) = if main.width >= 100 {
        let columns = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(62), Constraint::Percentage(38)])
            .spacing(1)
            .split(main);
        (columns[0], columns[1])
    } else {
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),
                Constraint::Length(12.min(main.height / 2)),
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
    let title = match screen.search {
        Search::Text => mode_title(app, Field::Keys, "Keys"),
        Search::Capturing(_) => " Keys · RECORDING ".into(),
        Search::Captured(_) => " Keys · keys pressed ".into(),
    };
    let block = panel(theme, title, focused, theme.accent)
        .padding(Padding::horizontal(1))
        .title_bottom(Line::from(styled(format!(" {shown} "), theme.muted)).right_aligned());
    let inner = block.inner(area);
    frame.render_widget(block, area);
    match &screen.search {
        Search::Text => {
            let available = inner.width.saturating_sub(2) as usize;
            let (mut line, cursor) = editable_line(app, Field::Keys, &screen.query, 0, available);
            if screen.query.is_empty() {
                line = Line::from(styled(
                    "type to find a function by name, description or key…",
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
        Search::Capturing(keys) | Search::Captured(keys) => {
            let pressed: Vec<_> = keys.iter().map(|k| crate::keymap::Key::press(*k)).collect();
            let shown = if pressed.is_empty() {
                styled(
                    "press keys to see what they do · Backspace takes one back · Esc returns to typing",
                    theme.muted,
                )
            } else {
                bold(crate::keymap::display(&pressed), theme.text)
            };
            frame.render_widget(
                Paragraph::new(Line::from(vec![bold("⌨ ", theme.accent), shown])),
                inner,
            );
        }
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
    let keys = !matches!(screen.search, Search::Text);
    let title = if keys {
        format!(" Keys · {} ", rows.len())
    } else {
        format!(" Functions · {} ", rows.len())
    };
    let mut block = panel(theme, title, screen.editor.is_none(), theme.accent);
    block = block.title_bottom(
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
    let columns: &[(&str, usize)] = if keys {
        &[
            ("KEY", 14),
            ("MODE", 14),
            ("FUNCTION", 30),
            ("APPLIES IN", 0),
        ]
    } else {
        &[
            ("FUNCTION", 30),
            ("INSERT MODE", 17),
            ("NORMAL MODE", 17),
            ("DESCRIPTION", 0),
        ]
    };
    let mut used = 0;
    let mut header = vec![Span::raw("  ")];
    for (name, w) in columns {
        let w = if *w == 0 {
            width.saturating_sub(used)
        } else {
            *w
        };
        used += w;
        header.push(heading(theme, cell(name, w)));
    }
    frame.render_widget(
        Paragraph::new(Line::from(header)),
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
                if keys {
                    "  Nothing has these keys."
                } else {
                    "  Nothing matches. Ctrl+a searches every function."
                },
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
    for b in app.keymap.of(function, mode) {
        if (screen.everywhere || screen.applies_here(b)) && !names.contains(&b.display()) {
            names.push(b.display());
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
        Listed::Binding(b) => {
            let here = screen.applies_here(b);
            let dim = |c: Color| if here { c } else { theme.muted };
            Line::from(vec![
                bold(cell(&b.display(), 14), dim(theme.accent)),
                styled(cell(mode_name(b.mode), 14), dim(theme.text)),
                bold(cell(b.function.name(), 30), dim(theme.text)),
                styled(truncate(&applies(b), width.saturating_sub(58)), theme.muted),
            ])
        }
    }
}
/// A label in a column of its own, then its value.
fn labelled(theme: Palette, label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![styled(format!("{label:<16}"), theme.muted), value])
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
        Some(Listed::Binding(b)) => Some(b.function),
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
            lines.push(Line::default());
            lines.push(labelled(
                theme,
                "Group",
                styled(f.group().title(), theme.text),
            ));
            lines.push(Line::default());
            lines.push(Line::from(heading(theme, "WORKS".into())));
            for mode in Mode::ALL {
                lines.push(labelled(
                    theme,
                    mode_name(mode),
                    styled(
                        truncate(
                            &works(f, mode).unwrap_or_else(|| "nowhere".into()),
                            width.saturating_sub(16),
                        ),
                        theme.text,
                    ),
                ));
            }
            if !screen.works_here(f) {
                lines.push(Line::from(styled(
                    "It does not work where you opened this screen.",
                    theme.muted,
                )));
            }
            lines.push(Line::default());
            lines.push(Line::from(heading(
                theme,
                format!("{:<14}{:<14}{}", "KEY", "MODE", "APPLIES IN"),
            )));
            let bindings = crate::ui::keys::binding_rows(&app.keymap, f);
            if bindings.is_empty() {
                lines.push(Line::from(styled("No keys. Enter adds one.", theme.muted)));
            }
            for b in bindings {
                let here = screen.applies_here(&b);
                lines.push(Line::from(vec![
                    bold(
                        cell(&b.display(), 14),
                        if here { theme.accent } else { theme.muted },
                    ),
                    styled(
                        cell(mode_name(b.mode), 14),
                        if here { theme.text } else { theme.muted },
                    ),
                    styled(
                        truncate(&applies(&b), width.saturating_sub(28)),
                        theme.muted,
                    ),
                ]));
            }
        }
        (None, Some(Listed::Fixed(i))) => {
            let (keys, place, what) = FIXED[*i];
            lines.push(labelled(theme, "Keys", bold(keys, theme.accent)));
            lines.push(labelled(theme, "Where", styled(place, theme.text)));
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
    frame.render_widget(Paragraph::new(lines), inner);
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

fn editor_sheet(frame: &mut Frame, main: Rect, app: &App, screen: &KeysScreen, editor: &Editor) {
    if let EditStep::Scope {
        draft,
        replaces,
        row,
        column,
    } = &editor.step
    {
        where_sheet(
            frame,
            main,
            app,
            editor,
            draft,
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
        EditStep::ChooseMode { .. } => &[("space", "Choose"), ("esc", "Back")],
        EditStep::Capture { .. } => &[("enter", "Add"), ("backspace", "Undo key"), ("esc", "Back")],
        _ => &[
            ("space", "Where it applies"),
            ("a", "Add"),
            ("d", "Delete"),
            ("R", "Defaults"),
            ("enter", "Save"),
            ("esc", "Discard"),
        ],
    };
    let mut body: Vec<Line<'static>> = prose(theme, theme.accent, function.description(), width);
    body.push(Line::default());
    body.push(Line::from(heading(
        theme,
        format!("  {:<16}{:<16}{}", "KEY", "MODE", "APPLIES IN"),
    )));
    if rows.is_empty() {
        body.push(Line::from(styled("  No keys: a adds one.", theme.muted)));
    }
    let browsing = matches!(editor.step, EditStep::Browse);
    for (i, b) in rows.iter().enumerate() {
        let selected = browsing && i == editor.index;
        let here = screen.applies_here(b);
        // A key's further places read as alternatives: "or".
        let again = i > 0 && rows[i - 1].keys == b.keys && rows[i - 1].mode == b.mode;
        let mut line = Line::from(vec![
            bold(if selected { "› " } else { "  " }, theme.marked),
            if again {
                styled(cell("  or", 16), theme.muted)
            } else {
                bold(
                    cell(&b.display(), 16),
                    if here { theme.accent } else { theme.muted },
                )
            },
            styled(
                cell(if again { "" } else { mode_name(b.mode) }, 16),
                theme.text,
            ),
            styled(truncate(&applies(b), width.saturating_sub(34)), theme.muted),
        ]);
        if selected {
            line = line.style(Style::default().bg(theme.selection));
        }
        body.push(line);
    }
    body.push(Line::default());
    match &editor.step {
        EditStep::ChooseMode { index } => {
            body.push(Line::from(styled("A new key, for:", theme.text)));
            for (i, mode) in Mode::ALL.into_iter().enumerate() {
                let mut line = Line::from(vec![
                    bold(if i == *index { "› " } else { "  " }, theme.marked),
                    styled(mode_name(mode), theme.text),
                ]);
                if i == *index {
                    line = line.style(Style::default().bg(theme.selection));
                }
                body.push(line);
            }
        }
        EditStep::Capture { mode, keys } => {
            body.push(Line::from(vec![
                styled(format!("New key, {}:  ", mode_name(*mode)), theme.text),
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
                "The first key can be any key. Then *Enter* adds them, *Backspace* takes one back and *Esc* gives up. To make a key apply in two separate places, add it again: its rows read *or*.",
                width,
            ));
        }
        _ => {}
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
fn saves_to(app: &App) -> String {
    match &app.keys_path {
        Some(path) => format!("saves to {}", tilde(path, app.home.as_deref())),
        None => "this session only".into(),
    }
}
/// Where one key applies: a checklist per dimension, side by side, and the
/// clashes left to settle. Its size does not change as you move.
fn where_sheet(
    frame: &mut Frame,
    main: Rect,
    app: &App,
    editor: &Editor,
    draft: &Binding,
    replaces: Option<&Binding>,
    (column, row): (usize, usize),
) {
    let theme = app.palette;
    let function = editor.function;
    let width = main.width.saturating_sub(4) as usize;
    let dims = crate::ui::keys::dimensions();
    let tallest = dims.iter().map(|(_, v)| v.len()).max().unwrap_or(0);
    let reach = function.available(draft.mode);
    let mut body: Vec<Line<'static>> = vec![
        Line::from(vec![
            styled("The key ", theme.text),
            bold(draft.display(), theme.accent),
            styled(
                format!(
                    " does {} in {} wherever all three columns tick:",
                    function.name(),
                    mode_name(draft.mode)
                ),
                theme.text,
            ),
        ]),
        Line::from(styled(
            format!("Applies in: {}", places(draft.scope)),
            theme.muted,
        )),
        Line::default(),
    ];
    let column_width = (width / 3).max(18);
    let mut header = Vec::new();
    for (label, _) in &dims {
        header.push(heading(
            theme,
            cell(&format!("  {}", label.to_uppercase()), column_width),
        ));
    }
    body.push(Line::from(header));
    for r in 0..tallest {
        let mut spans = Vec::new();
        for (c, (_, values)) in dims.iter().enumerate() {
            let Some((name, bit)) = values.get(r) else {
                spans.push(Span::raw(" ".repeat(column_width)));
                continue;
            };
            let mut scope = draft.scope;
            let on = *crate::ui::keys::bits(&mut scope, c) & bit != 0;
            let works = reach.iter().any(|s| {
                let mut s = *s;
                *crate::ui::keys::bits(&mut s, c) & bit != 0
            });
            let cursor = c == column && r == row;
            let color = if works { theme.text } else { theme.muted };
            let mut style = Style::default().fg(color);
            if cursor {
                style = style.bg(theme.selection).add_modifier(Modifier::BOLD);
            }
            let text = format!(
                "{}{} {}",
                if cursor { "›" } else { " " },
                if on { "●" } else { "○" },
                name
            );
            spans.push(Span::styled(cell(&text, column_width), style));
        }
        body.push(Line::from(spans));
    }
    body.push(Line::default());
    body.push(Line::from(vec![
        styled("● ticked   ○ not ticked   ", theme.muted),
        styled(
            format!("grey: {} does not work there", function.name()),
            theme.muted,
        ),
    ]));
    let clashes = editor.clashes(draft, replaces);
    for (other, at) in clashes.iter().take(3) {
        body.push(Line::from(styled(
            truncate(
                &format!(
                    "{} is already {}'s key {}",
                    other.display(),
                    other.function.name(),
                    match places(*at).as_str() {
                        "everywhere" => "everywhere".to_string(),
                        place => format!("in {place}"),
                    }
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
    let keys: &[(&str, &str)] = if clashes.is_empty() {
        &[
            ("space", "Tick"),
            ("up down", "Move"),
            ("enter", "Done"),
            ("esc", "Back"),
        ]
    } else {
        &[
            ("space", "Tick"),
            ("u", "Only where free"),
            ("t", "Take"),
            ("enter", "Done"),
            ("esc", "Back"),
        ]
    };
    // As tall as with three clashes, so the sheet stays put.
    let height = (tallest + 13) as u16;
    let inner = sheet(
        frame,
        main,
        app,
        height,
        Sheet {
            title: format!(" Where {} applies ", draft.display()),
            accent: theme.marked,
            keys,
            summary: Some(saves_to(app)),
        },
    );
    frame.render_widget(Paragraph::new(body), inner);
}
