use super::*;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};
fn fixture() -> App {
    let mut app = App::browse("/home/frank/notes".into(), 3, crate::Policy::default());
    for (name, directory) in [
        ("algorithms", true),
        ("research", true),
        ("report.tex", false),
        ("reading-list.md", false),
        ("todo.txt", false),
    ] {
        app.insert(
            0,
            Candidate {
                path: app.cwd.join(name),
                directory,
                depth: 1,
                root: 0,
            },
        );
    }
    app.refresh();
    app.scanning = false;
    app.marks = vec![
        "/home/frank/papers/Turing 1952.pdf".into(),
        "/home/frank/notes/report.tex".into(),
    ];
    app
}
#[test]
fn result_names_are_colored_by_file_kind() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("fff-kinds-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("folder")).unwrap();
    std::fs::write(root.join("empty"), b"").unwrap();
    std::fs::write(root.join("plain"), b"text").unwrap();
    std::fs::write(root.join("script"), b"#!/bin/sh").unwrap();
    std::fs::set_permissions(root.join("script"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::os::unix::fs::symlink("plain", root.join("link")).unwrap();
    std::os::unix::fs::symlink("folder", root.join("dirlink")).unwrap();
    std::os::unix::fs::symlink("gone", root.join("broken")).unwrap();
    let mut app = App::browse(root.clone(), 1, crate::Policy::default());
    for (name, directory) in [
        ("broken", false),
        ("empty", false),
        ("plain", false),
        ("script", false),
        ("link", false),
        ("dirlink", true),
        ("folder", true),
    ] {
        app.insert(
            0,
            Candidate {
                path: root.join(name),
                directory,
                depth: 1,
                root: 0,
            },
        );
    }
    app.refresh();
    app.scanning = false;
    app.highlight = None;
    let theme = app.palette;
    let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal
        .draw(|frame| {
            results_pane(frame, frame.area(), &mut app, false);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let color_of = |name: &str| {
        let row = (0..buffer.area.height)
            .find(|&y| {
                let line = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>();
                // A link's row also shows its target's name.
                line.contains(&format!(" {name}")) && !line.contains(&format!("→ {name}"))
            })
            .unwrap_or_else(|| panic!("{name} not shown"));
        let line = (0..buffer.area.width)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>();
        let column = line[..line.find(&format!(" {name}")).unwrap() + 1]
            .chars()
            .count();
        buffer[(column as u16, row)].fg
    };
    assert_eq!(color_of("empty"), theme.empty);
    assert_eq!(color_of("plain"), theme.text);
    assert_eq!(color_of("script"), theme.executable);
    assert_eq!(color_of("link"), theme.link);
    assert_eq!(color_of("dirlink/"), theme.link);
    assert_eq!(color_of("folder/"), theme.accent);
    assert_eq!(color_of("broken"), theme.error);
    let shown = text(buffer);
    assert!(shown.contains("link → plain"), "{shown}");
    assert!(shown.contains("dirlink/ → folder"), "{shown}");
    assert!(shown.contains("broken → gone"), "{shown}");
    std::fs::remove_dir_all(&root).unwrap();
}
#[test]
fn rows_and_footer_show_details_that_fit() {
    let root = std::env::temp_dir().join(format!("fff-columns-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("folder/inner")).unwrap();
    std::fs::write(root.join("folder/one"), b"").unwrap();
    std::fs::write(root.join("data.bin"), vec![0u8; 4300]).unwrap();
    let mut app = App::browse(root.clone(), 1, crate::Policy::default());
    for (name, directory) in [("data.bin", false), ("folder", true)] {
        app.insert(
            0,
            Candidate {
                path: root.join(name),
                directory,
                depth: 1,
                root: 0,
            },
        );
    }
    app.refresh();
    app.scanning = false;
    app.highlight = Some(root.join("data.bin"));
    app.columns = vec![Column::Permissions, Column::Size, Column::Modified];
    let draw = |app: &mut App, width: u16| {
        let mut terminal = Terminal::new(TestBackend::new(width, 8)).unwrap();
        terminal
            .draw(|frame| {
                results_pane(frame, Rect::new(0, 0, width, 7), app, true);
                footer(frame, Rect::new(0, 7, width, 1), app, &Focus::Results);
            })
            .unwrap();
        text(terminal.backend().buffer())
    };
    // The entry count arrives from the worker; until then the column is blank.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let _ = draw(&mut app, 100);
    while !app.details.drain() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let wide = draw(&mut app, 100);
    let row = |shown: &str, name: &str| {
        shown
            .lines()
            .find(|l| l.contains(name))
            .unwrap_or_else(|| panic!("{name} missing in {shown}"))
            .to_string()
    };
    let data = row(&wide, "data.bin");
    assert!(data.contains("-rw"), "{data}");
    assert!(data.contains(" 4.2K   now"), "{data}");
    let folder = row(&wide, "folder/");
    assert!(folder.contains("drwx"), "{folder}");
    assert!(folder.contains("     2   now"), "{folder}");
    let footer = wide.lines().last().unwrap();
    assert!(footer.contains("data.bin  4.2K (4,300 B)  "), "{footer}");
    assert!(footer.contains("-rw"), "{footer}");
    // Narrow panes drop permissions first; the name keeps its room.
    let narrow = draw(&mut app, 46);
    let data = row(&narrow, "data.bin");
    assert!(!data.contains("-rw") && data.contains("4.2K"), "{data}");
    let footer = narrow.lines().last().unwrap();
    assert!(
        footer.contains("data.bin") && !footer.contains(":"),
        "{footer}"
    );
    app.columns.clear();
    let bare = draw(&mut app, 100);
    assert!(!row(&bare, "data.bin").contains("4.2K"), "{bare}");
    std::fs::remove_dir_all(&root).unwrap();
}
#[test]
fn a_wheel_scroll_keeps_its_view_until_the_highlight_moves() {
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    for i in 0..20 {
        app.insert(
            0,
            Candidate {
                path: format!("/fixture/f{i:02}").into(),
                directory: false,
                depth: 1,
                root: 0,
            },
        );
    }
    app.refresh();
    app.scanning = false;
    app.highlight = Some("/fixture/f00".into());
    let mut terminal = Terminal::new(TestBackend::new(60, 7)).unwrap();
    let mut draw = |app: &mut App| {
        terminal
            .draw(|frame| {
                results_pane(frame, frame.area(), app, true);
            })
            .unwrap();
    };
    app.offset = 9;
    app.scrolled = Some(app.highlight.clone());
    draw(&mut app);
    assert_eq!(app.offset, 9);
    app.move_by(1);
    draw(&mut app);
    assert_eq!((app.offset, app.scrolled.clone()), (1, None));
}
fn text(buffer: &Buffer) -> String {
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn split_panes_colors_and_rendered_numbers() {
    let theme = Palette::default();
    let mut app = fixture();
    let mut focus = Focus::Results;
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    let mut targets = Vec::new();
    terminal
        .draw(|frame| targets = render(frame, &mut app, &mut focus).targets)
        .unwrap();
    let buffer = terminal.backend().buffer();
    let screen = text(buffer);
    let files = screen.lines().find(|s| s.contains("Files ·")).unwrap();
    assert!(files.find("Marks (2)").unwrap() > files.find("Files ·").unwrap());
    assert_eq!(
        targets,
        app.rows.iter().map(|c| c.path.clone()).collect::<Vec<_>>()
    );
    assert!(buffer.content().iter().any(|c| c.bg == theme.selection));
    assert!(buffer.content().iter().any(|c| c.fg == theme.accent));
    // Keys live in the F1 guide; the footer only points to it.
    assert!(screen.contains("F1 keys"));
    assert!(!screen.contains("Alt+#"));
    assert!(screen.contains("[MISSING]"));
    export_preview(buffer);
}
#[test]
fn narrow_tiny_unicode_and_resize_frames_have_only_visible_targets() {
    let mut app = fixture();
    app.insert(
        0,
        Candidate {
            path: app.cwd.join("研究_notes.md"),
            directory: false,
            depth: 1,
            root: 0,
        },
    );
    app.refresh();
    let mut focus = Focus::Results;
    let mut terminal = Terminal::new(TestBackend::new(50, 24)).unwrap();
    let mut targets = Vec::new();
    terminal
        .draw(|f| targets = render(f, &mut app, &mut focus).targets)
        .unwrap();
    assert!(!targets.is_empty());
    let screen = text(terminal.backend().buffer());
    assert!(screen.find("Marks (2)").unwrap() > screen.find("research").unwrap());
    terminal.backend_mut().resize(20, 5);
    terminal
        .draw(|f| targets = render(f, &mut app, &mut focus).targets)
        .unwrap();
    assert!(targets.is_empty());
    for (width, height) in [(36, 12), (80, 12), (80, 20), (160, 35)] {
        terminal.backend_mut().resize(width, height);
        terminal
            .draw(|f| targets = render(f, &mut app, &mut focus).targets)
            .unwrap();
        // Query (3), location line (1), footer (1), and pane borders (2).
        assert!(targets.len() <= height.saturating_sub(7) as usize);
    }
}
#[test]
fn marks_focus_retains_unmarked_rows_and_details_clamp_scroll() {
    let theme = Palette::default();
    let mut app = fixture();
    let unmarked = app.marks.remove(0);
    let mut focus = Focus::Marks {
        rows: vec![unmarked],
        index: 0,
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("› ○"));
    assert!(
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .any(|c| c.fg == theme.marked)
    );
    focus = Focus::Info {
        offset: usize::MAX,
        keys: false,
    };
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    assert!(matches!(focus,Focus::Info { offset, .. } if offset<100));
}
#[test]
fn relative_labels_compact_parents_without_changing_targets() {
    let base = Path::new("/work/notes/deep");
    for (path, label) in [
        ("/work/notes/deep/file.tex", "file.tex"),
        ("/work/notes/deep/topic/file.tex", "topic/file.tex"),
        ("/work/notes/file.tex", "../file.tex"),
        ("/work/papers/file.tex", ".../papers/file.tex"),
        ("/other/file.tex", ".../other/file.tex"),
        ("/work/notes/deep", "."),
        ("/work", ".../"),
        ("/work/notes/deep/.../file.tex", ".../file.tex"),
    ] {
        assert_eq!(display_path(Path::new(path), base), label);
    }
    let mut app = App::new(
        vec![base.into(), "/work/papers".into()],
        1,
        crate::Policy::default(),
    );
    let path = PathBuf::from("/work/papers/file.tex");
    app.insert(
        0,
        Candidate {
            path: path.clone(),
            directory: false,
            depth: 1,
            root: 1,
        },
    );
    app.refresh();
    app.marks.push(path.clone());
    app.scanning = false;
    let mut focus = Focus::Results;
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    let mut targets = Vec::new();
    terminal
        .draw(|f| targets = render(f, &mut app, &mut focus).targets)
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.matches(".../papers/file.tex").count() >= 3); // result, mark, footer
    assert!(!screen.contains("/work/papers/file.tex"));
    assert_eq!(targets, vec![path.clone()]);
    app.rendered = targets;
    assert_eq!(app.numbered(1), path);
    let mut bytes = Vec::new();
    crate::write_paths(&mut bytes, &[path], base, false, true).unwrap();
    assert_eq!(bytes, b"/work/papers/file.tex\0");
}
#[test]
fn no_dir_rows_show_names_with_only_the_parents_that_tell_them_apart() {
    let mut app = App::new(vec!["/work".into()], 3, crate::Policy::default());
    app.no_dir = true;
    app.icons = false;
    for (name, depth) in [
        ("folder/folder2/main.pdf", 3),
        ("folder/folder1/main.pdf", 3),
        ("folder/main2.pdf", 2),
    ] {
        app.insert(
            0,
            Candidate {
                path: Path::new("/work").join(name),
                directory: false,
                depth,
                root: 0,
            },
        );
    }
    app.scanning = false;
    app.refresh();
    let labels: Vec<_> = app
        .rows
        .iter()
        .map(|c| no_dir_label(&app, &c.path))
        .collect();
    assert_eq!(
        labels,
        ["main2.pdf", "folder1/main.pdf", "folder2/main.pdf"]
    );
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    let mut focus = Focus::Results;
    let mut targets = Vec::new();
    terminal
        .draw(|f| targets = render(f, &mut app, &mut focus).targets)
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains(" · folder1/main.pdf "), "{screen}");
    assert!(screen.contains(" · main2.pdf "), "{screen}");
    // Only the footer, which describes the highlighted main2.pdf, shows a
    // full relative path; the twins' rows carry just their distinguishing parent.
    assert!(!screen.contains("folder/folder1"), "{screen}");
    assert_eq!(targets[1], Path::new("/work/folder/folder1/main.pdf"));
}
#[test]
fn disambiguation_extends_only_colliding_names() {
    let paths = [
        "/r/a/x/main.pdf",
        "/r/b/x/main.pdf",
        "/r/c/main.pdf",
        "/r/main.pdf",
        "/r/solo.pdf",
        "/s/r/main.pdf",
    ]
    .map(PathBuf::from);
    let labels = crate::disambiguate(paths.iter().map(PathBuf::as_path));
    let label = |p: &str| labels.get(Path::new(p)).map(|l| l.display().to_string());
    assert_eq!(label("/r/a/x/main.pdf").as_deref(), Some("a/x/main.pdf"));
    assert_eq!(label("/r/b/x/main.pdf").as_deref(), Some("b/x/main.pdf"));
    assert_eq!(label("/r/c/main.pdf").as_deref(), Some("c/main.pdf"));
    // "r/main.pdf" ends both remaining paths, so each needs one more part.
    assert_eq!(label("/r/main.pdf").as_deref(), Some("/r/main.pdf"));
    assert_eq!(label("/s/r/main.pdf").as_deref(), Some("s/r/main.pdf"));
    assert_eq!(label("/r/solo.pdf"), None);
}
#[test]
fn configured_palette_and_location_line_reach_the_frame() {
    let mut app = fixture();
    app.palette=crate::config::Settings::parse("[ui]\npalette='custom'\n[palettes.custom]\nextends='light'\ncurrent_directory='#123456'\ncurrent_directory_text='#fefdfa'\naccent='#334455'",Path::new("/cfg"),None,None).unwrap().palette;
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    let mut focus = Focus::Results;
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    let screen = text(buffer);
    // The location line follows the three-row filter panel; no top banner.
    assert!(!screen.contains("CURRENT DIRECTORY"));
    assert!(screen.lines().next().unwrap().contains("Query"));
    assert!(
        screen
            .lines()
            .nth(3)
            .unwrap()
            .starts_with("  @  /home/frank/notes")
    );
    // The "@" badge uses the current-directory roles; the basename is bold.
    assert_eq!(buffer[(1, 3)].bg, Color::Rgb(0x12, 0x34, 0x56));
    assert_eq!(buffer[(2, 3)].fg, Color::Rgb(0xfe, 0xfd, 0xfa));
    assert_eq!(buffer[(17, 3)].symbol(), "n");
    assert!(buffer[(17, 3)].modifier.contains(Modifier::BOLD));
    assert!(!buffer[(16, 3)].modifier.contains(Modifier::BOLD));
    assert!(
        buffer
            .content()
            .iter()
            .any(|c| c.fg == Color::Rgb(0x33, 0x44, 0x55))
    );
    assert!(buffer.content().iter().any(|c| c.bg == app.palette.panel));
}
#[test]
fn palette_chooser_filters_marks_current_and_details_list_palette_keys() {
    let mut app = fixture();
    let settings = crate::config::Settings::parse(
        "[ui]\npalette='nord'\n[palettes.paper]\nextends='light'",
        Path::new("/cfg"),
        None,
        None,
    )
    .unwrap();
    app.apply_palette(crate::reload::PaletteState::from_settings(&settings));
    let mut focus = Focus::Palettes {
        query: "paper".into(),
        index: 0,
        original: app.palette,
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("Palettes · Enter applies · Esc reverts"));
    assert!(screen.contains("Find: paper"));
    assert!(screen.contains("› paper"));
    assert!(screen.contains("custom"));
    assert!(!screen.contains("gruvbox-dark"));
    assert_eq!(app.matching_palettes("paper").len(), 1);
    if let Focus::Palettes { query, .. } = &mut focus {
        query.clear();
    }
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("built-in · current"));
    focus = Focus::Info {
        offset: 0,
        keys: false,
    };
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("PALETTE"));
    focus = Focus::Info {
        offset: 0,
        keys: true,
    };
    // Tall enough for the whole key guide, whose palette keys come last.
    let mut terminal = Terminal::new(TestBackend::new(120, 50)).unwrap();
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("reload palette from config"));
}
#[test]
fn operation_sheets_align_with_the_pane_edges_and_list_keys_in_the_border() {
    for (width, height) in [(120, 26), (50, 16)] {
        let mut app = fixture();
        let current = app.cwd.join("todo.txt");
        let mut focus = Focus::ScopeChoice {
            current,
            selected: app.marks.clone(),
            action: crate::ui::ScopedAction::Delete,
        };
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render(frame, &mut app, &mut focus);
            })
            .unwrap();
        let screen = text(terminal.backend().buffer());
        let lines = screen.lines().collect::<Vec<_>>();
        let files = lines.iter().position(|l| l.contains("╭ Files")).unwrap();
        let sheet = lines.iter().position(|l| l.contains("╭ Delete")).unwrap();
        let bottom = lines.len() - 2;
        // Same outer columns as the panes, resting on their bottom border row.
        assert!(sheet > files);
        assert_eq!(lines[sheet].find('╭'), lines[files].find('╭'), "{screen}");
        assert_eq!(
            lines[sheet].trim_end().chars().count(),
            lines[0].trim_end().chars().count(),
            "{screen}"
        );
        assert!(lines[bottom].starts_with(" ╰"), "{screen}");
        // Narrow sheets compact their hints instead of clipping a key.
        let hints = if width > 60 {
            " c: Current ─── s: Selection ─── a: All ─── Esc: Cancel "
        } else {
            " c: Current ─ s: Selection ─ a: All "
        };
        assert!(lines[bottom].contains(hints), "{screen}");
        assert!(screen.contains("to delete both (3 entries)"), "{screen}");
    }
}
#[test]
fn deletion_confirmation_groups_the_smallest_rows_with_summed_sizes() {
    let app = fixture();
    let rows = (2..=7)
        .map(|n| (app.cwd.join(format!("file-{n}")), n))
        .collect::<Vec<_>>();
    let mut terminal = Terminal::new(TestBackend::new(80, 9)).unwrap();
    terminal
        .draw(|frame| delete_confirmation(frame, frame.area(), &app, &rows))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    let grouped = screen
        .lines()
        .find(|l| l.contains("and 3 more entries"))
        .unwrap();
    assert!(grouped.contains(" 9 B "), "{screen}");
    assert!(screen.find("file-7").unwrap() < screen.find("file-6").unwrap());
    assert!(!screen.contains("file-4"));
    assert!(screen.contains("6 entries · 27 B"), "{screen}");
    assert!(
        screen.contains(" Enter: Delete ─── Esc: Cancel "),
        "{screen}"
    );
}

