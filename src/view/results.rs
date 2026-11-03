//! The results pane: rows with icons, detail columns, labels and the
//! matched letters highlighted.
use super::*;

pub(super) fn results_pane(
    frame: &mut Frame,
    area: Rect,
    app: &mut App,
    focused: bool,
) -> Rendered {
    let theme = app.palette;
    // Which kinds are listed, beside the count, in a color of its own.
    let kinds = match app.listed_kinds() {
        crate::Kinds::Files => vec![bold("· files only ", theme.marked)],
        crate::Kinds::Directories => vec![bold("· folders only ", theme.marked)],
        crate::Kinds::All => Vec::new(),
    };
    let grep = app.grep.as_ref().is_some_and(|g| g.active());
    let block = panel_with(
        app.palette,
        if grep {
            format!(" Matches · {} ", app.shown().len())
        } else {
            format!(" Files · {} ", app.shown().len())
        },
        kinds,
        focused,
        theme.accent,
    );
    let inner = block.inner(area);
    let page = inner.height as usize;
    if app.scrolled.as_ref().is_some_and(|at| *at != app.highlight) {
        app.scrolled = None;
    }
    if page > 0
        && app.scrolled.is_none()
        && let Some(i) = app.highlighted()
    {
        if i < app.offset {
            app.offset = i;
        }
        if i >= app.offset + page {
            app.offset = i + 1 - page;
        }
    }
    app.offset = app.offset.min(app.shown().len().saturating_sub(1));
    let highlighted = app.highlighted();
    let app = &*app;
    let visible: Vec<_> = app.shown().iter().skip(app.offset).take(page).collect();
    let last = (app.offset + visible.len()).min(app.shown().len());
    let range = if visible.is_empty() {
        " 0 results ".into()
    } else {
        format!(" {}–{} of {} ", app.offset + 1, last, app.shown().len())
    };
    frame.render_widget(
        block
            .title_top(tab_strip(app))
            .title_bottom(Line::from(styled(
                format!(" depth {} ", app.depth),
                theme.text,
            )))
            .title_bottom(Line::from(styled(range, theme.muted)).right_aligned()),
        area,
    );
    if visible.is_empty() {
        use crate::keymap::Function;
        let deeper = || does(app.bound(Function::SearchDeeper), "searches deeper");
        let searching = app.grep.as_ref().and_then(|g| g.progress());
        let (title, hint) = if grep && searching.is_some() {
            (
                "Searching contents…",
                "Matches appear as files are read; documents are read once, then remembered."
                    .to_string(),
            )
        } else if grep {
            (
                "No matches",
                "No listed file contains the contents query; the names query chooses which are read."
                    .to_string(),
            )
        } else if app.scanning {
            (
                "Discovering files…",
                "You can keep typing while the scan runs.".to_string(),
            )
        } else if app.no_dir && app.query.is_empty() {
            (
                "No files here",
                [
                    deeper(),
                    does(app.bound(Function::Cancel), "leaves without a selection"),
                ]
                .into_iter()
                .flatten()
                .map(|clause| format!("{clause}. "))
                .collect::<String>()
                .trim_end()
                .to_string(),
            )
        } else if app.kinds == crate::Kinds::Files && app.query.is_empty() {
            (
                "No files here",
                sentence(
                    "Directories are hidden",
                    [
                        does(app.bound(Function::ToggleDirectories), "shows them"),
                        deeper(),
                    ],
                ),
            )
        } else if app.kinds == crate::Kinds::Directories && app.query.is_empty() {
            (
                "No folders here",
                sentence(
                    "Files are hidden",
                    [
                        does(app.bound(Function::ToggleFiles), "shows them"),
                        deeper(),
                    ],
                ),
            )
        } else if app.physical_empty {
            (
                "Empty directory",
                [
                    does(
                        app.bound(Function::QuitIntoHighlighted),
                        "returns this directory",
                    ),
                    does(app.bound(Function::GoToParent), "goes up"),
                ]
                .into_iter()
                .flatten()
                .map(|clause| format!("{clause}. "))
                .collect::<String>()
                .trim_end()
                .to_string(),
            )
        } else {
            (
                "No matching results",
                match app.bound(Function::ClearQuery) {
                    Some(esc) => format!("Change the query, or press {esc} to clear it."),
                    None => "Change the query.".into(),
                },
            )
        };
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(""),
                Line::from(bold(title, theme.text)),
                Line::from(""),
                Line::from(styled(hint, theme.muted)),
            ])
            .alignment(ratatui::layout::Alignment::Center)
            .wrap(Wrap { trim: false }),
            inner,
        );
    } else {
        let columns = fitting_columns(&app.columns, inner.width as usize);
        let now = std::time::SystemTime::now();
        // fff grep: names in one column, so that places and lines align;
        // at most two fifths of the row, leaving the rest to the line.
        let name_column = (0..visible.len())
            .filter(|i| app.hit(app.offset + i).is_some())
            .map(|i| short_label(app, &visible[i].path).width())
            .max()
            .unwrap_or(0)
            .min(inner.width as usize * 2 / 5);
        for (i, c) in visible.iter().enumerate() {
            let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            // By row: fff grep shows a file once per match.
            let selected = highlighted == Some(app.offset + i);
            let hit = app.hit(app.offset + i);
            let bg = if selected && focused {
                theme.selection
            } else {
                theme.panel
            };
            frame.render_widget(Block::default().style(Style::default().bg(bg)), row);
            let label = if i + 1 == visible.len() && i < 9 {
                format!("{}/0", i + 1)
            } else if i + 1 == visible.len() {
                "0".into()
            } else if i < 9 {
                (i + 1).to_string()
            } else {
                String::new()
            };
            let mut name = if app.no_dir || app.short_labels {
                short_label(app, &c.path)
            } else {
                display_path(&c.path, &app.cwd)
            };
            if c.directory && !name.ends_with('/') {
                name.push('/');
            }
            let prefix = format!("{} {:>3} ", if selected { "›" } else { " " }, label);
            let marked = app.marks.contains(&c.path);
            let info = app.details.info(&c.path);
            // Labels are relative to the current directory (or, in fff pick and fff seek,
            // as short as a shared name allows); the footer and F1 show the rest.
            let (kind, group) = crate::icons::marker(&c.path, c.directory, app.icons);
            let fixed = (prefix.width() + "● ".width() + kind.width()) as u16;
            let room = inner.width.saturating_sub(fixed) as usize;
            let meta = if hit.is_some() {
                String::new()
            } else {
                row_details(app, c, info.as_ref(), &columns, now)
            };
            let available = room.saturating_sub(meta.width());
            // A match's name leaves most of the row to where it is and its line.
            let name = match hit {
                Some(_) => {
                    let name = elide(&name, name_column.min(available));
                    let pad = name_column.min(available).saturating_sub(name.width());
                    name + &" ".repeat(pad)
                }
                None => elide(&name, available),
            };
            let mut spans = vec![
                styled(prefix, if selected { theme.accent } else { theme.muted }),
                styled(if marked { "● " } else { "  " }, theme.marked),
                styled(kind, icon_color(theme, group)),
            ];
            let color = info_color(theme, info.as_ref(), c.directory);
            // A file's further matches dim its name: the first one stands out.
            let color = if hit.is_some() && i > 0 && std::sync::Arc::ptr_eq(visible[i - 1], c) {
                theme.muted
            } else {
                color
            };
            spans.extend(filename_spans(app.palette, &name, c, &app.query, color));
            let mut used = name.width();
            if let Some(hit) = hit {
                let location = format!("  {:>7}  ", hit.location());
                let room = available.saturating_sub(used + location.width());
                used += location.width();
                spans.push(styled(location, theme.muted));
                let line = snippet_spans(theme, hit, room);
                used += line.iter().map(|s| s.width()).sum::<usize>();
                spans.extend(line);
            } else if let Some((target, _)) = info.as_ref().and_then(|i| i.link.as_ref()) {
                // A link's target follows its name while there is room for it.
                let arrow = " → ";
                let space = available.saturating_sub(used + arrow.width() + 1);
                if space >= 3 {
                    let target = elide(&safe(target.to_string_lossy()), space);
                    used += arrow.width() + target.width();
                    spans.push(styled(arrow, color));
                    spans.push(styled(target, theme.muted));
                }
            }
            let gap = available.saturating_sub(used);
            spans.push(styled(" ".repeat(gap) + &meta, theme.muted));
            frame.render_widget(
                Paragraph::new(Line::from(spans)).style(Style::default().bg(bg)),
                row,
            );
        }
    }
    Rendered {
        targets: visible.iter().map(|c| c.path.clone()).collect(),
        bookmark_targets: Vec::new(),
        page,
        results: inner,
        buttons: Vec::new(),
        tabs: tab_areas(app, area),
        marks_pane: Rect::default(),
        marks: Vec::new(),
        choices: Vec::new(),
    }
}
/// A match's line cut to `width` around the match, which is highlighted:
/// leading space dropped, and an ellipsis where the line is cut.
pub(super) fn snippet_spans(
    theme: Palette,
    hit: &crate::grep::Hit,
    width: usize,
) -> Vec<Span<'static>> {
    let line = &hit.context[hit.at];
    let trimmed = line.len() - line.trim_start().len();
    // Offsets come from the engine; one off a character boundary shows the
    // line unhighlighted rather than failing the frame.
    let (start, end) = (hit.matched.start.max(trimmed), hit.matched.end.max(trimmed));
    let matched = if end <= line.len() && line.is_char_boundary(start) && line.is_char_boundary(end)
    {
        start..end
    } else {
        trimmed..trimmed
    };
    let (text, matched) = (
        &line[trimmed..],
        matched.start - trimmed..matched.end - trimmed,
    );
    if width == 0 {
        return Vec::new();
    }
    let (before, found, after) = (
        safe(&text[..matched.start]),
        safe(&text[matched.clone()]),
        safe(&text[matched.end..]),
    );
    // Keep some of what comes before the match, the match, then the rest.
    let lead = before.width().min(width / 4);
    let before: String = if lead < before.width() {
        let kept: String = before
            .chars()
            .rev()
            .scan(1, |used, c| {
                *used += c.width().unwrap_or(0);
                (*used <= lead).then_some(c)
            })
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        format!("…{kept}")
    } else {
        before
    };
    let found = truncate(&found, width.saturating_sub(before.width()));
    let after = truncate(&after, width.saturating_sub(before.width() + found.width()));
    vec![
        styled(before, theme.text),
        styled(found, theme.marked).add_modifier(Modifier::BOLD),
        styled(after, theme.text),
    ]
}
/// Icons reuse palette roles, so custom palettes and `NO_COLOR` apply unchanged.
pub(super) fn icon_color(theme: Palette, group: crate::icons::Group) -> Color {
    use crate::icons::Group;
    match group {
        Group::Directory => theme.accent,
        Group::Code => theme.success,
        Group::Media | Group::Archive => theme.marked,
        Group::Document => theme.text,
        Group::Config | Group::Other => theme.muted,
    }
}
/// Broken links, links, then directories, empty files and executables get
/// their own roles. Only visible rows are inspected, so discovery stays free of
/// extra stats.
pub(super) fn info_color(
    theme: Palette,
    info: Option<&crate::details::Info>,
    directory: bool,
) -> Color {
    let file = |i: &crate::details::Info| i.mode & libc::S_IFMT == libc::S_IFREG;
    match info {
        Some(i) if i.broken() => theme.error,
        Some(i) if i.link.is_some() => theme.link,
        _ if directory => theme.accent,
        Some(i) if file(i) && i.size == 0 => theme.empty,
        Some(i) if file(i) && i.mode & 0o111 != 0 => theme.executable,
        _ => theme.text,
    }
}
pub(super) fn path_color(theme: Palette, path: &Path, directory: bool) -> Color {
    info_color(theme, crate::details::Info::read(path).as_ref(), directory)
}
/// Each column's width, so that columns line up from row to row.
pub(super) fn column_width(column: Column) -> usize {
    match column {
        Column::Permissions => 10,
        Column::Size => 5,
        Column::Modified => 4,
    }
}
/// Names come first: columns are dropped from the left until a name has at
/// least `NAME_ROOM` columns.
pub(super) const NAME_ROOM: usize = 20;
pub(super) fn fitting_columns(columns: &[Column], width: usize) -> Vec<Column> {
    let mut columns = columns.to_vec();
    let total = |c: &[Column]| c.iter().map(|&c| column_width(c) + 2).sum::<usize>();
    // The row prefix, mark and icon take about 10 columns.
    while !columns.is_empty() && width.saturating_sub(10 + total(&columns)) < NAME_ROOM {
        columns.remove(0);
    }
    columns
}
/// The right-hand details of a result row, each column right-aligned, with a
/// trailing space; blank where a value is unknown (yet).
pub(super) fn row_details(
    app: &App,
    candidate: &Candidate,
    info: Option<&crate::details::Info>,
    columns: &[Column],
    now: std::time::SystemTime,
) -> String {
    use crate::details;
    if columns.is_empty() {
        return " ".into();
    }
    let mut text = String::new();
    for &column in columns {
        let value = match (column, info) {
            (_, None) => String::new(),
            (Column::Permissions, Some(i)) => details::permissions(i.mode),
            (Column::Size, Some(i)) if candidate.directory || i.directory => app
                .details
                .count(&candidate.path)
                .map(details::entries)
                .unwrap_or_default(),
            (Column::Size, Some(i)) if i.broken() => String::new(),
            (Column::Size, Some(i)) => details::size(i.size),
            (Column::Modified, Some(i)) => {
                i.modified.map(|m| details::age(m, now)).unwrap_or_default()
            }
        };
        text.push_str(&format!("  {value:>0$}", column_width(column)));
    }
    text.push(' ');
    text
}
pub(super) fn filename_spans(
    theme: Palette,
    label: &str,
    candidate: &Candidate,
    query: &str,
    name: Color,
) -> Vec<Span<'static>> {
    let basename = candidate
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let start = label
        .len()
        .saturating_sub(safe(&basename).len() + usize::from(candidate.directory));
    let matched = matched_letters(label, candidate, query);
    label
        .char_indices()
        .enumerate()
        .map(|(n, (i, c))| {
            let matched = matched.contains(&n);
            // The parent part of a label is dimmed so the name stands out.
            let span = styled(
                c.to_string(),
                if matched {
                    theme.marked
                } else if i < start {
                    theme.muted
                } else {
                    name
                },
            );
            if matched {
                span.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
            } else {
                span
            }
        })
        .collect()
}
/// Char indices of `label` the query matches: letters of the name, or with
/// a `/` in the query, along the whole path. Only a row the query matches is
/// highlighted (a save dialog lists the others too).
pub(super) fn matched_letters(label: &str, candidate: &Candidate, query: &str) -> HashSet<usize> {
    let matcher = crate::Matcher::new(query);
    let (parts, positions) = if matcher.paths() {
        let parts = crate::relative_parts(candidate);
        let Some(positions) = matcher.path_positions(&parts) else {
            return HashSet::new();
        };
        (parts, positions)
    } else {
        let basename = candidate
            .path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        let Some(positions) = matcher.positions(&basename) else {
            return HashSet::new();
        };
        (vec![basename], vec![positions])
    };
    // The label ends with the name, or the path below the root (and a slash
    // after a folder); align from the end, where elision and shared-name
    // labels keep it intact.
    let mut path = Vec::new();
    let mut wanted = HashSet::new();
    for (part, at) in parts.iter().zip(&positions) {
        if !path.is_empty() {
            path.push('/');
        }
        wanted.extend(at.iter().map(|i| path.len() + i));
        path.extend(part.chars());
    }
    let label: Vec<char> = label.chars().collect();
    let end = label.len() - usize::from(candidate.directory && label.last() == Some(&'/'));
    wanted
        .into_iter()
        .filter_map(|i| {
            let at = (end + i).checked_sub(path.len())?;
            (label.get(at) == Some(&path[i])).then_some(at)
        })
        .collect()
}
