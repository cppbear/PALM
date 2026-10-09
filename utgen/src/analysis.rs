use crate::target::{TargetInfo, targets};
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
        if (if data.id.is_empty() { &data.name } else { &data.id }) != name {
            return Err(io::Error::other(format!(
                "Branch index mismatch for {name}"
            )));
        }
        let info = context
            .iter()
            .find(|info| (if info.id.is_empty() { &info.full_name } else { &info.id }) == name)
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
            if !map.get(if data.id.is_empty() { &data.name } else { &data.id }).is_some_and(|encoded| {
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
    let targets = targets(work_dir)?;
    let mixed = targets.len() > 1;
    let mut collected = Vec::new();
    for target in &targets {
        let output = if mixed {
            work_dir
                .join("brinfo/targets")
                .join(&target.kind)
                .join(&target.name)
        } else {
            work_dir.to_owned()
        };
        // RUSTC_WRAPPER changes do not invalidate Cargo's existing check cache.
        run(
            "clean",
            Command::new("cargo").arg("clean").current_dir(work_dir),
        )?;
        let mut command = Command::new("cargo");
        command.arg("brinfo").current_dir(work_dir);
        target.configure_analysis(&mut command, &output);
        run("brinfo", &mut command)?;
        let mut command = Command::new("focxt");
        command.arg("-c").arg(work_dir).current_dir(work_dir);
        target.configure_analysis(&mut command, &output);
        if let Some(library) = targets.iter().find(|t| t.kind == "lib") {
            command.env("PALM_LIBRARY_NAME", library.name.replace('-', "_"));
            command.env(
                "PALM_LIBRARY_OUTPUT",
                if mixed {
                    work_dir.join("brinfo/targets/lib").join(&library.name)
                } else {
                    work_dir.to_owned()
                },
            );
        }
        run("focxt", &mut command)?;
        collected.push((target.clone(), output));
    }
    combine_targets(work_dir, &collected, mixed)?;
    validate_analysis(work_dir, work_dir)
}

fn normalized_location(loc: &str, work_dir: &Path) -> io::Result<String> {
    let parts: Vec<_> = loc.rsplitn(5, ':').collect();
    let file = work_dir.join(parts[4]).canonicalize()?;
    let file = file.strip_prefix(work_dir).unwrap_or(&file);
    Ok(format!(
        "{}:{}:{}:{}:{}",
        file.display(),
        parts[3],
        parts[2],
        parts[1],
        parts[0]
    ))
}

fn definition(info: &serde_json::Value, work_dir: &Path) -> io::Result<String> {
    let loc = normalized_location(info["loc"].as_str().unwrap(), work_dir)?;
    let implementation = info["impl_loc"]
        .as_str()
        .map(|loc| normalized_location(loc, work_dir))
        .transpose()?
        .unwrap_or_default();
    Ok(format!("{loc}|{}|{implementation}", info["fn_name"]))
}

/// Keep all raw contexts for dependencies, but select library representatives for generation.
fn combine_targets(
    work_dir: &Path,
    collected: &[(TargetInfo, std::path::PathBuf)],
    mixed: bool,
) -> io::Result<()> {
    use serde_json::Value;
    use std::collections::{BTreeMap, BTreeSet};
    let mut libraries = BTreeSet::new();
    let mut library_branches = BTreeSet::new();
    for (target, output) in collected {
        if target.kind == "lib" {
            let infos: Vec<Value> = read_json(&output.join("focxt/impl_informations.json"))?;
            let map: BTreeMap<String, String> = read_json(&output.join("brinfo/name_map.json"))?;
            for info in infos {
                let key = definition(&info, work_dir)?;
                if map.contains_key(info["full_name"].as_str().unwrap()) {
                    library_branches.insert(key.clone());
                }
                libraries.insert(key);
            }
        }
    }
    let mut final_map = BTreeMap::new();
    let mut final_infos = Vec::new();
    let mut shared = 0;
    fs::create_dir_all(work_dir.join("brinfo/brdata"))?;
    fs::create_dir_all(work_dir.join("focxt"))?;
    for (target, output) in collected {
        let map: BTreeMap<String, String> = read_json(&output.join("brinfo/name_map.json"))?;
        let infos: Vec<Value> = read_json(&output.join("focxt/impl_informations.json"))?;
        for (name, encoded) in map {
            let mut info = infos
                .iter()
                .find(|info| info["full_name"].as_str() == Some(&name))
                .ok_or_else(|| {
                    io::Error::other(format!(
                        "Missing context index for {} in {}",
                        name,
                        target.key()
                    ))
                })?
                .clone();
            if target.kind == "bin" && libraries.contains(&definition(&info, work_dir)?) {
                if !library_branches.contains(&definition(&info, work_dir)?) {
                    return Err(io::Error::other(format!(
                        "Shared function {name} has no library branch analysis; cannot substitute its binary version"
                    )));
                }
                shared += 1;
                continue;
            }
            let id = if mixed {
                format!("{}::{name}", target.key())
            } else {
                name.clone()
            };
            let final_encoded = if mixed {
                format!("{}_{}_{}", target.kind, target.name, encoded)
            } else {
                encoded.clone()
            };
            let mut data: Value =
                read_json(&output.join("brinfo/brdata").join(format!("{encoded}.json")))?;
            data["id"] = id.clone().into();
            data["target"] = serde_json::to_value(target)?;
            info["id"] = id.clone().into();
            info["target"] = serde_json::to_value(target)?;
            info["encoded_name"] = final_encoded.clone().into();
            fs::write(
                work_dir
                    .join("brinfo/brdata")
                    .join(format!("{final_encoded}.json")),
                serde_json::to_vec_pretty(&data)?,
            )?;
            if mixed {
                fs::copy(
                    output.join("focxt").join(format!("{encoded}.rs")),
                    work_dir.join("focxt").join(format!("{final_encoded}.rs")),
                )?;
            }
            final_map.insert(id, final_encoded);
            final_infos.push(info);
        }
    }
    final_infos.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
    log::info!(
        "Selected {} focal functions; {shared} shared binary definitions use their library representative",
        final_map.len()
    );
    fs::write(
        work_dir.join("brinfo/name_map.json"),
        serde_json::to_vec_pretty(&final_map)?,
    )?;
    fs::write(
        work_dir.join("focxt/impl_informations.json"),
        serde_json::to_vec_pretty(&final_infos)?,
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn aggregation_preserves_distinct_definitions_and_prefers_library_in_either_order() {
        let root = std::env::temp_dir().join(format!(
            "palm-ownership-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("src")).unwrap();
        let root = root.canonicalize().unwrap();
        for name in ["shared", "lib", "main"] {
            fs::write(root.join(format!("src/{name}.rs")), "fn function() {}\n").unwrap();
        }
        let mut collected = Vec::new();
        for (kind, entry) in [("lib", "lib"), ("bin", "main")] {
            let output = root.join("raw").join(kind);
            fs::create_dir_all(output.join("brinfo/brdata")).unwrap();
            fs::create_dir_all(output.join("focxt")).unwrap();
            let mut infos = Vec::new();
            let mut map = std::collections::BTreeMap::new();
            for (name, file) in [("shared", "shared"), ("same_name", entry)] {
                let full_name = format!("demo::{name}");
                map.insert(full_name.clone(), name);
                infos.push(
                    json!({"fn_name": name, "full_name": full_name, "encoded_name": name,
                    "loc": format!("src/{file}.rs:1:1:1:17"), "impl_loc": null}),
                );
                fs::write(
                    output.join(format!("brinfo/brdata/{name}.json")),
                    json!({"name": full_name}).to_string(),
                )
                .unwrap();
                fs::write(
                    output.join(format!("focxt/{name}.rs")),
                    format!("{kind} {name}"),
                )
                .unwrap();
            }
            fs::write(
                output.join("brinfo/name_map.json"),
                serde_json::to_vec(&map).unwrap(),
            )
            .unwrap();
            fs::write(
                output.join("focxt/impl_informations.json"),
                serde_json::to_vec(&infos).unwrap(),
            )
            .unwrap();
            collected.push((
                TargetInfo {
                    kind: kind.into(),
                    name: "demo".into(),
                    src_path: format!("src/{entry}.rs"),
                },
                output,
            ));
        }
        combine_targets(&root, &collected, true).unwrap();
        let first_map = fs::read(root.join("brinfo/name_map.json")).unwrap();
        let first_infos = fs::read(root.join("focxt/impl_informations.json")).unwrap();
        collected.reverse();
        combine_targets(&root, &collected, true).unwrap();
        assert_eq!(
            first_map,
            fs::read(root.join("brinfo/name_map.json")).unwrap()
        );
        assert_eq!(
            first_infos,
            fs::read(root.join("focxt/impl_informations.json")).unwrap()
        );
        let map: std::collections::BTreeMap<String, String> =
            serde_json::from_slice(&first_map).unwrap();
        assert_eq!(map.len(), 3);
        assert!(!map.contains_key("bin:demo::demo::shared"));
        for (id, text) in [
            ("lib:demo::demo::shared", "lib shared"),
            ("lib:demo::demo::same_name", "lib same_name"),
            ("bin:demo::demo::same_name", "bin same_name"),
        ] {
            assert_eq!(
                fs::read_to_string(root.join("focxt").join(format!("{}.rs", map[id]))).unwrap(),
                text
            );
        }
        // An unavailable library analysis must not silently select a binary variant.
        let library = &collected
            .iter()
            .find(|(target, _)| target.kind == "lib")
            .unwrap()
            .1;
        fs::write(
            library.join("brinfo/name_map.json"),
            json!({"demo::same_name": "same_name"}).to_string(),
        )
        .unwrap();
        assert!(
            combine_targets(&root, &collected, true)
                .unwrap_err()
                .to_string()
                .contains("cannot substitute")
        );
        fs::remove_dir_all(root).unwrap();
    }
}
