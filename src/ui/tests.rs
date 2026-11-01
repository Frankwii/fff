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
    app.marks = vec![other.clone(), b.clone()];
    let mut focus = Focus::Marks {
        rows: vec![a.clone(), b.clone(), c.clone()],
        index: 0,
    };
    let mut editor = None;
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
