use crate::utils::{is_test_function, is_test_only, parse_source, source_span_offset};
use cargo_metadata::{MetadataCommand, TargetKind};
use quote::quote;
use std::{
    collections::BTreeSet,
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use syn::{Meta, Token, punctuated::Punctuated, spanned::Spanned, visit::Visit};
use walkdir::WalkDir;

#[derive(Clone, Copy)]
enum CoverageAttribute {
    Exclude,
    Feature,
}

// Collect the conditions under which an attribute already exists. Leave cfg
// evaluation to rustc, including test/non-test builds and Cargo feature flags.
fn attribute_conditions(
    meta: &Meta,
    kind: CoverageAttribute,
    conditions: &mut Vec<Meta>,
    found: &mut Vec<Vec<Meta>>,
) {
    let Meta::List(list) = meta else { return };
    let Ok(args) = list.parse_args_with(Punctuated::<Meta, Token![,]>::parse_terminated) else {
        return;
    };
    if list.path.is_ident("cfg_attr") {
        let mut args = args.into_iter();
        if let Some(condition) = args.next() {
            conditions.push(condition);
            for attribute in args {
                attribute_conditions(&attribute, kind, conditions, found);
            }
            conditions.pop();
        }
    } else {
        let matches = match kind {
            // Respect explicit coverage settings, including coverage(on).
            CoverageAttribute::Exclude => list.path.is_ident("coverage"),
            CoverageAttribute::Feature => {
                list.path.is_ident("feature")
                    && args
                        .iter()
                        .any(|arg| arg.path().is_ident("coverage_attribute"))
            }
        };
        if matches {
            found.push(conditions.clone());
        }
    }
}

fn missing_attribute(attrs: &[syn::Attribute], kind: CoverageAttribute, inner: bool) -> String {
    let mut found = Vec::new();
    for attr in attrs {
        attribute_conditions(&attr.meta, kind, &mut Vec::new(), &mut found);
    }
    if found.iter().any(Vec::is_empty) {
        return String::new(); // Already unconditional.
    }
    let attribute = match kind {
        CoverageAttribute::Exclude => quote!(coverage(off)),
        CoverageAttribute::Feature => quote!(feature(coverage_attribute)),
    };
    let attribute = if found.is_empty() {
        attribute
    } else {
        let conditions = found.iter().map(|path| quote!(all(#(#path),*)));
        quote!(cfg_attr(not(any(#(#conditions),*)), #attribute))
    };
    format!("#{}[{}] ", if inner { "!" } else { "" }, attribute)
}

#[derive(Default)]
struct TestAnnotations {
    insertions: Vec<(usize, String)>,
    test_impl: bool,
}

impl TestAnnotations {
    fn exclude(&mut self, attrs: &[syn::Attribute], position: usize) {
        let attribute = missing_attribute(attrs, CoverageAttribute::Exclude, false);
        if !attribute.is_empty() {
            self.insertions.push((position, attribute));
        }
    }
}

impl<'ast> Visit<'ast> for TestAnnotations {
    fn visit_item_mod(&mut self, item: &'ast syn::ItemMod) {
        if is_test_only(&item.attrs) {
            self.exclude(&item.attrs, item.span().byte_range().start);
        } else {
            syn::visit::visit_item_mod(self, item);
        }
    }
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        if is_test_function(&item.attrs) {
            self.exclude(&item.attrs, item.span().byte_range().start);
        } else {
            syn::visit::visit_item_fn(self, item);
        }
    }
    fn visit_item_impl(&mut self, item: &'ast syn::ItemImpl) {
        let previous = self.test_impl;
        self.test_impl |= is_test_only(&item.attrs);
        syn::visit::visit_item_impl(self, item);
        self.test_impl = previous;
    }
    fn visit_impl_item_fn(&mut self, item: &'ast syn::ImplItemFn) {
        if self.test_impl || is_test_function(&item.attrs) {
            self.exclude(&item.attrs, item.span().byte_range().start);
        } else {
            syn::visit::visit_impl_item_fn(self, item);
        }
    }
}

fn annotate_source(source: &str, crate_root: bool) -> syn::Result<String> {
    let Some(syntax) = parse_source(source, crate_root)? else {
        return Ok(source.to_owned());
    };
    let mut annotated = source.to_owned();
    let mut file_attributes = String::new();
    if crate_root {
        file_attributes.push_str(&missing_attribute(
            &syntax.attrs,
            CoverageAttribute::Feature,
            true,
        ));
    }
    if is_test_only(&syntax.attrs) {
        file_attributes.push_str(&missing_attribute(
            &syntax.attrs,
            CoverageAttribute::Exclude,
            true,
        ));
    } else {
        let mut visitor = TestAnnotations::default();
        visitor.visit_file(&syntax);
        let offset = source_span_offset(source, &syntax);
        // Reverse edits keep original offsets valid; no newline is inserted.
        for (position, attribute) in visitor.insertions.into_iter().rev() {
            annotated.insert_str(position + offset, &attribute);
        }
    }
    let prefix_end = if syntax.shebang.is_some() {
        source.find('\n').map_or(source.len(), |i| i + 1)
    } else if source.starts_with('\u{feff}') {
        '\u{feff}'.len_utf8()
    } else {
        0
    };
    annotated.insert_str(prefix_end, &file_attributes);
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
    collect_coverage_for_tests(work_dir, json, &[])
}

pub(crate) fn project_metadata(work_dir: &Path) -> io::Result<cargo_metadata::Metadata> {
    let metadata = MetadataCommand::new()
        .manifest_path(work_dir.join("Cargo.toml"))
        .current_dir(work_dir)
        .no_deps()
        .exec()
        .map_err(io::Error::other)?;
    metadata.root_package().ok_or_else(|| {
        io::Error::other("coverage requires a standalone crate with Cargo.toml and src/")
    })?;
    if metadata.workspace_members.len() != 1 || !work_dir.join("src").is_dir() {
        return Err(io::Error::other(
            "coverage currently supports one standalone crate",
        ));
    }
    Ok(metadata)
}

pub(crate) fn with_test_exclusions<T>(
    work_dir: &Path,
    metadata: &cargo_metadata::Metadata,
    action: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let package = metadata.root_package().unwrap();
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
        action()
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

pub(crate) fn collect_coverage_for_tests(
    work_dir: &Path,
    json: bool,
    targets: &[String],
) -> io::Result<Output> {
    let canonical = work_dir.canonicalize()?;
    let work_dir = canonical.as_path();
    let metadata = project_metadata(work_dir)?;
    with_test_exclusions(work_dir, &metadata, || {
        // The first command uses llvm-cov's default cleanup. --no-report would
        // imply --no-clean and mix profiles from different generated candidates.
        let mut args = Vec::new();
        if targets.is_empty() {
            args.push("--tests");
        } else {
            for target in targets {
                args.extend(["--test", target.as_str()]);
            }
        }
        args.extend([
            "--ignore-run-fail",
            "--branch",
            "--cobertura",
            "--output-path",
            "coverage.xml",
        ]);
        let output = coverage_command(work_dir, &args)?;
        if json {
            coverage_command(
                work_dir,
                &["report", "--json", "--output-path", "coverage.json"],
            )?;
        }
        Ok(output)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modules_helpers_and_same_line_tests_keep_production_lines() {
        let source = "#[cfg(test)] mod tests { fn helper() {} #[test] fn a() {} }\r\n#[cfg(test)] fn helper() {}\r\n#[test] fn b() {} pub fn production() {}\r\n#[cfg(any(test, feature = \"shared\"))] mod shared {}\r\n";
        let result = annotate_source(source, true).unwrap();
        assert_eq!(result.matches("#[coverage (off)]").count(), 3);
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
    fn unconditional_attributes_are_reused() {
        for setting in ["off", "on"] {
            let source = format!(
                "#![feature(coverage_attribute)]\n#[cfg(test)] #[coverage({setting})] mod tests {{}}\n"
            );
            assert_eq!(annotate_source(&source, true).unwrap(), source);
        }
    }

    #[test]
    fn expression_fragments_are_preserved_but_not_accepted_as_entry_files() {
        assert_eq!(annotate_source("42\n", false).unwrap(), "42\n");
        assert!(annotate_source("42\n", true).is_err());
        assert!(annotate_source("fn broken( {", false).is_err());
    }

    #[test]
    fn bom_and_shebang_precede_the_feature_gate() {
        let prefix = "\u{feff}#!/usr/bin/env rust-script\n";
        let source = format!("{prefix}#[test] fn test() {{}} fn keep() {{}}\n");
        let result = annotate_source(&source, true).unwrap();
        assert!(result.starts_with(&format!("{prefix}#![feature (coverage_attribute)]")));
        assert_eq!(result.lines().count(), source.lines().count());
        syn::parse_file(&result).unwrap();
    }
}
