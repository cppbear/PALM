use cargo_metadata::{MetadataCommand, TargetKind};
use serde::{Deserialize, Serialize};
use std::{io, path::Path, process::Command};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TargetInfo {
    pub kind: String,
    pub name: String,
    pub src_path: String,
}

impl TargetInfo {
    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.name)
    }

    pub fn unit_args(&self) -> Vec<&str> {
        if self.kind == "lib" {
            vec!["--lib"]
        } else {
            vec!["--bin", &self.name]
        }
    }

    pub fn configure_analysis(&self, command: &mut Command, output: &Path) {
        command
            .env("PALM_TARGET_KIND", &self.kind)
            .env("PALM_TARGET_NAME", &self.name)
            .env("PALM_OUTPUT_DIR", output);
    }
}

pub fn targets(work_dir: &Path) -> io::Result<Vec<TargetInfo>> {
    let metadata = MetadataCommand::new()
        .manifest_path(work_dir.join("Cargo.toml"))
        .current_dir(work_dir)
        .no_deps()
        .exec()
        .map_err(io::Error::other)?;
    let package = metadata
        .root_package()
        .ok_or_else(|| io::Error::other("A standalone Cargo package is required"))?;
    if metadata.workspace_members.len() != 1 {
        return Err(io::Error::other(
            "Analysis currently supports one standalone Cargo package",
        ));
    }
    let mut targets: Vec<_> = package
        .targets
        .iter()
        .filter_map(|target| {
            let kind = if target.is_kind(TargetKind::Lib) {
                "lib"
            } else if target.is_kind(TargetKind::Bin) {
                "bin"
            } else {
                return None;
            };
            Some(TargetInfo {
                kind: kind.into(),
                name: target.name.clone(),
                src_path: target
                    .src_path
                    .as_std_path()
                    .strip_prefix(work_dir)
                    .unwrap_or(target.src_path.as_std_path())
                    .to_string_lossy()
                    .into_owned(),
            })
        })
        .collect();
    targets.sort_by_key(|target| (target.kind != "lib", target.name.clone()));
    if targets.is_empty() {
        return Err(io::Error::other(
            "No ordinary library or binary target found",
        ));
    }
    Ok(targets)
}

/// Old single-target caches can still be used; mixed caches need recorded ownership.
pub fn resolve_target(work_dir: &Path, recorded: Option<&TargetInfo>) -> io::Result<TargetInfo> {
    resolve_from(&targets(work_dir)?, recorded)
}

pub fn resolve_from(
    available: &[TargetInfo],
    recorded: Option<&TargetInfo>,
) -> io::Result<TargetInfo> {
    if let Some(recorded) = recorded {
        return available
            .iter()
            .find(|target| *target == recorded)
            .cloned()
            .ok_or_else(|| {
                io::Error::other(format!(
                    "Recorded target {} changed; analyze a fresh working copy",
                    recorded.key()
                ))
            });
    }
    if available.len() == 1 {
        return Ok(available[0].clone());
    }
    Err(io::Error::other(
        "Mixed-target results lack target ownership; analyze and generate in a fresh working copy",
    ))
}
