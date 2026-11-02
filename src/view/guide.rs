use super::*;

/// F1 overlay with two tabs: the key guide (default) and session details.
pub(super) fn info(frame: &mut Frame, area: Rect, app: &App, offset: &mut usize, keys: bool) {
    let theme = app.palette;
    let tab = |name: &'static str, active: bool| {
        if active {
            pill(name, theme.background, theme.accent).add_modifier(Modifier::BOLD)
        } else {
            styled(format!(" {name} "), theme.muted)
        }
    };
    let block = panel(theme, "", true, theme.accent)
        .title(Line::from(vec![
            Span::raw(" "),
            tab("Keys", keys),
            Span::raw(" "),
            tab("Session", !keys),
            Span::raw(" "),
        ]))
        .title_bottom(
            Line::from(vec![
                bold(" Tab", theme.accent),
                styled(" switches  ", theme.muted),
                bold("↑↓", theme.accent),
                styled(" scroll  ", theme.muted),
                bold("Esc", theme.accent),
                styled(" closes ", theme.muted),
            ])
            .right_aligned(),
        )
        .padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let lines = if keys {
        key_guide(app, inner.width as usize)
    } else {
        session_lines(app, inner.width as usize)
    };
    *offset = (*offset).min(lines.len().saturating_sub(inner.height as usize));
    frame.render_widget(
        Paragraph::new(lines).scroll(((*offset).min(u16::MAX as usize) as u16, 0)),
        inner,
    );
}

