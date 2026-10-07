use crate::{
    types::TestGenInfo,
    utils::{delete_backup, has_backup, restore_file},
};
use std::{
    fs::{self},
    io::{Read, Write},
    path::{Path, PathBuf},
};

pub fn get_test_gen_infos(project_path: &Path, is_pre: bool) -> Vec<TestGenInfo> {
    let mut test_gen_infos: Vec<TestGenInfo> = Vec::new();
    let mut info_path = PathBuf::new();
    if is_pre {
        info_path = project_path.join("utgen/generation/pre_fix");
    } else {
        info_path = project_path.join("utgen/generation/llm_fix");
    }
    if info_path.is_dir() {
        for entry in fs::read_dir(info_path).unwrap() {
            let entry = entry.unwrap();
            let entry_path = entry.path();
            if entry_path.is_file() {
                let test_gen_info = TestGenInfo::from_json(&entry_path);
                test_gen_infos.push(test_gen_info);
            }
        }
    }
    test_gen_infos
}

pub fn restore_all_file(project_path: &Path, work_dir: &Path, test_gen_infos: &Vec<TestGenInfo>) {
    for test_gen_info in test_gen_infos.iter() {
        let file_rela = test_gen_info.get_file();
        let file_path = project_path.join(file_rela);
        if has_backup(&file_path) {
            restore_file(&file_path);
            delete_backup(&file_path);
        }
    }
}

pub fn add_ntest_dependency(work_dir: &Path) {
    let cargo_toml_path = work_dir.join("Cargo.toml");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .read(true)
        .append(true)
        .open(cargo_toml_path)
        .unwrap();
    let mut file_string = String::new();
    file.read_to_string(&mut file_string).unwrap();
    if file_string.contains("ntest") {
        return;
    }
    file.write("[dependencies.ntest]\nversion = \"0.9.3\"\n".as_bytes())
        .unwrap();
}
