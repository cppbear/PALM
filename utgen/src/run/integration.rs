use std::{collections::HashSet, fs, path::Path};

use log::info;

use crate::{
    run::TIMEOUT_DERIVE,
    types::TestGenInfo,
    utils::{cargo_check, use_check},
};

pub struct IntegrationInfo {
    pub function_name: String,
    pub file_name: String,
    pub test_function_names: Vec<String>,
    pub tests: i32,
    pub tests_lines: Vec<i32>,
    pub oracles: i32,
    pub oracles_compiled: i32,
    pub oracles_compiled_rate: f64,
    pub tests_compiled: i32,
    pub tests_compiled_rate: f64,
}

impl IntegrationInfo {
    fn new() -> Self {
        IntegrationInfo {
            function_name: String::new(),
            file_name: String::new(),
            test_function_names: Vec::new(),
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

fn backup_integration(work_dir: &Path) {
    let test_path = work_dir.join("/tests");
    let bak_test_path = work_dir.join("/test_bak");
    if fs::exists(&test_path).unwrap() {
        fs::rename(&test_path, &bak_test_path).unwrap();
    }
}

pub fn gen_integration(
    test_gen_infos: &Vec<TestGenInfo>,
    project_dir: &Path,
    work_dir: &Path,
) -> Vec<IntegrationInfo> {
    // backup_integration(work_dir);
    info!("Generate integration tests!");
    let mut integration_infos: Vec<IntegrationInfo> = Vec::new();
    for test_gen_info in test_gen_infos.iter() {
        if test_gen_info.get_visibility() {
            let file_path = test_gen_info.get_file();
            let stem = Path::new(&file_path).file_stem().unwrap().to_str().unwrap();
            let name = test_gen_info.get_name().to_string();
            let fn_name = name.split("::").last().unwrap();
            let rename = name
                .replace("::", "_")
                .replace("{", "")
                .replace("}", "")
                .replace("#", "_");
            let path = work_dir.join(format!("tests/test_{}_{}.rs", stem, rename));
            let mut integration_info = IntegrationInfo::new();
            integration_info.function_name = name.clone();
            integration_info.file_name = format!("{}_{}", stem, rename);
            let mut file_content = String::new();
            if !fs::exists(&path.parent().unwrap()).unwrap() {
                fs::create_dir_all(&path.parent().unwrap()).unwrap();
            }
            let mut use_set = test_gen_info
                .get_use_path()
                .into_iter()
                .collect::<HashSet<String>>();
            use_set.remove("use super::*;");
            use_set.insert("use ntest::timeout;".to_string());
            use_check(&mut use_set, work_dir);
            let mut id = 0;
            let mut tests = 0;
            let mut tests_compiled = 0;
            let mut tests_lines: Vec<i32> = Vec::new();
            let mut oracles = 0;
            let mut oracles_compiled = 0;
            let mut has_oracle_compiled = false;
            for chain_test in test_gen_info.get_tests().iter() {
                let mut answer_id = 0;
                for answer in chain_test.get_answers().iter() {
                    let mut mod_content = String::new();
                    let mut use_set = use_set.clone();
                    use_set.extend(answer.get_uses().clone());
                    let common = answer.get_common().clone();
                    for test in answer.get_tests().iter() {
                        oracles += 1;
                        for (num, test_code) in test.codes.iter().enumerate() {
                            tests += 1;
                            tests_lines.push(test_code.len() as i32);
                            if test.can_compile[num].is_ok() {
                                tests_compiled += 1;
                                has_oracle_compiled = true;
                                let sig = format!("fn test_{}_{}_{:02}()", stem, rename, id);
                                let mut fn_code =
                                    vec!["#[test]".to_string(), TIMEOUT_DERIVE.to_string()];
                                fn_code.extend(test.attrs.clone().iter().map(|attr| {
                                    if attr.contains("#[should_panic(") {
                                        return "#[should_panic]".to_string();
                                    } else {
                                        attr.clone()
                                    }
                                }));
                                fn_code.push(sig);
                                fn_code.extend(test_code.clone());
                                mod_content = mod_content + &fn_code.join("\n") + "\n\n";
                                integration_info
                                    .test_function_names
                                    .push(format!("test_{}_{}_{:02}", stem, rename, id));
                                id += 1;
                            }
                            if has_oracle_compiled {
                                oracles_compiled += 1;
                                has_oracle_compiled = false;
                            }
                        }
                    }
                    if mod_content != "".to_string() {
                        if !common.is_empty() {
                            mod_content =
                                format!(
                                    "mod chain_{}_answer_{}{{\n",
                                    chain_test.get_id(),
                                    answer_id
                                ) + &use_set.into_iter().collect::<Vec<String>>().join("\n")
                                    + "\n"
                                    + &common.join("\n")
                                    + "\n"
                                    + &mod_content
                                    + "}"
                                    + "\n\n";
                        } else {
                            mod_content =
                                format!(
                                    "mod chain_{}_answer_{}{{\n",
                                    chain_test.get_id(),
                                    answer_id
                                ) + &use_set.into_iter().collect::<Vec<String>>().join("\n")
                                    + "\n"
                                    + &mod_content
                                    + "}"
                                    + "\n\n";
                        }
                        file_content = file_content + &mod_content;
                        answer_id = answer_id + 1;
                    }
                }
            }
            let tests_compiled_rate = if tests > 0 {
                tests_compiled as f64 / tests as f64 * 100.0
            } else {
                0.0
            };
            let oracles_compiled_rate = if oracles > 0 {
                oracles_compiled as f64 / oracles as f64 * 100.0
            } else {
                0.0
            };
            if file_content != "".to_string() {
                fs::write(&path, file_content).unwrap();
                integration_info.tests = tests;
                integration_info.tests_lines = tests_lines;
                integration_info.oracles = oracles;
                integration_info.oracles_compiled = oracles_compiled;
                integration_info.oracles_compiled_rate = oracles_compiled_rate;
                integration_info.tests_compiled = tests_compiled;
                integration_info.tests_compiled_rate = tests_compiled_rate;
                integration_infos.push(integration_info);
                if !cargo_check(work_dir).is_ok() {
                    panic!();
                }
            }
        }
    }
    integration_infos
}
