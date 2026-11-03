use fuzzy_file_finder::config::{Palette, Settings, default_path};
use ratatui::style::Color;
use std::path::Path;
fn parse(text: &str) -> Result<Settings, String> {
    Settings::parse(
        text,
        Path::new("/config/fff"),
        Some(Path::new("/home/user")),
        None,
    )
}
#[test]
fn defaults_builtin_palettes_custom_inheritance_and_cli_override() {
    let defaults = parse("").unwrap();
    assert_eq!(defaults.palette, Palette::default());
    assert!(defaults.cache.enabled);
    for name in ["midnight", "light", "forest"] {
        assert!(Palette::named(name).is_some());
    }
    let text = "[ui]\npalette='paper'\n[palettes.paper]\nextends='light'\naccent='#123aBC'\ncurrent_directory='#224466'\nbackground='default'\n";
    let s = parse(text).unwrap();
    assert_eq!(s.palette.accent, Color::Rgb(0x12, 0x3a, 0xbc));
    assert_eq!(s.palette.background, Color::Reset);
    assert_eq!(s.palette.text, Palette::named("light").unwrap().text);
    assert_eq!(s.palette_name, "paper");
    // The chooser lists every built-in in documented order, then custom palettes.
    let names: Vec<_> = s.palettes.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names[..Palette::names().len()], *Palette::names());
    assert_eq!(names.last(), Some(&"paper"));
    assert_eq!(s.palettes.last().unwrap().1, s.palette);
    let s = Settings::parse(text, Path::new("/cfg"), None, Some("forest")).unwrap();
    assert_eq!(s.palette, Palette::named("forest").unwrap());
    assert_eq!(s.palette_name, "forest");
}
#[test]
fn bookmarks_preserve_order_expand_home_and_resolve_relative_to_config() {
    let s=parse("[[bookmarks]]\nname='Home'\npath='~'\n[[bookmarks]]\nname='Papers'\npath='~/My Papers'\n[[bookmarks]]\nname='Local'\npath='../notes'\n[[bookmarks]]\nname='Missing'\npath='/missing'\n").unwrap();
    assert_eq!(
        s.bookmarks
            .iter()
            .map(|b| b.name.as_str())
            .collect::<Vec<_>>(),
        ["Home", "Papers", "Local", "Missing"]
    );
    assert_eq!(s.bookmarks[0].path, Path::new("/home/user"));
    assert_eq!(s.bookmarks[1].path, Path::new("/home/user/My Papers"));
    assert_eq!(s.bookmarks[2].path, Path::new("/config/fff/../notes"));
    assert_eq!(s.bookmarks[3].path, Path::new("/missing"));
    assert!(s.bookmarks.iter().all(|b| b.icon.is_none()));
}
#[test]
fn bookmark_icons_are_optional_short_labels() {
    let s = parse(
        "[[bookmarks]]\nname='Downloads'\nicon='\u{f10e9}'\npath='/home/user/Downloads/'\n\
         [[bookmarks]]\nname='Wide'\nicon=' 📁 '\npath='/w'",
    )
    .unwrap();
    assert_eq!(s.bookmarks[0].icon.as_deref(), Some("\u{f10e9}"));
    assert_eq!(s.bookmarks[1].icon.as_deref(), Some("📁"));
    for icon in ["''", "'   '", "'abc'", "'📁📁'"] {
        let text = format!("[[bookmarks]]\nname='A'\nicon={icon}\npath='/a'");
        assert!(parse(&text).is_err(), "accepted icon {icon}");
    }
}
#[test]
fn malformed_unknown_and_out_of_range_settings_fail_clearly() {
    for text in [
        "not toml",
        "typo=true",
        "[ui]\npallete='light'",
        "[ui]\npalette='missing'",
        "[palettes.bad]\naccent='red'",
        "[palettes.bad]\naccent='#ff00'",
        "[palettes.bad]\naccent='#gggggg'",
        "[palettes.bad]\naccnet='#123456'",
        "[palettes.bad]\nextends='bad'",
        "[palettes.light]\naccent='#123456'",
        "[cache]\nmax_entries=0",
        "[cache]\nmax_candidates=100001",
        "[cache]\nttl_seconds=0",
        "[ui]\nmessage_seconds=3601",
        "[performance]\nrefresh_ms=1001",
        "[performance]\ncached_queries=65",
        "[performance]\nmax_paths=999",
        "[performance]\nmax_paths=100000001",
        "[performance]\nrefresh=50",
        "[ui]\nmessage_seconds=-1",
        "[[bookmarks]]\nname=''\npath='/tmp'",
        "[[bookmarks]]\nname='A'\npath=''",
        "[[bookmarks]]\nname='A'\npath='/a'\n[[bookmarks]]\nname='A'\npath='/b'",
    ] {
        assert!(parse(text).is_err(), "accepted {text}");
    }
    assert!(
        Settings::parse(
            "[[bookmarks]]\nname='Home'\npath='~/notes'",
            Path::new("/cfg"),
            None,
            None
        )
        .is_err()
    );
}
#[test]
fn standard_config_location_precedence() {
    assert_eq!(
        default_path(Some(Path::new("/custom")), Some(Path::new("/home/u"))).unwrap(),
        Path::new("/custom/fff/config.toml")
    );
    assert_eq!(
        default_path(Some(Path::new("relative")), Some(Path::new("/home/u"))).unwrap(),
        Path::new("/home/u/.config/fff/config.toml")
    );
    assert!(default_path(None, None).is_none());
}

