//! The Keys tab: the search line, the list of functions (or of bindings, in
//! a key search) with the highlighted one described below, and a
//! function's view with its bindings and the scope editor.
use super::*;
use crate::keymap::{Binding, Dimension, FIXED, Function, Mode};
use crate::ui::{EditStep, KeysScreen, Listed};

/// The border's key hints for the screen's state.
pub(super) fn keys_hints(screen: &KeysScreen) -> &'static [(&'static str, &'static str)] {
    use crate::ui::Search;
    match (&screen.editing, &screen.search) {
        (Some(editing), _) => match editing.step {
            EditStep::Browse => &[
                ("a", "add"),
                ("s", "scope"),
                ("d", "delete"),
                ("R", "reset"),
                ("esc", "back"),
            ],
            EditStep::ChooseMode => &[("i", "Insert"), ("n", "Normal"), ("esc", "back")],
            EditStep::Capture { .. } => &[
                ("enter", "done"),
                ("backspace", "undo key"),
                ("esc", "cancel"),
            ],
            EditStep::Scope { .. } => &[
                ("space", "toggle"),
                ("enter", "save"),
                ("u", "narrow"),
                ("t", "take"),
                ("esc", "back"),
            ],
        },
        (None, Search::Capturing(_)) => &[
            ("tab", "stop"),
            ("backspace", "undo key"),
            ("esc", "text search"),
        ],
        (None, Search::Captured(_)) => &[
            ("enter", "open"),
            ("tab", "text search"),
            ("esc", "text search"),
        ],
        (None, _) => &[
            ("enter", "open"),
            ("tab", "key search"),
            ("ctrl+a", "here / everywhere"),
            ("esc", "close"),
        ],
    }
}

