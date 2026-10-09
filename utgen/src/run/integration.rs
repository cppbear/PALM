use std::{
    collections::{BTreeSet, HashSet},
    fs, io,
    path::{Path, PathBuf},
    process::{Command, Output},
};

use log::info;
use quote::ToTokens;
use syn::spanned::Spanned;

use crate::{
    run::timed_test,
    types::{ChainTestAnswer, IntegrationContext, TestGenInfo, TestInfo},
    utils::{TemporaryFile, cargo_check_test, use_check},
};

pub(crate) const CANDIDATE_TARGET: &str = "palm_candidate";

pub(crate) fn candidate_path(work_dir: &Path) -> PathBuf {
    work_dir.join(format!("tests/{CANDIDATE_TARGET}.rs"))
}

pub(crate) fn write_candidate(work_dir: &Path, code: &[String]) -> io::Result<TemporaryFile> {
    let path = candidate_path(work_dir);
    fs::create_dir_all(path.parent().unwrap())?;
    let temporary = TemporaryFile::new(&path)?;
    fs::write(&path, code.join("\n"))?;
    Ok(temporary)
}

pub(crate) fn compile_candidate(work_dir: &Path) -> io::Result<Output> {
    Command::new("cargo")
        .args([
            "test",
            "--test",
            CANDIDATE_TARGET,
            "--no-run",
            "--message-format",
            "json",
        ])
        .current_dir(work_dir)
        .output()
}

