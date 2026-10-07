use super::{gen_input_range, gen_oracle, gen_prefix, gen_test};
use super::{inputprompts, oracleprompts, prefixprompts, testprompts};
use crate::utils::target_clean;
use crate::types::{BrData, ChainTestInfo, TestGenInfo};
use crate::utils::{backup_file, cargo_check, delete_backup, insert_test, restore_file, use_check};
use log::info;
use std::collections::HashSet;
use std::fs;
use std::path::Path;

pub async fn generation_tests(
    brdata: BrData,
    encoded_name: &str,
    focxt_encoded_name: &str,
    project_dir: &Path,
    work_dir: &Path,
    integration: bool,
    requirement: bool,
    context: bool,
    oracle: bool,
) -> Option<TestGenInfo> {
    if oracle {
        return gen_tests_cot(
            brdata,
            encoded_name,
            focxt_encoded_name,
            project_dir,
            work_dir,
            integration,
            requirement,
            context,
        )
        .await;
    } else {
        return gen_full_tests(
            brdata,
            encoded_name,
            focxt_encoded_name,
            project_dir,
            work_dir,
            integration,
            requirement,
            context,
        )
        .await;
    }
}

fn split_oracles(oracles: &Vec<String>) -> Vec<Vec<String>> {
    let mut result = vec![];
    let mut nonassert = vec![];
    for oracle in oracles {
        if oracle.trim().is_empty() || oracle.trim().starts_with("//") {
            // Skip empty lines and comments
            continue;
        } else {
            if oracle.trim().starts_with("assert") {
                let mut assert = nonassert.clone();
                assert.push(oracle.clone());
                result.push(assert);
            } else {
                nonassert.push(oracle.clone());
            }
        }
    }
    result
}

