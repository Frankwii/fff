use super::*;
use crate::config::Palette;
use std::time::Instant;

#[test]
fn exclusion_exit_applies_valid_edits_and_rejects_invalid_edits_atomically() {
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    app.policy.set_rules(vec!["build".into()]).unwrap();
    let mut discovery = Discovery::new(Default::default());
    for (text, expected, error) in [
        ("build", vec!["build"], false),
        ("build\n*.log", vec!["build", "*.log"], false),
        ("changed\n\n[", vec!["build", "*.log"], true),
        ("", vec![], false),
    ] {
        app.status.clear();
        let mut focus = Focus::Exclusions(text.into());
        apply_globs(&mut app, &mut focus, &mut discovery);
        assert!(matches!(focus, Focus::Results));
        assert_eq!(app.policy.rules, expected);
        assert_eq!(
            app.status.starts_with("Exclusion line 3:"),
            error,
            "{}",
            app.status
        );
    }
}

#[test]
fn filter_exit_applies_globs_without_touching_exclusions() {
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    app.policy.set_rules(vec!["build".into()]).unwrap();
    let mut discovery = Discovery::new(Default::default());
    let generation = app.generation;
    for (text, expected, error) in [
        ("*.pdf\n\n*.epub", vec!["*.pdf", "*.epub"], false),
        ("*.pdf\n[", vec!["*.pdf", "*.epub"], true),
        ("", vec![], false),
    ] {
        app.status.clear();
        let mut focus = Focus::Filters(text.into());
        apply_globs(&mut app, &mut focus, &mut discovery);
        assert!(matches!(focus, Focus::Results));
        assert_eq!(app.policy.filters, expected);
        assert_eq!(
            app.status.starts_with("Filter line 2:"),
            error,
            "{}",
            app.status
        );
    }
    assert_eq!(app.policy.rules, ["build"]);
    // Filters choose among discovered files; they never restart discovery.
    assert_eq!(app.generation, generation);
}

#[test]
fn palette_preview_waits_a_full_dwell_and_restarts_on_highlight_changes() {
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    let original = app.palette;
    let mut focus = Focus::Palettes {
        query: "nord".into(),
        index: 0,
        original,
    };
    let mut preview = PalettePreview::default();
    let now = Instant::now();
    assert!(!preview.tick(&mut app, &focus, now));
    assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(99)));
    assert_eq!(app.palette, original);
    assert!(preview.tick(&mut app, &focus, now + Duration::from_millis(100)));
    let nord = Palette::named("nord").unwrap();
    assert_eq!(app.palette, nord);
    if let Focus::Palettes { query, .. } = &mut focus {
        *query = "dracula".into();
    }
    assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(101)));
    if let Focus::Palettes { query, .. } = &mut focus {
        *query = "light".into();
    }
    assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(150)));
    assert!(!preview.tick(&mut app, &focus, now + Duration::from_millis(249)));
    assert_eq!(app.palette, nord);
    assert!(preview.tick(&mut app, &focus, now + Duration::from_millis(250)));
    assert_eq!(app.palette, Palette::named("light").unwrap());
    assert_eq!(preview.remaining(now + Duration::from_secs(1)), None);
}

#[test]
fn leaving_palette_chooser_discards_pending_preview() {
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    let original = app.palette;
    let focus = Focus::Palettes {
        query: "nord".into(),
        index: 0,
        original,
    };
    let now = Instant::now();
    let mut preview = PalettePreview::default();
    preview.tick(&mut app, &focus, now);
    assert!(!preview.tick(&mut app, &Focus::Results, now + Duration::from_millis(20)));
    assert!(!preview.tick(&mut app, &Focus::Results, now + Duration::from_secs(1)));
    assert_eq!(app.palette, original);
    assert_eq!(preview.remaining(now), None);
}

#[test]
fn m_marks_every_listed_row_and_shift_m_inverts_them() {
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    let [a, b, c, other]: [PathBuf; 4] =
        ["/fixture/a", "/fixture/b", "/fixture/c", "/elsewhere/d"].map(PathBuf::from);
    app.marks = vec![other.clone(), b.clone()].into();
    let mut focus = Focus::Marks {
        rows: vec![a.clone(), b.clone(), c.clone()],
        index: 0,
    };
    let Ok(mut editor) = crate::editor::Nvim::start() else {
        eprintln!("nvim missing; skipping");
        return;
    };
    let mut press = |app: &mut App, ch: char| {
        let shift = if ch.is_uppercase() { M::SHIFT } else { M::NONE };
        let key = KeyEvent::new(K::Char(ch), shift);
        assert!(matches!(
            normal_list_key(app, &mut focus, &mut editor, key),
            NormalKey::Consumed
        ));
    };
    press(&mut app, 'm');
    assert_eq!(app.marks, [&other, &b, &a, &c].map(Clone::clone));
    press(&mut app, 'M');
    assert_eq!(app.marks, std::slice::from_ref(&other));
    app.marks.push(b.clone());
    press(&mut app, 'M');
    assert_eq!(app.marks, [&other, &a, &c].map(Clone::clone));
}

