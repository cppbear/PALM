mod backup;
mod check;
mod clean;
mod insert;
mod preprocess;

use crate::types::BrData;
pub use backup::*;
pub use check::*;
pub use clean::*;
pub use insert::insert_test;
pub use preprocess::*;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// Get the use path of the function
/// For example, if the function name is `a::b::c::fn`, the use paths are:
/// - `use a::*;`
/// - `use a::b::*;`
/// - `use a::b::c::*;`
///
/// If the function name is `a::b::{impl#0}::fn`, the use paths are:
/// - `use a::*;`
/// - `use a::b::*;`
pub fn get_use_path(name: &str) -> Vec<String> {
    let parts: Vec<&str> = name.split("::").collect();
    let mut paths = Vec::new();
    let mut current_path = String::new();

    for part in parts.iter().take(parts.len() - 1) {
        if part.starts_with("{impl") {
            break;
        }
        if !current_path.is_empty() {
            current_path.push_str("::");
        }
        current_path.push_str(part);

        paths.push(format!("use {}::*;", current_path));
    }

    paths
}

pub fn get_brdata(crate_dir: &Path, name: &str) -> BrData {
    let brinfo_dir = crate_dir.join("brinfo");
    let map_path = brinfo_dir.join("name_map.json");
    let nmap: HashMap<String, String> =
        serde_json::from_str(&fs::read_to_string(map_path).unwrap()).unwrap();
    let encode_name = nmap.get(name).unwrap();
    let brdata_path = brinfo_dir.join("brdata").join(encode_name);
    serde_json::from_str(&fs::read_to_string(&brdata_path).unwrap()).unwrap()
}

pub fn get_focxt_path(crate_dir: &Path, name: &str) -> PathBuf {
    unimplemented!();
}
