use crate::types::InsertKind;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;

pub fn insert_test(insert_kind: InsertKind, path: &Path, code: &Vec<String>) {
    match insert_kind {
        InsertKind::EOF => {
            insert_eof(path, code);
        }
        InsertKind::EOM(end) => {
            insert_eom(path, end, code);
        }
    }
}

fn insert_eof(path: &Path, code: &Vec<String>) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    let buf = format!("\n{}\n", code.join("\n"));
    file.write_all(buf.as_bytes()).unwrap();
}

fn insert_eom(path: &Path, mod_end: usize, code: &Vec<String>) {
    let content = fs::read_to_string(path).unwrap();
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let mod_end_line = lines[mod_end - 1].clone();
    if let Some(pos) = mod_end_line.rfind('}') {
        let (before, after) = mod_end_line.split_at(pos);
        lines[mod_end - 1] = before.to_string();
        lines.insert(mod_end, after.trim().to_string());
    }
    lines.splice(mod_end..mod_end, code.clone());
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.write_all(lines.join("\n").as_bytes()).unwrap();
}
