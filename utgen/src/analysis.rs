use crate::types::{BrData, RfocxtNameInformation, TestGenInfo};
use std::{collections::HashMap, fs, io, path::Path, process::Command, time::Instant};

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> io::Result<T> {
    let data = fs::read(path).map_err(|e| io::Error::other(format!("{}: {e}", path.display())))?;
    serde_json::from_slice(&data).map_err(|e| io::Error::other(format!("{}: {e}", path.display())))
}

/// Check the complete analysis interface before generation changes the target.
pub fn validate_analysis(project_dir: &Path, work_dir: &Path) -> io::Result<()> {
    let map: HashMap<String, String> = read_json(&work_dir.join("brinfo/name_map.json"))?;
    let context: Vec<RfocxtNameInformation> =
        read_json(&work_dir.join("focxt/impl_informations.json"))?;
    if map.is_empty() {
        return Err(io::Error::other("Analysis contains no focal functions"));
    }
    for (name, encoded) in &map {
        let data: BrData = read_json(
            &work_dir
                .join("brinfo/brdata")
                .join(format!("{encoded}.json")),
        )?;
        if data.name != *name {
            return Err(io::Error::other(format!(
                "Branch index mismatch for {name}"
            )));
        }
        let info = context
            .iter()
            .find(|info| info.full_name == *name)
            .ok_or_else(|| io::Error::other(format!("No focal context index for {name}")))?;
        let file = work_dir
            .join("focxt")
            .join(format!("{}.rs", info.encoded_name));
        let source_context = fs::read_to_string(&file).map_err(|error| {
            io::Error::other(format!(
                "Cannot read focal context for {name} ({}): {error}", file.display()
            ))
        })?;
        if source_context.trim().is_empty() {
            return Err(io::Error::other(format!("Empty focal context for {name}")));
        }
        let source = project_dir.join(data.loc.get_file());
        if !source.is_file() {
            return Err(io::Error::other(format!(
                "Missing source for {name}: {}",
                source.display()
            )));
        }
    }
    // Also reject orphaned branch files, which the generator would otherwise read.
    for entry in fs::read_dir(work_dir.join("brinfo/brdata"))? {
        let path = entry?.path();
        if path.is_file() {
            let data: BrData = read_json(&path)?;
            if !map.get(&data.name).is_some_and(|encoded| {
                path.file_stem()
                    .is_some_and(|stem| stem == encoded.as_str())
            }) {
                return Err(io::Error::other(format!(
                    "Unindexed branch artifact: {}",
                    path.display()
                )));
            }
        }
    }
    Ok(())
}

pub fn validate_repair(project_dir: &Path, work_dir: &Path) -> io::Result<()> {
    validate_analysis(project_dir, work_dir)?;
    for entry in fs::read_dir(project_dir.join("utgen/generation/pre_fix"))? {
        let path = entry?.path();
        if path.is_file() {
            let _: TestGenInfo = read_json(&path)?;
        }
    }
    Ok(())
}

fn run(name: &str, command: &mut Command) -> io::Result<()> {
    let description = format!("{command:?}");
    let start = Instant::now();
    let status = command
        .status()
        .map_err(|e| io::Error::other(format!("Cannot run {description}: {e}")))?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "{description} failed with {status}"
        )));
    }
    log::info!("Analysis step {name}: {:.3}s", start.elapsed().as_secs_f64());
    Ok(())
}

/// Analyze one fresh, prepared crate. Existing output directories are rejected
/// rather than mixing old indices with newly generated artifacts.
pub fn analyze_project(work_dir: &Path) -> io::Result<()> {
    if !work_dir.join("Cargo.toml").is_file() || !work_dir.join("src").is_dir() {
        return Err(io::Error::other(
            "Analysis requires a crate with Cargo.toml and src/",
        ));
    }
    for directory in ["brinfo", "focxt"] {
        if work_dir.join(directory).exists() {
            return Err(io::Error::other(format!(
                "{directory}/ already exists; analyze a fresh prepared working copy"
            )));
        }
    }
    // RUSTC_WRAPPER does not by itself invalidate Cargo's existing check cache.
    run("clean", Command::new("cargo").arg("clean").current_dir(work_dir))?;
    run("brinfo", Command::new("cargo").arg("brinfo").current_dir(work_dir))?;
    run("focxt", Command::new("focxt")
        .arg("-c")
        .arg(work_dir)
        .current_dir(work_dir))?;
    validate_analysis(work_dir, work_dir)
}