async fn gen_tests_cot(
    brdata: BrData,
    encoded_name: &str,
    focxt_encoded_name: &str,
    project_dir: &Path,
    work_dir: &Path,
    integration: bool,
    requirement: bool,
    context: bool,
) -> Option<TestGenInfo> {
    let focxt_path = work_dir.join(&format!("focxt/{}.rs", focxt_encoded_name));
    // let focxt_path = get_focxt_path(work_dir, &brdata.name);
    info!("FOCXT Path: {}", &focxt_path.display());

    // Generate prompts
    let input_pt_info = inputprompts(&brdata, &focxt_path, requirement, context);
    let prefix_pt_info = prefixprompts(&brdata, &focxt_path, integration, requirement, context);
    let oracle_pt_info = oracleprompts(&brdata, &focxt_path, integration, requirement, context);

    let pt_dir = project_dir.join(format!("utgen/generation/prompt/{}", encoded_name));
    let answer_dir = project_dir.join(format!("utgen/generation/answer/{}", encoded_name));
    fs::create_dir_all(&pt_dir).unwrap();
    fs::create_dir_all(&answer_dir).unwrap();

    let input_pt_path = pt_dir.join("input.json");
    let prefix_pt_path = pt_dir.join("prefix.json");
    let oracle_pt_path = pt_dir.join("oracle.json");
    fs::write(
        &input_pt_path,
        serde_json::to_string_pretty(&input_pt_info).unwrap(),
    )
    .unwrap();
    fs::write(
        &prefix_pt_path,
        serde_json::to_string_pretty(&prefix_pt_info).unwrap(),
    )
    .unwrap();
    fs::write(
        &oracle_pt_path,
        serde_json::to_string_pretty(&oracle_pt_info).unwrap(),
    )
    .unwrap();

    let mut chain_test_info_list = vec![];
    let mut completion_tokens = 0;
    let mut prompt_tokens = 0;

    if requirement {
        for cond_chain in &brdata.cond_chains {
            if !cond_chain.min_set {
                continue;
            }
            let id = cond_chain.id;
            // Generate prompts for each condition chain
            let size = if cond_chain.ret.is_some() {
                cond_chain.conds.len() + 1
            } else {
                cond_chain.conds.len()
            };
            let mut cond_prompt = Vec::with_capacity(size);
            for cond in &cond_chain.conds {
                let prompt = if let Some(bound) = &cond.bound {
                    format!(
                        "// constraint: {} is {}, with bound {}\n",
                        cond.cond, cond.value, bound
                    )
                } else {
                    format!("// constraint: {} is {}\n", cond.cond, cond.value)
                };
                cond_prompt.push(prompt);
            }
            if let Some(ret) = &cond_chain.ret {
                let prompt = format!("// expected return value/type: {}\n", ret);
                cond_prompt.push(prompt);
            }

            // Infer the input range for each condition chain
            let input_range = gen_input_range(&input_pt_info, &cond_prompt).await;
            if input_range.is_none() {
                return None;
            }
            let (input_range, usage_completion, usage_prompt) = input_range.unwrap();
            completion_tokens += usage_completion;
            prompt_tokens += usage_prompt;

            let input_answer_path = answer_dir.join(format!("{:03}/input.txt", id));
            fs::create_dir_all(input_answer_path.parent().unwrap()).unwrap();
            fs::write(&input_answer_path, input_range.clone()).unwrap();

            // Generate test prefix for each condition chain
            let test_answer_list = gen_prefix(
                // project_dir,
                work_dir,
                &answer_dir,
                &prefix_pt_info,
                id,
                &cond_prompt,
                &input_range,
                integration,
            )
            .await;
            if test_answer_list.is_none() {
                return None;
            }
            let mut test_answer_list = test_answer_list.unwrap();

            // Generate test oracle for each prefix
            for chain_test_answer in test_answer_list.iter_mut() {
                let common = chain_test_answer.get_common().clone();
                for test_info in chain_test_answer.get_tests_mut() {
                    let mut code = common.clone();
                    code.extend(test_info.prefix_func());
                    let oracle = gen_oracle(&oracle_pt_info, &cond_prompt, &code).await;
                    if oracle.is_some() {
                        let (oracles , usage_completion, usage_prompt)= oracle.unwrap();
                        completion_tokens += usage_completion;
                        prompt_tokens += usage_prompt;
                        let oracles = oracles
                            .trim()
                            .split("\n")
                            .map(|s| format!("    {}", s.trim()))
                            .collect::<Vec<String>>();
                        let oracles = split_oracles(&oracles);
                        test_info.set_oracles(oracles);
                    }
                }
            }

            let oracle_answer_path = answer_dir.join(format!("{:03}/oracle.json", id));
            fs::write(
                &oracle_answer_path,
                serde_json::to_string_pretty(&test_answer_list).unwrap(),
            )
            .unwrap();

            let chain_test_info =
                ChainTestInfo::new(cond_chain.id, cond_prompt, input_range, completion_tokens, prompt_tokens, test_answer_list);
            chain_test_info_list.push(chain_test_info);
        }
    } else {
        let id = 0;

        // Infer the input range for each condition chain
        let input_range = gen_input_range(&input_pt_info, &vec![]).await;
        if input_range.is_none() {
            return None;
        }
        let (input_range , usage_completion, usage_prompt)= input_range.unwrap();
        completion_tokens += usage_completion;
        prompt_tokens += usage_prompt;

        let input_answer_path = answer_dir.join(format!("{:03}/input.txt", id));
        fs::create_dir_all(input_answer_path.parent().unwrap()).unwrap();
        fs::write(&input_answer_path, input_range.clone()).unwrap();

        // Generate test prefix for each condition chain
        let test_answer_list = gen_prefix(
            // project_dir,
            work_dir,
            &answer_dir,
            &prefix_pt_info,
            id,
            &vec![], // empty condition prompt
            &input_range,
            integration,
        )
        .await;
        if test_answer_list.is_none() {
            return None;
        }
        let mut test_answer_list = test_answer_list.unwrap();

        // Generate test oracle for each prefix
        for chain_test_answer in test_answer_list.iter_mut() {
            let common = chain_test_answer.get_common().clone();
            for test_info in chain_test_answer.get_tests_mut() {
                let mut code = common.clone();
                code.extend(test_info.prefix_func());
                let oracle = gen_oracle(&oracle_pt_info, &vec![], &code).await;
                if oracle.is_some() {
                    let (oracles , usage_completion, usage_prompt) = oracle.unwrap();
                    completion_tokens += usage_completion;
                    prompt_tokens += usage_prompt;
                    let oracles = oracles
                        .trim()
                        .split("\n")
                        .map(|s| format!("    {}", s.trim()))
                        .collect::<Vec<String>>();
                    let oracles = split_oracles(&oracles);
                    test_info.set_oracles(oracles);
                }
            }
        }

        let oracle_answer_path = answer_dir.join(format!("{:03}/oracle.json", id));
        fs::write(
            &oracle_answer_path,
            serde_json::to_string_pretty(&test_answer_list).unwrap(),
        )
        .unwrap();

        let chain_test_info = ChainTestInfo::new(id, vec![], input_range, completion_tokens, prompt_tokens, test_answer_list);
        chain_test_info_list.push(chain_test_info);
    }

    let test_gen_info = TestGenInfo::new(
        brdata.name,
        brdata.name_with_impl,
        brdata.mod_info,
        brdata.visible,
        brdata.loc,
        chain_test_info_list,
    );

    Some(test_gen_info)
}

