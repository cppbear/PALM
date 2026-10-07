use crate::utils::test_ranges;
use cargo_metadata::{MetadataCommand, TargetKind};
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use syn::{Meta, Token, punctuated::Punctuated};
use walkdir::WalkDir;

fn has_coverage_feature(meta: &Meta) -> bool {
    let Meta::List(list) = meta else { return false };
    let Ok(args) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) else {
        return false;
    };
    if list.path.is_ident("feature") {
        args.iter()
            .any(|arg| arg.path().is_ident("coverage_attribute"))
    } else if list.path.is_ident("cfg_attr")
        && args.first().is_some_and(|arg| {
            arg.path().is_ident("coverage") || arg.path().is_ident("coverage_nightly")
        })
    {
        args.iter().skip(1).any(has_coverage_feature)
    } else {
        false
    }
}

fn annotate_source(source: &str, crate_root: bool) -> syn::Result<String> {
    let mut annotated = source.to_owned();
    // Reverse edits keep the original byte offsets valid. Inserting no newline
    // preserves focal line ranges, including production following a same-line test.
    for range in test_ranges(source)?.into_iter().rev() {
        annotated.insert_str(range.start, "#[coverage(off)] ");
    }
    if crate_root {
        let syntax = syn::parse_file(source)?;
        if !syntax
            .attrs
            .iter()
            .any(|attr| has_coverage_feature(&attr.meta))
        {
            let bom = if source.starts_with('\u{feff}') { 3 } else { 0 };
            let offset = if syntax.shebang.is_some() {
                source.find('\n').map_or(source.len(), |i| i + 1)
            } else {
                bom
            };
            annotated.insert_str(offset, "#![feature(coverage_attribute)] ");
        }
    }
    Ok(annotated)
}

fn coverage_command(work_dir: &Path, args: &[&str]) -> io::Result<Output> {
    let output = Command::new("cargo")
        .arg("llvm-cov")
        .args(args)
        .current_dir(work_dir)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "cargo llvm-cov {} failed ({}):\n{}\n{}",
            args.join(" "),
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
        )));
    }
    Ok(output)
}

/// Run tests once, excluding test code from instrumentation, then export XML
/// and optionally JSON from that run. Use a standalone crate working copy.
/// Test assertion failures are retained in the output; build/report errors fail.
pub fn collect_coverage(work_dir: &Path, json: bool) -> io::Result<Output> {
    let canonical_dir = work_dir.canonicalize()?;
    let work_dir = canonical_dir.as_path();
    let metadata = MetadataCommand::new()
        .manifest_path(work_dir.join("Cargo.toml"))
        .current_dir(work_dir)
        .no_deps()
        .exec()
        .map_err(io::Error::other)?;
    let package = metadata.root_package().ok_or_else(|| {
        io::Error::other("coverage requires a standalone crate with Cargo.toml and src/")
    })?;
    if metadata.workspace_members.len() != 1 || !work_dir.join("src").is_dir() {
        return Err(io::Error::other(
            "coverage currently supports one standalone crate",
        ));
    }
    let roots: BTreeSet<PathBuf> = package
        .targets
        .iter()
        .filter(|target| !target.is_kind(TargetKind::CustomBuild))
        .map(|target| target.src_path.clone().into_std_path_buf())
        .collect();
    let mut files = roots.clone();
    for entry in WalkDir::new(work_dir.join("src")) {
        let entry = entry.map_err(io::Error::other)?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "rs") {
            files.insert(entry.into_path());
        }
    }
    // Prepare all edits before writing. Keep original bytes in memory, without
    // sharing the generation pipeline's .bak files or introducing fingerprints.
    let mut changes = Vec::new();
    for path in files {
        let original = fs::read_to_string(&path)?;
        let annotated = annotate_source(&original, roots.contains(&path)).map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {err}", path.display()),
            )
        })?;
        if annotated != original {
            changes.push((path, original, annotated));
        }
    }
    let result = (|| {
        for (path, _, annotated) in &changes {
            fs::write(path, annotated)?;
        }
        // The first command uses llvm-cov's default cleanup. --no-report would
        // imply --no-clean and mix profiles from different generated candidates.
        let output = coverage_command(
            work_dir,
            &[
                "--tests",
                "--ignore-run-fail",
                "--branch",
                "--cobertura",
                "--output-path",
                "coverage.xml",
            ],
        )?;
        if json {
            coverage_command(
                work_dir,
                &["report", "--json", "--output-path", "coverage.json"],
            )?;
        }
        Ok(output)
    })();
    // Restore even when compilation, test launch, or report export failed.
    // Try every changed file before reporting a restoration error.
    let mut restore_error = None;
    for (path, original, _) in changes {
        if let Err(err) = fs::write(&path, original) {
            restore_error.get_or_insert_with(|| {
                io::Error::other(format!("failed to restore {}: {err}", path.display()))
            });
        }
    }
    if let Some(err) = restore_error {
        return Err(err);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modules_helpers_and_same_line_tests_keep_production_lines() {
        let source = "#[cfg(test)] mod tests { fn helper() {} #[test] fn a() {} }\r\n#[cfg(test)] fn helper() {}\r\n#[test] fn b() {} pub fn production() {}\r\n#[cfg(any(test, feature = \"shared\"))] mod shared {}\r\n";
        let result = annotate_source(source, true).unwrap();
        assert_eq!(result.matches("#[coverage(off)]").count(), 3);
        assert_eq!(result.matches('\r').count(), source.matches('\r').count());
        assert_eq!(result.lines().count(), source.lines().count());
        assert!(
            result
                .lines()
                .nth(2)
                .unwrap()
                .ends_with("pub fn production() {}")
        );
        assert!(result.lines().nth(3).unwrap().starts_with("#[cfg(any("));
        syn::parse_file(&result).unwrap();
    }

    #[test]
    fn existing_coverage_feature_is_not_duplicated() {
        for gate in [
            "#![feature(coverage_attribute)]",
            "#![cfg_attr(coverage_nightly, feature(coverage_attribute))]",
        ] {
            let source = format!("{gate}\n#[test] fn test() {{}}\n");
            let result = annotate_source(&source, true).unwrap();
            assert_eq!(result.matches("coverage_attribute").count(), 1);
        }
    }

    #[test]
    fn bom_and_shebang_precede_the_feature_gate() {
        let prefix = "\u{feff}#!/usr/bin/env rust-script\n";
        let source = format!("{prefix}#[test] fn test() {{}} fn keep() {{}}\n");
        let result = annotate_source(&source, true).unwrap();
        assert!(result.starts_with(&format!("{prefix}#![feature(coverage_attribute)]")));
        assert_eq!(result.lines().count(), source.lines().count());
        syn::parse_file(&result).unwrap();
    }
}