#[test]
fn clicks_highlight_drags_mark_double_clicks_enter_and_the_wheel_only_scrolls() {
    let root = std::env::temp_dir().join(format!("fff-mouse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("dir")).unwrap();
    let mut app = App::browse(root.clone(), 1, crate::Policy::default());
    let names = ["dir", "a", "b", "c", "d", "e", "f", "g"];
    for name in names {
        app.insert(
            0,
            crate::Candidate {
                path: root.join(name),
                directory: name == "dir",
                depth: 1,
                root: 0,
            },
        );
    }
    app.refresh();
    app.scanning = false;
    // Six rows on screen, starting at y = 5.
    app.rendered = app.rows.iter().take(6).map(|c| c.path.clone()).collect();
    let mut discovery = Discovery::new(Default::default());
    let area = ratatui::layout::Rect::new(2, 5, 40, 6);
    let mut state = Mouse::default();
    let mut mouse = |app: &mut App, kind, column, row| {
        let event = MouseEvent {
            kind,
            column,
            row,
            modifiers: M::NONE,
        };
        mouse_results(app, &mut discovery, event, area, &mut state);
    };
    let (left, drag, up) = (
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Drag(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    );
    let path = |name: &str| root.join(name);
    let marked = vec![path("f"), path("g")];
    app.marks = marked.clone().into();
    // Outside the rows, past the last one, or another button: nothing changes.
    mouse(&mut app, left, 1, 8);
    mouse(&mut app, left, 10, 30);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 10, 8);
    assert_eq!(app.marks, marked);
    // The wheel scrolls the view, keeping the highlight and the marks.
    app.highlight = Some(path("a"));
    mouse(&mut app, MouseEventKind::ScrollDown, 0, 0);
    assert_eq!((app.offset, app.highlight.clone()), (2, Some(path("a"))));
    mouse(&mut app, MouseEventKind::ScrollUp, 0, 0);
    assert_eq!(app.offset, 0);
    assert_eq!(app.marks, marked);
    // A click clears the marks and highlights; a drag marks the rows it spans.
    mouse(&mut app, left, 10, 7);
    assert_eq!(app.highlight, Some(path("b")));
    assert!(app.marks.is_empty());
    mouse(&mut app, drag, 10, 7);
    assert!(app.marks.is_empty());
    mouse(&mut app, drag, 10, 9);
    assert_eq!(app.marks, [path("b"), path("c"), path("d")]);
    assert_eq!(app.highlight, Some(path("d")));
    // Upwards, past the top: back to the first row, in drag order.
    mouse(&mut app, drag, 10, 2);
    assert_eq!(app.marks, [path("b"), path("a"), path("dir")]);
    mouse(&mut app, up, 10, 2);
    mouse(&mut app, drag, 10, 9);
    assert_eq!(app.marks, [path("b"), path("a"), path("dir")]);
    // A second click on the same row enters the directory.
    mouse(&mut app, left, 10, 5);
    assert!(app.marks.is_empty());
    assert_eq!(app.cwd, root);
    mouse(&mut app, left, 10, 5);
    assert_eq!(app.cwd, root.join("dir"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn a_drag_held_past_an_edge_scrolls_and_marks_on() {
    let root = PathBuf::from("/fixture");
    let mut app = App::browse(root.clone(), 1, crate::Policy::default());
    let names = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"];
    for name in names {
        app.insert(
            0,
            crate::Candidate {
                path: root.join(name),
                directory: false,
                depth: 1,
                root: 0,
            },
        );
    }
    app.refresh();
    // Four rows on screen from y = 5; drawing follows the highlight.
    let area = ratatui::layout::Rect::new(2, 5, 40, 4);
    let draw = |app: &mut App| {
        let i = app.highlighted().unwrap_or(0);
        if app.scrolled.is_none() {
            app.offset = app.offset.min(i).max((i + 1).saturating_sub(4));
        }
        app.rendered = app.rows[app.offset..(app.offset + 4).min(app.rows.len())]
            .iter()
            .map(|c| c.path.clone())
            .collect();
    };
    draw(&mut app);
    let mut discovery = Discovery::new(Default::default());
    let mut state = Mouse::default();
    let event = |kind, row| MouseEvent {
        kind,
        column: 10,
        row,
        modifiers: M::NONE,
    };
    let path = |name: &str| root.join(name);
    let names_marked = |app: &App| {
        app.marks
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap().to_owned())
            .collect::<Vec<_>>()
            .join("")
    };
    mouse_results(
        &mut app,
        &mut discovery,
        event(MouseEventKind::Down(MouseButton::Left), 6),
        area,
        &mut state,
    );
    assert_eq!(app.highlight, Some(path("b")));
    // Below the bottom edge: the row beyond the visible ones.
    mouse_results(
        &mut app,
        &mut discovery,
        event(MouseEventKind::Drag(MouseButton::Left), 12),
        area,
        &mut state,
    );
    assert_eq!(names_marked(&app), "bcde");
    draw(&mut app);
    assert_eq!(app.offset, 1);
    // Held there, it scrolls on each step, not before.
    let due = state.edge_due().unwrap();
    assert!(!drag_at_edge(
        &mut app,
        area,
        &mut state,
        due - EDGE_STEP / 2
    ));
    for (step, marked) in [(1, "bcdef"), (2, "bcdefg")] {
        assert!(drag_at_edge(
            &mut app,
            area,
            &mut state,
            due + EDGE_STEP * step
        ));
        assert_eq!(names_marked(&app), marked);
        draw(&mut app);
    }
    // Back inside, it stops; past the top it scrolls up, in drag order.
    mouse_results(
        &mut app,
        &mut discovery,
        event(MouseEventKind::Drag(MouseButton::Left), 6),
        area,
        &mut state,
    );
    assert_eq!(state.edge_due(), None);
    draw(&mut app);
    let offset = app.offset;
    mouse_results(
        &mut app,
        &mut discovery,
        event(MouseEventKind::Drag(MouseButton::Left), 1),
        area,
        &mut state,
    );
    assert_eq!(app.highlight, Some(path(names[offset - 1])));
    draw(&mut app);
    let due = state.edge_due().unwrap();
    while app.offset > 0 {
        assert!(drag_at_edge(
            &mut app,
            area,
            &mut state,
            due + EDGE_STEP * 10
        ));
        draw(&mut app);
        state.edge = state.edge.map(|(e, _)| (e, due));
    }
    assert_eq!(names_marked(&app), "ba");
    // Releasing the button ends it.
    mouse_results(
        &mut app,
        &mut discovery,
        event(MouseEventKind::Up(MouseButton::Left), 1),
        area,
        &mut state,
    );
    assert_eq!(state.edge_due(), None);
}

#[test]
fn the_mouse_selects_and_opens_marks_and_chooser_rows() {
    use ratatui::layout::Rect;
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    let marks: Vec<PathBuf> = ["/m/a", "/m/b", "/m/c"].map(PathBuf::from).into();
    app.marks = marks.clone().into();
    let areas = ListAreas {
        marks_pane: Rect::new(50, 5, 20, 12),
        marks: vec![(Rect::new(50, 5, 20, 3), 0), (Rect::new(50, 8, 20, 3), 1)],
        choices: vec![(Rect::new(10, 10, 30, 1), 0), (Rect::new(10, 11, 30, 1), 1)],
    };
    let mut mouse = Mouse::default();
    let event = |kind, column, row| MouseEvent {
        kind,
        column,
        row,
        modifiers: M::NONE,
    };
    let click = MouseEventKind::Down(MouseButton::Left);
    let mut focus = Focus::Results;
    // A click on a mark focuses the marked list there; a second opens it.
    let mut act = |focus: &mut Focus, kind, column, row| {
        mouse_lists(&app, focus, event(kind, column, row), &areas, &mut mouse)
    };
    assert_eq!(act(&mut focus, click, 55, 9), ListMouse::Done);
    assert!(matches!(&focus, Focus::Marks { rows, index: 1 } if *rows == marks));
    assert_eq!(
        act(&mut focus, click, 55, 10),
        ListMouse::Open("/m/b".into())
    );
    // The wheel over the pane moves through the marks, stopping at the end.
    for _ in 0..3 {
        assert_eq!(
            act(&mut focus, MouseEventKind::ScrollDown, 60, 16),
            ListMouse::Done
        );
    }
    assert!(matches!(focus, Focus::Marks { index: 2, .. }));
    // The wheel elsewhere and the pane's empty part do nothing.
    assert_eq!(
        act(&mut focus, MouseEventKind::ScrollUp, 5, 5),
        ListMouse::Done
    );
    assert_eq!(act(&mut focus, click, 60, 15), ListMouse::Done);
    assert!(matches!(focus, Focus::Marks { index: 2, .. }));
    // A click outside returns to the results, which take it.
    assert_eq!(act(&mut focus, click, 5, 6), ListMouse::Pass);
    assert!(matches!(focus, Focus::Results));
    // The wheel over the pane from the results focuses the first mark.
    assert_eq!(
        act(&mut focus, MouseEventKind::ScrollDown, 60, 6),
        ListMouse::Done
    );
    assert!(matches!(focus, Focus::Marks { index: 0, .. }));
    // In a chooser: a click selects, a second chooses, the wheel moves, and
    // nothing else reaches the results.
    let mut focus = Focus::Palettes {
        query: String::new(),
        index: 0,
        original: Palette::default(),
    };
    assert_eq!(act(&mut focus, click, 12, 11), ListMouse::Done);
    assert!(matches!(focus, Focus::Palettes { index: 1, .. }));
    assert_eq!(act(&mut focus, click, 12, 11), ListMouse::Enter);
    assert_eq!(
        act(&mut focus, MouseEventKind::ScrollUp, 0, 0),
        ListMouse::Done
    );
    assert!(matches!(focus, Focus::Palettes { index: 0, .. }));
    assert_eq!(act(&mut focus, click, 5, 6), ListMouse::Done);
    assert!(matches!(focus, Focus::Palettes { index: 0, .. }));
    // Go-to suggestions start unselected.
    let mut focus = Focus::Location(crate::location::Location::new(
        &[("one".into(), "/tmp".into())],
        vec![],
        std::path::Path::new("/"),
    ));
    assert_eq!(
        act(&mut focus, MouseEventKind::ScrollUp, 0, 0),
        ListMouse::Done
    );
    assert!(matches!(&focus, Focus::Location(l) if l.selected.is_none()));
    assert_eq!(
        act(&mut focus, MouseEventKind::ScrollDown, 0, 0),
        ListMouse::Done
    );
    assert!(matches!(&focus, Focus::Location(l) if l.selected == Some(0)));
}

#[test]
fn buttons_do_what_their_keys_do() {
    let root = std::env::temp_dir().join(format!("fff-buttons-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("dir")).unwrap();
    std::fs::write(root.join("file"), "").unwrap();
    let mut app = App::browse(root.clone(), 1, crate::Policy::default());
    for (name, directory) in [("dir", true), ("file", false)] {
        app.insert(
            0,
            crate::Candidate {
                path: root.join(name),
                directory,
                depth: 1,
                root: 0,
            },
        );
    }
    app.refresh();
    app.scanning = false;
    let mut discovery = Discovery::new(Default::default());
    assert!(!Button::Back.available(&app));
    assert!(press(&mut app, &mut discovery, Button::Back).is_none());
    app.highlight = Some(root.join("file"));
    assert_eq!(Button::Mark.label(&app), "Mark");
    press(&mut app, &mut discovery, Button::Mark);
    assert_eq!(app.marks, [root.join("file")]);
    assert_eq!(Button::Mark.label(&app), "Unmark");
    // Where marks are not returned (the default mode), Select is Enter on
    // the highlighted entry, whatever is marked.
    app.highlight = Some(root.join("dir"));
    assert!(matches!(
        press(&mut app, &mut discovery, Button::Select),
        Some(Finish::Folder(p)) if p == root.join("dir")
    ));
    assert_eq!(app.marks, [root.join("file")]);
    // With marks, Select returns them like Ctrl+w when picking paths.
    app.output_paths = true;
    app.highlight = Some(root.join("dir"));
    assert!(matches!(
        press(&mut app, &mut discovery, Button::Select),
        Some(Finish::Paths(p)) if p == [root.join("file")]
    ));
    // Without, Normal-mode Enter on the highlighted entry: into a directory.
    app.marks.clear();
    assert!(matches!(
        press(&mut app, &mut discovery, Button::Select),
        Some(Finish::Folder(p)) if p == root.join("dir")
    ));
    assert!(matches!(
        press(&mut app, &mut discovery, Button::Cancel),
        Some(Finish::Cancelled)
    ));
    assert_eq!(Button::Hidden.label(&app), "Show hidden");
    press(&mut app, &mut discovery, Button::Hidden);
    assert!(app.policy.hidden);
    assert_eq!(Button::Hidden.label(&app), "Hide hidden");
    press(&mut app, &mut discovery, Button::Open);
    assert_eq!(app.cwd, root.join("dir"));
    assert!(Button::Back.available(&app));
    press(&mut app, &mut discovery, Button::Up);
    assert_eq!(app.cwd, root);
    press(&mut app, &mut discovery, Button::Back);
    assert_eq!(app.cwd, root.join("dir"));
    // With a window of its own, Select enters a directory and stays.
    app.window = true;
    app.output_paths = false;
    app.refresh();
    app.highlight = Some(root.join("dir"));
    assert!(press(&mut app, &mut discovery, Button::Select).is_none());
    assert_eq!(app.cwd, root.join("dir"));
    app.navigate(root.clone(), false);
    app.window = false;
    // fff seek cannot change directory.
    app.no_dir = true;
    assert!(!Button::Up.available(&app) && !Button::Back.available(&app));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn the_button_bar_is_opt_in_and_can_wait_for_the_mouse() {
    use crate::config::Buttons;
    let mut app = App::browse("/fixture".into(), 1, crate::Policy::default());
    assert!(!app.buttons_shown());
    app.mouse_used = true;
    assert!(!app.buttons_shown());
    app.buttons = Buttons::Mouse;
    app.mouse_used = false;
    assert!(!app.buttons_shown());
    app.mouse_used = true;
    assert!(app.buttons_shown());
    app.buttons = Buttons::Always;
    app.mouse_used = false;
    assert!(app.buttons_shown());
}

#[test]
fn drops_are_existing_absolute_paths_in_shell_or_uri_form() {
    let root = std::env::temp_dir().join(format!("fff-drop-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    for name in ["a b", "c", "ü%", "x (1)"] {
        std::fs::write(root.join(name), "").unwrap();
    }
    let r = root.display();
    let ab = root.join("a b");
    let c = root.join("c");
    assert_eq!(
        dropped_paths(&format!("'{r}/a b' {r}/c "), None),
        Some(vec![ab.clone(), c.clone()])
    );
    assert_eq!(
        dropped_paths(&format!("{r}/a\\ b"), None),
        Some(vec![ab.clone()])
    );
    assert_eq!(
        dropped_paths(
            &format!("file://{r}/a%20b\r\nfile://host{r}/%C3%BC%25\r\n"),
            None
        ),
        Some(vec![ab.clone(), root.join("ü%")])
    );
    // st's drag-n-drop patch: backslash escapes, a space after each path.
    assert_eq!(
        dropped_paths(&format!("{r}/a\\ b {r}/x\\ \\(1\\) "), None),
        Some(vec![ab.clone(), root.join("x (1)")])
    );
    // Text, relative or missing paths, and a mix of both stay text.
    assert_eq!(dropped_paths("hello", None), None);
    assert_eq!(dropped_paths("", None), None);
    assert_eq!(dropped_paths("c", None), None);
    assert_eq!(dropped_paths(&format!("{r}/c {r}/missing"), None), None);
    assert_eq!(dropped_paths(&format!("{r}/c (draft)"), None), None);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn terminal_support_comes_from_replies_before_the_device_attributes() {
    let parse = |replies: &[u8]| Support::parse(replies);
    let support = |keyboard, synchronized| {
        Some(Support {
            keyboard,
            synchronized,
        })
    };
    // st: only the device attributes.
    assert_eq!(parse(b"\x1b[?6c"), support(false, false));
    assert_eq!(
        parse(b"\x1b[?0u\x1b[?2026;2$y\x1b[?62;22c"),
        support(true, true)
    );
    // Mode 2026 not recognized (0) or permanently reset (4).
    assert_eq!(parse(b"\x1b[?2026;0$y\x1b[?62c"), support(false, false));
    assert_eq!(parse(b"\x1b[?2026;4$y\x1b[?62c"), support(false, false));
    // Incomplete: keep reading.
    assert_eq!(parse(b"\x1b[?1u\x1b[?62"), None);
    assert_eq!(parse(b""), None);
}

#[test]
fn the_tty_writes_whole_frames_without_underline_resets() {
    use std::io::Write;
    let path = std::env::temp_dir().join(format!("fff-tty-{}", std::process::id()));
    let file = std::fs::File::create(&path).unwrap();
    let mut tty = Tty::new(file);
    // crossterm writes the reset in pieces.
    for piece in [&b"a\x1b[39m\x1b["[..], b"59", b"m\x1b[0mb"] {
        tty.write_all(piece).unwrap();
    }
    assert_eq!(std::fs::read(&path).unwrap(), b"");
    tty.flush().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"a\x1b[39m\x1b[0mb");
    std::fs::remove_file(path).unwrap();
}
#[test]
fn stopped_messages_keep_the_names_in_a_title() {
    assert_eq!(
        lowercase_first("Moving README.md to the trash"),
        "moving README.md to the trash"
    );
    assert_eq!(lowercase_first(""), "");
}
