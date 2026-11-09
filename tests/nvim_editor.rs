use crossterm::event::{KeyCode as K, KeyEvent as Key, KeyModifiers as M};
use fuzzy_file_finder::editor::{Field, Nvim};
fn engine() -> Option<Nvim> {
    if std::process::Command::new("nvim")
        .arg("--version")
        .env("NVIM_LOG_FILE", "/dev/null")
        .output()
        .is_err()
    {
        eprintln!("Neovim integration skipped: nvim is not installed");
        return None;
    }
    Some(Nvim::start().unwrap())
}
fn chars(editor: &mut Nvim, field: Field, text: &str) -> String {
    let mut result = String::new();
    for c in text.chars() {
        result = editor
            .input(field, Key::new(K::Char(c), M::NONE))
            .unwrap()
            .0;
    }
    result
}
#[test]
fn real_insert_normal_operators_unicode_undo_and_redo() {
    let Some(mut e) = engine() else {
        return;
    };
    assert_eq!(e.activate(Field::Query, "").unwrap().label(), "NORMAL");
    assert_eq!(chars(&mut e, Field::Query, "ihello 世界"), "hello 世界");
    e.input(Field::Query, Key::new(K::Esc, M::NONE)).unwrap();
    chars(&mut e, Field::Query, "0dw");
    assert_eq!(e.activate(Field::Query, "世界").unwrap().label(), "NORMAL");
    assert_eq!(chars(&mut e, Field::Query, "u"), "hello 世界");
    assert_eq!(
        e.input(Field::Query, Key::new(K::Char('r'), M::CONTROL))
            .unwrap()
            .0,
        "世界"
    );
    assert_eq!(chars(&mut e, Field::Query, "ciwtext"), "text");
}
#[test]
fn pending_find_commands_do_not_block_or_lose_the_following_character() {
    let Some(mut e) = engine() else {
        return;
    };
    e.activate(Field::Query, "hello world").unwrap();
    e.input(Field::Query, Key::new(K::Esc, M::NONE)).unwrap();
    chars(&mut e, Field::Query, "0");
    let (text, view) = e
        .input(Field::Query, Key::new(K::Char('f'), M::NONE))
        .unwrap();
    assert_eq!(text, "hello world");
    assert!(view.blocking);
    let (_, view) = e
        .input(Field::Query, Key::new(K::Char('o'), M::NONE))
        .unwrap();
    assert!(!view.blocking);
    assert_eq!(view.column, 4);
    let (_, view) = e
        .input(Field::Query, Key::new(K::Char('r'), M::NONE))
        .unwrap();
    assert!(view.blocking);
    assert_eq!(chars(&mut e, Field::Query, "X"), "hellX world");
}
#[test]
fn isolated_buffers_visual_yank_paste_and_multiline_edits() {
    let Some(mut e) = engine() else {
        return;
    };
    e.activate(Field::Query, "abc def").unwrap();
    e.input(Field::Query, Key::new(K::Esc, M::NONE)).unwrap();
    chars(&mut e, Field::Query, "0vll");
    let view = e.activate(Field::Query, "abc def").unwrap();
    assert_eq!(view.label(), "VISUAL");
    assert_eq!(chars(&mut e, Field::Query, "yp"), "aabcbc def");
    e.activate(Field::Exclusions, "build\n*.log").unwrap();
    e.input(Field::Exclusions, Key::new(K::Esc, M::NONE))
        .unwrap();
    assert_eq!(chars(&mut e, Field::Exclusions, "ggdd"), "*.log");
    assert_eq!(chars(&mut e, Field::Exclusions, "u"), "build\n*.log");
    assert_eq!(
        e.activate(Field::Query, "aabcbc def").unwrap().label(),
        "NORMAL"
    );
    e.reset(Field::Query);
    assert_eq!(e.activate(Field::Query, "").unwrap().label(), "NORMAL");
    // A reset puts the cursor on the retained query's end, in Normal mode.
    e.reset(Field::Query);
    let view = e.activate(Field::Query, "kept").unwrap();
    assert_eq!((view.label(), view.column), ("NORMAL", 3));
    e.reset(Field::Query);
    assert_eq!(e.activate(Field::Query, "").unwrap().label(), "NORMAL");
    assert_eq!(chars(&mut e, Field::Query, "inew"), "new");
    e.reset(Field::Query);
    e.activate(Field::Query, "").unwrap();
    assert_eq!(
        e.paste(Field::Query, "<literal> café").unwrap().0,
        "<literal> café"
    );
}
#[test]
fn command_line_substitution_and_buffer_replacement() {
    let Some(mut e) = engine() else {
        return;
    };
    e.activate(Field::Location, "old/path").unwrap();
    e.input(Field::Location, Key::new(K::Esc, M::NONE)).unwrap();
    chars(&mut e, Field::Location, ":s/old/new/");
    let view = e.activate(Field::Location, "old/path").unwrap();
    assert_eq!(view.label(), "COMMAND");
    assert!(view.command.contains("s/old/new/"));
    assert_eq!(
        e.input(Field::Location, Key::new(K::Enter, M::NONE))
            .unwrap()
            .0,
        "new/path"
    );
    assert_eq!(
        e.activate(Field::Workspaces, "Notes").unwrap().label(),
        "NORMAL"
    );
    assert_eq!(
        e.activate(Field::Location, "external reset")
            .unwrap()
            .label(),
        "NORMAL"
    );
}
#[test]
fn unavailable_backend_returns_an_error_instead_of_hanging() {
    assert!(Nvim::start_program("/definitely/not/a/neovim").is_err());
}