#[test]
fn documented_keys_example_is_valid() {
    use fuzzy_file_finder::keymap::{Function, Keymap, Mode};
    let keymap = Keymap::parse(include_str!("../docs/keys.example.toml")).unwrap();
    assert_eq!(keymap.of(Function::Quit, Mode::Normal)[0].display(), "Q");
    assert_eq!(keymap.of(Function::GoToHighlighted, Mode::Normal).len(), 4);
}
#[test]
fn documented_example_is_valid() {
    let settings = parse(include_str!("../docs/config.example.toml")).unwrap();
    assert_eq!(settings.bookmarks.len(), 3);
    assert_eq!(settings.cache.max_entries, 16);
}
#[test]
fn cli_config_precedence_validation_and_missing_files() {
    use std::{fs, process::Command};
    let base = std::env::temp_dir().join(format!("fff-config-cli-{}", std::process::id()));
    fs::create_dir_all(base.join("home/.config/fff")).unwrap();
    fs::create_dir_all(base.join("xdg/fff")).unwrap();
    let home = base.join("home");
    let xdg = base.join("xdg");
    fs::write(
        home.join(".config/fff/config.toml"),
        "[ui]\npalette='unknown'",
    )
    .unwrap();
    fs::write(xdg.join("fff/config.toml"), "[ui]\npalette='light'").unwrap();
    let run = |args: &[&str], use_xdg: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_fff"));
        // Every run checks the configuration; the others are global options.
        cmd.arg("setup")
            .args(args)
            .env("HOME", &home)
            .env_remove("XDG_CONFIG_HOME");
        if use_xdg {
            cmd.env("XDG_CONFIG_HOME", &xdg);
        }
        cmd.output().unwrap()
    };
    // Neither the old executable name nor the crate name changes the config directory.
    for name in ["picker", "fuzzy-file-finder"] {
        fs::create_dir_all(xdg.join(name)).unwrap();
        fs::write(xdg.join(name).join("config.toml"), "invalid config").unwrap();
        use std::os::unix::process::CommandExt;
        let renamed = Command::new(env!("CARGO_BIN_EXE_fff"))
            .arg0(name)
            .args(["setup", "--check-config"])
            .env("HOME", &home)
            .env("XDG_CONFIG_HOME", &xdg)
            .output()
            .unwrap();
        assert!(renamed.status.success());
        assert!(
            String::from_utf8_lossy(&renamed.stderr)
                .contains(&xdg.join("fff/config.toml").display().to_string())
        );
    }
    // XDG_CONFIG_HOME wins over ~/.config, and fff setup --check-config names the file.
    let checked = run(&["--check-config"], true);
    assert!(checked.status.success());
    assert!(
        String::from_utf8_lossy(&checked.stderr)
            .contains(&xdg.join("fff/config.toml").display().to_string())
    );
    let invalid = run(&["--check-config"], false);
    assert!(!invalid.status.success());
    assert!(String::from_utf8_lossy(&invalid.stderr).contains("Unknown palette"));
    assert!(invalid.stdout.is_empty());
    assert!(
        run(&["--no-config", "--check-config"], false)
            .status
            .success()
    );
    assert!(
        run(&["--palette", "forest", "--check-config"], false)
            .status
            .success()
    );
    // Programs a configuration names must be installed; the built-in
    // terminals (`st`) are not checked when nothing names them.
    let empty = base.join("empty-path");
    fs::create_dir_all(&empty).unwrap();
    let check = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_fff"))
            .arg("setup")
            .args(args)
            .arg("--check-config")
            .env("HOME", &home)
            .env("PATH", &empty)
            .env_remove("XDG_CONFIG_HOME")
            .output()
            .unwrap()
    };
    assert!(check(&["--no-config"]).status.success());
    let openers = base.join("openers.toml");
    fs::write(
        &openers,
        "[open]\nterminal = 'fff-absent-terminal -e'\n\
         [[open.rules]]\ncommand = 'fff-absent-viewer'\nextensions = ['pdf']\n",
    )
    .unwrap();
    let absent = check(&["--config", openers.to_str().unwrap()]);
    let stderr = String::from_utf8_lossy(&absent.stderr);
    assert!(!absent.status.success());
    assert!(
        stderr.contains("fff-absent-terminal") && stderr.contains("fff-absent-viewer"),
        "{stderr}"
    );
    assert!(!stderr.contains("st:"), "{stderr}");
    let explicit = xdg.join("fff/config.toml");
    assert!(
        run(
            &["--config", explicit.to_str().unwrap(), "--check-config"],
            false
        )
        .status
        .success()
    );
    let missing = base.join("missing.toml");
    let result = run(
        &["--config", missing.to_str().unwrap(), "--check-config"],
        true,
    );
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(String::from_utf8_lossy(&result.stderr).contains("missing.toml"));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn official_palette_variants_and_aliases() {
    for name in Palette::names() {
        assert!(Palette::named(name).is_some(), "{name}");
    }
    for (name, color) in [
        ("catppuccin-latte", 0xeff1f5),
        ("catppuccin-frappe", 0x303446),
        ("catppuccin-macchiato", 0x24273a),
        ("catppuccin-mocha", 0x1e1e2e),
        ("nord", 0x2e3440),
        ("gruvbox-dark", 0x282828),
        ("gruvbox-light", 0xfbf1c7),
        ("tokyonight-night", 0x1a1b26),
        ("tokyonight-storm", 0x24283b),
        ("tokyonight-moon", 0x222436),
        ("tokyonight-day", 0xe1e2e7),
        ("dracula", 0x282a36),
        ("everforest-dark", 0x2d353b),
        ("everforest-light", 0xfdf6e3),
        ("kanagawa-wave", 0x1f1f28),
        ("kanagawa-dragon", 0x181616),
        ("onedark", 0x282c34),
        ("rose-pine", 0x191724),
        ("rose-pine-moon", 0x232136),
        ("rose-pine-dawn", 0xfaf4ed),
        ("solarized-dark", 0x002b36),
        ("solarized-light", 0xfdf6e3),
        ("ayu-dark", 0x0b0e14),
        ("ayu-light", 0xf8f9fa),
        ("ayu-mirage", 0x1f2430),
        ("monokai", 0x272822),
        ("material-darker", 0x212121),
        ("zenburn", 0x383838),
        ("ocean", 0x2b303b),
        ("eighties", 0x2d2d2d),
        ("ashes", 0x1c2023),
        ("atelier-dune", 0x20201d),
        ("black-metal", 0x000000),
        ("tomorrow-night", 0x1d1f21),
    ] {
        assert_eq!(
            Palette::named(name).unwrap().background,
            Color::Rgb((color >> 16) as u8, (color >> 8) as u8, color as u8)
        );
        let text = format!("[ui]\npalette='{name}'\n[palettes.mine]\nextends='{name}'\n");
        assert!(parse(&text).is_ok());
    }
    assert_eq!(Palette::named("gruvbox"), Palette::named("gruvbox-dark"));
    for (alias, name) in [
        ("everforest", "everforest-dark"),
        ("kanagawa", "kanagawa-wave"),
        ("solarized", "solarized-dark"),
    ] {
        assert!(!Palette::names().contains(&alias));
        assert_eq!(Palette::named(alias), Palette::named(name));
    }
    assert_eq!(
        Palette::named("tokyonight"),
        Palette::named("tokyonight-night")
    );
}

