use super::*;

/// F1 / F2 overlay with two tabs: the key guide and session details.
pub(super) fn info(frame: &mut Frame, area: Rect, app: &App, offset: &mut usize, keys: bool) {
    let theme = app.palette;
    // Each tab with the key that opens it, as the files pane numbers its tabs.
    let tab = |key: &str, name: &'static str, active: bool| {
        let key = app
            .key(key)
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
        &[("tab", "switch"), ("up down", "scroll"), ("esc", "close")],
        theme.accent,
        (area.width as usize).saturating_sub(4),
    );
    let block = panel(theme, "", true, theme.accent)
        .title(Line::from(
            [
                vec![Span::raw(" ")],
                tab("f1", "Keys", keys),
                vec![Span::raw(" ")],
                tab("f2", "Session", !keys),
                vec![Span::raw(" ")],
            ]
            .concat(),
        ))
        .title_bottom(Line::from(hints).centered())
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
pub(super) type GuideSection = (&'static str, Vec<(String, String, bool)>);
/// Every binding, grouped, each key named as the configuration binds it:
/// `[keys.map]` chords by the key that now does their work, `[keys.normal]`
/// sequences as configured. Navigation keys are dimmed in `fff seek`.
pub(super) fn key_guide(app: &App, width: usize) -> Vec<Line<'static>> {
    use crate::normal_keys::ListAction::*;
    let theme = app.palette;
    let normal = |action| app.sequence(action).unwrap_or_else(|| "—".into());
    // Chords by their default names ("up down" for a pair drawn together);
    // those no key does are left out.
    let keys = |names: &[&str]| {
        let named = names
            .iter()
            .filter_map(|n| keys_named(app, n))
            .collect::<Vec<_>>();
        if named.is_empty() {
            "—".to_string()
        } else {
            named.join("  ")
        }
    };
    let key = |name: &str| app.key_or_none(name);
    let digits = |modifier: &str, name: &str| {
        if app.digits_pick(modifier) {
            format!("{name}+1…9 {name}+0")
        } else {
            "—".into()
        }
    };
    let row = |keys: String, description: &str, navigation: bool| {
        (keys, description.to_string(), navigation)
    };
    let note = |description: &str| (String::new(), description.to_string(), false);
    let sections: Vec<GuideSection> = vec![
        (
            "Navigate",
            vec![
                row(
                    keys(&["up down", "alt+k", "alt+j"]),
                    "move the highlight",
                    false,
                ),
                row(keys(&["pageup", "pagedown"]), "previous / next page", false),
                row(
                    key("enter"),
                    if app.window {
                        "Normal: open file and leave; enter dir"
                    } else {
                        "Normal: open file and leave; leave into dir"
                    },
                    false,
                ),
                note("typing: open dir, reveal / return file"),
                row(
                    key("alt+l"),
                    "enter directory, or open file in background",
                    true,
                ),
                row(key("alt+h"), "parent directory, keeping query", true),
                row(
                    key("alt+g"),
                    "go to a path, bookmark or zoxide folder",
                    true,
                ),
                row(
                    keys(&["ctrl+o", "ctrl+i"]),
                    "back / forward through visited folders",
                    true,
                ),
                row(key("alt+b"), "open a bookmark", true),
                row(key("alt+t"), "open the trash; again, the next one", true),
                row(
                    keys(&["alt+up", "alt+down"]),
                    "next / previous of 3 tabs; marks shared",
                    true,
                ),
            ],
        ),
        (
            "Select and return",
            vec![
                row(key("shift+space"), "mark / unmark, then move down", false),
                row(key("alt+space"), "focus the marked list", false),
                row(key("ctrl+w"), "return the marked paths (fff pick)", false),
                row(
                    digits("alt", "Alt"),
                    &format!("like {} on row n", key("alt+l")),
                    false,
                ),
                row(
                    digits("ctrl", "Ctrl"),
                    &format!("like {} on row n", key("enter")),
                    false,
                ),
                note("0 is the last visible row"),
                row(key("ctrl+c"), "quit without returning anything", false),
            ],
        ),
        (
            "Search",
            vec![
                row("type".into(), "fuzzy-filter names; with /, paths", false),
                row(key("esc"), "leave Insert, then clear query", false),
                row(
                    app.key("esc")
                        .map_or_else(|| "—".into(), |esc| format!("{esc} {esc}")),
                    "with no query: clear the marks",
                    false,
                ),
                row(
                    keys(&["alt+left", "alt+right"]),
                    "search shallower / deeper",
                    false,
                ),
                row(key("alt+."), "show / hide hidden entries", false),
                row(key("alt+d"), "hide / show directories", true),
                row(key("alt+D"), "hide / show files", true),
                row(key("alt+v"), "show / hide the preview pane", false),
                row(
                    keys(&["ctrl+j", "ctrl+k"]),
                    "scroll the preview down / up",
                    false,
                ),
                row(
                    [Move, Copy, Link].map(normal).join(" / "),
                    "move / copy / symlink selected (Normal)",
                    false,
                ),
                row(
                    [Delete, Rename, Extract, New].map(normal).join(" / "),
                    "trash, rename, extract, new (Normal)",
                    false,
                ),
                row(key("f5"), "rescan, bypassing the cache", false),
            ],
        ),
        (
            "Exclusions and filters",
            vec![
                row(key("alt+e"), "edit exclusions, one glob per line", false),
                note("matching entries and subtrees hidden"),
                note("by name or scope-relative path"),
                row(key("alt+H"), "enforce exclusions on / off", false),
                row(key("alt+f"), "edit file filters, one glob per line", false),
                row(key("alt+F"), "file filters off / on, keeping them", false),
                row(
                    [
                        EditExclusions,
                        EnforceExclusions,
                        EditFilters,
                        ToggleFilters,
                    ]
                    .map(normal)
                    .join(" / "),
                    "the same four in Normal mode",
                    false,
                ),
                note("only files whose name matches one"),
                note("are listed; empty lists every file"),
                row("ZZ".into(), "apply", false),
                row("ZQ".into(), "discard", false),
                note("ZZ/ZQ also apply/discard renames"),
            ],
        ),
        (
            "Normal mode in lists",
            vec![
                note("Alt chords for features work while typing;"),
                note("these keys do their work in Normal mode"),
                row(normal(Down) + "  " + &normal(Up), "move down / up", false),
                row(
                    normal(Left),
                    &format!("parent directory, like {}", key("alt+h")),
                    true,
                ),
                row(
                    normal(Right),
                    &format!("enter directory / open file, like {}", key("alt+l")),
                    true,
                ),
                row(
                    normal(Open),
                    "open current / marked files here, come back",
                    false,
                ),
                row(
                    normal(First) + "  " + &normal(Last),
                    "first / last row",
                    false,
                ),
                row(
                    normal(Mark),
                    &format!("mark / unmark, like {}", key("shift+space")),
                    false,
                ),
                row(
                    normal(MarkAll) + "  " + &normal(InvertMarks),
                    "mark all listed / invert their marks",
                    false,
                ),
                row(
                    normal(Shallower) + "  " + &normal(Deeper),
                    "search shallower / deeper",
                    false,
                ),
                row(
                    if app.normal_keys.depth_digits {
                        "1…9".into()
                    } else {
                        "—".into()
                    },
                    "search to depth n (results)",
                    false,
                ),
                row(normal(Hidden), "show / hide hidden entries", false),
                row(normal(Directories), "hide / show directories", true),
                row(normal(Files), "hide / show files", true),
                row(normal(Preview), "show / hide the preview pane", false),
                row(normal(GoTo), "go to a path, bookmark or zoxide", true),
                row(normal(Bookmarks), "open a bookmark", true),
                row(
                    normal(NextTab) + "  " + &normal(PrevTab),
                    "next / previous tab",
                    true,
                ),
                row(normal(Quit), "exit, choosing nothing", true),
                row(
                    normal(QuitCurrent),
                    if app.window {
                        "exit into a shell in this directory"
                    } else {
                        "exit, return current directory"
                    },
                    true,
                ),
                row(
                    normal(Run),
                    "run a program on current / marked, come back",
                    true,
                ),
                row(
                    normal(RunExit),
                    "run a program on current / marked, exit",
                    true,
                ),
                row(
                    normal(Edit),
                    "open current / marked in $EDITOR, come back",
                    true,
                ),
                row(
                    normal(DeletePermanently),
                    "delete current / marked permanently",
                    false,
                ),
                row(normal(Undo), "undo the last file operation", false),
                row(
                    normal(Trash),
                    &format!("open the trash, like {}", key("alt+t")),
                    true,
                ),
                row(
                    normal(Restore),
                    "in the trash: restore current / marked",
                    false,
                ),
                row("i  a".into(), "enter Insert mode", false),
            ],
        ),
        (
            "Look",
            vec![
                row(key("alt+p"), "choose palette (100 ms preview)", false),
                row(key("alt+P"), "reload palette from config", false),
                row(
                    normal(Palette) + "  " + &normal(ReloadPalette),
                    "the same two in Normal mode",
                    false,
                ),
                row(
                    normal(EditConfig),
                    "edit the configuration, then apply it",
                    false,
                ),
                row(key("f1"), "this guide", false),
                row(key("f2"), "session details and config errors", false),
            ],
        ),
        (
            "Bookmarks",
            vec![
                row(digits("alt", "Alt"), "open visible bookmark / last", true),
                row(
                    "n / e / d / r".into(),
                    if ["alt+n", "alt+e", "alt+d", "alt+r"]
                        .iter()
                        .all(|k| app.key_map.keeps(k))
                    {
                        "new / edit / delete / reload (Normal, else Alt+)"
                    } else {
                        "new / edit / delete / reload (Normal)"
                    },
                    true,
                ),
                row(
                    [key("up"), key("down")].join(" / "),
                    "move highlighted bookmark",
                    true,
                ),
                row(
                    format!("{} / Shift+Tab", key("tab")),
                    "switch bookmark form fields",
                    true,
                ),
                row("Space".into(), "choose icon on the icon row", true),
                row(
                    [key("enter"), key("esc")].join(" / "),
                    "save / cancel bookmark form",
                    true,
                ),
            ],
        ),
        (
            "Mouse",
            vec![
                row("click".into(), "highlight, clearing marks", false),
                row(
                    "double click".into(),
                    &format!("like {}", key("alt+l")),
                    true,
                ),
                row("drag".into(), "mark the rows it spans", false),
                row("wheel".into(), "scroll the results", false),
                row("click 1 2 3".into(), "switch to that tab", true),
                row("drop files".into(), "copy them here (Normal)", true),
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
            // A narrow guide cuts a description short at its end: its first
            // words say what the key does.
            let description = truncate(description, width.saturating_sub(key_width));
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
            if app.query.is_empty() {
                format!("Depth {} · no query", app.depth)
            } else {
                format!("Depth {} · query “{}”", app.depth, safe(&app.query))
            },
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
            "Hidden entries {} · exclusions {}{}",
            if app.policy.hidden {
                "shown"
            } else {
                "not shown"
            },
            if app.policy.ignore { "enforced" } else { "off" },
            if app.no_dir {
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
