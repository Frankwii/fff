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
    let s = Settings::parse(text, Path::new("/cfg"), None, Some("forest")).unwrap();
    assert_eq!(s.palette, Palette::named("forest").unwrap());
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
    assert!(run(&["--check-config"], true).status.success());
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