#[test]
fn editor_config_defaults_to_unspecified_nvim_and_is_validated() {
    use fuzzy_file_finder::config::EditorBackend;
    assert_eq!(parse("").unwrap().editor.backend, None);
    assert_eq!(EditorBackend::default(), EditorBackend::Nvim);
    assert_eq!(
        parse("[editor]\nbackend='nvim'").unwrap().editor.backend,
        Some(EditorBackend::Nvim)
    );
    // The basic editor was removed; Neovim is the only engine.
    assert!(parse("[editor]\nbackend='basic'").is_err());
    assert!(parse("[editor]\nbackend='unknown'").is_err());
    assert!(parse("[editor]\nbacked='nvim'").is_err());
}
#[test]
fn ui_icons_default_on_can_be_disabled_and_reject_non_booleans() {
    assert!(parse("").unwrap().icons);
    assert!(parse("[ui]\nicons=true").unwrap().icons);
    assert!(!parse("[ui]\nicons=false\npalette='light'").unwrap().icons);
    for text in ["[ui]\nicons='no'", "[ui]\nicons=0", "[ui]\nicon=false"] {
        assert!(parse(text).is_err(), "accepted {text}");
    }
}
#[test]
fn keys_moved_to_keys_toml() {
    // Still read, with the table left alone and sessions told so.
    assert!(parse("[keys.normal]\nquit='Q'\n").unwrap().old_keys);
    assert!(!parse("").unwrap().old_keys);
}
#[test]
fn message_seconds_defaults_to_four_and_zero_keeps_messages() {
    use std::time::Duration;
    assert_eq!(
        parse("").unwrap().message_timeout,
        Some(Duration::from_secs(4))
    );
    assert_eq!(
        parse("[ui]\nmessage_seconds=10").unwrap().message_timeout,
        Some(Duration::from_secs(10))
    );
    assert_eq!(
        parse("[ui]\nmessage_seconds=0").unwrap().message_timeout,
        None
    );
}
#[test]
fn built_in_palettes_keep_text_readable() {
    // WCAG relative luminance contrast against the panel color.
    fn contrast(a: Color, b: Color) -> f64 {
        let luminance = |c: Color| {
            let Color::Rgb(r, g, b) = c else {
                panic!("{c:?}")
            };
            let channel = |v: u8| {
                let v = v as f64 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
        };
        let (x, y) = (luminance(a), luminance(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }
    for name in Palette::names() {
        let p = Palette::named(name).unwrap();
        assert!(contrast(p.text, p.panel) >= 4.0, "{name} text");
        for (role, color) in [
            ("muted", p.muted),
            ("accent", p.accent),
            ("marked", p.marked),
            ("success", p.success),
            ("error", p.error),
            ("link", p.link),
            ("executable", p.executable),
            ("empty", p.empty),
        ] {
            assert!(contrast(color, p.panel) >= 2.0, "{name} {role}");
        }
        assert!(contrast(p.text, p.selection) >= 2.5, "{name} selection");
        // Symlinks must read as a different blue from directories.
        let (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) = (p.link, p.accent) else {
            panic!("{name}")
        };
        let distance = [(r1, r2), (g1, g2), (b1, b2)]
            .map(|(x, y)| (x as f64 - y as f64).powi(2))
            .iter()
            .sum::<f64>()
            .sqrt();
        assert!(distance >= 60.0, "{name} link too close to directories");
        assert_ne!(p.selection, p.panel, "{name} selection");
    }
}
#[test]
fn performance_refresh_interval_defaults_to_fifty_and_accepts_zero() {
    assert_eq!(parse("").unwrap().performance.refresh_ms, 50);
    assert_eq!(
        parse("[performance]\nrefresh_ms=0")
            .unwrap()
            .performance
            .refresh_ms,
        0
    );
}
#[test]
fn performance_cached_queries_default_to_five_and_accept_zero() {
    assert_eq!(parse("").unwrap().performance.cached_queries, 5);
    let config = parse("[performance]\ncached_queries=0").unwrap();
    assert_eq!(config.performance.cached_queries, 0);
}
#[test]
fn performance_max_paths_defaults_to_a_million() {
    assert_eq!(parse("").unwrap().performance.max_paths, 1_000_000);
    let config = parse("[performance]\nmax_paths=1000").unwrap();
    assert_eq!(config.performance.max_paths, 1000);
}

#[test]
fn cache_freshness_and_prefetch_configuration() {
    let settings = parse("").unwrap();
    assert_eq!(settings.cache.fresh_seconds, 3);
    assert!(settings.cache.prefetch);
    let settings = parse("[cache]\nfresh_seconds=0\nprefetch=false").unwrap();
    assert_eq!(settings.cache.fresh_seconds, 0);
    assert!(!settings.cache.prefetch);
    for text in [
        "[cache]\nfresh_seconds=61",
        "[cache]\nfresh_seconds=-1",
        "[cache]\nprefetch='yes'",
    ] {
        assert!(parse(text).is_err(), "{text}");
    }
}
#[test]
fn trash_defaults_to_a_month_and_asks_above_ten_gib() {
    use fuzzy_file_finder::config::TrashConfig;
    let trash = parse("").unwrap().trash;
    assert_eq!(
        trash,
        TrashConfig {
            enabled: true,
            days: 30,
            ask_above: 10 << 30,
        }
    );
    let trash = parse("[trash]\nenabled=false\ndays=7\nask_above='500 MiB'")
        .unwrap()
        .trash;
    assert_eq!(
        (trash.enabled, trash.days, trash.ask_above),
        (false, 7, 500 << 20)
    );
    // Plain numbers are bytes.
    assert_eq!(
        parse("[trash]\nask_above=4096").unwrap().trash.ask_above,
        4096
    );
    for (text, message) in [
        ("[trash]\ndays=0", "trash.days must be 1..36500"),
        ("[trash]\ndays=36501", "trash.days must be 1..36500"),
        ("[trash]\nask_above='lots'", "trash.ask_above"),
        ("[trash]\nask_above='10 parsecs'", "unknown size unit"),
        ("[trash]\nenabled='yes'", "invalid type"),
        ("[trash]\nkeep=3", "unknown field"),
    ] {
        let error = parse(text).err().unwrap();
        assert!(error.contains(message), "{text}: {error}");
    }
}
#[test]
fn a_configuration_with_errors_gives_way_to_its_backup() {
    use fuzzy_file_finder::config::{backup_path, brief, config_error, reading_backup};
    let folder = std::env::temp_dir().join(format!("fff-config-backup-{}", std::process::id()));
    std::fs::create_dir_all(&folder).unwrap();
    let config = folder.join("config.toml");
    let backup = backup_path(&config);
    assert_eq!(backup, folder.join("config.toml.bak"));
    std::fs::write(&config, "[ui]\npalette = 'nord'\n").unwrap();
    // A file that loads is read as it is, whatever is beside it.
    std::fs::write(&backup, "[ui]\npalette = 'forest'\n").unwrap();
    let s = Settings::load_or_backup(Some(&config), false, None).unwrap();
    assert_eq!((s.palette_name.as_str(), s.broken), ("nord", None));
    // With errors, the backup is read and the errors kept for the session.
    std::fs::write(&config, "[ui]\npalette = \n").unwrap();
    let s = Settings::load_or_backup(Some(&config), false, None).unwrap();
    assert_eq!(s.palette_name, "forest");
    assert_eq!(s.source.as_deref(), Some(config.as_path()));
    let error = s.broken.unwrap();
    assert!(Settings::load(Some(&config), false, None).is_err());
    assert!(reading_backup(&config).contains("config.toml.bak"));
    let message = brief(config_error(&config, &error));
    assert!(
        message.starts_with("TOML parse error at line 2"),
        "{message}"
    );
    assert!(
        !message.contains('\n') && !message.contains('^'),
        "{message}"
    );
    assert!(!brief(&error).contains('|'));
    // Without a backup that loads, the errors stand.
    std::fs::write(&backup, "[ui]\nnope = 1\n").unwrap();
    assert!(Settings::load_or_backup(Some(&config), false, None).is_err());
    std::fs::remove_file(&backup).unwrap();
    assert!(Settings::load_or_backup(Some(&config), false, None).is_err());
    std::fs::remove_dir_all(folder).unwrap();
}
#[test]
fn preview_syntax_colors_default_on_and_can_be_turned_off() {
    assert!(parse("").unwrap().highlight);
    assert!(!parse("[ui]\nhighlight = false\n").unwrap().highlight);
    assert!(parse("[ui]\nhighlight = 'yes'\n").is_err());
}
