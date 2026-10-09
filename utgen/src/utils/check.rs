use super::TemporaryFile;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn cargo_check(work_dir: &Path, target: &crate::target::TargetInfo) -> Result<(), String> {
    let output = Command::new("cargo")
        .arg("test")
        .args(target.unit_args())
        .arg("--no-run")
        .current_dir(work_dir)
        .output()
        .expect("failed to compile unit tests");
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

pub(crate) fn cargo_check_test(work_dir: &Path, target: &str) -> Result<(), String> {
    cargo_build(work_dir, &["--test", target])
}

fn cargo_build(work_dir: &Path, targets: &[&str]) -> Result<(), String> {
    let output = Command::new("cargo")
        .arg("build")
        .args(targets)
        .current_dir(work_dir)
        .output()
        .expect("failed to execute process");

    // println!("stdout: {}", String::from_utf8_lossy(&output.stdout));
    // println!("stderr: {}", String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        // println!("check succeeded");
        return Ok(());
    } else {
        // println!("check failed");
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
}

pub fn use_check(use_set: &mut HashSet<String>, work_dir: &Path) {
    let path = work_dir.join("tests/use_check.rs");
    let temporary = TemporaryFile::new(&path).unwrap();
    fs::create_dir_all(&path.parent().unwrap()).unwrap();

    use_set.retain(|x| {
        let code = format!("{}\n", x);
        fs::write(&path, code).unwrap();
        cargo_check_test(work_dir, "use_check").is_ok()
    });
    temporary.finish().unwrap();
}
