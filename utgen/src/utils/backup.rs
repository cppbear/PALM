use std::{fs, path::Path};
use walkdir::WalkDir;

pub fn has_backup(file_path: &Path) -> bool {
    let backup_path = file_path.with_extension(format!(
        "{}.bak",
        file_path
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
    ));
    return fs::exists(backup_path).unwrap();
}

pub fn delete_backup(file_path: &Path) {
    let backup_path = file_path.with_extension(format!(
        "{}.bak",
        file_path
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
    ));
    fs::remove_file(backup_path).unwrap();
}

pub fn backup_file(path: &Path) {
    let backup_path = path.with_extension(format!(
        "{}.bak",
        path.extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
    ));
    fs::copy(path, &backup_path).expect(format!("{:?}", path).as_str());
}

pub fn restore_file(path: &Path) {
    let backup_path = path.with_extension(format!(
        "{}.bak",
        path.extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
    ));
    fs::copy(&backup_path, path).unwrap();
}

pub fn delete_all_backups(work_path: &Path) {
    for entry in WalkDir::new(work_path).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_file() && path.extension().unwrap_or_default() == "rs" && has_backup(path) {
            delete_backup(path);
        }
    }
}
