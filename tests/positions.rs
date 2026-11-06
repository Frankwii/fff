//! Remembered cursor positions while navigating.
use fuzzy_file_finder::{App, Candidate, Policy};
use std::path::PathBuf;

fn listed(app: &mut App, names: &[&str]) {
    for name in names {
        app.insert(
            app.generation,
            Candidate {
                path: app.cwd.join(name),
                directory: true,
                depth: 1,
                root: 0,
                order: 0,
            },
        );
    }
    app.refresh();
}

#[test]
fn going_up_highlights_the_way_back_and_revisits_restore_the_cursor() {
    let root = PathBuf::from("/r");
    let mut app = App::browse(root.join("a/b/c"), 1, Policy::default());
    listed(&mut app, &["x", "y"]);
    app.step(true);
    assert_eq!(app.highlight, Some(root.join("a/b/c/y")));
    // Several levels up: the child on the way back is wanted, and applied as
    // soon as its row streams in, even after other rows arrived first.
    app.navigate(root.clone(), false);
    listed(&mut app, &["0", "z"]);
    assert_eq!(app.highlight, Some(root.join("0")));
    listed(&mut app, &["a"]);
    assert_eq!(app.highlight, Some(root.join("a")));
    // Revisiting restores the remembered entry; moving first cancels it.
    app.navigate(root.join("a/b/c"), false);
    listed(&mut app, &["x", "y"]);
    assert_eq!(app.highlight, Some(root.join("a/b/c/y")));
    app.navigate(root.clone(), false);
    app.step(true);
    listed(&mut app, &["a"]);
    assert_ne!(app.highlight, Some(root.join("a")));
}