/// A titled group of (keys, description, needs navigation) rows in the F1 guide.
pub(super) type GuideSection = (&'static str, Vec<(String, &'static str, bool)>);
/// Every binding, grouped. Navigation keys are dimmed in `fff seek`, and the
/// Normal-mode section shows the configured `[keys.normal]` sequences.
pub(super) fn key_guide(app: &App, width: usize) -> Vec<Line<'static>> {
    use crate::normal_keys::ListAction::*;
    let theme = app.palette;
    let normal = |action| {
        let keys = app.normal_keys.describe(action);
        if keys.is_empty() { "—".into() } else { keys }
    };
    // (keys, description, needs navigation)
    let sections: Vec<GuideSection> = vec![
        (
            "Navigate",
            vec![
                ("↑↓  Alt+k Alt+j".into(), "move the highlight", false),
                ("PgUp PgDn".into(), "previous / next page", false),
                (
                    "Enter".into(),
                    if app.window {
                        "Normal: open file and leave; enter dir"
                    } else {
                        "Normal: open file and leave; leave into dir"
                    },
                    false,
                ),
                (
                    String::new(),
                    "typing: open dir, reveal / return file",
                    false,
                ),
                (
                    "Alt+l".into(),
                    "enter directory, or open file in background",
                    true,
                ),
                ("Alt+h".into(), "parent directory, keeping query", true),
                (
                    "Alt+g".into(),
                    "go to a path, bookmark or zoxide folder",
                    true,
                ),
                (
                    "Ctrl+o  Ctrl+i".into(),
                    "back / forward through visited folders",
                    true,
                ),
                ("Alt+b".into(), "open a bookmark", true),
                (
                    "Alt+↑  Alt+↓".into(),
                    "next / previous of 3 tabs; marks shared",
                    true,
                ),
            ],
        ),
        (
            "Select and return",
            vec![
                ("Tab".into(), "mark / unmark, then move down", false),
                ("Alt+Space".into(), "focus the marked list", false),
                ("Ctrl+w".into(), "open / return the marked paths", false),
                ("Alt+1…9 Alt+0".into(), "like Alt+l on row n", false),
                ("Ctrl+1…9 Ctrl+0".into(), "like Enter on row n", false),
                (String::new(), "0 is the last visible row", false),
                ("Ctrl+c".into(), "quit without returning anything", false),
            ],
        ),
        (
            "Search",
            vec![
                ("type".into(), "fuzzy-filter names", false),
                ("Esc".into(), "leave Insert, then clear query", false),
                ("Alt+← Alt+→".into(), "search shallower / deeper", false),
                ("Alt+.".into(), "show / hide hidden entries", false),
                ("Alt+d".into(), "hide / show directories", false),
                ("Alt+v".into(), "show / hide the preview pane", false),
                (
                    [Move, Copy, Link].map(normal).join(" / "),
                    "move / copy / symlink selected (Normal)",
                    false,
                ),
                (
                    [Delete, Rename, Extract, New].map(normal).join(" / "),
                    "delete / rename / extract / create (Normal)",
                    false,
                ),
                ("F5".into(), "rescan, bypassing the cache", false),
            ],
        ),
        (
            "Exclusions and filters",
            vec![
                ("Alt+e".into(), "edit exclusions, one glob per line", false),
                (String::new(), "matching entries and subtrees hidden", false),
                (String::new(), "by name or scope-relative path", false),
                ("Alt+H".into(), "enforce exclusions on / off", false),
                (
                    "Alt+f".into(),
                    "edit file filters, one glob per line",
                    false,
                ),
                (String::new(), "only files whose name matches one", false),
                (String::new(), "are listed; empty lists every file", false),
                ("ZZ".into(), "apply", false),
                ("ZQ".into(), "discard", false),
                (String::new(), "ZZ/ZQ also apply/discard renames", false),
            ],
        ),
        (
            "Normal mode in lists",
            vec![
                (normal(Down) + "  " + &normal(Up), "move down / up", false),
                (normal(Left), "parent directory, like Alt+h", true),
                (
                    normal(Right),
                    "enter directory / open file, like Alt+l",
                    true,
                ),
                (
                    normal(Open),
                    "open current / marked files here, come back",
                    false,
                ),
                (
                    normal(First) + "  " + &normal(Last),
                    "first / last row",
                    false,
                ),
                (normal(Mark), "mark / unmark, like Tab", false),
                (
                    normal(MarkAll) + "  " + &normal(InvertMarks),
                    "mark all listed / invert their marks",
                    false,
                ),
                (
                    normal(Shallower) + "  " + &normal(Deeper),
                    "search shallower / deeper",
                    false,
                ),
                (
                    if app.normal_keys.depth_digits {
                        "1…9".into()
                    } else {
                        "—".into()
                    },
                    "search to depth n (results)",
                    false,
                ),
                (normal(Hidden), "show / hide hidden entries", false),
                (normal(Directories), "hide / show directories", false),
                (normal(Preview), "show / hide the preview pane", false),
                (normal(GoTo), "go to a path, bookmark or zoxide", true),
                (normal(Bookmarks), "open a bookmark", true),
                (
                    normal(NextTab) + "  " + &normal(PrevTab),
                    "next / previous tab",
                    true,
                ),
                (normal(Quit), "exit, return launch directory", true),
                (
                    normal(QuitCurrent),
                    if app.window {
                        "exit into a shell in this directory"
                    } else {
                        "exit, return current directory"
                    },
                    true,
                ),
                (
                    normal(Run),
                    "run a program on current / marked, come back",
                    true,
                ),
                (
                    normal(RunExit),
                    "run a program on current / marked, exit",
                    true,
                ),
                (
                    normal(Edit),
                    "open current / marked in $EDITOR, come back",
                    true,
                ),
                ("i  a".into(), "enter Insert mode", false),
            ],
        ),
        (
            "Look",
            vec![
                ("Alt+p".into(), "choose palette (100 ms preview)", false),
                ("Alt+P".into(), "reload palette from config", false),
                ("F1".into(), "this guide · Tab: session", false),
            ],
        ),
        (
            "Bookmarks",
            vec![
                ("Alt+1…9 Alt+0".into(), "open visible bookmark / last", true),
                (
                    "n / e / d / r".into(),
                    "new / edit / delete / reload (Normal, else Alt+)",
                    true,
                ),
                ("↑ / ↓".into(), "move highlighted bookmark", true),
                (
                    "Tab / Shift+Tab".into(),
                    "switch bookmark form fields",
                    true,
                ),
                ("Space".into(), "choose icon on the icon row", true),
                ("Enter / Esc".into(), "save / cancel bookmark form", true),
            ],
        ),
        (
            "Mouse",
            vec![
                ("click".into(), "highlight, clearing marks", false),
                ("double click".into(), "like Alt+l", true),
                ("drag".into(), "mark the rows it spans", false),
                ("wheel".into(), "scroll the results", false),
                ("click 1 2 3".into(), "switch to that tab", true),
                ("drop files".into(), "copy them here (Normal)", true),
            ],
        ),
    ];
    let key_width = 17;
    let column = |section: &GuideSection, width: usize| {
        let mut lines = vec![vec![bold(section.0.to_uppercase(), theme.marked)]];
        if app.no_dir && section.1.iter().any(|entry| entry.2) {
            lines.push(vec![styled("crossed out: off in fff seek", theme.muted)]);
        }
        for (keys, description, navigation) in &section.1 {
            let description = elide(description, width.saturating_sub(key_width));
            if keys.is_empty() {
                lines.push(vec![styled(
                    format!("{:key_width$}{description}", ""),
                    theme.muted,
                )]);
                continue;
            }
            let key = format!("{:<key_width$}", elide(keys, key_width - 1));
            let mut line = vec![bold(key, theme.accent), styled(description, theme.text)];
            if *navigation && app.no_dir {
                line = line
                    .into_iter()
                    .map(|span| {
                        span.fg(theme.muted)
                            .add_modifier(Modifier::CROSSED_OUT)
                            .remove_modifier(Modifier::BOLD)
                    })
                    .collect();
            }
            lines.push(line);
        }
        lines.push(Vec::new());
        lines
    };
    // Two columns side by side when wide enough; otherwise one.
    let gap = 4;
    let half = width.saturating_sub(gap) / 2;
    if half >= key_width + 32 {
        // Split between sections where the taller column is shortest.
        let columns: Vec<_> = sections.iter().map(|s| column(s, half)).collect();
        let total: usize = columns.iter().map(Vec::len).sum();
        let split = (1..columns.len())
            .min_by_key(|&i| {
                let left: usize = columns[..i].iter().map(Vec::len).sum();
                left.max(total - left)
            })
            .unwrap_or(columns.len());
        let (left, right) = columns.split_at(split);
        let left: Vec<_> = left.concat();
        let right: Vec<_> = right.concat();
        (0..left.len().max(right.len()))
            .map(|i| {
                let mut spans = left.get(i).cloned().unwrap_or_default();
                let used = spans.iter().map(|s| s.width()).sum::<usize>();
                spans.push(Span::raw(" ".repeat(half + gap - used.min(half))));
                spans.extend(right.get(i).cloned().unwrap_or_default());
                Line::from(spans)
            })
            .collect()
    } else {
        sections
            .iter()
            .flat_map(|s| column(s, width))
            .map(Line::from)
            .collect()
    }
}