#[test]
fn a_missing_neovim_is_a_clear_startup_error() {
    use fuzzy_file_finder::ui::start_editor;
    for program in ["/definitely/not/a/neovim", "true"] {
        let error = start_editor(program).err().unwrap().to_string();
        assert!(error.starts_with("fff needs Neovim"), "{error}");
    }
}
#[test]
fn default_backend_starts_neovim_when_installed() {
    if engine().is_none() {
        return;
    }
    let mut e = fuzzy_file_finder::ui::start_editor("nvim").unwrap();
    assert_eq!(e.activate(Field::Query, "").unwrap().label(), "NORMAL");
    assert_eq!(chars(&mut e, Field::Query, "iabc"), "abc");
}

#[test]
fn field_switches_restore_insert_cursor_at_end_and_in_middle() {
    let Some(mut e) = engine() else {
        return;
    };
    e.activate(Field::Query, "ab").unwrap();
    chars(&mut e, Field::Query, "a");
    e.input(Field::Query, Key::new(K::Left, M::NONE)).unwrap();
    e.activate(Field::Location, "").unwrap();
    e.activate(Field::Query, "ab").unwrap();
    assert_eq!(chars(&mut e, Field::Query, "X"), "aXb");
    e.input(Field::Query, Key::new(K::End, M::NONE)).unwrap();
    e.activate(Field::Workspaces, "Notes").unwrap();
    e.activate(Field::Query, "aXb").unwrap();
    assert_eq!(chars(&mut e, Field::Query, "Y"), "aXbY");
}

#[test]
fn every_text_field_initially_starts_normal() {
    let Some(mut e) = engine() else {
        return;
    };
    for field in [
        Field::Query,
        Field::Operation,
        Field::FileName,
        Field::Extraction,
        Field::Location,
        Field::Exclusions,
        Field::Workspaces,
        Field::Palettes,
        Field::WorkspaceName,
        Field::WorkspacePath,
        Field::WorkspaceIcons,
    ] {
        let view = e.activate(field, "sample").unwrap();
        assert!(view.normal(), "{field:?}: {view:?}");
        assert_eq!(chars(&mut e, field, "A!"), "sample!");
        e.reset(field);
        assert!(e.activate(field, "").unwrap().normal(), "reset {field:?}");
    }
}

#[test]
fn navigation_keeps_insert_normal_replace_and_visual_modes() {
    let Some(mut e) = engine() else {
        return;
    };
    for (input, mode) in [
        ("", "n"),
        ("a", "i"),
        ("R", "R"),
        ("gR", "Rv"),
        ("v", "v"),
        ("V", "V"),
        ("gh", "s"),
        ("gH", "S"),
    ] {
        e.reset(Field::Query);
        e.activate(Field::Query, "sample").unwrap();
        chars(&mut e, Field::Query, input);
        e.reset_preserving_mode(Field::Query);
        assert_eq!(e.activate(Field::Query, "").unwrap().mode, mode);
        e.reset_preserving_mode(Field::Query);
        assert_eq!(e.activate(Field::Query, "retained").unwrap().mode, mode);
    }
    e.reset(Field::Query);
    e.activate(Field::Query, "block").unwrap();
    e.input(Field::Query, Key::new(K::Char('v'), M::CONTROL))
        .unwrap();
    e.reset_preserving_mode(Field::Query);
    assert_eq!(e.activate(Field::Query, "").unwrap().mode, "\u{16}");
    e.reset(Field::Location);
    e.activate(Field::Location, "new/path").unwrap();
    chars(&mut e, Field::Location, "A");
    // Going to a typed folder leaves the prompt's typing behind.
    e.reset_preserving_mode(Field::Query);
    assert_eq!(e.activate(Field::Query, "").unwrap().label(), "NORMAL");
}

