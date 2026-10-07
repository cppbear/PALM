use cargo_metadata::MetadataCommand;
use std::fs;
use std::path::{Path, PathBuf};

fn get_target_dir(work_dir: &Path) -> Option<PathBuf> {
    MetadataCommand::new()
        .no_deps()
        .current_dir(work_dir)
        .exec()
        .ok()
        .map(|m| m.target_directory.into())
}

pub fn target_clean(work_dir: &Path) -> std::io::Result<()> {
    let whitelist = ["deps", ".fingerprint", "incremental"];
    if let Some(target_path) = get_target_dir(work_dir) {
        let target_dir = target_path.join("debug");
        if target_dir.exists() {
            for entry in fs::read_dir(&target_dir)? {
                let entry = entry?;
                let path = entry.path();
                let name = path.file_name().unwrap().to_str().unwrap();
                if whitelist.contains(&name) {
                    continue;
                }
                if path.is_dir() {
                    fs::remove_dir_all(path)?;
                } else {
                    fs::remove_file(path)?;
                }
            }
        }
    }
    Ok(())
}
