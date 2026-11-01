//! Command-line surface: help text, completion scripts and dynamic completion.
use std::process::{Command, Output};

fn picker(args: &[&str], complete: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_picker"));
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
    let long = stdout(&picker(&["--help"], None));
    for text in [
        "[default: 3]",
        "Examples:",
        "Exit status:",
        "130",
        "Keys:",
        "Alt+p",
        "[keys.normal]",
        "--completions <SHELL>",
        "[possible values: bash, elvish, fish, powershell, zsh]",
        "source <(picker --completions bash)",
    ] {
        assert!(long.contains(text), "missing {text:?}");
    }
    assert!(!long.contains('\x1b'), "NO_COLOR help must be plain");
    let short = stdout(&picker(&["-h"], None));
    assert!(short.contains("Examples:") && short.len() < long.len());
    assert_eq!(picker(&["--bogus"], None).status.code(), Some(2));
    for bad in [
        &["--map-key", "space=down"][..],
        &["--map-key", "bogus"],
        &["--glob", "["],
        &["--exclude", "["],
    ] {
        let mut line = vec!["--no-config"];
        line.extend_from_slice(bad);
        assert_eq!(picker(&line, None).status.code(), Some(2), "{bad:?}");
    }
}

#[test]
fn completion_scripts_register_for_each_shell() {
    for shell in ["bash", "zsh", "fish"] {
        let flag = stdout(&picker(&["--completions", shell], None));
        let env = stdout(&picker(&[], Some(shell)));
        assert!(
            flag.contains("COMPLETE") && flag.contains("picker"),
            "{shell}"
        );
        assert_eq!(flag, env, "{shell}: flag and COMPLETE= scripts differ");
    }
    assert_eq!(
        picker(&["--completions", "tcsh"], None).status.code(),
        Some(2)
    );
}

#[test]
fn dynamic_completion_reads_custom_palettes_from_the_named_config() {
    let line = |args: &[&str]| {
        let mut full = vec!["--", "picker"];
        full.extend(args);
        stdout(&picker(&full, Some("fish")))
    };
    let palettes = line(&["--config", "docs/config.example.toml", "--palette", ""]);
    assert!(palettes.contains("ember\tcustom") && palettes.contains("nord\tbuilt-in"));
    assert!(!line(&["--no-config", "--palette", ""]).contains("ember"));
    assert!(line(&["--edi"]).starts_with("--editor"));
    assert!(line(&["--config", "docs/config.ex"]).contains("docs/config.example.toml"));
    assert!(line(&["--map-key", "alt+j=no"]).contains("alt+j=none"));
}