#[test]
fn closing_fields_preserves_final_text_and_keeps_engine_alive() {
    let Some(mut e) = engine() else {
        return;
    };
    for command in [":q!", ":wq", ":x", ":qa!", ":bd!", ":bw!", "ZQ", "ZZ"] {
        e.reset(Field::Exclusions);
        e.activate(Field::Exclusions, "build").unwrap();
        chars(&mut e, Field::Exclusions, "o*.log");
        e.input(Field::Exclusions, Key::new(K::Esc, M::NONE))
            .unwrap();
        let (text, closed) = if command.starts_with(':') {
            chars(&mut e, Field::Exclusions, command);
            e.input(Field::Exclusions, Key::new(K::Enter, M::NONE))
                .unwrap()
        } else {
            chars(&mut e, Field::Exclusions, &command[..1]);
            e.input(
                Field::Exclusions,
                Key::new(K::Char(command.chars().nth(1).unwrap()), M::NONE),
            )
            .unwrap()
        };
        assert!(closed.closed, "{command}: {closed:?}");
        assert_eq!(text, "build\n*.log", "{command}");
        // Closing the field leaves a usable Neovim backend.
        let state = &e.activate(Field::Query, "kept").unwrap();
        assert!(state.normal(), "{command}: {state:?}");
        e.activate(Field::Exclusions, &text).unwrap();
        assert_eq!(
            chars(&mut e, Field::Exclusions, "A!"),
            "build\n*.log!",
            "{command}"
        );
        e.input(Field::Exclusions, Key::new(K::Esc, M::NONE))
            .unwrap();
    }
}

#[test]
fn closing_a_buffer_larger_than_a_pipe_keeps_its_text() {
    let Some(mut e) = engine() else {
        return;
    };
    // Rename lists every row: far more than a pipe holds when Neovim exits.
    let text = (0..20_000)
        .map(|i| format!("folder{}/file{i}.txt", i / 200))
        .collect::<Vec<_>>()
        .join("\n");
    for command in ["ZZ", "ZQ"] {
        e.reset(Field::Operation);
        e.activate(Field::Operation, &text).unwrap();
        chars(&mut e, Field::Operation, "Ax");
        e.input(Field::Operation, Key::new(K::Esc, M::NONE))
            .unwrap();
        let (closed, view) = e
            .input(Field::Operation, Key::new(K::Char('Z'), M::NONE))
            .and_then(|_| {
                let quit = command.chars().nth(1).unwrap();
                e.input(Field::Operation, Key::new(K::Char(quit), M::NONE))
            })
            .unwrap();
        assert!(view.closed, "{command}: {view:?}");
        assert_eq!(view.saved, command == "ZZ", "{command}");
        assert_eq!(closed.lines().count(), 20_000, "{command}");
        assert!(closed.ends_with("file19999.txtx"), "{command}");
        assert!(e.activate(Field::Query, "kept").unwrap().normal());
    }
}

#[test]
fn close_distinguishes_written_contents_from_unsaved_edits() {
    let Some(mut e) = engine() else {
        return;
    };
    for field in [
        Field::Exclusions,
        Field::WorkspaceName,
        Field::WorkspacePath,
    ] {
        e.activate(field, "original").unwrap();
        chars(&mut e, field, "ccsaved");
        e.input(field, Key::new(K::Esc, M::NONE)).unwrap();
        chars(&mut e, field, ":w");
        let (_, view) = e.input(field, Key::new(K::Enter, M::NONE)).unwrap();
        assert!(view.written);
        assert!(!view.closed);
        chars(&mut e, field, "A-unsaved");
        e.input(field, Key::new(K::Esc, M::NONE)).unwrap();
        chars(&mut e, field, ":q!");
        let (text, view) = e.input(field, Key::new(K::Enter, M::NONE)).unwrap();
        assert!(view.closed && view.saved);
        assert!(!view.written);
        assert_eq!(text, "saved");
        e.activate(field, "original").unwrap();
        chars(&mut e, field, "ccdiscarded");
        e.input(field, Key::new(K::Esc, M::NONE)).unwrap();
        chars(&mut e, field, ":q!");
        let (_, view) = e.input(field, Key::new(K::Enter, M::NONE)).unwrap();
        assert!(view.closed && !view.saved);
    }
}

#[test]
fn native_quit_refuses_unwritten_changes() {
    let Some(mut e) = engine() else {
        return;
    };
    e.activate(Field::Exclusions, "build").unwrap();
    chars(&mut e, Field::Exclusions, "A-changed");
    e.input(Field::Exclusions, Key::new(K::Esc, M::NONE))
        .unwrap();
    chars(&mut e, Field::Exclusions, ":q");
    let (text, view) = e
        .input(Field::Exclusions, Key::new(K::Enter, M::NONE))
        .unwrap();
    assert!(!view.closed);
    assert_eq!(text, "build-changed");
    e.input(Field::Exclusions, Key::new(K::Enter, M::NONE))
        .unwrap();
    chars(&mut e, Field::Exclusions, ":q!");
    let (_, view) = e
        .input(Field::Exclusions, Key::new(K::Enter, M::NONE))
        .unwrap();
    assert!(view.closed && !view.saved);
}

