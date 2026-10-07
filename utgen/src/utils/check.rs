use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;

pub fn cargo_check(work_dir: &Path) -> Result<(), String> {
    let output = Command::new("cargo")
        .args(["build", "--tests"])
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
    if path.exists() {
        fs::remove_file(&path).unwrap();
    }
    fs::create_dir_all(&path.parent().unwrap()).unwrap();

    use_set.retain(|x| {
        let code = format!("{}\n", x);
        fs::write(&path, code).unwrap();
        cargo_check(&work_dir).is_ok()
    });
}
