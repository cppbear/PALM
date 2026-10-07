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
        vec!["analyze", "-p", "."],
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
