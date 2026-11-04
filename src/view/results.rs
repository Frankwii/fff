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
    let grep = app.contents().is_some_and(|g| g.active());
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
        let searching = app.contents().and_then(|g| g.progress());
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
        } else if !app.program().navigates() && app.query.is_empty() {
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
        let short = app.labels_shown();
        let name_of = |c: &crate::Candidate| {
            let mut name = if short {
                short_label(app, &c.path)
            } else {
                display_path(&c.path, &app.cwd)
            };
            if c.directory && !name.ends_with('/') {
                name.push('/');
            }
            name
        };
        // Matches: names in one column, so that places and lines align;
        // at most two fifths of the row, leaving the rest to the line.
        let name_column = (0..visible.len())
            .filter(|i| app.hit(app.offset + i).is_some())
            .map(|i| name_of(visible[i]).width())
            .max()
            .unwrap_or(0)
            .min(inner.width as usize * 2 / 5);
        for (i, c) in visible.iter().enumerate() {
            let row = Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            // By row: a contents query shows a file once per match.
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
            let name = name_of(c);
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
            let width = match hit {
                Some(_) => name_column.min(available),
                None => available,
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
            let (name, mut used) = filename_spans(app.palette, &name, c, &app.query, color, width);
            spans.extend(name);
            if let Some(hit) = hit {
                let pad = width.saturating_sub(used);
                spans.push(styled(" ".repeat(pad), color));
                used += pad;
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
/// A match's line, joined with those it goes on to, cut to `width` around
/// the match, which is highlighted: leading space dropped, and an ellipsis
/// where the line is cut.
pub(super) fn snippet_spans(
    theme: Palette,
    hit: &crate::grep::Hit,
    width: usize,
) -> Vec<Span<'static>> {
    let (text, parts) = hit.joined();
    let (text, parts) = lead_window(&text, &parts, width);
    fit(marked(&text, &parts, theme.text, theme), width, false)
        .pop()
        .map(|line| line.spans)
        .unwrap_or_default()
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
/// A listed path's color, from the details read in the background: drawing
/// never waits for the filesystem.
pub(super) fn path_color(app: &App, path: &Path, directory: bool) -> Color {
    info_color(app.palette, app.details.info(path).as_ref(), directory)
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
/// `label` cut to `width` columns, its matched letters highlighted. A
/// label too long to fit keeps its matched letters in view, else the start
/// of its name (the end of the label while the whole name fits), and shows
/// an ellipsis where it is cut. Returns the spans and their width.
pub(super) fn filename_spans(
    theme: Palette,
    label: &str,
    candidate: &Candidate,
    query: &str,
    name: Color,
    width: usize,
) -> (Vec<Span<'static>>, usize) {
    let basename = candidate
        .path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let start = label
        .len()
        .saturating_sub(safe(&basename).len() + usize::from(candidate.directory));
    let start = label[..start].chars().count();
    let matched = matched_letters(label, candidate, query);
    let chars: Vec<char> = label.chars().collect();
    let (from, to) = name_window(&chars, start, &matched, width);
    let mut spans = Vec::new();
    if from > 0 {
        spans.push(styled("…", theme.muted));
    }
    spans.extend(chars[from..to].iter().enumerate().map(|(n, c)| {
        let n = from + n;
        let matched = matched.contains(&n);
        // The parent part of a label is dimmed so the name stands out.
        let span = styled(
            c.to_string(),
            if matched {
                theme.marked
            } else if n < start {
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
    }));
    if to < chars.len() {
        spans.push(styled("…", if to <= start { theme.muted } else { name }));
    }
    let used = spans.iter().map(|s| s.width()).sum();
    (spans, used)
}
/// The chars `from..to` of a label shown in `width` columns, with an
/// ellipsis before a cut start and after a cut end. The matched letters
/// stay in view, centered when the default cannot hold them; the default
/// is the label's end while the name (from char `start`) fits, else the
/// name's start.
pub(super) fn name_window(
    chars: &[char],
    start: usize,
    matched: &HashSet<usize>,
    width: usize,
) -> (usize, usize) {
    let w = |c: &char| c.width().unwrap_or(0);
    let n = chars.len();
    if chars.iter().map(w).sum::<usize>() <= width {
        return (0, n);
    }
    if width == 0 {
        return (0, 0);
    }
    // From `from` rightwards, leaving room for the ellipses.
    let rightwards = |from: usize| {
        let room = width - usize::from(from > 0);
        let mut used = 0;
        let mut to = from;
        while to < n && used + w(&chars[to]) <= room {
            used += w(&chars[to]);
            to += 1;
        }
        while to < n && used + 1 > room && to > from {
            to -= 1;
            used -= w(&chars[to]);
        }
        (from, to)
    };
    // The label's end, with an ellipsis before it.
    let tail = {
        let mut used = 1;
        let mut from = n;
        while from > 0 && used + w(&chars[from - 1]) <= width {
            from -= 1;
            used += w(&chars[from]);
        }
        (from, n)
    };
    let default = if tail.0 <= start {
        tail
    } else {
        rightwards(start)
    };
    let (Some(&lo), Some(&hi)) = (matched.iter().min(), matched.iter().max()) else {
        return default;
    };
    let holds = |(from, to): (usize, usize)| from <= lo && hi < to;
    if holds(default) {
        return default;
    }
    // Center the matched letters, or start at the first when they are wider.
    let span: usize = chars[lo..=hi].iter().map(w).sum();
    let mut before = width.saturating_sub(span + 2) / 2;
    let mut from = lo;
    while from > 0 && w(&chars[from - 1]) <= before {
        from -= 1;
        before -= w(&chars[from]);
    }
    let window = rightwards(from);
    if window.1 == n || holds(tail) {
        tail
    } else {
        window
    }
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