/// Exact paths, scopes, policies, palette, and marks.
pub(super) fn session_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let theme = app.palette;
    let mut sections: Vec<(String, Color)> = vec![
        ("CURRENT DIRECTORY".into(), theme.accent),
        (safe(app.cwd.display().to_string()), theme.text),
        (String::new(), theme.text),
        (
            format!("Depth {} · query “{}”", app.depth, safe(&app.query)),
            theme.muted,
        ),
        (String::new(), theme.text),
        ("DISCOVERY SCOPES".into(), theme.accent),
    ];
    for (i, path) in app.scope.iter().enumerate() {
        sections.push((
            format!(
                "{} · {}",
                if i < app.roots.len() {
                    "pinned"
                } else {
                    "current"
                },
                safe(path.display().to_string())
            ),
            theme.text,
        ));
    }
    if let Some(path) = &app.highlight {
        sections.push((String::new(), theme.text));
        sections.push(("HIGHLIGHTED PATH".into(), theme.accent));
        sections.push((safe(path.display().to_string()), theme.text));
        if let Some(candidate) = app.rows.iter().find(|c| &c.path == path) {
            let root = &app.scope[candidate.root];
            sections.push((
                format!(
                    "depth {} below {} · {}",
                    candidate.depth,
                    root_name(root),
                    safe(
                        path.strip_prefix(root)
                            .unwrap_or(path)
                            .display()
                            .to_string()
                    )
                ),
                theme.muted,
            ));
        }
    }
    if !app.no_dir {
        sections.push((String::new(), theme.text));
        sections.push(("TABS".into(), theme.accent));
        for (i, dir) in app.tab_dirs().into_iter().enumerate() {
            sections.push((
                format!(
                    "{}{} · {}",
                    i + 1,
                    if i == app.tab { " (active)" } else { "" },
                    safe(dir.display().to_string())
                ),
                if i == app.tab {
                    theme.text
                } else {
                    theme.muted
                },
            ));
        }
    }
    sections.push((String::new(), theme.text));
    sections.push(("POLICIES".into(), theme.accent));
    sections.push((
        format!(
            "Hidden: {} · exclusions: {}{}",
            app.policy.hidden,
            app.policy.ignore,
            if app.no_dir {
                " · files only (fff seek)"
            } else if app.files_only {
                " · directories hidden"
            } else {
                ""
            }
        ),
        theme.text,
    ));
    if app.policy.filters.is_empty() {
        sections.push(("Filters: none".into(), theme.text));
    }
    sections.extend(
        app.policy
            .filters
            .iter()
            .map(|f| (format!("Filter: {}", safe(f)), theme.muted)),
    );
    sections.extend(
        app.policy
            .rules
            .iter()
            .map(|r| (format!("Exclude: {}", safe(r)), theme.muted)),
    );
    sections.push((String::new(), theme.text));
    sections.push(("PALETTE".into(), theme.accent));
    sections.push((safe(&app.palette_name), theme.text));
    sections.push((String::new(), theme.text));
    sections.push(("MARKED PATHS".into(), theme.marked));
    sections.extend(app.marks.iter().map(|p| {
        (
            format!(
                "{}{}",
                safe(p.display().to_string()),
                if p.exists() { "" } else { " [MISSING]" }
            ),
            if p.exists() { theme.text } else { theme.error },
        )
    }));
    sections
        .into_iter()
        .flat_map(|(text, color)| {
            // Section headings are the only accent- or marked-colored entries.
            let heading = color == theme.accent || color == theme.marked;
            wrap_text(&text, width.max(1)).into_iter().map(move |s| {
                let span = styled(s, color);
                Line::from(if heading {
                    span.add_modifier(Modifier::BOLD)
                } else {
                    span
                })
            })
        })
        .collect()
}
