use crossterm::event::{KeyCode as K, KeyEvent as Key, KeyModifiers as M};
use picker::editor::{Field, Nvim};
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
    assert_eq!(e.activate(Field::Query, "").unwrap().label(), "INSERT");
    assert_eq!(chars(&mut e, Field::Query, "hello 世界"), "hello 世界");
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
    assert_eq!(e.activate(Field::Query, "").unwrap().label(), "INSERT");
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
        e.activate(Field::Bookmarks, "Notes").unwrap().label(),
        "INSERT"
    );
    assert_eq!(
        e.activate(Field::Location, "external reset")
            .unwrap()
            .label(),
        "INSERT"
    );
}
#[test]
fn unavailable_backend_returns_an_error_instead_of_hanging() {
    assert!(Nvim::start_program("/definitely/not/a/neovim").is_err());
}

#[test]
fn field_switches_restore_insert_cursor_at_end_and_in_middle() {
    let Some(mut e) = engine() else {
        return;
    };
    e.activate(Field::Query, "ab").unwrap();
    e.input(Field::Query, Key::new(K::Left, M::NONE)).unwrap();
    e.activate(Field::Location, "").unwrap();
    e.activate(Field::Query, "ab").unwrap();
    assert_eq!(chars(&mut e, Field::Query, "X"), "aXb");
    e.input(Field::Query, Key::new(K::End, M::NONE)).unwrap();
    e.activate(Field::Bookmarks, "Notes").unwrap();
    e.activate(Field::Query, "aXb").unwrap();
    assert_eq!(chars(&mut e, Field::Query, "Y"), "aXbY");
}
