use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "palm-cli-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(
            path.join("Cargo.toml"),
            "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        Self(path)
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_utgen"));
        command.current_dir(&self.0);
        for name in ["PALM_CONFIG", "PALM_API_BASE", "PALM_API_KEY", "PALM_MODEL"] {
            command.env_remove(name);
        }
        command
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn help_and_non_model_commands_do_not_load_configuration() {
    let fixture = Fixture::new();
    for args in [
        vec!["--help"],
        vec!["gen", "--help"],
        vec!["fix", "--help"],
        vec!["analyze", "--help"],
        vec!["coverage", "--help"],
        vec!["pre-process", "-p", "."],
    ] {
        let output = fixture
            .command()
            .args(args)
            .env("PALM_CONFIG", "missing.json")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn tasks_default_to_four_and_reject_zero_before_loading_configuration() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.0.join("Cargo.toml")).unwrap();
    for command in ["gen", "fix"] {
        let help = fixture
            .command()
            .args([command, "--help"])
            .output()
            .unwrap();
        assert!(help.status.success());
        assert!(String::from_utf8_lossy(&help.stdout).contains("[default: 4]"));
        let output = fixture
            .command()
            .args([command, "-p", ".", "--tasks", "0"])
            .env("PALM_CONFIG", "missing.json")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("--tasks"), "{error}");
        assert!(!error.contains("Cannot read"), "{error}");
        assert_eq!(before, fs::read(fixture.0.join("Cargo.toml")).unwrap());
        assert!(!fixture.0.join("utgen").exists());
    }
}

#[test]
fn request_timeout_rejects_zero_before_loading_configuration() {
    let fixture = Fixture::new();
    for command in ["gen", "fix"] {
        let help = fixture
            .command()
            .args([command, "--help"])
            .output()
            .unwrap();
        assert!(String::from_utf8_lossy(&help.stdout).contains("[default: 180]"));
        let output = fixture
            .command()
            .args([command, "-p", ".", "--request-timeout", "0"])
            .env("PALM_CONFIG", "missing.json")
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("--request-timeout"), "{error}");
        assert!(!error.contains("Cannot read"), "{error}");
        assert!(!fixture.0.join("utgen").exists());
    }
}

#[test]
fn analyze_validates_the_crate_without_loading_model_configuration() {
    let fixture = Fixture::new();
    let output = fixture
        .command()
        .args(["analyze", "-p", "."])
        .env("PALM_CONFIG", "missing.json")
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("Analysis requires a crate"), "{error}");
    assert!(!error.contains("model configuration"));
}

#[test]
fn missing_analysis_does_not_modify_the_manifest() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.0.join("Cargo.toml")).unwrap();
    for command in ["gen", "fix"] {
        let output = fixture
            .command()
            .args([command, "-p", "."])
            .env("PALM_API_BASE", "http://127.0.0.1:9/v1")
            .env("PALM_API_KEY", "unused")
            .env("PALM_MODEL", "unused")
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("name_map.json"));
        assert_eq!(before, fs::read(fixture.0.join("Cargo.toml")).unwrap());
    }
}

#[test]
fn preprocessing_only_changes_the_selected_crate() {
    let fixture = Fixture::new();
    for name in ["selected", "untouched"] {
        fs::create_dir_all(fixture.0.join(name).join("src")).unwrap();
        fs::create_dir_all(fixture.0.join(name).join("tests")).unwrap();
        fs::write(
            fixture.0.join(name).join("src/lib.rs"),
            "#[test] fn old() {} fn keep() {}\n",
        )
        .unwrap();
    }
    let output = fixture
        .command()
        .args(["pre-process", "-p", ".", "-w", "selected"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.0.join("selected/tests.bak").is_dir());
    assert!(!fixture.0.join("selected/tests").exists());
    assert!(fixture.0.join("untouched/tests").is_dir());
    assert_eq!(
        fs::read_to_string(fixture.0.join("untouched/src/lib.rs")).unwrap(),
        "#[test] fn old() {} fn keep() {}\n"
    );
}

#[test]
fn preprocessing_refuses_to_overwrite_an_existing_backup() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.0.join("src")).unwrap();
    fs::create_dir(fixture.0.join("tests")).unwrap();
    fs::create_dir(fixture.0.join("tests.bak")).unwrap();
    fs::write(fixture.0.join("src/lib.rs"), "#[test] fn old() {}\n").unwrap();
    let output = fixture
        .command()
        .args(["pre-process", "-p", "."])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("tests.bak already exists"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("src/lib.rs")).unwrap(),
        "#[test] fn old() {}\n"
    );
}

#[cfg(unix)]
#[test]
fn analysis_stops_after_a_failed_tool_command() {
    use std::os::unix::fs::PermissionsExt;
    let fixture = Fixture::new();
    fs::create_dir(fixture.0.join("src")).unwrap();
    fs::create_dir(fixture.0.join("bin")).unwrap();
    let cargo = fixture.0.join("bin/cargo");
    fs::write(
        &cargo,
        "#!/bin/sh\necho fixture-cargo-failure >&2\nexit 42\n",
    )
    .unwrap();
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let output = fixture
        .command()
        .args(["analyze", "-p", "."])
        .env("PATH", fixture.0.join("bin"))
        .output()
        .unwrap();
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert_eq!(error.matches("fixture-cargo-failure").count(), 1, "{error}");
    assert!(error.contains("42"), "{error}");
    assert!(!fixture.0.join("brinfo").exists());
    assert!(!fixture.0.join("focxt").exists());
}

#[test]
fn generation_and_repair_reject_missing_configuration_before_modifying_target() {
    let fixture = Fixture::new();
    let before = fs::read(fixture.0.join("Cargo.toml")).unwrap();
    // A current-directory api.json is never implicitly loaded.
    fs::write(
        fixture.0.join("api.json"),
        r#"{"base":"http://127.0.0.1:9/v1","key":"unused","model":"unused"}"#,
    )
    .unwrap();
    for command in ["gen", "fix"] {
        let output = fixture
            .command()
            .args([command, "-p", "."])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("PALM_API_BASE"), "{error}");
        assert!(!error.contains("panicked"));
        assert_eq!(before, fs::read(fixture.0.join("Cargo.toml")).unwrap());
        assert!(!fixture.0.join("utgen").exists());
    }
}

#[test]
fn explicit_config_path_overrides_environment_path_before_or_after_subcommand() {
    let fixture = Fixture::new();
    fs::write(
        fixture.0.join("config.json"),
        r#"{"base":"http://127.0.0.1:9/v1","model":"test"}"#,
    )
    .unwrap();
    for command in ["gen", "fix"] {
        for args in [
            vec!["--config", "config.json", command, "-p", "."],
            vec![command, "-p", ".", "--config", "config.json"],
        ] {
            let output = fixture
                .command()
                .args(args)
                .env("PALM_CONFIG", "missing.json")
                .output()
                .unwrap();
            assert!(!output.status.success());
            let error = String::from_utf8_lossy(&output.stderr);
            assert!(error.contains("PALM_API_KEY"), "{error}");
            assert!(!error.contains("Cannot read"), "{error}");
        }
    }
}