#[test]
fn naming_buffer_insert_reset_is_fresh_on_every_activation() {
    let Some(mut e) = engine() else {
        return;
    };
    for name in ["file", "directory"] {
        e.reset_insert(Field::FileName);
        assert_eq!(e.activate(Field::FileName, "").unwrap().label(), "INSERT");
        assert_eq!(chars(&mut e, Field::FileName, name), name);
        e.input(Field::FileName, Key::new(K::Esc, M::NONE)).unwrap();
        assert!(e.activate(Field::FileName, name).unwrap().normal());
    }
    // Multiline rename buffers retain their Normal start.
    e.reset(Field::Operation);
    assert!(e.activate(Field::Operation, "rename").unwrap().normal());
}

#[test]
fn single_line_open_keys_enter_insert_without_adding_rows() {
    let Some(mut e) = engine() else {
        return;
    };
    for field in [Field::Query, Field::FileName, Field::Extraction] {
        for key in ['o', 'O'] {
            e.reset(field);
            e.activate(field, "path").unwrap();
            let (text, view) = e.input(field, Key::new(K::Char(key), M::NONE)).unwrap();
            assert_eq!(text, "path");
            assert_eq!(view.label(), "INSERT");
            assert_eq!(view.row, 0);
            assert_eq!(
                e.input(field, Key::new(K::Enter, M::NONE)).unwrap().0,
                "path"
            );
            e.input(field, Key::new(K::Esc, M::NONE)).unwrap();
            let (text, view) = e.input(field, Key::new(K::Enter, M::NONE)).unwrap();
            assert_eq!(text, "path");
            assert!(view.normal());
            assert_eq!(view.row, 0);
        }
        assert!(!e.paste(field, "multi\nline").unwrap().0.contains('\n'));
    }
    // Genuine multiline editors retain native open-line and Enter behavior.
    for field in [Field::Exclusions, Field::Operation] {
        e.reset(field);
        e.activate(field, "first").unwrap();
        assert_eq!(chars(&mut e, field, "osecond"), "first\nsecond");
        assert_eq!(
            e.input(field, Key::new(K::Enter, M::NONE)).unwrap().0,
            "first\nsecond\n"
        );
        e.input(field, Key::new(K::Esc, M::NONE)).unwrap();
    }
}

#[test]
fn invalid_utf8_lines_stay_visible_instead_of_becoming_empty() {
    let Some(mut e) = engine() else {
        return;
    };
    e.reset(Field::Operation);
    e.activate(Field::Operation, "1 a").unwrap();
    chars(&mut e, Field::Operation, ":call setline(1, \"1 a\\x80b\")");
    let (text, _) = e
        .input(Field::Operation, Key::new(K::Enter, M::NONE))
        .unwrap();
    assert_eq!(text, "1 a\u{fffd}b");
}

#[test]
fn switching_queries_carries_the_mode_and_keeps_each_cursor() {
    let Some(mut e) = engine() else {
        return;
    };
    // Typing in the names query: the contents query, new, is typed into.
    e.activate(Field::Query, "").unwrap();
    chars(&mut e, Field::Query, "aname");
    e.carry_mode(Field::Contents);
    assert_eq!(e.activate(Field::Contents, "").unwrap().mode, "i");
    assert_eq!(chars(&mut e, Field::Contents, "word"), "word");
    // Back at the names query, still typing, at its own cursor.
    e.input(Field::Contents, Key::new(K::Left, M::NONE))
        .unwrap();
    e.carry_mode(Field::Query);
    assert_eq!(e.activate(Field::Query, "name").unwrap().mode, "i");
    assert_eq!(chars(&mut e, Field::Query, "!"), "name!");
    // Normal mode carries too, over the Insert mode the contents query was
    // left in, and its cursor stays where it was.
    e.input(Field::Query, Key::new(K::Esc, M::NONE)).unwrap();
    e.carry_mode(Field::Contents);
    let view = e.activate(Field::Contents, "word").unwrap();
    assert!(view.normal(), "{view:?}");
    assert_eq!(chars(&mut e, Field::Contents, "x"), "wor");
    // A visual selection starts again in the other query.
    chars(&mut e, Field::Contents, "v");
    e.carry_mode(Field::Query);
    assert_eq!(e.activate(Field::Query, "name!").unwrap().mode, "v");
}