pub(super) fn keys_tab(frame: &mut Frame, area: Rect, app: &App, screen: &KeysScreen) {
    if let Some(editing) = &screen.editing {
        function_view(frame, area, app, screen, editing);
        return;
    }
    let theme = app.palette;
    let width = area.width as usize;
    let rows = screen.listed(&app.keymap);
    let mut lines: Vec<Line<'static>> = Vec::new();
    // The search line, and where the list applies.
    let where_ = if screen.everywhere {
        "everywhere".to_string()
    } else {
        format!(
            "here: {} · {}",
            screen.here.program.name(),
            screen.here.place.name()
        )
    };
    let search = match &screen.search {
        crate::ui::Search::Text => vec![
            bold("Search ", theme.accent),
            styled(safe(&screen.query), theme.text),
            styled("▏", theme.accent),
        ],
        crate::ui::Search::Capturing(keys) => vec![
            bold("Press keys ", theme.accent),
            styled(pressed(keys), theme.text),
            styled("▏", theme.accent),
        ],
        crate::ui::Search::Captured(keys) => vec![
            bold("Keys ", theme.accent),
            styled(pressed(keys), theme.text),
        ],
    };
    let used: usize = search.iter().map(|s| s.width()).sum();
    let mut first = search;
    first.push(Span::raw(
        " ".repeat(width.saturating_sub(used + where_.width()).max(1)),
    ));
    first.push(styled(where_, theme.muted));
    lines.push(Line::from(first));
    lines.push(match &screen.message {
        Some((text, error)) => Line::from(styled(
            truncate(&safe(text), width),
            if *error { theme.error } else { theme.success },
        )),
        None => Line::default(),
    });
    // The list, then the highlighted row described.
    let detail = detail_lines(app, screen, rows.get(screen.index), width);
    let room = (area.height as usize)
        .saturating_sub(lines.len() + detail.len() + 2)
        .max(1);
    let top = screen
        .index
        .saturating_sub(room / 2)
        .min(rows.len().saturating_sub(room));
    let bindings = matches!(rows.first(), Some(Listed::Binding(_)));
    lines.push(Line::from(if bindings {
        header(
            theme,
            &[("MODE", 8), ("KEYS", 14), ("SCOPE", 34), ("FUNCTION", 0)],
            width,
        )
    } else {
        header(
            theme,
            &[
                ("FUNCTION", 23),
                ("INSERT", 16),
                ("NORMAL", 16),
                ("WHAT IT DOES", 0),
            ],
            width,
        )
    }));
    if rows.is_empty() {
        lines.push(Line::from(styled("Nothing matches.", theme.muted)));
    }
    for (i, row) in rows.iter().enumerate().skip(top).take(room) {
        let selected = i == screen.index;
        lines.push(row_line(app, screen, row, selected, width));
    }
    while lines.len() < area.height as usize - detail.len() {
        lines.push(Line::default());
    }
    lines.extend(detail);
    frame.render_widget(Paragraph::new(lines), area);
}
fn pressed(keys: &[KeyEvent]) -> String {
    let keys: Vec<_> = keys.iter().map(|k| crate::keymap::Key::press(*k)).collect();
    crate::keymap::display(&keys)
}
use crossterm::event::KeyEvent;
fn header(theme: Palette, columns: &[(&str, usize)], width: usize) -> Vec<Span<'static>> {
    let mut used = 0;
    columns
        .iter()
        .map(|(name, w)| {
            let w = if *w == 0 {
                width.saturating_sub(used)
            } else {
                *w
            };
            used += w;
            styled(format!("{:<w$}", truncate(name, w)), theme.muted)
        })
        .collect()
}
/// `text` cut to `w` columns and padded to them.
fn cell(text: &str, w: usize) -> String {
    let text = truncate(text, w.saturating_sub(1));
    let pad = w.saturating_sub(text.width());
    format!("{text}{}", " ".repeat(pad))
}
/// The keys of `function` in `mode`: those that apply here, or all with
/// `*` after scoped ones when the list shows everywhere.
fn keys_of(app: &App, screen: &KeysScreen, function: Function, mode: Mode) -> String {
    let names: Vec<String> = app
        .keymap
        .of(function, mode)
        .into_iter()
        .filter(|b| screen.everywhere || screen.applies_here(b))
        .map(|b| {
            if screen.everywhere && b.scope != crate::keymap::Scope::ALL {
                format!("{}*", b.display())
            } else {
                b.display()
            }
        })
        .fold(Vec::new(), |mut names, n| {
            if !names.contains(&n) {
                names.push(n);
            }
            names
        });
    if names.is_empty() {
        if function.available(mode).is_empty() {
            String::new()
        } else {
            "—".into()
        }
    } else {
        names.join(" ")
    }
}
fn row_line(
    app: &App,
    screen: &KeysScreen,
    row: &Listed,
    selected: bool,
    width: usize,
) -> Line<'static> {
    let theme = app.palette;
    let mark = if selected { "▌" } else { " " };
    let mut spans = vec![bold(mark, theme.accent)];
    let rest = width.saturating_sub(1);
    match row {
        Listed::Function(f) => {
            let custom = Mode::ALL.into_iter().any(|m| app.keymap.is_custom(*f, m));
            let name = format!("{}{}", if custom { "•" } else { "" }, f.name());
            spans.push(bold(cell(&name, 22), theme.text));
            spans.push(styled(
                cell(&keys_of(app, screen, *f, Mode::Insert), 16),
                theme.accent,
            ));
            spans.push(styled(
                cell(&keys_of(app, screen, *f, Mode::Normal), 16),
                theme.accent,
            ));
            spans.push(styled(
                truncate(f.description(), rest.saturating_sub(54)),
                theme.muted,
            ));
        }
        Listed::Fixed(i) => {
            let (keys, place, what) = FIXED[*i];
            spans.push(styled(cell("fixed", 22), theme.muted));
            spans.push(styled(cell(keys, 32), theme.accent));
            spans.push(styled(
                truncate(&format!("{place}: {what}"), rest.saturating_sub(54)),
                theme.muted,
            ));
        }
        Listed::Binding(b) => {
            let color = |c: Color| {
                if screen.applies_here(b) {
                    c
                } else {
                    theme.muted
                }
            };
            let scope = b.describe_reach();
            spans.push(styled(cell(b.mode.title(), 7), color(theme.text)));
            spans.push(bold(cell(&b.display(), 14), color(theme.accent)));
            spans.push(styled(
                cell(
                    if scope.is_empty() {
                        "everywhere it works"
                    } else {
                        &scope
                    },
                    34,
                ),
                color(theme.muted),
            ));
            spans.push(bold(
                truncate(b.function.name(), rest.saturating_sub(55)),
                color(theme.text),
            ));
        }
    }
    Line::from(spans)
}
/// The highlighted row described: its function, what it does, and every
/// binding with its scope.
fn detail_lines(
    app: &App,
    screen: &KeysScreen,
    row: Option<&Listed>,
    width: usize,
) -> Vec<Line<'static>> {
    let theme = app.palette;
    let mut lines = vec![Line::from(styled("─".repeat(width), theme.muted))];
    let function = match row {
        Some(Listed::Function(f)) => *f,
        Some(Listed::Binding(b)) => b.function,
        Some(Listed::Fixed(i)) => {
            let (keys, place, what) = FIXED[*i];
            lines.push(Line::from(vec![
                bold(keys.to_string(), theme.accent),
                styled(format!(" · {place} · fixed"), theme.muted),
            ]));
            lines.extend(prose(theme, theme.accent, what, width));
            return lines;
        }
        None => return lines,
    };
    lines.push(Line::from(vec![
        bold(function.name().to_string(), theme.text),
        styled(format!(" · {}", function.group().title()), theme.muted),
        styled(
            if screen.works_here(function) {
                String::new()
            } else {
                " · does not work here".to_string()
            },
            theme.muted,
        ),
    ]));
    lines.extend(prose(theme, theme.text, function.description(), width));
    let mut keys = Vec::new();
    for mode in Mode::ALL {
        for b in app.keymap.of(function, mode) {
            let scope = b.describe_reach();
            keys.push(format!(
                "{} {}{}",
                mode.title(),
                b.display(),
                if scope.is_empty() {
                    String::new()
                } else {
                    format!(" ({scope})")
                }
            ));
        }
    }
    lines.push(Line::from(styled(
        truncate(
            &if keys.is_empty() {
                "No keys".to_string()
            } else {
                keys.join(" · ")
            },
            width,
        ),
        theme.accent,
    )));
    lines
}
/// Where `function` works in `mode`, in words.
fn available(function: Function, mode: Mode) -> String {
    let boxes = function.available(mode);
    if boxes.is_empty() {
        return "not in this mode".into();
    }
    boxes
        .into_iter()
        .map(|s| {
            let s = s.describe();
            if s.is_empty() {
                "everywhere".to_string()
            } else {
                s
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}
fn function_view(
    frame: &mut Frame,
    area: Rect,
    app: &App,
    screen: &KeysScreen,
    editing: &crate::ui::Editing,
) {
    let theme = app.palette;
    let width = area.width as usize;
    let function = editing.function;
    let mut lines = vec![Line::from(vec![
        bold(function.name().to_string(), theme.text),
        styled(format!(" · {}", function.group().title()), theme.muted),
    ])];
    lines.extend(prose(theme, theme.text, function.description(), width));
    lines.push(Line::default());
    for mode in Mode::ALL {
        lines.push(Line::from(vec![
            styled(format!("Works in {:<8}", mode.title()), theme.muted),
            styled(
                truncate(&available(function, mode), width.saturating_sub(16)),
                theme.muted,
            ),
        ]));
    }
    lines.push(Line::default());
    let rows = crate::ui::keys::binding_rows(&app.keymap, function);
    let mut mode = None;
    for (i, b) in rows.iter().enumerate() {
        if mode != Some(b.mode) {
            mode = Some(b.mode);
            lines.push(Line::from(bold(
                b.mode.title().to_uppercase(),
                theme.marked,
            )));
        }
        lines.push(binding_line(app, screen, b, i == editing.index, width));
    }
    if rows.is_empty() {
        lines.push(Line::from(styled("No keys: a adds one.", theme.muted)));
    }
    lines.push(Line::default());
    match &editing.step {
        EditStep::Browse => {}
        EditStep::ChooseMode => lines.push(Line::from(vec![
            styled("Add a key for ", theme.text),
            bold("i", theme.accent),
            styled(" Insert or ", theme.text),
            bold("n", theme.accent),
            styled(" Normal mode?", theme.text),
        ])),
        EditStep::Capture { mode, keys } => {
            lines.push(Line::from(vec![
                styled(format!("New {} key: ", mode.title()), theme.text),
                bold(
                    if keys.is_empty() {
                        "press it…".to_string()
                    } else {
                        crate::keymap::display(keys)
                    },
                    theme.accent,
                ),
            ]));
            lines.push(Line::from(styled(
                "The first key may be any key; then Enter adds it, Backspace takes back the last, Esc cancels.",
                theme.muted,
            )));
        }
        EditStep::Scope {
            draft,
            replaces,
            cursor,
        } => {
            lines.push(Line::from(vec![
                styled("Where ", theme.text),
                bold(draft.display(), theme.accent),
                styled(
                    format!(" ({}) does {}:", draft.mode.title(), function.name()),
                    theme.text,
                ),
            ]));
            let reach: Vec<_> = function.available(draft.mode);
            for (i, (dim, name, bit, _)) in crate::ui::keys::toggles().into_iter().enumerate() {
                let on = crate::ui::keys::dimension_of(draft.scope, dim) & bit != 0;
                let possible = reach
                    .iter()
                    .any(|s| crate::ui::keys::dimension_of(*s, dim) & bit != 0);
                let text = format!(
                    "{:<10}[{}] {name}{}",
                    if i == 0 || crate::ui::keys::toggles()[i - 1].0 != dim {
                        dim
                    } else {
                        ""
                    },
                    if on { "x" } else { " " },
                    if possible {
                        ""
                    } else {
                        "  (does not work there)"
                    }
                );
                let style = if i == *cursor {
                    Style::default().fg(theme.background).bg(theme.accent)
                } else if possible {
                    Style::default().fg(theme.text)
                } else {
                    Style::default().fg(theme.muted)
                };
                lines.push(Line::from(Span::styled(text, style)));
            }
            let clashes = crate::ui::keys::clashes(&app.keymap, draft, *replaces);
            for (other, at) in clashes.iter().take(3) {
                lines.push(Line::from(styled(
                    truncate(&crate::keymap::clash(draft, other, *at), width),
                    theme.error,
                )));
            }
            if !clashes.is_empty() {
                lines.push(Line::from(styled(
                    "u narrows this key to where nothing else has it; t takes it from the others there.",
                    theme.muted,
                )));
            }
        }
    }
    if let Some((text, error)) = &screen.message {
        lines.push(Line::from(styled(
            truncate(&safe(text), width),
            if *error { theme.error } else { theme.success },
        )));
    }
    // Keep the scope editor's cursor in view.
    let scroll = match &editing.step {
        EditStep::Scope { cursor, .. } => {
            let at = lines.len().saturating_sub(18) + cursor;
            at.saturating_sub(area.height as usize - 4)
        }
        _ => 0,
    };
    frame.render_widget(
        Paragraph::new(lines).scroll((scroll.min(u16::MAX as usize) as u16, 0)),
        area,
    );
}
fn binding_line(
    app: &App,
    screen: &KeysScreen,
    b: &Binding,
    selected: bool,
    width: usize,
) -> Line<'static> {
    let theme = app.palette;
    let scope = b.describe_reach();
    let here = screen.applies_here(b);
    let style = |c: Color| {
        let s = Style::default().fg(if here { c } else { theme.muted });
        if selected {
            s.add_modifier(Modifier::BOLD)
        } else {
            s
        }
    };
    Line::from(vec![
        Span::styled(if selected { "▌ " } else { "  " }, style(theme.accent)),
        Span::styled(cell(&b.display(), 16), style(theme.accent)),
        Span::styled(
            truncate(
                if scope.is_empty() {
                    "everywhere it works"
                } else {
                    &scope
                },
                width.saturating_sub(18),
            ),
            style(theme.muted),
        ),
    ])
}