#[test]
fn command_entry_keeps_buffer_visible_above_its_cursor() {
    for (width, height) in [(120, 26), (36, 12)] {
        for field in [Field::Query, Field::Exclusions] {
            let mut app = fixture();
            app.query = "visible-query".into();
            app.edit_view = Some(crate::editor::EditView {
                field,
                mode: "c".into(),
                blocking: false,
                row: if field == Field::Exclusions { 12 } else { 0 },
                column: 0,
                anchor: None,
                command: ":wq".into(),
                command_column: 3,
                closed: false,
                written: false,
                saved: false,
            });
            let mut focus = if field == Field::Exclusions {
                Focus::Exclusions(
                    (0..15)
                        .map(|i| format!("exclude-{i}"))
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            } else {
                Focus::Results
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|f| {
                    render(f, &mut app, &mut focus);
                })
                .unwrap();
            let screen = text(terminal.backend().buffer());
            let lines: Vec<_> = screen.lines().collect();
            let content = if field == Field::Exclusions {
                "exclude-12"
            } else {
                "visible-query"
            };
            let buffer_row = lines
                .iter()
                .position(|line| line.contains(content))
                .expect("buffer stays visible");
            let command_row = lines
                .iter()
                .position(|line| line.contains(":wq"))
                .expect("command is visible");
            assert!(command_row > buffer_row, "{screen}");
            assert_eq!(screen.matches(":wq").count(), 1);
            let cursor = terminal.get_cursor_position().unwrap();
            assert_eq!(cursor.y as usize, command_row);
            assert_eq!(
                terminal.backend().buffer()[(cursor.x - 1, cursor.y)].symbol(),
                "q"
            );
        }
    }
}

#[test]
fn vim_mode_selection_and_cursor_are_rendered_in_the_field() {
    let mut app = fixture();
    app.query = "abcdef".into();
    app.edit_view = Some(crate::editor::EditView {
        field: Field::Query,
        mode: "v".into(),
        blocking: false,
        row: 0,
        column: 1,
        anchor: Some((0, 0)),
        command: String::new(),
        command_column: 0,
        closed: false,
        written: false,
        saved: false,
    });
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    let mut focus = Focus::Results;
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(text(buffer).contains("Query · VISUAL"));
    assert_eq!(buffer[(5, 1)].bg, app.palette.selection);
    assert_eq!(buffer[(6, 1)].bg, app.palette.selection);
    let cursor = terminal.get_cursor_position().unwrap();
    assert_eq!((cursor.x, cursor.y), (6, 1));
}
#[test]
fn bookmark_chooser_shows_configured_and_default_icons() {
    let mut app = fixture();
    let bookmark = |name: &str, icon: Option<&str>| crate::config::Bookmark {
        name: name.into(),
        path: "/tmp".into(),
        icon: icon.map(str::to_string),
    };
    app.bookmarks = vec![
        bookmark("Downloads", Some("\u{f10e9}")),
        bookmark("Temp", None),
    ];
    let mut focus = Focus::Bookmarks {
        query: String::new(),
        index: 0,
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 26)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("\u{f10e9}  Downloads"), "{screen}");
    assert!(screen.contains("\u{f07b}  Temp"), "{screen}");
    app.icons = false;
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    // The configured icon stays; the other name keeps its alignment.
    assert!(screen.contains("\u{f10e9}  Downloads"), "{screen}");
    assert!(screen.contains("   Temp"), "{screen}");
    assert!(!screen.contains('\u{f07b}'), "{screen}");
}
#[test]
fn bookmark_numbers_follow_visible_scrolled_and_filtered_rows() {
    let mut app = fixture();
    app.bookmarks = (0..12)
        .map(|i| crate::config::Bookmark {
            name: format!("place{i:02}"),
            path: "/tmp".into(),
            icon: None,
        })
        .collect();
    let mut focus = Focus::Bookmarks {
        query: String::new(),
        index: 11,
    };
    let mut terminal = Terminal::new(TestBackend::new(100, 26)).unwrap();
    let mut targets = Vec::new();
    terminal
        .draw(|f| targets = render(f, &mut app, &mut focus).bookmark_targets)
        .unwrap();
    assert_eq!(targets, vec![7, 8, 9, 10, 11]);
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("5/0"), "{screen}");
    assert!(screen.contains("place11"), "{screen}");
    assert!(!screen.contains("place06"), "{screen}");
    focus = Focus::Bookmarks {
        query: "place1".into(),
        index: 0,
    };
    terminal
        .draw(|f| targets = render(f, &mut app, &mut focus).bookmark_targets)
        .unwrap();
    assert_eq!(targets, vec![1, 10, 11]);
    let mut tiny = Terminal::new(TestBackend::new(35, 11)).unwrap();
    tiny.draw(|f| targets = render(f, &mut app, &mut focus).bookmark_targets)
        .unwrap();
    assert!(targets.is_empty());
}
fn draw(app: &mut App, width: u16, height: u16) -> (String, Vec<PathBuf>) {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    let mut focus = Focus::Results;
    let mut targets = Vec::new();
    terminal
        .draw(|f| targets = render(f, app, &mut focus).targets)
        .unwrap();
    (text(terminal.backend().buffer()), targets)
}
#[test]
fn tab_strip_ends_the_results_title_and_marks_the_active_tab() {
    let mut app = fixture();
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    let mut focus = Focus::Results;
    app.tab = 1;
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let screen = text(buffer);
    let title = screen.lines().find(|l| l.contains("Files ·")).unwrap();
    assert!(title.contains("─  1  2  3  ╮"), "{screen}");
    // The location line stays free of tab numbers.
    assert!(!screen.lines().nth(3).unwrap().contains(" 1  2  3"));
    let y = screen.lines().position(|l| l.contains("Files ·")).unwrap() as u16;
    let x = title
        .find(" 2 ")
        .map(|i| title[..i].chars().count() as u16)
        .unwrap();
    let theme = app.palette;
    for dx in 0..3 {
        assert_eq!(buffer[(x + dx, y)].bg, theme.current_directory);
    }
    assert_ne!(buffer[(x - 3, y)].bg, theme.current_directory);
    app.no_dir = true;
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(!screen.contains(" 1  2  3 "), "{screen}");
}
#[test]
fn location_line_sits_between_filter_and_results_with_status_on_border() {
    let mut app = fixture();
    let (screen, _) = draw(&mut app, 120, 26);
    let lines: Vec<_> = screen.lines().collect();
    assert!(lines[0].contains("Query"));
    assert!(lines[1].contains("❯ type to filter"));
    // Progress and depth always; only non-default settings get a pill.
    assert!(
        lines[2].trim_end().ends_with("● complete   depth 3  ╯"),
        "{screen}"
    );
    assert!(!lines[2].contains("hidden"));
    assert_eq!(lines[3].trim_end(), "  @  /home/frank/notes");
    assert!(lines[4].contains("Files ·"));
    assert!(!screen.contains("pinned"));
    app.scanning = true;
    app.policy.set_filters(vec!["*.tex".into()]).unwrap();
    app.policy.set_rules(vec!["build".into()]).unwrap();
    app.policy.ignore = true;
    let (screen, _) = draw(&mut app, 120, 26);
    // Filters and exclusions are named, not spelled out, and only while active.
    assert!(
        screen.contains("◌ scanning · incomplete   depth 3   filters on   exclusions on"),
        "{screen}"
    );
    assert!(!screen.contains("*.tex"), "{screen}");
    app.policy.ignore = false;
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(!screen.contains("exclusions"), "{screen}");
    // The minimum frame keeps depth and an explicit incomplete label.
    let (screen, targets) = draw(&mut app, 36, 12);
    let status = screen.lines().nth(2).unwrap();
    assert!(status.contains("◌ incomplete   depth 3"), "{screen}");
    assert!(
        screen
            .lines()
            .nth(3)
            .unwrap()
            .contains("@  /home/frank/notes")
    );
    assert!(screen.lines().nth(3).unwrap().contains("notes"));
    assert!(!targets.is_empty());
}
#[test]
fn pinned_roots_are_compact_and_separate_from_the_current_directory() {
    let roots: Vec<PathBuf> = vec![
        "/work/notes".into(),
        "/work/papers".into(),
        "/srv/shared/library".into(),
    ];
    let mut app = App::new(roots, 2, crate::Policy::default());
    app.scanning = false;
    // Wide: path and pinned root names share one line.
    let (screen, _) = draw(&mut app, 120, 26);
    let line = screen.lines().nth(3).unwrap();
    assert!(line.starts_with("  @  /work/notes "), "{line}");
    assert!(
        line.trim_end()
            .ends_with("\u{f435} pinned  notes  papers  library"),
        "{line}"
    );
    assert!(screen.lines().nth(4).unwrap().contains("Files ·"));
    // Only the current root's name is underlined, not the gap before it.
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    let mut focus = Focus::Results;
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let underlined: String = (0..buffer.area.width)
        .filter(|&x| buffer[(x, 3)].modifier.contains(Modifier::UNDERLINED))
        .map(|x| buffer[(x, 3)].symbol())
        .collect();
    assert_eq!(underlined, "notes");
    // Away from every root, the pinned list moves to a second line.
    app.navigate(
        "/work/notes/a-rather-long-topic-directory-name/chapter".into(),
        false,
    );
    let (screen, targets) = draw(&mut app, 50, 14);
    let lines: Vec<_> = screen.lines().collect();
    assert!(lines[3].contains("@ "), "{screen}");
    assert!(lines[3].trim_end().ends_with("chapter"), "{screen}");
    assert!(
        lines[4].starts_with("     \u{f435} pinned  notes  papers"),
        "{screen}"
    );
    assert!(lines[5].contains("Files ·"));
    assert!(targets.len() <= 14 - 8);
    // Too narrow for all roots: keep whole entries and point to F1.
    app.icons = false;
    app.roots
        .push("/srv/archive/collected-correspondence".into());
    let (screen, _) = draw(&mut app, 36, 12);
    let pinned = screen.lines().nth(4).unwrap();
    assert!(pinned.contains("pinned  notes  papers"), "{screen}");
    assert!(pinned.contains("+2 (F1)"), "{screen}");
    assert!(!pinned.contains("library") && !pinned.contains("correspondence"));
}
#[test]
fn location_line_stays_distinct_without_color() {
    let mut app = fixture();
    app.palette = crate::config::Settings::parse(
            "[ui]\npalette='plain'\n[palettes.plain]\nbackground='default'\npanel='default'\ntext='default'\nmuted='default'\nborder='default'\naccent='default'\nmarked='default'\nsuccess='default'\nerror='default'\nselection='default'\ncurrent_directory='default'\ncurrent_directory_text='default'\nlink='default'\nexecutable='default'\nempty='default'",
            Path::new("/cfg"),
            None,
            None,
        )
        .unwrap()
        .palette;
    let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
    let mut focus = Focus::Results;
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    // Without colors, "@" plus a bold basename still identify the location.
    assert_eq!(buffer[(2, 3)].symbol(), "@");
    assert!(buffer[(2, 3)].modifier.contains(Modifier::BOLD));
    assert!(buffer[(17, 3)].modifier.contains(Modifier::BOLD));
    assert!(text(buffer).contains("● complete"));
}
#[test]
fn icons_keep_columns_elision_and_numbered_targets_aligned() {
    let mut app = fixture();
    for name in ["研究_notes.md", &format!("{}tail.rs", "long_".repeat(30))] {
        app.insert(
            0,
            Candidate {
                path: app.cwd.join(name),
                directory: false,
                depth: 1,
                root: 0,
            },
        );
    }
    app.marks = vec!["/home/frank/notes/report.tex".into()];
    app.refresh();
    let mut frames = Vec::new();
    for icons in [true, false] {
        app.icons = icons;
        let mut focus = Focus::Results;
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        let mut targets = Vec::new();
        terminal
            .draw(|f| targets = render(f, &mut app, &mut focus).targets)
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let screen = text(&buffer);
        // Names start in the same column with or without icons.
        let start = screen
            .lines()
            .find_map(|l| l.find("research/").map(|i| l[..i].chars().count()))
            .unwrap();
        assert_eq!(targets.len(), app.rows.len());
        frames.push((screen, start, targets));
    }
    let (on, on_start, on_targets) = &frames[0];
    let (off, off_start, off_targets) = &frames[1];
    assert_eq!((on_start, on_targets), (off_start, off_targets));
    assert!(on.contains("\u{f07b} research/")); // folder
    assert!(on.contains("\u{e7a8} …")); // rust, elided keeping the basename
    assert!(on.contains("tail.rs"));
    assert!(on.contains("● \u{f15c} report.tex")); // marked row
    assert!(off.contains("▸ research/") && off.contains("· todo.txt"));
    assert!(!off.contains('\u{f07b}') && off.contains("● report.tex"));
}
#[test]
fn key_guide_lists_configured_normal_keys_and_dims_navigation_with_no_dir() {
    let mut app = fixture();
    app.normal_keys = crate::normal_keys::NormalKeysConfig {
        down: crate::normal_keys::Sequences::Many(vec!["j".into(), "<space>j".into()]),
        mark: crate::normal_keys::Sequences::One("none".into()),
        ..Default::default()
    }
    .build()
    .unwrap();
    let mut focus = Focus::Info {
        offset: 0,
        keys: true,
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    // Two columns at this width, every group present, the footer gone.
    let row = screen.lines().find(|l| l.contains("NAVIGATE")).unwrap();
    assert!(row.contains("NORMAL MODE IN LISTS"), "{screen}");
    for group in [
        "SELECT AND RETURN",
        "SEARCH",
        "EXCLUSIONS AND FILTERS",
        "LOOK",
        "BOOKMARKS",
    ] {
        assert!(screen.contains(group), "{screen}");
    }
    assert!(screen.contains("j / <space>j  k"), "{screen}");
    assert!(screen.contains("gg  G"), "{screen}");
    assert!(
        screen.contains("←  →") && screen.contains("1…9"),
        "{screen}"
    );
    assert!(screen.contains("Alt+e") && screen.contains("one glob per line"));
    assert!(!screen.contains("off with --no-dir"));
    app.no_dir = true;
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert!(text(buffer).contains("crossed out: off with --no-dir"));
    let (x, y) = (0..buffer.area.height)
        .find_map(|y| {
            let line: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            line.find("Alt+l")
                .map(|i| (line[..i].chars().count() as u16, y))
        })
        .unwrap();
    assert!(buffer[(x, y)].modifier.contains(Modifier::CROSSED_OUT));
}
fn export_preview(buffer: &Buffer) {
    let Ok(path) = std::env::var("PICKER_PREVIEW") else {
        return;
    };
    use std::io::Write;
    let mut file = std::fs::File::create(path).unwrap();
    writeln!(file, "{} {}", buffer.area.width, buffer.area.height).unwrap();
    for cell in buffer.content() {
        let (r, g, b) = match cell.fg {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => (218, 228, 240),
        };
        let (br, bg, bb) = match cell.bg {
            Color::Rgb(r, g, b) => (r, g, b),
            _ => (15, 23, 36),
        };
        writeln!(file, "{r},{g},{b};{br},{bg},{bb};{}", cell.symbol()).unwrap();
    }
}
