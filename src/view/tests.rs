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
                order: 0,
            },
        );
    }
    app.refresh();
    app.scanning = false;
    app.marks = vec![
        PathBuf::from("/home/frank/papers/Turing 1952.pdf"),
        PathBuf::from("/home/frank/notes/report.tex"),
    ]
    .into();
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
    // A copy still being written: its staging folder and what is in it.
    std::fs::create_dir_all(root.join(".fff-stage-1-2-3/copied")).unwrap();
    std::fs::write(root.join(".fff-stage-1-2-3/copied/half"), "x").unwrap();
    let mut app = App::browse(
        root.clone(),
        3,
        crate::Policy {
            hidden: true,
            ..Default::default()
        },
    );
    for (name, directory) in [
        (".fff-stage-1-2-3", true),
        (".fff-stage-1-2-3/copied", true),
        (".fff-stage-1-2-3/copied/half", false),
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
                depth: name.split('/').count(),
                root: 0,
                order: 0,
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
    assert_eq!(color_of(".fff-stage-1-2-3/"), theme.staged_folder);
    // Labelled by path, the parent folders are muted; the name is colored.
    let name_color = |label: &str, name: &str| {
        let row = (0..buffer.area.height)
            .find(|&y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
                    .contains(&format!(" {label}"))
            })
            .unwrap_or_else(|| panic!("{label} not shown"));
        let line = (0..buffer.area.width)
            .map(|x| buffer[(x, row)].symbol())
            .collect::<String>();
        let start = line.find(&format!(" {label}")).unwrap() + 1 + label.rfind(name).unwrap();
        buffer[(line[..start].chars().count() as u16, row)].fg
    };
    assert_eq!(
        name_color(".fff-stage-1-2-3/copied/", "copied"),
        theme.staged_folder
    );
    assert_eq!(
        name_color(".fff-stage-1-2-3/copied/half", "half"),
        theme.marked
    );
    assert_ne!(theme.staged_folder, theme.marked);
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
    std::fs::write(root.join("folder/one"), vec![0u8; 100]).unwrap();
    std::fs::write(root.join("folder/inner/two"), vec![0u8; 1500]).unwrap();
    // A file's second link adds nothing.
    std::fs::hard_link(root.join("folder/inner/two"), root.join("folder/again")).unwrap();
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
                order: 0,
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
    // Details and folders' sizes arrive from workers, asked for by a drawn
    // frame; until then the column is blank.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut wide = draw(&mut app, 100);
    assert!(!wide.contains("1.6K"), "{wide}");
    while !wide.contains("1.6K") && std::time::Instant::now() < deadline {
        app.details.commit_sizes();
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.details.drain();
        wide = draw(&mut app, 100);
    }
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
    assert!(folder.contains("  1.6K   now"), "{folder}");
    let footer = wide.lines().last().unwrap();
    assert!(footer.contains("data.bin  4.2K (4,300 B)  "), "{footer}");
    // The footer has a folder's exact size, then its entries.
    app.highlight = Some(root.join("folder"));
    let mut shown = draw(&mut app, 120);
    while !shown.contains("entries") && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(5));
        app.details.drain();
        shown = draw(&mut app, 120);
    }
    let folder_footer = shown.lines().last().unwrap();
    assert!(
        folder_footer.contains("1.6K (1,600 B)  3 entries"),
        "{folder_footer}"
    );
    app.highlight = Some(root.join("data.bin"));
    assert!(footer.contains("-rw"), "{footer}");
    // Colored by kind as eza -l colors them, in the palette's roles.
    let mut terminal = Terminal::new(TestBackend::new(100, 1)).unwrap();
    terminal
        .draw(|frame| super::footer(frame, Rect::new(0, 0, 100, 1), &app, &Focus::Results))
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let at = |needle: &str| {
        let x = footer
            .find(needle)
            .unwrap_or_else(|| panic!("{needle}: {footer}"));
        buffer[(footer[..x].chars().count() as u16, 0)].fg
    };
    let theme = app.palette;
    assert_eq!(at("4.2K"), theme.success);
    assert_eq!(at("-rw"), theme.muted);
    assert_eq!(at("rw-"), theme.marked);
    assert_eq!(at("w-"), theme.error);
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
                order: 0,
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
#[test]
fn tab_click_areas_cover_the_drawn_numbers() {
    let mut app = fixture();
    let mut terminal = Terminal::new(TestBackend::new(70, 10)).unwrap();
    let mut tabs = Vec::new();
    terminal
        .draw(|frame| {
            tabs = results_pane(frame, Rect::new(3, 1, 60, 8), &mut app, true).tabs;
        })
        .unwrap();
    let buffer = terminal.backend().buffer();
    assert_eq!(tabs.len(), crate::TABS);
    for &(area, index) in &tabs {
        let cells: String = (area.x..area.x + area.width)
            .map(|x| buffer[(x, area.y)].symbol())
            .collect();
        assert_eq!(cells, format!(" {} ", index + 1));
    }
    app.set_program(crate::program::Program::Seek);
    terminal
        .draw(|frame| {
            tabs = results_pane(frame, Rect::new(3, 1, 60, 8), &mut app, true).tabs;
        })
        .unwrap();
    assert!(tabs.is_empty());
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
    // Keys live in the F1 guide; the footer only points to it and says how
    // to leave, in every kind of session.
    assert!(screen.contains("F1 keys  q quit"));
    assert!(!screen.contains("Alt+#"));
    assert!(screen.contains("[MISSING]"));
    export_preview(buffer);
}
#[test]
fn the_preview_takes_the_bottom_half_below_or_between_the_panes() {
    use crate::preview::{Key, Preview, Tone};
    let mut app = fixture();
    let path = app.cwd.join("todo.txt");
    app.preview.set(
        Key {
            path,
            size: 9,
            modified: None,
            hidden: false,
        },
        Preview {
            kind: "text · 2 lines".into(),
            facts: vec![("Pages".into(), "12".into())],
            lines: vec![
                ("first line".into(), Tone::Text),
                ("x".repeat(300), Tone::Text),
            ],
            ..Default::default()
        },
    );
    let frame = |app: &mut App, width, height| {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        let mut page = 0;
        terminal
            .draw(|frame| page = render(frame, app, &mut Focus::Results).page)
            .unwrap();
        (text(terminal.backend().buffer()), page)
    };
    let row = |screen: &str, token: &str| screen.lines().position(|l| l.contains(token));
    // Wide: results and marks side by side on top, the preview full width below.
    let (screen, page) = frame(&mut app, 120, 40);
    let preview = row(&screen, "Preview · text · 2 lines").unwrap();
    assert!(row(&screen, "Files ·").unwrap() < preview);
    assert_eq!(row(&screen, "Files ·"), row(&screen, "Marks (2)"));
    let title = screen.lines().nth(preview).unwrap();
    assert!(title.starts_with(" ╭ Preview") && title.trim_end().ends_with('╮'));
    assert!(title.chars().count() >= 118);
    assert!(row(&screen, "Pages  12").unwrap() > preview);
    assert!(row(&screen, "first line").unwrap() > row(&screen, "Pages").unwrap());
    // Long lines wrap onto the next rows; with wrapping off, they are cut.
    assert!(!screen.lines().any(|l| l.contains("xxx…")));
    assert!(page <= 17, "results keep the top half: {page}");
    app.preview.wrap = false;
    let (cut, _) = frame(&mut app, 120, 40);
    assert!(cut.lines().any(|l| l.contains("xxx…")));
    app.preview.wrap = true;
    // Narrow: results, then the preview, then the marks.
    let (screen, _) = frame(&mut app, 60, 40);
    let (files, preview, marks) = (
        row(&screen, "Files ·").unwrap(),
        row(&screen, "Preview ·").unwrap(),
        row(&screen, "Marks (2)").unwrap(),
    );
    assert!(files < preview && preview < marks);
    // Too short for half of it to help: no preview.
    let (screen, _) = frame(&mut app, 120, 16);
    assert!(!screen.contains("Preview"));
    app.preview.shown = false;
    let (screen, page) = frame(&mut app, 120, 40);
    assert!(!screen.contains("Preview") && page > 25);
}
#[test]
fn text_previews_draw_their_syntax_colors_below_a_rule() {
    use crate::highlight::Syntax;
    use crate::preview::{Key, Preview, Tone};
    let mut app = fixture();
    let theme = app.palette;
    let preview = Preview {
        kind: "text · 2 lines".into(),
        facts: vec![("From".into(), "/somewhere".into())],
        lines: vec![
            ("fn main() {}".into(), Tone::Text),
            (
                "// a long comment cut at the pane's edge, past it".into(),
                Tone::Text,
            ),
        ],
        syntax: vec![
            vec![(0, 2, Syntax::Keyword), (3, 7, Syntax::Function)],
            vec![(0, 50, Syntax::Comment)],
        ],
        ..Default::default()
    };
    let key = Key {
        path: app.cwd.join("main.rs"),
        size: 9,
        modified: None,
        hidden: false,
    };
    app.preview.set(key, preview);
    app.preview.highlight = true;
    // Cut rather than wrapped, so that each line keeps its row.
    app.preview.wrap = false;
    let mut terminal = Terminal::new(TestBackend::new(40, 30)).unwrap();
    terminal
        .draw(|frame| {
            render(frame, &mut app, &mut Focus::Results);
        })
        .unwrap();
    let buffer = terminal.backend().buffer().clone();
    let screen = text(&buffer);
    let at = |token: &str| {
        let row = screen.lines().position(|l| l.contains(token)).unwrap();
        let line = screen.lines().nth(row).unwrap();
        let column = line[..line.find(token).unwrap()].chars().count();
        (column as u16, row as u16)
    };
    let fn_at = at("fn main");
    assert_eq!(buffer[fn_at].fg, theme.accent);
    assert_eq!(buffer[(fn_at.0 + 3, fn_at.1)].fg, theme.link);
    assert_eq!(buffer[(fn_at.0 + 8, fn_at.1)].fg, theme.text);
    let comment = at("// a long");
    assert_eq!(buffer[comment].fg, theme.muted);
    assert!(screen.contains("…"), "{screen}");
    // The facts and the content are set apart by a rule.
    let facts = screen
        .lines()
        .position(|l| l.contains("/somewhere"))
        .unwrap();
    let rule = screen.lines().nth(facts + 1).unwrap();
    assert!(
        rule.contains("│ ────") && !rule.contains("Pages"),
        "{screen}"
    );
    assert_eq!(fn_at.1 as usize, facts + 2);
    // With [ui] highlight off, the same text is plain.
    app.preview.highlight = false;
    terminal
        .draw(|frame| {
            render(frame, &mut app, &mut Focus::Results);
        })
        .unwrap();
    assert_eq!(terminal.backend().buffer()[fn_at].fg, theme.text);
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
            order: 0,
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
fn marks_pane_shows_the_window_list_would_scroll_to() {
    let mut app = fixture();
    let rows: Vec<PathBuf> = (0..60)
        .map(|i| PathBuf::from(format!("/elsewhere/m{i:03}")))
        .collect();
    app.marks = rows.clone().into();
    let screen = |app: &mut App, mut focus: Focus| {
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        terminal
            .draw(|f| {
                render(f, app, &mut focus);
            })
            .unwrap();
        text(terminal.backend().buffer())
    };
    let selected = |screen: &str, name: &str| {
        screen
            .lines()
            .any(|l| l.contains(name) && l.contains("│› ●"))
    };
    let unfocused = screen(&mut app, Focus::Results);
    assert!(unfocused.contains("m000") && !unfocused.contains("m059"));
    let near_top = screen(
        &mut app,
        Focus::Marks {
            rows: rows.clone(),
            index: 2,
        },
    );
    assert!(near_top.contains("m000") && selected(&near_top, "m002"));
    // Deep in the list the selection is the last row, as List scrolls it.
    let deep = screen(&mut app, Focus::Marks { rows, index: 45 });
    assert!(selected(&deep, "m045") && deep.contains("m044"));
    assert!(!deep.contains("m046") && !deep.contains("m000"));
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
    focus = Focus::Info { offset: usize::MAX };
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
            order: 0,
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
fn short_labels_show_names_with_only_the_parents_that_tell_them_apart() {
    let mut app = App::new(vec!["/work".into()], 3, crate::Policy::default());
    app.set_program(crate::program::Program::Seek);
    app.short_labels = true;
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
                order: 0,
            },
        );
    }
    app.scanning = false;
    app.refresh();
    let labels: Vec<_> = app
        .rows
        .iter()
        .map(|c| short_label(&app, &c.path))
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
fn match_rows_fit_their_labels_and_keep_them_until_relabelled() {
    let root = std::env::temp_dir().join(format!("fff-label-column-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("deep/folder")).unwrap();
    std::fs::write(root.join("deep/folder/notes.txt"), "a canard\n").unwrap();
    let mut app = App::new(vec![root.clone()], 6, crate::Policy::default());
    app.icons = false;
    app.grep.cache = Some(root.join("cache"));
    app.insert(
        0,
        Candidate {
            path: root.join("deep/folder/notes.txt"),
            directory: false,
            depth: 3,
            root: 0,
            order: 0,
        },
    );
    app.scanning = false;
    app.refresh();
    app.set_contents("canard".into());
    let start = std::time::Instant::now();
    while app.grep.progress().is_some() || app.shown().is_empty() {
        assert!(start.elapsed() < std::time::Duration::from_secs(10));
        app.poll_grep();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let screen = |app: &mut App| {
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        terminal
            .draw(|f| {
                render(f, app, &mut Focus::Results);
            })
            .unwrap();
        text(terminal.backend().buffer())
    };
    // By path, the name column is as wide as the path, not the name.
    assert!(!app.labels_shown());
    let shown = screen(&mut app);
    assert!(shown.contains("· deep/folder/notes.txt  "), "{shown}");
    // Rows keep the labels they were ranked with until a ranking labels
    // them again, rather than showing bare names meanwhile.
    app.short_labels = true;
    assert!(!app.labels_shown());
    assert!(screen(&mut app).contains("· deep/folder/notes.txt  "));
    app.refresh();
    assert!(app.labels_shown());
    let shown = screen(&mut app);
    assert!(shown.contains("· notes.txt  "), "{shown}");
    std::fs::remove_dir_all(&root).unwrap();
}
#[test]
fn pick_rows_show_names_folders_included_and_follow_navigation() {
    let mut app = App::browse("/work".into(), 3, crate::Policy::default());
    app.short_labels = true;
    for (name, directory, depth) in [
        ("a", true, 1),
        ("b", true, 1),
        ("a/notes", true, 2),
        ("b/notes", true, 2),
        ("a/notes/plan.md", false, 3),
        ("b/todo.md", false, 2),
    ] {
        app.insert(
            0,
            Candidate {
                path: Path::new("/work").join(name),
                directory,
                depth,
                root: 0,
                order: 0,
            },
        );
    }
    app.scanning = false;
    app.refresh();
    let labels =
        |app: &App| -> Vec<String> { app.rows.iter().map(|c| short_label(app, &c.path)).collect() };
    assert_eq!(
        labels(&app),
        ["a", "b", "a/notes", "b/notes", "todo.md", "plan.md"]
    );
    // In a/, only one notes folder is listed: its bare name is enough.
    app.navigate("/work/a".into(), false);
    for (name, directory, depth) in [("notes", true, 1), ("notes/plan.md", false, 2)] {
        app.insert(
            app.generation,
            Candidate {
                path: Path::new("/work/a").join(name),
                directory,
                depth,
                root: 0,
                order: 0,
            },
        );
    }
    app.refresh();
    assert_eq!(labels(&app), ["notes", "plan.md"]);
    let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut Focus::Results))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains(" notes/ "), "{screen}");
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
fn the_sorting_picker_names_the_direction_and_the_border_the_order() {
    use crate::rows::{Criterion, Sort};
    let mut app = fixture();
    app.set_sort(Some(Sort {
        by: Criterion::Name,
        descending: true,
    }));
    let mut focus = Focus::Picker {
        query: String::new(),
        index: 1,
        kind: Picker::Sort { descending: true },
        back: Box::default(),
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    // The direction it sorts in, alone; the keys on the bottom border.
    assert!(screen.contains("╭ Sort descending ─"), "{screen}");
    assert!(!screen.contains("ascending"), "{screen}");
    assert!(
        screen.contains("Tab: Toggle ordering ─── Enter: Sort ─── Esc: Cancel"),
        "{screen}"
    );
    assert!(screen.contains("↓ type a criterion's name"), "{screen}");
    assert!(
        screen.contains("›   2 name        current ↓   Names alphabetically"),
        "{screen}"
    );
    assert!(screen.contains("ranking"));
    assert!(screen.contains("depth 3 · sorted by name ↓"));
    if let Focus::Picker { query, .. } = &mut focus {
        *query = "newest".into();
    }
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("No order matches."), "{screen}");
    // The rows themselves, names descending.
    let rows: Vec<_> = app.rows.iter().map(|c| c.path.clone()).collect();
    assert_eq!(rows[0], app.cwd.join("todo.txt"));
    assert_eq!(rows[4], app.cwd.join("algorithms"));
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
    assert!(screen.contains("› 1/0 paper"), "{screen}");
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
    focus = Focus::Info { offset: 0 };
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("PALETTE"));
    let mut screen = keys_screen(&app);
    screen.query = "palette".into();
    focus = Focus::Keys(screen);
    let mut terminal = Terminal::new(TestBackend::new(120, 50)).unwrap();
    terminal
        .draw(|f| {
            render(f, &mut app, &mut focus);
        })
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("reload_palette"), "{screen}");
}
#[test]
fn operation_sheets_align_with_the_pane_edges_and_list_keys_in_the_border() {
    for (width, height) in [(120, 26), (50, 16)] {
        let mut app = fixture();
        let current = app.cwd.join("todo.txt");
        let mut focus = Focus::ScopeChoice {
            current,
            selected: app.marks.to_vec(),
            action: crate::ui::ScopedAction::Delete(false),
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
        // The sentence, however it wraps.
        let sentence = lines[sheet + 1..bottom]
            .iter()
            .map(|l| l.trim().trim_matches('│').trim())
            .collect::<Vec<_>>()
            .join(" ");
        assert!(
            sentence.contains("Press c to permanently delete only it")
                && sentence.contains("or a both (3 entries)"),
            "{screen}"
        );
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

fn sheet_text(width: u16, height: u16, draw: impl FnOnce(&mut Frame)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(draw).unwrap();
    text(terminal.backend().buffer())
}
#[test]
fn the_trash_sheet_explains_why_it_asks_and_offers_the_answers_that_apply() {
    use crate::operations::{DeletePlan, Fate};
    let mut app = fixture();
    app.trash = Some(crate::trash::Trash::new(Path::new("/data"), 30, 10 << 30));
    let big = app.cwd.join("disk.img");
    let small = app.cwd.join("todo.txt");
    let plan = DeletePlan::shown(
        vec![(big.clone(), 12 << 30), (small.clone(), 3)],
        vec![Fate::Large, Fate::Trash],
    );
    let screen = sheet_text(160, 12, |f| trash_confirmation(f, f.area(), &app, &plan));
    for expected in [
        "╭ Too large for the trash",
        "This entry is larger than 10.0 GiB",
        "for 30 days",
        "With d or t, the other entry goes to the trash.",
        "12.0 GiB",
        "disk.img",
        " d: Delete permanently ─── t: Trash ─── Esc: Cancel ",
        "1 entry · 12.0 GiB",
    ] {
        assert!(screen.contains(expected), "{expected}\n{screen}");
    }
    // Only the entries asked about are listed.
    assert!(!screen.contains("todo.txt"), "{screen}");
    // Entries that cannot go to the trash leave only permanent deletion.
    let plan = DeletePlan::shown(
        vec![
            (big.clone(), 12 << 30),
            (app.cwd.join("bin"), 5),
            (small.clone(), 3),
            (app.cwd.join("more"), 1),
        ],
        vec![
            Fate::Large,
            Fate::Unavailable("already in the trash".into()),
            Fate::Trash,
            Fate::Trash,
        ],
    );
    let screen = sheet_text(160, 14, |f| trash_confirmation(f, f.area(), &app, &plan));
    for expected in [
        "╭ Delete permanently",
        "Some of these entries cannot go to the trash (already in the trash), and the others are \
         larger than 10.0 GiB.",
        "With d, the other 2 entries go to the trash.",
        " d: Delete permanently ─── Esc: Cancel ",
        "2 entries · 12.0 GiB",
    ] {
        assert!(screen.contains(expected), "{expected}\n{screen}");
    }
    assert!(!screen.contains("t: Trash"), "{screen}");
    let plan = DeletePlan::shown(
        vec![(small, 3)],
        vec![Fate::Unavailable(
            "its filesystem has no trash folder".into(),
        )],
    );
    let screen = sheet_text(160, 10, |f| trash_confirmation(f, f.area(), &app, &plan));
    assert!(
        screen.contains("This entry cannot go to the trash (its filesystem has no trash folder)"),
        "{screen}"
    );
    assert!(!screen.contains("other"), "{screen}");
}
#[test]
fn the_progress_sheet_shows_a_bar_figures_and_the_stop_key_while_it_applies() {
    use std::sync::atomic::Ordering::Relaxed;
    let app = fixture();
    let draw = |job: &crate::ui::Job| sheet_text(80, 6, |f| progress(f, f.area(), &app, job));
    let job = crate::ui::Job::shown("Copying 3 entries", false);
    job.progress.checking.store(true, Relaxed);
    job.progress.stoppable.store(true, Relaxed);
    let screen = draw(&job);
    assert!(screen.contains("╭ Copying 3 entries"), "{screen}");
    assert!(screen.contains("checking the entries · 0 s"), "{screen}");
    assert!(screen.contains(" Ctrl+c: Stop "), "{screen}");
    // Bytes, with a known total: the bar fills, with a percentage and speed.
    job.progress.checking.store(false, Relaxed);
    job.progress.total.store(4 << 20, Relaxed);
    job.progress.done.store(1 << 20, Relaxed);
    let screen = draw(&job);
    let bar = screen.lines().find(|l| l.contains('━')).unwrap();
    assert!(bar.contains("25%"), "{screen}");
    let filled = bar.matches('━').count();
    let empty = bar.matches('─').count();
    assert!(
        filled * 4 <= filled + empty && (filled + 1) * 4 >= filled + empty,
        "{bar}"
    );
    assert!(screen.contains("1.0 MiB of 4.0 MiB · "), "{screen}");
    assert!(screen.contains("/s · 0 s"), "{screen}");
    // Unknown totals sweep instead of filling, and count what is done.
    job.progress.total.store(0, Relaxed);
    let screen = draw(&job);
    assert!(!screen.contains('%'), "{screen}");
    assert!(screen.contains("│ 1.0 MiB · "), "{screen}");
    // Past the point where stopping changes nothing, Ctrl+c is not offered.
    job.progress.stoppable.store(false, Relaxed);
    assert!(!draw(&job).contains("Ctrl+c"));
    // Once asked to stop, it says so.
    job.progress.stoppable.store(true, Relaxed);
    job.progress.cancel.store(true, Relaxed);
    let screen = draw(&job);
    assert!(screen.contains("stopping…"), "{screen}");
    assert!(!screen.contains("Ctrl+c"), "{screen}");
    // Nothing to count: only the time.
    let job = crate::ui::Job::shown("Undoing", false);
    let screen = draw(&job);
    assert!(screen.contains("│ 0 s "), "{screen}");
    assert!(!screen.contains(" B"), "{screen}");
    // Deletions count entries.
    let job = crate::ui::Job::shown("Deleting tree", true);
    job.progress.total.store(10, Relaxed);
    job.progress.done.store(3, Relaxed);
    let screen = draw(&job);
    assert!(screen.contains("3 of 10 entries · 0 s"), "{screen}");
    assert!(screen.contains("30%"), "{screen}");
}

#[test]
fn running_operations_take_the_lower_half_of_the_marks_pane() {
    use std::sync::atomic::Ordering::Relaxed;
    let mut app = fixture();
    let draw = |app: &mut App, mut focus: Focus| {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal
            .draw(|f| {
                render(f, app, &mut focus);
            })
            .unwrap();
        text(terminal.backend().buffer())
    };
    assert!(!draw(&mut app, Focus::Results).contains("Jobs"));
    let copying = crate::ui::Job::running("Copying photos", &[], false);
    copying.progress.total.store(4 << 20, Relaxed);
    copying.progress.done.store(1 << 20, Relaxed);
    app.jobs.push(copying);
    app.jobs
        .push(crate::ui::Job::running("Deleting old", &[], true));
    let screen = draw(&mut app, Focus::Results);
    assert!(
        screen.contains("Marks (2)") && screen.contains("Jobs (2)"),
        "{screen}"
    );
    assert!(
        screen.contains("Copying photos") && screen.contains("25%"),
        "{screen}"
    );
    assert!(screen.contains("1.0 MiB of 4.0 MiB"), "{screen}");
    assert!(!screen.contains("x: Stop and undo"), "{screen}");
    // Focused, it highlights a job and names its keys.
    let screen = draw(&mut app, Focus::Jobs { index: 1 });
    assert!(screen.contains("› Deleting old"), "{screen}");
    assert!(screen.contains("x: Stop and undo"), "{screen}");
    // Leaving asks first, naming what would stop.
    let screen = draw(
        &mut app,
        Focus::QuitConfirm(Box::new(crate::ui::Finish::Cancelled)),
    );
    assert!(
        screen.contains("Quit stops 2 running operations"),
        "{screen}"
    );
    assert!(screen.contains("• Copying photos"), "{screen}");
    assert!(screen.contains("Enter: Stop them and quit"), "{screen}");
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
fn workspace_chooser_shows_configured_and_default_icons() {
    let mut app = fixture();
    let workspace = |name: &str, icon: Option<&str>| crate::config::Workspace {
        name: name.into(),
        path: Some("/tmp".into()),
        icon: icon.map(str::to_string),
        ..Default::default()
    };
    app.workspaces = vec![
        workspace("Downloads", Some("\u{f10e9}")),
        workspace("Temp", None),
    ];
    let mut focus = Focus::Workspaces {
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
fn workspace_numbers_follow_visible_scrolled_and_filtered_rows() {
    let mut app = fixture();
    app.workspaces = (0..12)
        .map(|i| crate::config::Workspace {
            name: format!("place{i:02}"),
            path: Some("/tmp".into()),
            icon: None,
            ..Default::default()
        })
        .collect();
    let mut focus = Focus::Workspaces {
        query: String::new(),
        index: 11,
    };
    // A row per workspace; a short terminal scrolls to the highlight.
    let mut terminal = Terminal::new(TestBackend::new(100, 16)).unwrap();
    let mut targets = Vec::new();
    terminal
        .draw(|f| targets = rows(render(f, &mut app, &mut focus)))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert_eq!(targets, (5..12).collect::<Vec<_>>(), "{screen}");
    let row = |name: &str| screen.lines().find(|l| l.contains(name)).unwrap_or("");
    assert!(row("place11").contains("› 7/0"), "{screen}");
    assert!(row("place05").contains("  1 "), "{screen}");
    assert!(!screen.contains("place04"), "{screen}");
    focus = Focus::Workspaces {
        query: "place1".into(),
        index: 0,
    };
    terminal
        .draw(|f| targets = rows(render(f, &mut app, &mut focus)))
        .unwrap();
    // The rows are the matches: place01, place10 and place11.
    assert_eq!(targets, vec![0, 1, 2]);
    let mut tiny = Terminal::new(TestBackend::new(35, 11)).unwrap();
    tiny.draw(|f| targets = rows(render(f, &mut app, &mut focus)))
        .unwrap();
    assert!(targets.is_empty());
}
/// The indices of the rows a chooser drew.
fn rows(rendered: Rendered) -> Vec<usize> {
    rendered.choices.into_iter().map(|(_, i)| i).collect()
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
    app.set_program(crate::program::Program::Seek);
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
    // Progress for a moment after work settles; only non-default settings
    // are named. The depth is on the results' bottom border, under the count.
    assert!(lines[2].trim_end().ends_with("✓ complete ╯"), "{screen}");
    assert!(!lines[2].contains("hidden") && !lines[2].contains("depth"));
    assert_eq!(lines[3].trim_end(), "  @  /home/frank/notes");
    assert!(lines[4].contains("Files ·"));
    let bottom = lines.iter().rposition(|l| l.contains("depth 3")).unwrap();
    assert!(lines[bottom].starts_with(" ╰ depth 3 ─"), "{screen}");
    // Once settled for a while, the progress marker goes.
    app.settled_at = Some(std::time::Instant::now() - crate::PROGRESS_SHOWN);
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(!screen.contains("complete"), "{screen}");
    assert!(screen.lines().nth(2).unwrap().contains("╰───"), "{screen}");
    app.settled_at = None;
    assert!(!screen.contains("pinned"));
    app.scanning = true;
    app.policy.set_filters(vec!["*.tex".into()]).unwrap();
    app.policy.set_rules(vec!["build".into()]).unwrap();
    app.policy.ignore = true;
    let (screen, _) = draw(&mut app, 120, 26);
    // Filters and exclusions are named, not spelled out, and only while active.
    assert!(
        screen.contains("◌ scanning, incomplete · filters on · exclusions on"),
        "{screen}"
    );
    assert!(!screen.contains("*.tex"), "{screen}");
    // The configuration's exclusions alone are not named.
    app.config_rules = vec!["build".into()];
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(!screen.contains("exclusions"), "{screen}");
    app.policy
        .set_rules(vec!["build".into(), "*.log".into()])
        .unwrap();
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(screen.contains("exclusions on"), "{screen}");
    app.policy.ignore = false;
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(!screen.contains("exclusions"), "{screen}");
    // The minimum frame keeps an explicit incomplete label.
    let (screen, targets) = draw(&mut app, 36, 12);
    let status = screen.lines().nth(2).unwrap();
    assert!(status.contains("incomplete ╯"), "{screen}");
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
    assert!(pinned.contains("+2 (F2)"), "{screen}");
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
    assert!(text(buffer).contains("✓ complete"));
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
                order: 0,
            },
        );
    }
    app.marks = vec![PathBuf::from("/home/frank/notes/report.tex")].into();
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
    // Rust; a name too long for the row keeps its start.
    assert!(on.contains("\u{e7a8} long_long_") && on.contains("long_… "));
    assert!(!on.contains("tail.rs"));
    assert!(on.contains("● \u{f15c} report.tex")); // marked row
    assert!(off.contains("▸ research/") && off.contains("· todo.txt"));
    assert!(!off.contains('\u{f07b}') && off.contains("● report.tex"));
}
fn keys_screen(app: &App) -> Box<crate::ui::KeysScreen> {
    Box::new(crate::ui::KeysScreen::new(app.cell()))
}
#[test]
fn the_keys_screen_lists_what_works_here_with_its_keys() {
    let mut app = fixture();
    app.keymap = crate::keymap::Keymap::parse(
        "[normal]\nhighlight_next = ['j', '<Space>j']\ntoggle_mark = 'none'\n",
    )
    .unwrap();
    let mut focus = Focus::Keys(keys_screen(&app));
    let mut terminal = Terminal::new(TestBackend::new(140, 46)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(
        screen.contains("showing what works in fff roam · results"),
        "{screen}"
    );
    let row = |name: &str| {
        screen
            .lines()
            .find(|l| {
                l.trim_start_matches(['│', ' ', '›'])
                    .starts_with(&format!("{name} "))
            })
            .unwrap_or_else(|| panic!("{name}: {screen}"))
            .to_string()
    };
    assert!(row("highlight_next").contains("j  Space j"), "{screen}");
    assert!(row("highlight_previous").contains("Alt+k"), "{screen}");
    // The first row is described below the list.
    assert!(
        screen.contains("Highlight the previous row of the list in focus"),
        "{screen}"
    );
    // Only what works here: no dialog answers in fff roam.
    assert!(!screen.contains("save_here"), "{screen}");
    let Focus::Keys(keys) = &mut focus else {
        unreachable!()
    };
    keys.everywhere = true;
    keys.query = "save".into();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("save_here"), "{screen}");
    // The Keys view lists keys with what they do, place by place.
    let Focus::Keys(keys) = &mut focus else {
        unreachable!()
    };
    keys.query = "Enter".into();
    keys.keys = true;
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(
        screen.contains("Keys · ")
            && screen.contains("quit_into_highlighted")
            && screen.contains("open_and_quit"),
        "{screen}"
    );
}
#[test]
fn compact_columns_say_the_shorter_of_a_list_and_all_but() {
    use crate::{keymap::Dimension, program::Program};
    let bits = |programs: &[Program]| programs.iter().fold(0, |b, p| b | p.bit());
    assert_eq!(keys::compact(1, Program::every()), "all");
    assert_eq!(
        keys::compact(1, bits(&[Program::Roam, Program::Pick])),
        "roam, pick"
    );
    assert_eq!(
        keys::compact(1, Program::every() & !Program::Seek.bit()),
        "all but seek"
    );
    let explorers = bits(&[
        Program::ExplorerOpen,
        Program::ExplorerMultiple,
        Program::ExplorerFolder,
        Program::ExplorerSave,
    ]);
    assert_eq!(keys::compact(1, explorers), "explorer");
    assert_eq!(keys::compact(0, 0b11), "both");
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
#[test]
fn path_queries_highlight_letters_along_the_whole_label() {
    let mut app = App::browse("/home/frank/notes".into(), 3, crate::Policy::default());
    for (name, directory, depth) in [
        ("pde", true, 1),
        ("pde/notes", true, 2),
        ("pde/notes/stability.tex", false, 3),
        ("notes/stability.tex", false, 2),
    ] {
        app.insert(
            0,
            Candidate {
                path: app.cwd.join(name),
                directory,
                depth,
                root: 0,
                order: 0,
            },
        );
    }
    app.scanning = false;
    app.query = "pde/stab".into();
    app.refresh();
    let (screen, targets) = draw(&mut app, 80, 14);
    assert_eq!(
        targets,
        [PathBuf::from("/home/frank/notes/pde/notes/stability.tex")],
        "{screen}"
    );
    let mut terminal = Terminal::new(TestBackend::new(80, 14)).unwrap();
    let mut focus = Focus::Results;
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let row = (0..buffer.area.height)
        .find(|&y| {
            text(buffer)
                .lines()
                .nth(y as usize)
                .unwrap()
                .contains("stability")
        })
        .unwrap();
    let underlined: String = (0..buffer.area.width)
        .filter(|&x| buffer[(x, row)].modifier.contains(Modifier::UNDERLINED))
        .map(|x| buffer[(x, row)].symbol())
        .collect();
    assert_eq!(underlined, "pdestab");
}
#[test]
fn marks_and_chooser_rows_record_where_they_were_drawn() {
    let mut app = fixture();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    let mut focus = Focus::Results;
    let mut rendered = None;
    terminal
        .draw(|f| rendered = Some(render(f, &mut app, &mut focus)))
        .unwrap();
    let rendered = rendered.unwrap();
    let buffer = terminal.backend().buffer().clone();
    // One area per mark, inside the pane, each starting at its title.
    assert_eq!(
        rendered.marks.iter().map(|(_, i)| *i).collect::<Vec<_>>(),
        [0, 1]
    );
    for (area, i) in &rendered.marks {
        assert!(rendered.marks_pane.contains(area.as_position()));
        let line: String = (area.x..area.right())
            .map(|x| buffer[(x, area.y)].symbol())
            .collect();
        let name = ["Turing 1952.pdf", "report.tex"][*i];
        assert!(line.contains(name), "{line}");
    }
    let mut focus = Focus::Palettes {
        query: String::new(),
        index: 0,
        original: app.palette,
    };
    let mut rendered = None;
    terminal
        .draw(|f| rendered = Some(render(f, &mut app, &mut focus)))
        .unwrap();
    let rendered = rendered.unwrap();
    let buffer = terminal.backend().buffer();
    assert!(rendered.choices.len() > 3);
    for (area, i) in &rendered.choices {
        let line: String = (area.x..area.right())
            .map(|x| buffer[(x, area.y)].symbol())
            .collect();
        assert!(line.contains(&app.palettes[*i].0), "{line}");
    }
}
#[test]
fn long_names_elided_to_fit_still_highlight_without_panicking() {
    let mut app = App::browse("/fixture".into(), 2, crate::Policy::default());
    let name = "deps/libfuzzy_file_finder-0123456789abcdef0123456789abcdef.rlib";
    app.insert(
        0,
        Candidate {
            path: app.cwd.join(name),
            directory: false,
            depth: 2,
            root: 0,
            order: 0,
        },
    );
    app.scanning = false;
    // Any width that cuts into the name, with and without a query.
    for query in ["", "rlib", "deps/rlib"] {
        app.query = query.into();
        app.refresh();
        for width in 40..200 {
            let (screen, targets) = draw(&mut app, width, 12);
            assert_eq!(targets.len(), 1, "{query:?} at {width}:\n{screen}");
        }
    }
}
#[test]
fn chosen_go_to_suggestion_stays_visible_on_short_terminals() {
    let mut app = fixture();
    let places: Vec<PathBuf> = (0..10).map(|i| format!("/place{i}").into()).collect();
    let mut location = crate::location::Location::new(&[], places, Path::new("/"));
    location.selected = Some(5);
    let mut focus = Focus::Location(location);
    for height in [14, 16, 18, 26] {
        let mut terminal = Terminal::new(TestBackend::new(80, height)).unwrap();
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(screen.contains("place5"), "{height} rows:\n{screen}");
    }
}
#[test]
fn elision_keeps_the_end_and_truncation_the_start_measured_in_columns() {
    assert_eq!(elide("notes/report.tex", 16), "notes/report.tex");
    assert_eq!(elide("notes/report.tex", 11), "…report.tex");
    // Wide characters take two columns, the ellipsis one; none is split.
    assert_eq!(elide("日本語.txt", 7), "…語.txt");
    assert_eq!(elide("日本語.txt", 6), "….txt");
    assert_eq!(elide("abc", 0), "");
    assert_eq!(truncate("Moved 2 entries", 15), "Moved 2 entries");
    assert_eq!(truncate("Moved 2 entries", 8), "Moved 2…");
    assert_eq!(truncate("日本語", 4), "日…");
    assert_eq!(truncate("abc", 1), "…");
    assert_eq!(truncate("abc", 0), "");
}
#[test]
fn detail_columns_give_way_from_the_left_to_names() {
    use Column::*;
    let all = [Permissions, Size, Modified];
    // Each column takes its width and two spaces; names keep 20 columns
    // after the row prefix's 10.
    assert_eq!(fitting_columns(&all, 55), all);
    assert_eq!(fitting_columns(&all, 54), [Size, Modified]);
    assert_eq!(fitting_columns(&all, 43), [Size, Modified]);
    assert_eq!(fitting_columns(&all, 42), [Modified]);
    assert_eq!(fitting_columns(&all, 36), [Modified]);
    assert!(fitting_columns(&all, 35).is_empty());
    assert!(fitting_columns(&[], 200).is_empty());
}
#[test]
fn pinned_roots_that_do_not_fit_are_counted() {
    let spans = |names: &[&str]| {
        let mut spans = vec![Span::raw("pinned")];
        for name in names {
            spans.extend([Span::raw("  "), Span::raw(name.to_string())]);
        }
        spans
    };
    let text =
        |spans: Vec<Span<'static>>| spans.iter().map(|s| s.content.as_ref()).collect::<String>();
    let roots = ["notes", "papers", "books"];
    assert_eq!(
        text(fit_spans(spans(&roots), 28, Some("F1"))),
        "pinned  notes  papers  books"
    );
    assert_eq!(
        text(fit_spans(spans(&roots), 27, Some("F1"))),
        "pinned  notes  +2 (F1)"
    );
    assert_eq!(
        text(fit_spans(spans(&roots), 15, Some("F1"))),
        "pinned  +3 (F1)"
    );
    // Too narrow even for the count: it is elided, never wider.
    assert!(text(fit_spans(spans(&roots), 10, Some("F1"))).width() <= 10);
}
#[test]
fn status_items_drop_from_the_end_before_the_label_shortens() {
    let mut app = fixture();
    app.scanning = true;
    app.policy.set_filters(vec!["*.tex".into()]).unwrap();
    app.policy.hidden = true;
    let text = |width| {
        status_line(&app, width)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect::<String>()
    };
    let full = text(200);
    for item in ["◌ scanning, incomplete", "filters on", "hidden shown"] {
        assert!(full.contains(item), "{item:?} in {full:?}");
    }
    let shortest = text(0);
    assert_eq!(shortest.trim(), "◌ incomplete");
    for width in 0..200 {
        let shown = text(width);
        assert!(
            shown.width() <= width || shown == shortest,
            "{width}: {shown:?}"
        );
        // Later items go first, and the label shortens only once they are gone.
        assert!(!shown.contains("hidden shown") || shown.contains("filters on"));
        assert!(
            shown.contains("scanning") || !shown.contains("filters on"),
            "{shown:?}"
        );
    }
}
#[test]
fn text_wraps_between_words_and_the_guide_keeps_descriptions_first_words() {
    assert_eq!(
        wrap_text("Could not open a.pdf: zathura exited", 12),
        ["Could not", "open a.pdf:", "zathura", "exited"]
    );
    // A word longer than a row is broken where the row ends.
    assert_eq!(wrap_text("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    assert_eq!(wrap_text("", 4), [""]);
    let mut app = fixture();
    // A compact marked list shows marks, not gaps between them.
    app.marks = vec![app.cwd.join("a"), app.cwd.join("b")].into();
    let mut terminal = Terminal::new(TestBackend::new(60, 16)).unwrap();
    let mut focus = Focus::Results;
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(
        screen.contains(" a [MISSING]") && screen.contains(" b [MISSING]"),
        "{screen}"
    );
}
#[test]
fn the_preview_scrolls_its_content_and_keeps_its_facts() {
    use crate::preview::{Key, Preview, Tone};
    let mut app = fixture();
    let key = Key {
        path: app.cwd.join("todo.txt"),
        size: 9,
        modified: None,
        hidden: false,
    };
    app.preview.set(
        key,
        Preview {
            kind: "text · 100 lines".into(),
            facts: vec![("Pages".into(), "12".into())],
            lines: (1..=100)
                .map(|n| (format!("line {n:03}"), Tone::Text))
                .collect(),
            ..Default::default()
        },
    );
    let (screen, _) = draw(&mut app, 120, 40);
    assert!(screen.contains("line 001") && !screen.contains("line 100"));
    // Down by half the room, never past the last line; facts stay.
    assert!(app.preview.scroll(true));
    let (screen, _) = draw(&mut app, 120, 40);
    assert!(!screen.contains("line 001") && screen.contains("Pages  12"));
    while app.preview.scroll(true) {}
    let (screen, _) = draw(&mut app, 120, 40);
    assert!(screen.contains("line 100"), "{screen}");
    let below = screen.lines().position(|l| l.contains("line 100")).unwrap();
    assert!(screen.lines().nth(below + 1).unwrap().contains('╰'));
    while app.preview.scroll(false) {}
    let (screen, _) = draw(&mut app, 120, 40);
    assert!(screen.contains("line 001"));
    assert!(!app.preview.scroll(false));
}
#[test]
fn the_trash_is_named_on_the_location_line_with_its_restore_key() {
    let mut app = fixture();
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(!screen.contains("trash ·"));
    app.trash_folders = vec![app.cwd.clone()];
    let (screen, _) = draw(&mut app, 120, 26);
    let line = screen.lines().find(|l| l.contains(" @ ")).unwrap();
    assert!(line.contains("trash · R restores"), "{screen}");
    // The key shown is the configured one.
    app.keymap = crate::keymap::Keymap::parse("[normal]\nrestore = 'gr'\n").unwrap();
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(screen.contains("trash · gr restores"), "{screen}");
}
#[test]
fn a_taken_place_offers_to_overwrite_keep_both_or_cancel() {
    use crate::ui::{Replace, Replacing};
    let mut app = fixture();
    let notes = app.cwd.join("notes.md");
    let both = app.cwd.join("notes (1).md");
    let bin = app.cwd.join("trash/files/notes.md");
    let draw_focus = |app: &mut App, mut focus: Focus| {
        let mut terminal = Terminal::new(TestBackend::new(120, 26)).unwrap();
        terminal
            .draw(|frame| _ = render(frame, app, &mut focus))
            .unwrap();
        text(terminal.backend().buffer())
    };
    let plan =
        crate::operations::RestorePlan::shown(vec![(bin, notes.clone())], vec![(0, both.clone())]);
    let screen = draw_focus(
        &mut app,
        Focus::Replace(Replace {
            taken: vec![(notes.clone(), both.clone())],
            then: Replacing::Restore(plan),
        }),
    );
    assert!(screen.contains("╭ Restore"), "{screen}");
    assert!(screen.contains("notes (1).md") && screen.contains("goes to the trash"));
    assert!(screen.contains("Enter: Overwrite") && screen.contains("Esc: Cancel"));
    // Saving a file dialog's answer asks the same way.
    let screen = draw_focus(
        &mut app,
        Focus::Replace(Replace {
            taken: vec![(notes, both)],
            then: Replacing::Save,
        }),
    );
    assert!(
        screen.contains("╭ Replace") && screen.contains("Esc: Back"),
        "{screen}"
    );
}
#[test]
fn rebound_keys_are_shown_as_bound() {
    let mut app = fixture();
    app.keymap = crate::keymap::Keymap::parse(
        "[insert]\nshow_keys = '<C-g>'\n[normal]\nshow_keys = '<C-g>'\nquit = 'Q'\ngo_to_trash = 'T'\n",
    )
    .unwrap();
    // The footer points to the Keys screen and says how to leave, as bound.
    let (screen, _) = draw(&mut app, 120, 26);
    assert!(screen.contains("Ctrl+g keys  Q quit"), "{screen}");
    // The Keys screen names its own key, and the keys as bound.
    let mut focus = Focus::Keys(keys_screen(&app));
    let mut terminal = Terminal::new(TestBackend::new(140, 60)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(screen.contains("Ctrl+g close"), "{screen}");
    let line = screen
        .lines()
        .find(|l| l.contains(" go_to_trash ") && l.contains('•'))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(line.contains("Alt+t") && line.contains(" T "), "{screen}");
}
#[test]
fn preview_lines_wrap_at_spaces_and_inside_words_only_when_they_fill_a_line() {
    let rows = |text: &str, width, wrap| -> Vec<String> {
        fit(vec![Span::raw(text.to_owned())], width, wrap)
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    };
    assert_eq!(rows("short", 10, true), ["short"]);
    assert_eq!(
        rows("the quick brown fox", 10, true),
        ["the quick", "brown fox"]
    );
    assert_eq!(rows("abcdefghijkl", 5, true), ["abcde", "fghij", "kl"]);
    assert_eq!(rows("the quick brown fox", 10, false), ["the quick…"]);
    // Styles survive the break.
    let lines = fit(
        vec![
            Span::raw("aa "),
            Span::styled("bb", Style::default().add_modifier(Modifier::BOLD)),
        ],
        3,
        true,
    );
    assert_eq!(lines[1].spans[0].content, "bb");
    assert!(
        lines[1].spans[0]
            .style
            .add_modifier
            .contains(Modifier::BOLD)
    );
}
#[test]
fn a_sheet_that_does_not_fit_asks_for_a_larger_terminal_instead_of_being_cut() {
    let mut app = fixture();
    let mut screen = keys_screen(&app);
    screen.editor = Some(crate::ui::Editor::new(
        &app.keymap,
        crate::keymap::Function::HighlightPrevious,
        0,
    ));
    let mut focus = Focus::Keys(screen);
    let mut frame = |height| {
        let mut terminal = Terminal::new(TestBackend::new(100, height)).unwrap();
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        text(terminal.backend().buffer())
    };
    // Room for all of it.
    let screen = frame(40);
    assert!(screen.contains("Keys of highlight_previous"), "{screen}");
    assert!(
        screen.contains("APPLIES IN THE HIGHLIGHTED ROW"),
        "{screen}"
    );
    // Less: what repeats the highlighted row goes first.
    let screen = frame(24);
    assert!(screen.contains("Keys of highlight_previous"), "{screen}");
    assert!(
        !screen.contains("APPLIES IN THE HIGHLIGHTED ROW"),
        "{screen}"
    );
    // Too little for its keys: nothing is drawn but what size it needs.
    let screen = frame(14);
    assert!(!screen.contains("Keys of"), "{screen}");
    assert!(
        screen.contains("Resize terminal to at least 100 × "),
        "{screen}"
    );
}
#[test]
fn the_contents_bar_sits_right_of_the_names_and_the_typed_one_gives_way_last() {
    let mut app = fixture();
    // fff roam shows one bar until the contents query is typed into.
    let (screen, _) = draw(&mut app, 100, 20);
    assert!(screen.contains(" Query ") && !screen.contains("Contents"));
    app.grep.editing = true;
    let (screen, _) = draw(&mut app, 100, 20);
    let top = screen.lines().next().unwrap();
    let (names, contents) = (top.find("Names").unwrap(), top.find("Contents").unwrap());
    assert!(names < contents, "{top}");
    // The bar typed into takes three fifths.
    assert_eq!(query_widths(100, "", "", true), (40, 60));
    assert_eq!(query_widths(100, "", "", false), (60, 40));
    // A long query widens its bar; the other keeps a third and cuts its
    // query with an ellipsis.
    assert_eq!(query_widths(100, "", &"x".repeat(80), true), (33, 67));
    app.query = "a names query much longer than its bar".into();
    app.grep.text = "y".repeat(80);
    let (screen, _) = draw(&mut app, 100, 20);
    let line = screen.lines().nth(1).unwrap();
    assert!(line.contains("❯ a names query much longer…"), "{line}");
    assert!(line.contains("❯ …yyyy"), "{line}");
}
#[test]
fn a_cut_line_keeps_its_match_marked_wherever_the_cut_falls() {
    // Every cut before the match, the nearest ones included: the ellipsis
    // can be longer than what it replaces.
    let line = "0123456789abcdef needle and more";
    let at = line.find("needle").unwrap();
    let parts = vec![at..at + 6, line.len() - 4..line.len()];
    for width in 0..=80 {
        let (text, parts) = lead_window(line, &parts, width);
        assert_eq!(&text[parts[0].clone()], "needle", "{width}: {text}");
        assert_eq!(&text[parts[1].clone()], "more", "{width}: {text}");
    }
}
#[test]
fn long_names_keep_their_matched_letters_else_their_start_in_view() {
    let shown = |label: &str, start: usize, query: &str, width: usize| {
        let chars: Vec<char> = label.chars().collect();
        let matched: HashSet<usize> = label
            .find(query)
            .filter(|_| !query.is_empty())
            .map(|at| {
                let at = label[..at].chars().count();
                (at..at + query.chars().count()).collect()
            })
            .unwrap_or_default();
        let (from, to) = results::name_window(&chars, start, &matched, width);
        let text: String = chars[from..to].iter().collect();
        let text = format!(
            "{}{text}{}",
            if from > 0 { "…" } else { "" },
            if to < chars.len() { "…" } else { "" }
        );
        assert!(text.width() <= width, "{text:?} is wider than {width}");
        text
    };
    let name = "Introduction to the theory of linear nonselfadjoint operators \
                (Gohberg, I. (Israel) ,1928-, Kreĭn etc.) (z-library.sk).pdf";
    assert_eq!(
        shown(name, 0, "", 40),
        "Introduction to the theory of linear no…"
    );
    assert_eq!(
        shown(name, 0, "Gohberg", 40),
        "…int operators (Gohberg, I. (Israel) ,1…"
    );
    // Letters the start shows stay there; ones near the end show the end.
    assert_eq!(
        shown(name, 0, "theory", 40),
        "Introduction to the theory of linear no…"
    );
    assert_eq!(
        shown(name, 0, "z-library", 30),
        "…reĭn etc.) (z-library.sk).pdf"
    );
    // A path's parent is cut first, while the whole name fits.
    assert_eq!(shown("notes/report.tex", 6, "", 11), "…report.tex");
    assert_eq!(shown("notes/report.tex", 6, "notes", 11), "notes/repo…");
    assert_eq!(shown("short.txt", 0, "", 9), "short.txt");
    assert_eq!(shown("日本語の長い名前.txt", 0, "", 9), "日本語の…");
}
#[test]
fn frames_drawn_while_a_scan_adds_paths_show_what_a_fresh_ranking_would() {
    // A breadth-first scan's paths, shallow first, names shared across
    // folders, merged in rounds that each become a run of the rows.
    let mut seen = std::collections::HashSet::new();
    let mut paths = Vec::new();
    for depth in 1..=3usize {
        for i in 0..1500u64 {
            let n = i.wrapping_mul(2_654_435_761) % 1500;
            let folders: String = (1..depth as u64)
                .map(|d| format!("d{}/", (n + d) % 4))
                .collect();
            let path = PathBuf::from(format!("/fixture/{folders}n{:03}", n % 700));
            if seen.insert(path.clone()) {
                paths.push(Candidate {
                    path,
                    directory: n.is_multiple_of(5),
                    depth,
                    root: 0,
                    order: 0,
                });
            }
        }
    }
    // Both orders; `fff pick`'s short labels catch up within budgets of
    // their own, so they are left out.
    for program in [crate::program::Program::Roam, crate::program::Program::Seek] {
        let mut app = App::browse("/fixture".into(), 3, crate::Policy::default());
        app.set_program(program);
        app.scanning = false;
        app.refresh();
        let mut inserted = 0;
        for batch in paths.chunks(700) {
            for c in batch {
                app.insert(0, c.clone());
            }
            inserted += batch.len();
            app.refresh_within(Some(std::time::Duration::ZERO));
            let merged = inserted - app.unmerged();
            let mut fresh = App::browse("/fixture".into(), 3, crate::Policy::default());
            fresh.set_program(program);
            fresh.scanning = false;
            for c in &paths[..merged] {
                fresh.insert(0, c.clone());
            }
            fresh.refresh();
            // Somewhere down the list, with rows above and below.
            for (at, offset) in [(0, 0), (30, 20), (fresh.rows.len() - 1, 0)] {
                let path = fresh.rows[at].path.clone();
                for app in [&mut app, &mut fresh] {
                    app.highlight = Some(path.clone());
                    app.offset = offset;
                    app.scrolled = None;
                }
                assert_eq!(draw(&mut app, 120, 30), draw(&mut fresh, 120, 30));
                assert_eq!(app.offset, fresh.offset);
            }
        }
    }
}
#[test]
fn workspace_form_explains_the_optional_path_under_it() {
    let mut app = fixture();
    let draft =
        crate::workspaces::Draft::new(&app.cwd.clone(), None, app.snapshot(), String::new(), 0)
            .unwrap();
    let mut focus = Focus::WorkspaceEdit(draft);
    let mut terminal = Terminal::new(TestBackend::new(100, 34)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    let lines: Vec<_> = screen.lines().collect();
    let path = lines.iter().position(|l| l.contains("Path  ")).unwrap();
    // The fixture's folder is not on disk: that comes first, then that the
    // path is optional, both in the value's column.
    assert!(
        lines[path + 1].contains("This directory does not exist"),
        "{screen}"
    );
    assert!(lines[path + 2].contains("Optional: left empty"), "{screen}");
    let column = |l: &str, word: &str| l.find(word).map(|i| l[..i].chars().count());
    assert_eq!(
        column(lines[path + 2], "Optional"),
        column(lines[path], "/home"),
        "{screen}"
    );
    assert!(lines[path - 1].contains("› Name   notes"), "{screen}");
    assert!(screen.contains("  KEEPS"), "{screen}");
    assert!(
        screen.contains("Tab: Next field ─── Enter: Save ─── Esc: Cancel"),
        "{screen}"
    );
    // An empty path still says it is optional, and nothing is missing.
    if let Focus::WorkspaceEdit(draft) = &mut focus {
        draft.path.clear();
    }
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(!screen.contains("does not exist"), "{screen}");
    assert!(screen.contains("Optional: left empty"), "{screen}");
}
#[test]
fn workspace_chooser_is_drawn_as_the_pickers_are() {
    let mut app = fixture();
    app.icons = false;
    let workspace = |name: &str, path: Option<&str>| crate::config::Workspace {
        name: name.into(),
        path: path.map(Into::into),
        depth: path.is_none().then_some(2),
        ..Default::default()
    };
    app.workspaces = vec![
        workspace("Downloads", Some("/tmp")),
        workspace("Shallow", None),
    ];
    let mut focus = Focus::Workspaces {
        query: String::new(),
        index: 1,
    };
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    // The count on the top border, the keys on the bottom one, the filter
    // with its prompt glyph and one row per workspace, names aligned.
    assert!(screen.contains("╭ Workspaces ─"), "{screen}");
    assert!(screen.contains(" 2 workspaces ╮"), "{screen}");
    assert!(screen.contains("❯ type a workspace's name…"), "{screen}");
    assert!(screen.contains("    1 Downloads  /tmp"), "{screen}");
    assert!(screen.contains("› 2/0 Shallow    depth 2"), "{screen}");
    assert!(
        screen.contains("Enter: Open ─ Alt+n: New ─ Alt+e: Edit"),
        "{screen}"
    );
    assert!(screen.contains("Esc: Close"), "{screen}");
    assert!(!screen.contains("Find:"), "{screen}");
    app.workspaces.clear();
    terminal
        .draw(|f| _ = render(f, &mut app, &mut focus))
        .unwrap();
    let screen = text(terminal.backend().buffer());
    assert!(
        screen.contains("No workspaces yet. Alt+n adds one"),
        "{screen}"
    );
}
#[test]
fn workspace_names_and_paths_never_reach_the_terminal_as_control_characters() {
    let mut app = fixture();
    app.workspaces = vec![crate::config::Workspace {
        name: "evil\u{1b}]52;c;payload\u{7}".into(),
        path: Some("/tmp/\u{1b}[2Jgone\u{9b}31m".into()),
        ..Default::default()
    }];
    let draft = crate::workspaces::Draft::new(
        &app.cwd.clone(),
        Some((0, &app.workspaces[0].clone())),
        app.snapshot(),
        String::new(),
        0,
    )
    .unwrap();
    for mut focus in [
        Focus::Workspaces {
            query: String::new(),
            index: 0,
        },
        Focus::WorkspaceEdit(draft),
        Focus::WorkspaceDelete {
            target: 0,
            query: String::new(),
            index: 0,
        },
    ] {
        let mut terminal = Terminal::new(TestBackend::new(120, 34)).unwrap();
        terminal
            .draw(|f| _ = render(f, &mut app, &mut focus))
            .unwrap();
        let screen = text(terminal.backend().buffer());
        assert!(
            !screen.chars().any(|c| c.is_control() && c != '\n'),
            "{screen:?}"
        );
        assert!(screen.contains("evil\\u{1b}]52"), "{screen}");
        if let Focus::Workspaces { .. } = focus {
            assert!(screen.contains("/tmp/\\u{1b}[2Jgone"), "{screen}");
        }
    }
}
#[test]
fn disambiguation_matches_counting_suffixes_length_by_length() {
    // The labels as first computed: suffixes counted at each length, the
    // paths they resolve dropping out.
    use std::collections::HashMap;
    fn counted(paths: &[PathBuf]) -> HashMap<PathBuf, PathBuf> {
        let mut groups: HashMap<_, Vec<_>> = HashMap::new();
        for path in paths {
            groups
                .entry(path.file_name().unwrap())
                .or_default()
                .push(path);
        }
        let mut labels = HashMap::new();
        for group in groups.into_values().filter(|g| g.len() > 1) {
            let parts: Vec<Vec<_>> = group
                .iter()
                .map(|p| p.components().rev().collect())
                .collect();
            let mut pending: Vec<usize> = (0..group.len()).collect();
            let mut length = 2;
            while !pending.is_empty() {
                let mut counts: HashMap<&[_], usize> = HashMap::new();
                for &i in &pending {
                    *counts
                        .entry(&parts[i][..length.min(parts[i].len())])
                        .or_default() += 1;
                }
                pending.retain(|&i| {
                    let suffix = &parts[i][..length.min(parts[i].len())];
                    if counts[suffix] > 1 && length < parts[i].len() {
                        return true;
                    }
                    labels.insert(group[i].clone(), suffix.iter().rev().collect::<PathBuf>());
                    false
                });
                length += 1;
            }
        }
        labels
    }
    // Paths of up to five parts from a few names, absolute and relative.
    let names = ["a", "b", "c"];
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    for _ in 0..500 {
        let mut paths = Vec::new();
        for _ in 0..12 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let mut path = PathBuf::from(if seed & 1 == 0 { "/" } else { "" });
            for k in 0..1 + (seed >> 1) % 5 {
                path.push(names[((seed >> (8 + 2 * k)) % 3) as usize]);
            }
            if !paths.contains(&path) {
                paths.push(path);
            }
        }
        assert_eq!(
            crate::disambiguate(paths.iter().map(PathBuf::as_path)),
            counted(&paths),
            "{paths:?}"
        );
    }
}
