use std::{
    collections::{BTreeSet, HashMap},
    fs, io,
    path::{Path, PathBuf},
};

/// Exact analysis-index names. No file means all functions.
#[derive(Default)]
pub struct FunctionSelection {
    names: Option<BTreeSet<String>>,
}

impl FunctionSelection {
    pub fn load(path: Option<&Path>, work_dirs: &[PathBuf]) -> io::Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let names: BTreeSet<String> = fs::read_to_string(path)?
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_owned)
            .collect();
        if names.is_empty() {
            return Err(io::Error::other(
                "--functions-file contains no function names",
            ));
        }
        let mut available = BTreeSet::new();
        for work_dir in work_dirs {
            let map: HashMap<String, String> =
                serde_json::from_slice(&fs::read(work_dir.join("brinfo/name_map.json"))?)?;
            available.extend(map.into_keys());
        }
        let unknown: Vec<_> = names.difference(&available).cloned().collect();
        if !unknown.is_empty() {
            return Err(io::Error::other(format!(
                "Unknown functions in --functions-file: {}",
                unknown.join(", ")
            )));
        }
        Ok(Self { names: Some(names) })
    }

    pub fn contains(&self, name: &str) -> bool {
        self.names.as_ref().is_none_or(|names| names.contains(name))
    }

    pub fn names(&self) -> Option<&BTreeSet<String>> {
        self.names.as_ref()
    }
}
