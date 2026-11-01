use picker::config::{Palette, Settings, default_path};
use ratatui::style::Color;
use std::path::Path;
fn parse(text: &str) -> Result<Settings, String> {
    Settings::parse(
        text,
        Path::new("/config/picker"),
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
    assert_eq!(s.bookmarks[2].path, Path::new("/config/picker/../notes"));
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
        Path::new("/custom/picker/config.toml")
    );
    assert_eq!(
        default_path(Some(Path::new("relative")), Some(Path::new("/home/u"))).unwrap(),
        Path::new("/home/u/.config/picker/config.toml")
    );
    assert!(default_path(None, None).is_none());
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
    let base = std::env::temp_dir().join(format!("picker-config-cli-{}", std::process::id()));
    fs::create_dir_all(base.join("home/.config/picker")).unwrap();
    fs::create_dir_all(base.join("xdg/picker")).unwrap();
    let home = base.join("home");
    let xdg = base.join("xdg");
    fs::write(
        home.join(".config/picker/config.toml"),
        "[ui]\npalette='unknown'",
    )
    .unwrap();
    fs::write(xdg.join("picker/config.toml"), "[ui]\npalette='light'").unwrap();
    let run = |args: &[&str], use_xdg: bool| {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_picker"));
        cmd.args(args)
            .env("HOME", &home)
            .env_remove("XDG_CONFIG_HOME");
        if use_xdg {
            cmd.env("XDG_CONFIG_HOME", &xdg);
        }
        cmd.output().unwrap()
    };
    // XDG_CONFIG_HOME wins over ~/.config, and --check-config names the file.
    let checked = run(&["--check-config"], true);
    assert!(checked.status.success());
    assert!(
        String::from_utf8_lossy(&checked.stderr)
            .contains(&xdg.join("picker/config.toml").display().to_string())
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
    let explicit = xdg.join("picker/config.toml");
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
    use picker::config::EditorBackend;
    assert_eq!(parse("").unwrap().editor.backend, None);
    assert_eq!(EditorBackend::default(), EditorBackend::Nvim);
    assert_eq!(
        parse("[editor]\nbackend='nvim'").unwrap().editor.backend,
        Some(EditorBackend::Nvim)
    );
    assert_eq!(
        parse("[editor]\nbackend='basic'").unwrap().editor.backend,
        Some(EditorBackend::Basic)
    );
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
fn normal_mode_list_keys_are_validated() {
    assert!(parse("[keys.normal]\nfirst=['gg','<space>g']\nlast='none'\nleft=''\n").is_ok());
    for bad in [
        "[keys.normal]\nfirst='g'\nlast='gg'\n",
        "[keys.normal]\ndown='x'\nup='x'\n",
        "[keys.normal]\nfirst='<c-d>'\n",
        "[keys.normal]\ntop='gg'\n",
        "[keys.insert]\ndown='j'\n",
        "[keys.normal]\nfirst=1\n",
    ] {
        assert!(parse(bad).is_err(), "accepted {bad:?}");
    }
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
        ] {
            assert!(contrast(color, p.panel) >= 2.0, "{name} {role}");
        }
        assert!(contrast(p.text, p.selection) >= 2.5, "{name} selection");
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
