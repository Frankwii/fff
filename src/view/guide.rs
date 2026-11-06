use super::*;

/// F2: the session details, with a tab to the Keys screen beside.
pub(super) fn info(frame: &mut Frame, area: Rect, app: &App, offset: &mut usize) {
    let theme = app.palette;
    // Each tab with the key that opens it, as the files pane numbers its tabs.
    let tab = |function, name: &'static str, active: bool| {
        let key = app
            .bound(function)
            .map(|key| format!("{key} "))
            .unwrap_or_default();
        if active {
            vec![
                pill(format!("{key}{name}"), theme.background, theme.accent)
                    .add_modifier(Modifier::BOLD),
            ]
        } else {
            vec![
                Span::raw(" "),
                bold(key, theme.accent),
                styled(format!("{name} "), theme.muted),
            ]
        }
    };
    let hints = border_hints(
        app,
        &[("up down", "scroll"), ("esc", "close")],
        theme.accent,
        (area.width as usize).saturating_sub(4),
    );
    let block = panel(theme, "", true, theme.accent)
        .title(Line::from(
            [
                vec![Span::raw(" ")],
                tab(crate::keymap::Function::ShowKeys, "Keys", false),
                vec![Span::raw(" ")],
                tab(crate::keymap::Function::ShowSession, "Session", true),
                vec![Span::raw(" ")],
            ]
            .concat(),
        ))
        .title_bottom(Line::from(hints).centered())
        .padding(Padding::new(2, 2, 1, 0));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let lines = session_lines(app, inner.width as usize);
    *offset = (*offset).min(lines.len().saturating_sub(inner.height as usize));
    frame.render_widget(
        Paragraph::new(lines).scroll(((*offset).min(u16::MAX as usize) as u16, 0)),
        inner,
    );
}

/// Exact paths, scopes, policies, palette, and marks.
pub(super) fn session_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let theme = app.palette;
    let mut sections: Vec<(String, Color)> = vec![
        ("CURRENT DIRECTORY".into(), theme.accent),
        (safe(app.cwd.display().to_string()), theme.text),
        (String::new(), theme.text),
        (
            match (app.program().scans(), app.query.is_empty()) {
                (true, true) => format!("Depth {} · no query", app.depth),
                (true, false) => format!("Depth {} · query “{}”", app.depth, safe(&app.query)),
                (false, true) => "Paths read from stdin · no query".into(),
                (false, false) => format!("Paths read from stdin · query “{}”", safe(&app.query)),
            },
            theme.muted,
        ),
    ];
    if app.program().scans() {
        sections.push((String::new(), theme.text));
        sections.push(("DISCOVERY SCOPES".into(), theme.accent));
    }
    for (i, path) in app
        .scope
        .iter()
        .enumerate()
        .filter(|_| app.program().scans())
    {
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
        if let Some(candidate) = app
            .shown_position(|c| &c.path == path)
            .and_then(|i| app.shown_row(i))
            .filter(|_| app.program().scans())
        {
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
    if app.program().navigates() {
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
    if !app.program().scans() {
        sections.push((
            format!(
                "Listed as read: nothing hidden or excluded{}",
                match app.kinds {
                    crate::Kinds::Files => " · directories hidden",
                    crate::Kinds::Directories => " · files hidden",
                    crate::Kinds::All => "",
                }
            ),
            theme.text,
        ));
    } else {
        sections.push((
            format!(
                "Hidden entries {} · exclusions {}{}",
                if app.policy.hidden {
                    "shown"
                } else {
                    "not shown"
                },
                if app.policy.ignore { "enforced" } else { "off" },
                if app.program().files_only() {
                    " · files only (fff seek)"
                } else {
                    match app.kinds {
                        crate::Kinds::Files => " · directories hidden",
                        crate::Kinds::Directories => " · files hidden",
                        crate::Kinds::All => "",
                    }
                }
            ),
            theme.text,
        ));
        if app.policy.filters.is_empty() {
            sections.push(("Filters: none".into(), theme.text));
        } else if app.policy.filters_off {
            sections.push(("Filters: off".into(), theme.text));
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
    }
    sections.push((String::new(), theme.text));
    sections.push(("PALETTE".into(), theme.accent));
    sections.push((safe(&app.palette_name), theme.text));
    sections.push((String::new(), theme.text));
    sections.push(("CONFIGURATION".into(), theme.accent));
    match (&app.config_path, &app.config_broken) {
        (Some(path), Some(error)) => {
            sections.push((safe(crate::config::reading_backup(path) + ":"), theme.error));
            // TOML's excerpt of the file, its carets under the error.
            sections.extend(
                crate::config::config_error(path, error)
                    .lines()
                    .map(|line| (safe(line), theme.text)),
            );
        }
        (Some(path), None) => sections.push((safe(path.display().to_string()), theme.text)),
        (None, _) => sections.push(("None (--no-config)".into(), theme.text)),
    }
    if let Some(error) = &app.keys_broken {
        sections.push((String::new(), theme.text));
        sections.push(("KEYS".into(), theme.accent));
        sections.push((
            "keys.toml has errors, so the default keys are in use:".into(),
            theme.error,
        ));
        sections.extend(error.lines().map(|line| (safe(line), theme.text)));
    }
    sections.push((String::new(), theme.text));
    sections.push(("MARKED PATHS".into(), theme.marked));
    sections.extend(app.marks.iter().map(|p| {
        let found = app.details.lookup(p);
        let missing = matches!(&found, Some(None)) || found.iter().flatten().any(|i| i.broken());
        (
            format!(
                "{}{}",
                safe(p.display().to_string()),
                if missing { " [MISSING]" } else { "" }
            ),
            if missing { theme.error } else { theme.text },
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
