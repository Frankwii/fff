//! How long listing a large tree takes headless, until the rows are settled:
//! the scan and the interface thread's work on what it delivers, without
//! drawing. Not run by default; for measurements:
//!
//! cargo test --offline --release --test scan_timing -- --ignored --nocapture
//! FFF_SCAN_ROOT=/some/tree cargo test ... (default /usr)
use fuzzy_file_finder::{App, Policy, config::CacheConfig, discovery::Discovery, program::Program};
use std::{path::PathBuf, time::Instant};

fn list(program: Program, query: &str) {
    let root = PathBuf::from(std::env::var("FFF_SCAN_ROOT").unwrap_or_else(|_| "/usr".into()));
    let mut app = App::browse(root, 64, Policy::default());
    app.set_program(program);
    app.query = query.into();
    let (waker, _woken) = std::sync::mpsc::sync_channel(1);
    app.set_waker(waker);
    let mut discovery = Discovery::new(CacheConfig {
        enabled: false,
        ..CacheConfig::default()
    });
    let start = Instant::now();
    discovery.start(&mut app, true);
    while app.scanning || !app.settled() || discovery.busy(&app) {
        discovery.drain(&mut app);
        app.poll_ranking();
        std::thread::yield_now();
    }
    let listed = start.elapsed();
    let all = Instant::now();
    let first = app.rows.first().map(|c| c.path.clone());
    let count = app.rows.len();
    println!(
        "{program:?} {query:?}: {count} rows in {:.3} s, then every row in order in {:.3} s (first {first:?})",
        listed.as_secs_f64(),
        all.elapsed().as_secs_f64(),
    );
    if program.short_labels() {
        // What labelling a screen of rows adds to a frame: the first time,
        // when their groups are sorted, and again.
        for time in ["first", "again"] {
            let start = Instant::now();
            for c in app.rows.iter().take(SCREEN) {
                std::hint::black_box(app.label(&c.path));
            }
            println!(
                "  labels of {SCREEN} rows, {time}: {:.3} ms",
                start.elapsed().as_secs_f64() * 1e3
            );
        }
    }
}

/// Rows a tall terminal shows.
const SCREEN: usize = 100;

#[test]
#[ignore]
fn listing_a_large_tree() {
    list(Program::Roam, "");
    list(Program::Pick, "");
    list(Program::Pick, "lib");
    // The largest groups of a name in /usr.
    list(Program::Pick, "__init__");
}
