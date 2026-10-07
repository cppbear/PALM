use crate::{
    types::{InsertKind, TestGenInfo},
    utils::{backup_file, delete_backup, insert_test, restore_file},
};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
struct GeneratedTestFn {
    name: String,
    attrs: Vec<String>,
    body: Vec<String>,
}

#[derive(Debug, Clone)]
struct FileInjectionPlan {
    file_path: PathBuf,
    insert_kind: InsertKind,
    common_lines: Vec<String>,
    tests: Vec<GeneratedTestFn>,
}

fn sanitize_attrs(attrs: &[String]) -> Vec<String> {
    let mut out = Vec::new();

    for attr in attrs {
        if attr.contains("timeout") {
            continue;
        }
        if attr.contains("#[should_panic(") {
            out.push("#[should_panic]".to_string());
        } else {
            out.push(attr.clone());
        }
    }

    out
}

fn normalize_common_lines(lines: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();

    for line in lines {
        let trimmed = line.trim().to_string();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.contains("timeout") {
            continue;
        }
        if seen.insert(trimmed.clone()) {
            out.push(trimmed);
        }
    }

    out
}

fn test_body_needs_alloc(body: &[String]) -> bool {
    return false;
    body.iter().any(|line| {
        line.contains("String")
            || line.contains(".to_string()")
            || line.contains("Vec<")
            || line.contains("vec![")
            || line.contains("format!")
    })
}

fn build_generated_module(plan: &FileInjectionPlan) -> Vec<String> {
    let mut lines = Vec::new();

    lines.push("#[cfg(test)]".to_string());
    lines.push("mod __utgen_generated_tests {".to_string());
    lines.push("    use super::*;".to_string());
    lines.push("    use ntest::timeout;".to_string());

    let needs_alloc = plan.tests.iter().any(|t| test_body_needs_alloc(&t.body));

    if needs_alloc {
        lines.push("    extern crate alloc;".to_string());
        lines.push("    use alloc::string::{String, ToString};".to_string());
        lines.push("    use alloc::vec::Vec;".to_string());
        lines.push("    use alloc::format;".to_string());
    }

    for common in &plan.common_lines {
        lines.push(format!("    {}", common));
    }

    if !plan.common_lines.is_empty() {
        lines.push(String::new());
    }

    for test_fn in &plan.tests {
        for attr in &test_fn.attrs {
            lines.push(format!("    {}", attr));
        }
        lines.push(format!("    fn {}()", test_fn.name));

        for body_line in &test_fn.body {
            if body_line.is_empty() {
                lines.push(String::new());
            } else {
                lines.push(format!("    {}", body_line));
            }
        }

        lines.push(String::new());
    }

    lines.push("}".to_string());
    lines
}

fn build_all_unit_test_injection_plans(
    project_dir: &Path,
    test_gen_infos: &[TestGenInfo],
) -> HashMap<PathBuf, FileInjectionPlan> {
    let mut plans: HashMap<PathBuf, FileInjectionPlan> = HashMap::new();
    let mut used_test_names: HashSet<String> = HashSet::new();

    for test_gen_info in test_gen_infos {
        let file_rela = test_gen_info.get_file();
        let file_path = project_dir.join(&file_rela);

        let function_name = test_gen_info.get_name().to_string();
        let fn_tail = function_name
            .split("::")
            .last()
            .unwrap_or("unknown")
            .replace(|c: char| !c.is_ascii_alphanumeric(), "_");

        let insert_kind = test_gen_info.get_insert_kind();

        let plan = plans.entry(file_path.clone()).or_insert_with(|| FileInjectionPlan {
            file_path: file_path.clone(),
            insert_kind: insert_kind,
            common_lines: Vec::new(),
            tests: Vec::new(),
        });

        let mut local_index = 0usize;

        for chain_test in test_gen_info.get_tests().iter() {
            for answer in chain_test.get_answers().iter() {
                let commons = normalize_common_lines(&answer.get_common());
                for common in commons {
                    if !plan.common_lines.contains(&common) {
                        plan.common_lines.push(common);
                    }
                }

                for test in answer.get_tests().iter() {
                    for (num, code) in test.codes.iter().enumerate() {
                        if !test.can_compile[num].is_ok() {
                            continue;
                        }

                        let attrs = sanitize_attrs(&test.attrs);

                        let mut candidate_name =
                            format!("ut_{}_{}_{}", fn_tail, local_index, num);
                        while used_test_names.contains(&candidate_name) {
                            local_index += 1;
                            candidate_name =
                                format!("ut_{}_{}_{}", fn_tail, local_index, num);
                        }
                        used_test_names.insert(candidate_name.clone());

                        plan.tests.push(GeneratedTestFn {
                            name: candidate_name,
                            attrs: attrs
                                .into_iter()
                                .filter(|a| a.trim() != "#[test]")
                                .collect::<Vec<_>>()
                                .into_iter()
                                .fold(vec!["#[test]".to_string(), "#[cfg_attr(coverage_nightly, coverage(off))]".to_string()], |mut acc, a| {
                                    acc.push(a);
                                    acc
                                }),
                            body: code.clone(),
                        });

                        local_index += 1;
                    }
                }
            }
        }
    }

    plans
}

fn inject_all_unit_tests_once(
    plans: &HashMap<PathBuf, FileInjectionPlan>,
) -> Vec<PathBuf> {
    let mut touched_files = Vec::new();

    for (file_path, plan) in plans {
        if plan.tests.is_empty() {
            continue;
        }

        info!(
            "Injecting {} generated tests into {}",
            plan.tests.len(),
            file_path.display()
        );

        backup_file(file_path);

        let mod_code = build_generated_module(plan);
        insert_test(plan.insert_kind.clone(), file_path, &mod_code);

        touched_files.push(file_path.clone());
    }

    touched_files
}

pub fn prepare_all_unit_tests_for_one_shot_run(
    project_dir: &Path,
    test_gen_infos: &[TestGenInfo],
    manifest_path: Option<&Path>,
) -> Vec<PathBuf> {
    let plans = build_all_unit_test_injection_plans(project_dir, test_gen_infos);


    let touched_files = inject_all_unit_tests_once(&plans);

    info!(
        "Prepared one-shot injection for {} files",
        touched_files.len()
    );

    touched_files
}