pub(crate) fn initial_uses(test_gen: &TestGenInfo, work_dir: &Path) -> Vec<String> {
    let mut uses: HashSet<_> = test_gen.get_use_path().into_iter().collect();
    uses.remove("use super::*;");
    uses.insert("use ntest::timeout;".to_string());
    use_check(&mut uses, work_dir);
    uses.into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

pub(crate) fn candidate_context(
    answer: &ChainTestAnswer,
    test: &TestInfo,
    num: usize,
    initial: &[String],
) -> IntegrationContext {
    test.integration_contexts
        .get(num)
        .cloned()
        .unwrap_or_else(|| IntegrationContext {
            uses: initial
                .iter()
                .chain(answer.get_uses())
                .cloned()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            common: answer.get_common().clone(),
        })
}

pub(crate) fn candidate_code(
    context: &IntegrationContext,
    attrs: &[String],
    body: &[String],
    module: &str,
    name: &str,
    timed: bool,
) -> Vec<String> {
    let mut code = vec![format!("mod {module} {{")];
    code.extend(context.uses.clone());
    code.extend(context.common.clone());
    let signature = format!("fn {name}()");
    if timed {
        code.extend(timed_test(&signature, attrs, body));
    } else {
        code.push("#[test]".to_string());
        code.extend_from_slice(attrs);
        code.push(signature);
        code.extend_from_slice(body);
    }
    code.push("}".to_string());
    code
}

// Keep imports and helpers at module scope. Only the single candidate's body
// and supporting items may change; its test declaration must remain intact.
pub(crate) fn repaired_candidate(
    code: &[String],
    attrs: &[String],
) -> Option<(IntegrationContext, Vec<String>)> {
    let source = code.join("\n");
    let syntax = syn::parse_file(&source).ok()?;
    let [syn::Item::Mod(module)] = syntax.items.as_slice() else {
        return None;
    };
    if module.ident != "palm_candidate" || !module.attrs.is_empty() || !syntax.attrs.is_empty() {
        return None;
    }
    let (_, items) = module.content.as_ref()?;
    let expected = syn::parse_str::<syn::ItemFn>(&format!(
        "#[test]\n{}\nfn test_candidate() {{}}",
        attrs.join("\n")
    ))
    .ok()?;
    let mut context = IntegrationContext {
        uses: Vec::new(),
        common: Vec::new(),
    };
    let mut body = None;
    for item in items {
        let text = &source[item.span().byte_range()];
        match item {
            syn::Item::Fn(function)
                if function
                    .attrs
                    .iter()
                    .any(|attr| attr.path().is_ident("test")) =>
            {
                if body.is_some()
                    || function.sig.to_token_stream().to_string()
                        != expected.sig.to_token_stream().to_string()
                    || function
                        .attrs
                        .iter()
                        .map(ToTokens::to_token_stream)
                        .map(|t| t.to_string())
                        .collect::<Vec<_>>()
                        != expected
                            .attrs
                            .iter()
                            .map(ToTokens::to_token_stream)
                            .map(|t| t.to_string())
                            .collect::<Vec<_>>()
                {
                    return None;
                }
                body = Some(
                    source[function.block.span().byte_range()]
                        .lines()
                        .map(str::to_owned)
                        .collect(),
                );
            }
            syn::Item::Use(_) => context.uses.push(text.to_owned()),
            _ => context.common.extend(text.lines().map(str::to_owned)),
        }
    }
    Some((context, body?))
}

pub struct IntegrationInfo {
    pub function_name: String,
    pub file_name: String,
    pub test_functions: Vec<(String, usize)>, // (complete libtest name, oracle group)
    pub tests: i32,
    pub tests_lines: Vec<i32>,
    pub oracles: i32,
    pub oracles_compiled: i32,
    pub oracles_compiled_rate: f64,
    pub tests_compiled: i32,
    pub tests_compiled_rate: f64,
}

impl IntegrationInfo {
    /// Keep the existing run-count convention (registered tests, including ignored).
    /// Count a group once if any of its candidates was registered/passed.
    pub fn execution_counts(&self, output: &[String]) -> (i32, i32, i32, i32) {
        let passed: HashSet<_> = output
            .iter()
            .filter_map(|line| {
                let (name, outcome) = line.strip_prefix("test ")?.split_once(" ... ")?;
                (outcome == "ok").then_some(name)
            })
            .collect();
        let mut run_groups = BTreeSet::new();
        let mut passed_groups = BTreeSet::new();
        let mut passed_tests = 0;
        for (name, group) in &self.test_functions {
            run_groups.insert(group);
            if passed.contains(name.as_str()) {
                passed_tests += 1;
                passed_groups.insert(group);
            }
        }
        (
            self.test_functions.len() as i32,
            passed_tests,
            run_groups.len() as i32,
            passed_groups.len() as i32,
        )
    }

    fn new() -> Self {
        IntegrationInfo {
            function_name: String::new(),
            file_name: String::new(),
            test_functions: Vec::new(),
            tests: 0,
            tests_lines: Vec::new(),
            oracles: 0,
            oracles_compiled: 0,
            oracles_compiled_rate: 0.0,
            tests_compiled: 0,
            tests_compiled_rate: 0.0,
        }
    }
}

pub fn gen_integration(test_gen_infos: &Vec<TestGenInfo>, work_dir: &Path) -> Vec<IntegrationInfo> {
    info!("Generate integration tests!");
    let mut integration_infos = Vec::new();
    for test_gen in test_gen_infos.iter().filter(|info| info.get_visibility()) {
        let file = test_gen.get_file();
        let stem = Path::new(&file).file_stem().unwrap().to_str().unwrap();
        let name = test_gen.get_name();
        let rename = test_gen.rust_name()
            .replace("::", "_")
            .replace(['{', '}'], "")
            .replace('#', "_");
        let mut result = IntegrationInfo::new();
        result.function_name = name.to_string();
        result.file_name = format!("test_{stem}_{rename}");
        let path = work_dir.join(format!("tests/{}.rs", result.file_name));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let initial = initial_uses(test_gen, work_dir);
        let mut content = Vec::new();
        for chain in test_gen.get_tests() {
            for answer in chain.get_answers() {
                for test in answer.get_tests() {
                    result.oracles += 1;
                    let mut compiled = false;
                    for (num, body) in test.codes.iter().enumerate() {
                        result.tests += 1;
                        result.tests_lines.push(body.len() as i32);
                        if test.can_compile[num].is_ok() {
                            compiled = true;
                            let id = result.tests_compiled;
                            let test_name = format!("test_{stem}_{rename}_{id:02}");
                            let context = candidate_context(answer, test, num, &initial);
                            content.extend(candidate_code(
                                &context,
                                &test.attrs,
                                body,
                                &format!("candidate_{id}"),
                                &test_name,
                                true,
                            ));
                            result.test_functions.push((format!("candidate_{id}::{test_name}"), result.oracles as usize - 1));
                            result.tests_compiled += 1;
                        }
                    }
                    result.oracles_compiled += i32::from(compiled);
                }
            }
        }
        if result.tests > 0 {
            result.tests_compiled_rate = result.tests_compiled as f64 / result.tests as f64 * 100.0;
        }
        if result.oracles > 0 {
            result.oracles_compiled_rate =
                result.oracles_compiled as f64 / result.oracles as f64 * 100.0;
        }
        // Also replace an older file when no candidate still compiles.
        fs::write(&path, content.join("\n")).unwrap();
        cargo_check_test(work_dir, &result.file_name)
            .expect("generated integration tests must compile");
        integration_infos.push(result);
    }
    integration_infos
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_counts_use_complete_names_and_distinct_oracle_groups() {
        let mut info = IntegrationInfo::new();
        info.test_functions = vec![
            ("candidate_10::test_10".into(), 0),
            ("candidate_100::test_100".into(), 0),
            ("candidate_2::test_2".into(), 1),
        ];
        let output = [
            "test candidate_100::test_100 ... ok",
            "test candidate_10::test_10 ... FAILED",
            "test candidate_2::test_2 ... ignored",
            "test unrelated::test_10 ... ok",
        ]
        .map(str::to_owned);
        assert_eq!(info.execution_counts(&output), (3, 1, 2, 1));
        assert_eq!(
            IntegrationInfo::new().execution_counts(&output),
            (0, 0, 0, 0)
        );
    }

    #[test]
    fn repair_cannot_disable_the_candidate_to_hide_a_compile_error() {
        let context = IntegrationContext {
            uses: vec![],
            common: vec![],
        };
        let code = candidate_code(
            &context,
            &[],
            &["{ missing(); }".to_string()],
            "palm_candidate",
            "test_candidate",
            false,
        );
        assert!(repaired_candidate(&code, &[]).is_some());
        // Rust would accept this by compiling no tests. It must not be saved
        // as a successful repair and then reconstructed without the cfg.
        let mut disabled = code.clone();
        disabled.insert(0, "#[cfg(any())]".to_string());
        assert!(repaired_candidate(&disabled, &[]).is_none());
        let removed = code
            .into_iter()
            .filter(|line| line != "#[test]")
            .collect::<Vec<_>>();
        assert!(repaired_candidate(&removed, &[]).is_none());
    }
}
