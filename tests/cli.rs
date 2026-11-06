//! Command-line surface: help text, completion scripts and dynamic completion.
use std::process::{Command, Output};

fn fff(args: &[&str], complete: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fff"));
    command
        .args(args)
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .env("NO_COLOR", "1")
        .env_remove("COMPLETE");
    if let Some(shell) = complete {
        command.env("COMPLETE", shell);
    }
    command.output().unwrap()
}
fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

#[test]
fn help_describes_defaults_examples_exit_codes_and_completions() {
    let long = stdout(&fff(&["--help"], None));
    for text in [
        "[default: 1; in fff pick, 3; in fff seek, 6]",
        "Examples:",
        "Exit status:",
        "130",
        "Keys:",
        "Alt+p",
        "keys.toml",
        "source <(fff setup --completions bash)",
        "Commands (without one, fff roam):",
    ] {
        assert!(long.contains(text), "missing {text:?}");
    }
    let setup = stdout(&fff(&["setup", "--help"], None));
    for text in [
        "--completions <SHELL>",
        "[possible values: bash, elvish, fish, powershell, zsh]",
        "--browser",
        "--man [<DIR>]",
        "--check-config",
    ] {
        assert!(setup.contains(text), "missing {text:?}");
    }
    assert!(!long.contains('\x1b'), "NO_COLOR help must be plain");
    let short = stdout(&fff(&["-h"], None));
    assert!(short.contains("Examples:") && short.len() < long.len());
    assert_eq!(fff(&["--bogus"], None).status.code(), Some(2));
    for bad in [&["--glob", "["][..], &["--exclude", "["]] {
        let mut line = vec!["--no-config"];
        line.extend_from_slice(bad);
        assert_eq!(fff(&line, None).status.code(), Some(2), "{bad:?}");
    }
}

#[test]
fn seek_is_a_command_and_options_belong_to_their_commands() {
    let help = stdout(&fff(&["seek", "--help"], None));
    assert!(
        help.contains("Usage: fff seek [OPTIONS] [ROOTS]..."),
        "{help}"
    );
    for text in ["--glob", "--depth"] {
        assert!(help.contains(text), "missing {text:?}");
    }
    // Seeking does not browse, only the picker modes print paths, and the
    // options of earlier versions are gone.
    for bad in [
        &["seek", "--window"][..],
        &["--resume", "seek"],
        &["--window", "pick"],
        &["pick", "--resume"],
        &["seek", "--relative"],
        &["seek", "--output-paths"],
        &["explorer", "--output-paths"],
        &["explorer"],
        &["explorer", "--save"],
        &["--no-dir"],
        &["--check-config"],
        &["--here"],
        &["--resume", "src"],
        &["setup"],
        &["setup", "--desktop", "--check-config"],
    ] {
        assert_eq!(fff(bad, None).status.code(), Some(2), "{bad:?}");
    }
}

#[test]
fn completion_scripts_register_for_each_shell() {
    for shell in ["bash", "zsh", "fish"] {
        let flag = stdout(&fff(&["setup", "--completions", shell], None));
        let env = stdout(&fff(&[], Some(shell)));
        assert!(flag.contains("COMPLETE") && flag.contains("fff"), "{shell}");
        assert_eq!(flag, env, "{shell}: flag and COMPLETE= scripts differ");
    }
    assert_eq!(
        fff(&["setup", "--completions", "tcsh"], None).status.code(),
        Some(2)
    );
}

#[test]
fn dynamic_completion_reads_custom_palettes_from_the_named_config() {
    let line = |args: &[&str]| {
        let mut full = vec!["--", "fff"];
        full.extend(args);
        stdout(&fff(&full, Some("fish")))
    };
    let palettes = line(&["--config", "docs/config.example.toml", "--palette", ""]);
    assert!(palettes.contains("ember\tcustom") && palettes.contains("nord\tbuilt-in"));
    assert!(!line(&["--no-config", "--palette", ""]).contains("ember"));
    assert!(line(&["--pal"]).starts_with("--palette"));
    assert!(line(&["--config", "docs/config.ex"]).contains("docs/config.example.toml"));
}

#[test]
fn renamed_binary_reports_fff_version_and_usage() {
    assert_eq!(
        stdout(&fff(&["--version"], None)).trim(),
        format!("fff {}", env!("CARGO_PKG_VERSION"))
    );
    let help = stdout(&fff(&["--help"], None));
    assert!(help.contains("Usage: fff"), "{help}");
}

#[test]
fn missing_root_error_names_the_path() {
    let output = fff(&["--no-config", "no/such/root"], None);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.starts_with("fff: no/such/root: "), "{stderr}");
}