async fn gen_full_tests(
    brdata: BrData,
    encoded_name: &str,
    focxt_encoded_name: &str,
    project_dir: &Path,
    work_dir: &Path,
    integration: bool,
    requirement: bool,
    context: bool,
) -> Option<TestGenInfo> {
    let focxt_path = work_dir.join(&format!("focxt/{}.rs", focxt_encoded_name));
    // let focxt_path = get_focxt_path(work_dir, &brdata.name);
    info!("FOCXT Path: {}", &focxt_path.display());

    // Generate prompts
    let test_pt_info = testprompts(&brdata, &focxt_path, integration, requirement, context);

    let pt_dir = project_dir.join(format!("utgen/generation/prompt/{}", encoded_name));
    let answer_dir = project_dir.join(format!("utgen/generation/answer/{}", encoded_name));
    fs::create_dir_all(&pt_dir).unwrap();
    fs::create_dir_all(&answer_dir).unwrap();

    let test_pt_path = pt_dir.join("test.json");
    fs::write(
        &test_pt_path,
        serde_json::to_string_pretty(&test_pt_info).unwrap(),
    )
    .unwrap();

    let mut chain_test_info_list = vec![];

    if requirement {
        for cond_chain in &brdata.cond_chains {
            if !cond_chain.min_set {
                continue;
            }
            let id = cond_chain.id;
            // Generate prompts for each condition chain
            let size = if cond_chain.ret.is_some() {
                cond_chain.conds.len() + 1
            } else {
                cond_chain.conds.len()
            };
            let mut cond_prompt = Vec::with_capacity(size);
            for cond in &cond_chain.conds {
                let prompt = if cond.may_panic {
                    format!(
                        "// may panic: {} may panic in certain situations\n",
                        cond.cond
                    )
                } else if let Some(bound) = &cond.bound {
                    format!(
                        "// constraint: {} is {}, with bound {}\n",
                        cond.cond, cond.value, bound
                    )
                } else {
                    format!("// constraint: {} is {}\n", cond.cond, cond.value)
                };
                cond_prompt.push(prompt);
            }
            if let Some(ret) = &cond_chain.ret {
                let prompt = format!("// expected return value/type: {}\n", ret);
                cond_prompt.push(prompt);
            }

            // Generate test functions for each condition chain
            let test_answer_list = gen_test(
                // project_dir,
                work_dir,
                &answer_dir,
                &test_pt_info,
                id,
                &cond_prompt,
                integration,
            )
            .await;
            if test_answer_list.is_none() {
                return None;
            }
            let test_answer_list = test_answer_list.unwrap();

            let chain_test_info =
                ChainTestInfo::new(id, cond_prompt, String::new(), 0, 0, test_answer_list);
            chain_test_info_list.push(chain_test_info);
        }
    } else {
        let id = 0;

        // Generate test functions for each condition chain
        let test_answer_list = gen_test(
            // project_dir,
            work_dir,
            &answer_dir,
            &test_pt_info,
            id,
            &vec![], // empty condition prompt
            integration,
        )
        .await;
        if test_answer_list.is_none() {
            return None;
        }
        let test_answer_list = test_answer_list.unwrap();

        let chain_test_info = ChainTestInfo::new(id, vec![], String::new(), 0, 0, test_answer_list);
        chain_test_info_list.push(chain_test_info);
    }

    let test_gen_info = TestGenInfo::new(
        brdata.name,
        brdata.name_with_impl,
        brdata.mod_info,
        brdata.visible,
        brdata.loc,
        chain_test_info_list,
    );

    Some(test_gen_info)
}

