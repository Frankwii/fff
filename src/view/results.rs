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
    let block = panel(
        app.palette,
        format!(" Files · {} ", app.rows.len()),
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
    app.offset = app.offset.min(app.rows.len().saturating_sub(1));
    let visible: Vec<_> = app.rows.iter().skip(app.offset).take(page).collect();
    let last = (app.offset + visible.len()).min(app.rows.len());
    let range = if visible.is_empty() {
        " 0 results ".into()
    } else {
        format!(" {}–{} of {} ", app.offset + 1, last, app.rows.len())
    };
    frame.render_widget(
        block
            .title_top(tab_strip(app))
            .title_bottom(Line::from(styled(range, theme.muted)).right_aligned()),
        area,
    );
    if visible.is_empty() {
        let (title, hint) = if app.scanning {
            (
                "Discovering files…",
                "You can keep typing while the scan runs.",
            )
        } else if app.no_dir && app.query.is_empty() {
            (
                "No files here",
                "Alt+→ searches deeper. Ctrl+c leaves without a selection.",
            )
        } else if app.kinds == crate::Kinds::Files && app.query.is_empty() {
            (
                "No files here",
                "Directories are hidden: Alt+d shows them, Alt+→ searches deeper.",
            )
        } else if app.kinds == crate::Kinds::Directories && app.query.is_empty() {
            (
                "No folders here",
                "Files are hidden: Alt+D shows them, Alt+→ searches deeper.",
            )
        } else if app.physical_empty {
            (
                "Empty directory",
                "Enter returns this directory. Alt+h goes up.",
            )
        } else {
            (
                "No matching results",
                "Change the query, or press Esc to clear it.",
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
        for (i, c) in visible.iter().enumerate() {
            let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            let selected = Some(&c.path) == app.highlight.as_ref();
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
            let meta = row_details(app, c, info.as_ref(), &columns, now);
            let available = room.saturating_sub(meta.width());
            let name = elide(&name, available);
            let mut spans = vec![
                styled(prefix, if selected { theme.accent } else { theme.muted }),
                styled(if marked { "● " } else { "  " }, theme.marked),
                styled(kind, icon_color(theme, group)),
            ];
            let color = info_color(theme, info.as_ref(), c.directory);
            spans.extend(filename_spans(app.palette, &name, c, &app.query, color));
            let mut used = name.width();
            // A link's target follows its name while there is room for it.
            if let Some((target, _)) = info.as_ref().and_then(|i| i.link.as_ref()) {
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
    let matched = matched_letters(label, candidate, query, start);
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
/// Char indices of `label` the query matches: letters of the name, which
/// starts at byte `start`, or with a `/` in the query, along the whole path.
/// Only a row the query matches is highlighted (a save dialog lists the
/// others too).
pub(super) fn matched_letters(
    label: &str,
    candidate: &Candidate,
    query: &str,
    start: usize,
) -> HashSet<usize> {
    let matcher = crate::Matcher::new(query);
    if matcher.paths() {
        let parts = crate::relative_parts(candidate);
        let Some(positions) = matcher.path_positions(&parts) else {
            return HashSet::new();
        };
        // The label ends with the path below the root (and a slash after a
        // folder); align from the end, where elision and shared-name labels
        // keep it intact.
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
        return wanted
            .into_iter()
            .filter_map(|i| {
                let at = (end + i).checked_sub(path.len())?;
                (label.get(at) == Some(&path[i])).then_some(at)
            })
            .collect();
    }
    let basename = candidate
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    if matcher.score(&basename).is_none() {
        return HashSet::new();
    }
    // `start` is a byte offset that may fall inside the `…` of an elided
    // label: count the characters before it rather than slicing there.
    let first = label.char_indices().take_while(|&(i, _)| i < start).count();
    let letters: Vec<char> = crate::fold(query).chars().collect();
    let name: String = label.chars().skip(first).collect();
    crate::greedy(&letters, &name)
        .into_iter()
        .map(|i| first + i)
        .collect()
}