#[test]
fn init_wrappers_change_to_the_directory_fff_writes() {
    use std::os::unix::fs::PermissionsExt;
    let root = std::env::temp_dir().join(format!("fff-init-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    // A space and a trailing newline: the bytes must survive the round trip.
    let target = root.join("dir with space\n");
    let stub = root.join("stub");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::create_dir_all(&stub).unwrap();
    // A stand-in fff that writes TARGET to the file given with --cd-file.
    std::fs::write(
        stub.join("fff"),
        "#!/bin/sh\nwhile [ $# -gt 0 ]; do\n  if [ \"$1\" = --cd-file ]; then shift; printf '%s' \"$TARGET\" > \"$1\"; fi\n  shift\ndone\nexit 3\n",
    )
    .unwrap();
    std::fs::set_permissions(stub.join("fff"), std::fs::Permissions::from_mode(0o755)).unwrap();
    for (shell, flags) in [("zsh", &["-f", "-c"][..]), ("bash", &["--norc", "-c"][..])] {
        if Command::new(shell).arg("--version").output().is_err() {
            continue;
        }
        let wrapper = stdout(&fff(&["setup", "--init", shell], None));
        assert!(wrapper.contains("command fff --cd-file"), "{shell}");
        let out = Command::new(shell)
            .args(flags)
            .arg(r#"eval "$WRAPPER"; PATH="$STUB:$PATH"; fff --anything; printf '%s|%s' "$?" "$PWD""#)
            .env("WRAPPER", &wrapper)
            .env("STUB", &stub)
            .env("TARGET", &target)
            .current_dir(&root)
            .output()
            .unwrap();
        let shown = String::from_utf8(out.stdout.clone()).unwrap();
        assert_eq!(shown, format!("3|{}", target.display()), "{shell}: {out:?}");
    }
    let fish = stdout(&fff(&["setup", "--init", "fish"], None));
    assert!(fish.contains("function fff") && fish.contains("command fff --cd-file"));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn output_formatting_needs_the_output_paths_mode() {
    for flag in ["--relative", "--null", "--output-paths"] {
        assert_eq!(
            fff(&["--no-config", flag], None).status.code(),
            Some(2),
            "{flag}"
        );
    }
}

#[test]
fn browse_takes_at_most_one_path() {
    assert_eq!(
        fff(&["--no-config", "--browse", "src", "docs"], None)
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn desktop_setup_writes_an_entry_without_touching_stdout() {
    let root = std::env::temp_dir().join(format!("fff-setup-cli-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_fff"))
            .args(["--no-config", "setup", "--desktop"])
            .env("HOME", &root)
            .env("XDG_DATA_HOME", root.join("data"))
            // No xdg-mime: setup says how to make fff the default instead.
            .env("PATH", &root)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(
        stderr.contains("xdg-mime default fff.desktop inode/directory"),
        "{stderr}"
    );
    let entry = std::fs::read_to_string(root.join("data/applications/fff.desktop")).unwrap();
    let launcher = root.join("data/fff/open-folder");
    assert!(
        entry.contains(&format!("\nExec={} %f\n", launcher.display())),
        "{entry}"
    );
    let exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_fff")).unwrap();
    let script = std::fs::read_to_string(&launcher).unwrap();
    assert!(
        script.contains(&format!(
            "exec 'st' '-c' 'fff-picker' '-e' '{}' 'window' \"$@\" >/dev/null 2>&1",
            exe.display()
        )),
        "{script}"
    );
    // "Show in folder" starts fff over D-Bus, with the same launcher.
    let service = std::fs::read_to_string(
        root.join("data/dbus-1/services/org.freedesktop.FileManager1.service"),
    )
    .unwrap();
    assert!(
        service.contains("\nName=org.freedesktop.FileManager1\n"),
        "{service}"
    );
    assert!(
        service.contains(&format!(
            "\nExec='{}' dbus-file-manager '{}'\n",
            exe.display(),
            launcher.display()
        )),
        "{service}"
    );
    assert!(
        String::from_utf8(run().stderr)
            .unwrap()
            .contains("unchanged")
    );
    // A desktop entry fff did not write is left alone.
    std::fs::write(
        root.join("data/applications/fff.desktop"),
        "[Desktop Entry]\n",
    )
    .unwrap();
    assert_eq!(run().status.code(), Some(1));
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn setup_writes_a_manual_page_per_command() {
    let dir = std::env::temp_dir().join(format!("fff-man-{}", std::process::id()));
    let output = fff(
        &["setup", "--man", dir.join("man1").to_str().unwrap()],
        None,
    );
    assert!(output.status.success());
    assert!(output.stdout.is_empty(), "stdout stays empty");
    for page in [
        "fff.1",
        "fff-roam.1",
        "fff-window.1",
        "fff-pick.1",
        "fff-seek.1",
        "fff-explorer-open.1",
        "fff-setup.1",
    ] {
        let text = std::fs::read_to_string(dir.join("man1").join(page)).unwrap();
        assert!(text.starts_with(".ie") || text.contains(".TH"), "{page}");
        assert!(!text.contains('\x1b'), "{page} has escape codes");
    }
    assert!(!dir.join("man1/fff-dbus-file-manager.1").exists());
    std::fs::remove_dir_all(dir).unwrap();
}