pub fn check_unit(test_gen: &mut TestGenInfo, project_dir: &Path, work_dir: &Path) {
    let path = project_dir.join(test_gen.get_file());
    let name = test_gen.get_name().to_string();
    let fn_name = name.split("::").last().unwrap();
    let insert_kind = test_gen.get_insert_kind();

    let template = include_str!("../../res/code_template.json");
    let code_template: Vec<String> = serde_json::from_str(template).unwrap();
    // Backup the file
    backup_file(&path);
    let mut id = 0;
    for chain_test in test_gen.get_tests_mut() {
        for test_answer in chain_test.get_answers_mut() {
            let common = test_answer.get_common().clone();
            for test_info in test_answer.get_tests_mut() {
                for (num, test_code) in test_info.codes.iter().enumerate() {
                    let sig = format!("fn test_{}_{:02}()", fn_name, id);
                    let mut fn_code = vec!["#[test]".to_string()];
                    fn_code.extend(test_info.attrs.clone());
                    fn_code.push(sig);
                    fn_code.extend(test_code.clone());
                    let insert_code = if !common.is_empty() {
                        let mut code = common.clone();
                        code.push("".to_string());
                        code.extend(fn_code);
                        code
                    } else {
                        fn_code
                    };
                    let mut mod_code = code_template.clone();
                    let pos = mod_code.len() - 1;
                    mod_code.splice(pos..pos, insert_code);
                    insert_test(insert_kind, &path, &mod_code);
                    let _ = target_clean(&work_dir);
                    let result = cargo_check(&work_dir);
                    test_info.can_compile[num] = result;
                    // test.can_compile = false;
                    id += 1;
                    // Restore the file
                    restore_file(&path);
                }
            }
        }
    }
    delete_backup(&path);
}

pub fn check_integration(test_gen: &mut TestGenInfo, work_dir: &Path) {
    let file_path = test_gen.get_file();
    let stem = Path::new(&file_path).file_stem().unwrap().to_str().unwrap();
    let path = work_dir.join("tests/test_check.rs");
    if path.exists() {
        fs::remove_file(&path).unwrap();
    }
    fs::create_dir_all(&path.parent().unwrap()).unwrap();
    let name = test_gen.get_name().to_string();
    let fn_name = name.split("::").last().unwrap();

    let mut init_use_set = test_gen
        .get_use_path()
        .into_iter()
        .collect::<HashSet<String>>();
    use_check(&mut init_use_set, work_dir);
    let mut id = 0;
    for chain_test in test_gen.get_tests_mut() {
        for test_answer in chain_test.get_answers_mut() {
            let mut answer_use_set = init_use_set.clone();
            answer_use_set.extend(test_answer.get_uses().clone());
            let common = test_answer.get_common().clone();
            for test_info in test_answer.get_tests_mut() {
                for (num, test_code) in test_info.codes.iter().enumerate() {
                    let sig = format!("fn test_{}_{}_{:02}()", stem, fn_name, id);
                    let mut fn_code = vec!["#[test]".to_string()];
                    fn_code.extend(test_info.attrs.clone());
                    fn_code.push(sig);
                    fn_code.extend(test_code.clone());
                    let mut code = answer_use_set
                        .clone()
                        .into_iter()
                        .collect::<Vec<String>>()
                        .join("\n")
                        + "\n\n";
                    if !common.is_empty() {
                        code += &(common.join("\n") + "\n\n");
                    }
                    code += &(fn_code.join("\n") + "\n");
                    fs::write(&path, code).unwrap();
                    let _ = target_clean(&work_dir);
                    let result = cargo_check(&work_dir);
                    test_info.can_compile[num] = result;
                    id += 1;
                }
            }
        }
    }
}
