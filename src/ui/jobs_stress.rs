//! Stress tests for file operations running side by side: random
//! operations on generated trees start, collide, stop and are undone, on real
//! threads, and no data may be lost, half written or left in a staging
//! folder. Each test runs `FFF_STRESS_ROUNDS` rounds (1 by default) from
//! `FFF_STRESS_SEED` (else the clock); a failure names its seed. They write
//! and read gigabytes, so `cargo test` skips them: run them with `--ignored`
//! after changing operations, jobs, the trash or undo.
use super::*;
use std::{collections::BTreeMap, fs, path::Path};

/// xorshift64*: reproducible from its seed, and enough for picking.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
    fn percent(&mut self, p: u64) -> bool {
        self.next() % 100 < p
    }
}
fn rounds() -> u64 {
    std::env::var("FFF_STRESS_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(1)
}
fn first_seed() -> u64 {
    std::env::var("FFF_STRESS_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() as u64
        })
        | 1
}

/// A round's folder: `w` holds the trees operated on, `data` the trash, and
/// `here` is where the session browses, out of the way.
struct World {
    root: PathBuf,
    w: PathBuf,
    /// Every file's contents as created; each is unique.
    originals: Vec<Vec<u8>>,
    /// What else a whole file may hold: the archive's members, extracted.
    extracted: Vec<Vec<u8>>,
}
impl World {
    fn new(name: &str, rng: &mut Rng) -> Self {
        let root = std::env::temp_dir().join(format!("fff-stress-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let w = root.join("w");
        for dir in ["w/d0", "w/d1", "w/d2/inner", "here", "data"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        let mut world = Self {
            root: root.canonicalize().unwrap(),
            w: w.canonicalize().unwrap(),
            originals: vec![],
            extracted: vec![b"tar one".to_vec(), b"tar two".to_vec()],
        };
        // Trees of many files, some large, so that operations overlap.
        for t in 0..3 {
            let tree = world.w.join(format!("t{t}"));
            let files = 1500 + rng.below(4000);
            for i in 0..files {
                let dir = tree.join(format!("s{}", i % 7));
                fs::create_dir_all(&dir).unwrap();
                let size = if rng.percent(2) {
                    1 << 20
                } else {
                    rng.below(4096)
                };
                world.file(&dir.join(format!("f{i}")), size);
            }
        }
        for i in 0..4 {
            let path = world.w.join(format!("loose{i}"));
            world.file(&path, rng.below(10_000));
        }
        tar(
            &world.w.join("bundle.tar"),
            &[("x/one", b"tar one"), ("x/two", b"tar two")],
        );
        world
            .originals
            .push(fs::read(world.w.join("bundle.tar")).unwrap());
        world
    }
    fn file(&mut self, path: &Path, size: usize) {
        let mut data = format!("{}:", self.originals.len()).into_bytes();
        data.resize(data.len() + size, b'.');
        fs::write(path, &data).unwrap();
        self.originals.push(data);
    }
    fn app(&self) -> App {
        let mut app = App::browse(self.root.join("here"), 1, crate::Policy::default());
        app.trash = Some(crate::trash::Trash::new(
            &self.root.join("data"),
            30,
            u64::MAX,
        ));
        app.refresh_trash();
        app
    }
}
impl Drop for World {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}
/// A tar archive of regular files.
fn tar(path: &Path, entries: &[(&str, &[u8])]) {
    let mut bytes = vec![];
    for (name, data) in entries {
        let mut h = [0u8; 512];
        h[..name.len()].copy_from_slice(name.as_bytes());
        h[100..108].copy_from_slice(b"0000644\0");
        h[108..116].copy_from_slice(b"0000000\0");
        h[116..124].copy_from_slice(b"0000000\0");
        h[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        h[136..148].copy_from_slice(b"00000000000\0");
        h[148..156].fill(b' ');
        h[156] = b'0';
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        let sum = h.iter().map(|b| *b as u32).sum::<u32>();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        bytes.extend(h);
        bytes.extend_from_slice(data);
        while bytes.len() % 512 != 0 {
            bytes.push(0);
        }
    }
    bytes.extend([0; 1024]);
    fs::write(path, bytes).unwrap();
}
/// Every entry below `dir`, relative, with a file's contents.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Option<Vec<u8>>> {
    let mut all = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(at) = stack.pop() {
        for entry in fs::read_dir(&at).unwrap().flatten() {
            let path = entry.path();
            let kind = entry.file_type().unwrap();
            let relative = path.strip_prefix(dir).unwrap().to_path_buf();
            if kind.is_dir() {
                stack.push(path);
                all.insert(relative, None);
            } else {
                all.insert(relative, Some(fs::read(&path).unwrap_or_default()));
            }
        }
    }
    all
}
/// The entries below `dir` that are, or are in, a staging folder.
fn staged(dir: &Path) -> Vec<PathBuf> {
    snapshot(dir)
        .into_keys()
        .filter(|p| crate::operations::staged(p))
        .collect()
}
/// Ends every job, as the session loop would.
fn settle(app: &mut App, discovery: &mut Discovery) {
    let deadline = std::time::Instant::now() + Duration::from_secs(120);
    while !app.jobs.is_empty() {
        assert!(
            std::time::Instant::now() < deadline,
            "operations never ended"
        );
        poll_jobs(app, discovery);
        std::thread::sleep(Duration::from_millis(2));
    }
}
/// Entries below `w` to operate on: the trees, loose files, and some of
/// what is inside the trees, as they are now.
fn candidates(w: &Path, rng: &mut Rng) -> Vec<PathBuf> {
    let mut found = vec![];
    let mut stack = vec![(w.to_path_buf(), 0)];
    while let Some((at, depth)) = stack.pop() {
        let Ok(read) = fs::read_dir(&at) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if crate::operations::staged(&path) {
                continue;
            }
            // Folders below the top are many; take a few of them.
            if depth == 0 || rng.percent(3) {
                found.push(path.clone());
            }
            if depth < 2 && path.is_dir() {
                stack.push((path, depth + 1));
            }
        }
    }
    found.sort();
    found
}
/// A random operation on what is there now: copies, moves, trashing,
/// renames, new entries and extractions. Some fail (moving a folder into
/// itself, a name that is taken); a failure must change nothing.
fn request(w: &Path, rng: &mut Rng, n: usize) -> Option<crate::operations::Request> {
    use crate::operations::Request;
    let all = candidates(w, rng);
    let top: Vec<PathBuf> = all
        .iter()
        .filter(|p| p.parent() == Some(w))
        .cloned()
        .collect();
    let folders: Vec<PathBuf> = all.iter().filter(|p| p.is_dir()).cloned().collect();
    let pick = |rng: &mut Rng, from: &[PathBuf]| from.get(rng.below(from.len())).cloned();
    // Whole trees mostly, so that long operations run side by side.
    let entry = |rng: &mut Rng| {
        if rng.percent(60) {
            pick(rng, &top)
        } else {
            pick(rng, &all)
        }
    };
    let sources = |rng: &mut Rng| {
        let mut chosen: Vec<PathBuf> = (0..1 + rng.below(2)).filter_map(|_| entry(rng)).collect();
        chosen.sort();
        chosen.dedup();
        // Overlapping selections are refused anyway; keep them apart.
        let kept: Vec<PathBuf> = chosen
            .iter()
            .filter(|p| !chosen.iter().any(|q| q != *p && p.starts_with(q)))
            .cloned()
            .collect();
        kept
    };
    Some(match rng.below(7) {
        0 | 1 => Request::Copy {
            sources: sources(rng),
            directory: pick(rng, &folders)?,
            replace: vec![],
            confirmed: vec![],
            into: None,
        },
        2 => Request::Move {
            sources: sources(rng),
            directory: pick(rng, &folders)?,
            replace: vec![],
            confirmed: vec![],
            into: None,
        },
        3 => {
            let sources = sources(rng);
            Request::Delete {
                trash: vec![true; sources.len()],
                sources,
                confirmed: vec![],
            }
        }
        4 => {
            let source = entry(rng)?;
            let mut name = source.file_name()?.to_os_string();
            name.push(format!("-r{n}"));
            Request::Rename {
                targets: vec![source.with_file_name(name)],
                sources: vec![source],
            }
        }
        5 => Request::Create {
            path: pick(rng, &folders)?.join(format!("new{n}")),
            directory: rng.percent(50),
        },
        _ => Request::Extract {
            archives: vec![w.join("bundle.tar")],
            destination: Some(pick(rng, &folders)?.join(format!("out{n}"))),
        },
    })
}
/// Slows every step of every operation down while it lives, so that
/// operations overlap and stop partway even on tmpfs.
struct Slow;
impl Slow {
    fn new() -> Self {
        crate::operations::SLOW_MICROS.store(20, std::sync::atomic::Ordering::Relaxed);
        Slow
    }
}
impl Drop for Slow {
    fn drop(&mut self) {
        crate::operations::SLOW_MICROS.store(0, std::sync::atomic::Ordering::Relaxed);
    }
}
/// Says which seed failed, then fails.
fn check(ok: bool, seed: u64, what: impl FnOnce() -> String) {
    assert!(ok, "seed {seed} (FFF_STRESS_SEED={seed}): {}", what());
}

#[test]
#[ignore = "stress test: run with --ignored after changing operations"]
fn a_stopped_operation_puts_everything_back() {
    let _slow = Slow::new();
    let mut seed = first_seed();
    for _ in 0..rounds() * 6 {
        let mut rng = Rng(seed);
        let world = World::new("stop", &mut rng);
        let mut app = world.app();
        let mut discovery = Discovery::new(Default::default());
        let mut focus = Focus::Results;
        let before = snapshot(&world.w);
        let Some(request) = request(&world.w, &mut rng, 0) else {
            continue;
        };
        let described = format!("{request:?}");
        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
        std::thread::sleep(Duration::from_millis(rng.below(300) as u64));
        // Still running: x (or u) stops it and undoes what it did. Ended:
        // u undoes it.
        if app.jobs.is_empty() {
            if !app.undo.is_empty() {
                undo(&mut app, &mut discovery);
            }
        } else {
            stop_job(&mut app, 0, rng.percent(50));
        }
        settle(&mut app, &mut discovery);
        check(app.undo.is_empty(), seed, || {
            format!("{described}: left on the undo list")
        });
        let after = snapshot(&world.w);
        check(after == before, seed, || {
            let changed: Vec<_> = before
                .iter()
                .filter(|(p, c)| after.get(*p) != Some(c))
                .map(|(p, _)| p)
                .chain(after.keys().filter(|p| !before.contains_key(*p)))
                .take(5)
                .collect();
            format!(
                "{described} then stopped: not put back ({}): {changed:?}",
                app.status
            )
        });
        check(staged(&world.root).is_empty(), seed, || {
            format!("{described}: left {:?}", staged(&world.root))
        });
        seed = Rng(seed).next() | 1;
    }
}

#[test]
#[ignore = "stress test: run with --ignored after changing operations"]
fn operations_side_by_side_lose_nothing_and_undo_in_order() {
    let _slow = Slow::new();
    let mut seed = first_seed();
    for _ in 0..rounds() {
        let mut rng = Rng(seed);
        let world = World::new("side", &mut rng);
        let mut app = world.app();
        let mut discovery = Discovery::new(Default::default());
        let mut focus = Focus::Results;
        let before = snapshot(&world.w);
        let mut log = vec![];
        let (mut most, mut refused, mut stopped) = (0, 0, 0);
        for n in 1..=30 {
            most = most.max(app.jobs.len());
            match rng.below(100) {
                0..50 => {
                    if let Some(request) = request(&world.w, &mut rng, n) {
                        log.push(format!("{n}: {request:?}"));
                        app.status.clear();
                        perform_operation(&mut app, &mut discovery, &mut focus, request, false);
                        log.push(format!("  -> {} ({} running)", app.status, app.jobs.len()));
                        refused += app.status.ends_with("wait for it to finish") as usize;
                    }
                }
                50..65 if !app.jobs.is_empty() => {
                    let index = rng.below(app.jobs.len());
                    let remark = rng.percent(50);
                    log.push(format!("{n}: stop {} ({remark})", app.jobs[index].title));
                    stop_job(&mut app, index, remark);
                    stopped += 1;
                }
                65..75 => {
                    log.push(format!("{n}: u"));
                    app.status.clear();
                    undo(&mut app, &mut discovery);
                    log.push(format!("  -> {}", app.status));
                }
                _ => {
                    std::thread::sleep(Duration::from_millis(rng.below(30) as u64));
                    poll_jobs(&mut app, &mut discovery);
                }
            }
        }
        settle(&mut app, &mut discovery);
        eprintln!("seed {seed}: at most {most} running, {refused} refused, {stopped} stopped");
        let log = log.join("\n");
        check(staged(&world.root).is_empty(), seed, || {
            format!("left {:?}\n{log}", staged(&world.root))
        });
        // Nothing lost or half written: every file is whole, and every
        // original is somewhere, in place or in the trash.
        let everywhere = snapshot(&world.root);
        let contents: std::collections::HashSet<&Vec<u8>> = everywhere.values().flatten().collect();
        for (path, data) in &everywhere {
            // New files (`n`) are empty.
            if let Some(data) = data
                && !path.starts_with("data")
                && !data.is_empty()
            {
                check(
                    world.originals.contains(data) || world.extracted.contains(data),
                    seed,
                    || format!("{} is not a whole original\n{log}", path.display()),
                );
            }
        }
        for (i, original) in world.originals.iter().enumerate() {
            check(contents.contains(original), seed, || {
                format!("file {i} was lost\n{log}")
            });
        }
        // Undone last to first, everything comes back as it was.
        let mut undone = vec![];
        while !app.undo.is_empty() {
            app.status.clear();
            undo(&mut app, &mut discovery);
            settle(&mut app, &mut discovery);
            undone.push(app.status.clone());
            check(app.status.starts_with("Undid"), seed, || {
                format!("undo failed: {}\n{log}\nundone: {undone:#?}", app.status)
            });
        }
        let after = snapshot(&world.w);
        check(after == before, seed, || {
            let changed: Vec<_> = before
                .keys()
                .filter(|p| after.get(*p) != before.get(*p))
                .chain(after.keys().filter(|p| !before.contains_key(*p)))
                .take(5)
                .collect();
            format!("not as before after undoing: {changed:?}\n{log}\nundone: {undone:#?}")
        });
        seed = Rng(seed).next() | 1;
    }
}
