use super::{
    llm_fix_type::{ChangeLog, CompilerMessage, ErrorMessage, TestCode},
    prepare::get_test_gen_infos,
    run::{run_test, TestType},
    TIMEOUT_DERIVE,
};
use crate::{
    gene::LLM,
    types::{InsertKind, TestGenInfo},
    utils::{
        backup_file, cargo_check, delete_all_backups, has_backup, insert_test, restore_file,
        target_clean,
    },
};
use log::{error, info, warn};
use rand::Rng;
use serde::Deserialize;
use std::{
    cmp::min,
    collections::HashMap,
    error::Error,
    fs::{self, create_dir_all, exists, read_to_string, File},
    i32,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use tokio::time::{sleep, Duration};
use tokio::{
    sync::{Mutex, OnceCell},
    time::Instant,
};

static FIX_LOCK: OnceCell<Mutex<()>> = OnceCell::const_new();
static FIX_TIMEOUT: u64 = 28800;

async fn get_fixes_from_llm(prompt: &String, n: i32) -> Result<(Vec<String>, u32, u32), Box<dyn Error + Send>> {
    let llm = LLM::new().unwrap();
    let answers = llm.get_answer(&prompt, n as u8, false).await;
    answers
}

fn compiler_error_parser_from_json(work_path: &Path) -> Vec<CompilerMessage> {
    let file_path = work_path.join("error_output.json");
    let file_content = read_to_string(&file_path).unwrap();
    let mut file_content_vec: Vec<String> = Vec::new();
    let mut start = 0;
    let mut level = 0;
    let mut in_str = false;
    for (i, c) in file_content.char_indices() {
        match c {
            '"' => {
                if in_str == true {
                    in_str = false;
                } else {
                    in_str = true;
                }
            }
            '{' => {
                if in_str {
                    continue;
                }
                if level == 0 {
                    start = i;
                }
                level += 1;
            }
            '}' => {
                if in_str {
                    continue;
                }
                level -= 1;
                if level == 0 {
                    let one_json = file_content[start..=i].to_string();
                    if one_json.contains("compiler-message") {
                        file_content_vec.push(one_json);
                    }
                }
            }
            _ => {}
        }
    }
    let compiler_messages = file_content_vec
        .iter()
        .filter_map(|s| {
            let one_json: Result<CompilerMessage, serde_json::Error> = serde_json::from_str(s);
            if let Ok(one_json) = one_json {
                if one_json.has_spans() {
                    return Some(one_json);
                } else {
                    return None;
                }
            } else {
                return None;
            }
        })
        .collect();
    fs::remove_file(&file_path).unwrap();
    compiler_messages
}

#[derive(Debug, Deserialize)]
struct RustAssistantPrompt {
    rustassistant_preamble: String,
    rustassistant_errorinformation_and_code_snippets: String,
    rustassistant_instructions_for_fixing_the_error: String,
    rustassistant_instructions_and_examples_for_formatting_the_changelog_output: String,
}

impl RustAssistantPrompt {
    fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap()
    }
}

pub fn llm_return_content_parser(work_path: &Path, llm_return_content: &String) -> Vec<ChangeLog> {
    let mut result_changelog_list: Vec<ChangeLog> = Vec::new();
    let llm_return_changes: Vec<&str> = llm_return_content.split("ChangeLog:").collect();
    for llm_return_change in llm_return_changes.iter() {
        if !llm_return_change.contains("OriginalCode") || !llm_return_change.contains("FixedCode") {
            continue;
        }
        let mut llm_return_content: Vec<&str> = llm_return_change.lines().collect();
        let llm_return_content_0 = format!("ChangeLog:{}", llm_return_content[0]);
        llm_return_content[0] = llm_return_content_0.as_str();
        let new_change_log = ChangeLog::new(work_path, &llm_return_content);
        if let Ok(log) = new_change_log {
            result_changelog_list.push(log);
        }
    }
    return result_changelog_list;
}

async fn compilation_fix_assistant_for_an_error(
    error_message: &ErrorMessage,
    compile_error_set: &Vec<ErrorMessage>,
    project_path: &Path,
    work_path: &Path,
    file_path: &Path,
    test_code: &TestCode,
    sig: String,
    test_name: String,
    insert_kind: InsertKind,
    common: &Vec<String>,
) -> Result<(Vec<ErrorMessage>, TestCode, u32, u32), ()> {
    // let mut local_error_group: Vec<ErrorMessage> = Vec::new();
    // local_error_group.push(error_message.clone());
    let mut iterative_time = 0;
    let mut min_error_set = compile_error_set.clone();
    let mut min_error_content = test_code.clone();
    let mut min_set_num = min_error_set.len();
    let mut max_time_to_iterative = 2;

    let current_error = error_message;
    let rust_assistant_prompt_json_str = include_str!("../../res/rustassistant_prompt.json");
    // println!("{:#?}", rust_assistant_prompt_json_path);
    let rust_assistant_prompt = RustAssistantPrompt::from_json(&rust_assistant_prompt_json_str);
    let preamble_prompt = rust_assistant_prompt
        .rustassistant_preamble
        .replace("{cmd}", "cargo test --tests")
        + "\n";
    let error_snippet_prompt = current_error.code_snippets.clone();
    let final_prompt = preamble_prompt.clone()
        + &error_snippet_prompt.join("\n")
        + &rust_assistant_prompt
            .rustassistant_instructions_for_fixing_the_error
            .replace("{file}", &file_path.to_string_lossy().to_string())
            .replace("{start}", (test_code.start + 1).to_string().as_str())
            .replace("{end}", (test_code.end - 1).to_string().as_str())
        + &rust_assistant_prompt
            .rustassistant_instructions_and_examples_for_formatting_the_changelog_output;
    // let fix_prompt_path = work_path.join("fix_prompt.txt");
    // let mut file = File::create(fix_prompt_path).unwrap();
    // file.write_all(final_prompt.as_bytes()).unwrap();

    let lock = FIX_LOCK.get_or_init(|| async { Mutex::new(()) }).await;
    let mut retry = 0;
    let mut completion_tokens = 0;
    let mut prompt_tokens = 0;

    while iterative_time < max_time_to_iterative {
        iterative_time += 1;
        let request_choices_result = get_fixes_from_llm(&final_prompt, 1).await;
        // let request_choices_result = Err(());
        // let request_choices_result = read_to_string(work_path.join("fix_request.txt"));
        let mut request_choices: Vec<String> = Vec::new();
        if let Ok((s, usage_completion, usage_prompt)) = request_choices_result {
            completion_tokens += usage_completion;
            prompt_tokens += usage_prompt;
            request_choices = s;
            // request_choices = s.split("ChangeLog:").map(|s| s.to_string()).collect();
            // request_choices.remove(0);
        } else {
            if retry < 3 {
                retry += 1;
                max_time_to_iterative += 1;
                let random_secs = {
                    let mut rng = rand::rng();
                    rng.random_range(10..=30)
                };
                sleep(Duration::from_secs(random_secs)).await;
            } else {
                return Err(());
            }
        }
        // for request_choice in request_choices.iter_mut() {
        //     *request_choice = "ChangeLog:".to_string() + request_choice;
        // }
        // let mut request_result: Vec<(Vec<ErrorMessage>, TestCode)> = Vec::new();
        let mut request_choices_string = String::new();
        // for request in request_choices.iter() {
        //     request_choices_string = request_choices_string + request + "\n";
        // }
        request_choices_string = request_choices.join("\n");
        // let fix_request_path = work_path.join("fix_request.txt");
        // let mut file = File::create(fix_request_path).unwrap();
        // file.write_all(request_choices_string.as_bytes()).unwrap();
        // println!("{:#?}", request_choices);
        // let mut modified_file_right = true;

        // for i in request_choices.iter() {
        let mut new_test_code = test_code.clone();
        let mut changelog_list = llm_return_content_parser(work_path, &request_choices_string);
        let changelog_list_copy = changelog_list.clone();
        changelog_list.clear();
        for mut changelog in changelog_list_copy {
            if changelog.file_path == file_path
                && changelog
                    .reduce_the_scope(new_test_code.start as usize, new_test_code.end as usize)
            {
                changelog_list.push(changelog);
            }
        }
        if changelog_list.len() == 0 {
            continue;
        }
        if !new_test_code.change_codes(&changelog_list) {
            continue;
        }

        let template = include_str!("../../res/code_template.json");
        let code_template: Vec<String> = serde_json::from_str(&template).unwrap();
        let mut fn_code = vec![
            "#[test]".to_string(),
            TIMEOUT_DERIVE.to_string(),
            sig.clone(),
        ];
        fn_code.extend(new_test_code.codes.clone());
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

        let guard = lock.lock().await;
        insert_test(insert_kind, Path::new(&file_path), &mod_code);
        let _ = target_clean(&work_path);

        let test_type = TestType::Error;
        run_test(project_path, work_path, test_type, false, false);

        let compiler_message_set = compiler_error_parser_from_json(work_path);

        let mut compile_error_set: Vec<ErrorMessage> = Vec::new();
        for compiler_message in compiler_message_set.iter() {
            let compile_error = ErrorMessage::new(work_path, compiler_message);
            compile_error_set.push(compile_error);
        }

        // request_result.push((compile_error_set, new_test_code));
        // }
        // if request_result.len() != 0 {
        new_test_code = TestCode::new(&file_path, &new_test_code.codes);
        restore_file(Path::new(&file_path));
        drop(guard);
        // let mut min_set_index = MAX;
        // let mut i = 0;
        // for request_r in request_result.iter() {
        // if request_r.0.len() < min_set_num {
        //     min_set_num = request_r.0.len();
        //     min_set_index = i;
        // }
        // i += 1;
        // }
        if compile_error_set.len() < min_set_num {
            min_error_set = compile_error_set;
            min_error_content = new_test_code;
            min_set_num = min_error_set.len();
        }
        // min_error_set = request_result[min_set_index].0.clone();
        // min_error_content = request_result[min_set_index].1.clone();
        if min_set_num == 0 {
            return Ok((min_error_set, min_error_content, completion_tokens, prompt_tokens));
        }

        // let template = include_str!("../../res/code_template.json");
        // let code_template: Vec<String> = serde_json::from_str(&template).unwrap();
        // let mut fn_code = vec!["#[test]".to_string(), sig.clone()];
        // fn_code.extend(min_error_content.codes.clone());
        // let mut mod_code = code_template.clone();
        // let pos = mod_code.len() - 1;
        // mod_code.splice(pos..pos, fn_code);
        // restore_file(Path::new(&file_path));
        // insert_test(insert_kind, Path::new(&file_path), &mod_code);
        // }
    }
    return Ok((min_error_set, min_error_content, completion_tokens, prompt_tokens));
}

async fn compilation_fix_assistant_for_one_fn(
    project_dir: PathBuf,
    work_path: PathBuf,
    test_gen_info: TestGenInfo,
) -> TestGenInfo {
    let start_time = Instant::now();
    let timeout = Duration::from_secs(FIX_TIMEOUT); // 设置超时时间为 30 秒

    let mut test_gen_info = test_gen_info;
    let file_rela = test_gen_info.get_file();
    let file_path = project_dir.join(file_rela);
    let name = test_gen_info.get_name().to_string();
    let fn_name = name.split("::").last().unwrap();
    let insert_kind = test_gen_info.get_insert_kind();
    let template = include_str!("../../res/code_template.json");
    let code_template: Vec<String> = serde_json::from_str(template).unwrap();
    // Backup the file
    // info!("Fix for {}", fn_name);
    let lock = FIX_LOCK.get_or_init(|| async { Mutex::new(()) }).await;
    let guard = lock.lock().await;
    if !has_backup(Path::new(&file_path)) {
        backup_file(Path::new(&file_path));
    }
    drop(guard);

    let mut id = 0;
    for fn_test in test_gen_info.get_tests_mut().iter_mut() {
        for answer in fn_test.get_answers_mut().iter_mut() {
            let mut common = answer.get_common().clone();
            let mut completion_tokens = answer.get_completion_tokens();
            let mut prompt_tokens = answer.get_prompt_tokens();
            common.push("".to_string());
            for chain_test in answer.get_tests_mut().iter_mut() {
                for (num, test_code) in chain_test.codes.iter_mut().enumerate() {
                    let mut network_error = false;
                    let mut network_error_times = 0;
                    if !chain_test.can_compile[num].is_ok() && !chain_test.repaired[num] {
                        let sig = format!("fn test_{}_{:02}()", fn_name, id);
                        let mut fn_code = vec!["#[test]".to_string(), TIMEOUT_DERIVE.to_string()];
                        fn_code.extend(chain_test.attrs.clone().iter().map(|attr| {
                            if attr.contains("#[should_panic(") {
                                return "#[should_panic]".to_string();
                            } else {
                                attr.clone()
                            }
                        }));
                        fn_code.push(sig.clone());
                        fn_code.extend(test_code.clone());
                        let insert_code = fn_code;
                        let mut mod_code = code_template.clone();
                        let pos = mod_code.len() - 1;
                        mod_code.splice(pos..pos, insert_code);

                        let guard = lock.lock().await;
                        insert_test(insert_kind, Path::new(&file_path), &mod_code);
                        let _ = target_clean(&work_path);

                        let test_type = TestType::Error;
                        let test_name = format!("test_{}", fn_name);
                        run_test(&project_dir, &work_path, test_type, false, false);

                        // restore_file(&file_path);
                        let mut test_file_content = TestCode::new(&file_path, &test_code);
                        let compiler_message_set = compiler_error_parser_from_json(&work_path);

                        // println!("{:#?}", compiler_message_set);
                        let mut compile_error_set: Vec<ErrorMessage> = Vec::new();
                        for compiler_message in compiler_message_set.iter() {
                            let compile_error = ErrorMessage::new(&work_path, compiler_message);
                            compile_error_set.push(compile_error);
                        }
                        restore_file(Path::new(&file_path));
                        drop(guard);

                        let mut initial_error_num = min(compile_error_set.len() + 2, 10);
                        let mut i = 0;
                        // let mut already_rng: Vec<usize> = Vec::new();
                        while i < initial_error_num && compile_error_set.len() > 0 {
                            i += 1;
                            info!("fix {} iter {}", sig, i);
                            let random_num = rand::rng().random_range(0..compile_error_set.len());
                            // while already_rng.contains(&random_num) {
                            //     random_num = rand::thread_rng().gen_range(0..compile_error_set.len());
                            // }
                            // already_rng.push(random_num);
                            let random_error = compile_error_set.get(random_num).unwrap();
                            let once_fix_result = compilation_fix_assistant_for_an_error(
                                &random_error,
                                &compile_error_set,
                                &project_dir,
                                &work_path,
                                &file_path,
                                &test_file_content,
                                sig.clone(),
                                test_name.clone(),
                                insert_kind,
                                &common,
                            )
                            .await;
                            if let Ok((new_error_set, new_error_content, usage_completion, usage_prompt)) = once_fix_result {
                                compile_error_set = new_error_set;
                                test_file_content = new_error_content;
                                completion_tokens += usage_completion;
                                prompt_tokens += usage_prompt;
                                // }
                                if compile_error_set.len() == 0 {
                                    break;
                                }
                            } else {
                                if network_error_times < 3 {
                                    network_error_times += 1;
                                    initial_error_num += 1;
                                } else {
                                    network_error = true;
                                    break;
                                }
                            }
                            // if new_error_set.len() < compile_error_set.len() {
                        }
                        if network_error {
                            error!("Fix failed for {}: network error", fn_name);
                            continue;
                        }
                        let sig = format!("fn test_{}_{:02}()", fn_name, id);
                        let mut fn_code = vec!["#[test]".to_string(), TIMEOUT_DERIVE.to_string()];
                        fn_code.extend(chain_test.attrs.clone().iter().map(|attr| {
                            if attr.contains("#[should_panic(") {
                                return "#[should_panic]".to_string();
                            } else {
                                attr.clone()
                            }
                        }));
                        fn_code.push(sig.clone());
                        fn_code.extend(test_file_content.codes.clone());
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

                        let guard = lock.lock().await;
                        insert_test(insert_kind, &file_path, &mod_code);
                        *test_code = test_file_content.codes;
                        chain_test.repaired[num] = true;
                        let _ = target_clean(&work_path);

                        chain_test.can_compile[num] = cargo_check(&work_path);
                        id += 1;
                        restore_file(Path::new(&file_path));
                        drop(guard);
                        if start_time.elapsed() >= timeout {
                            warn!("Fix time out for {}", fn_name);
                            return test_gen_info;
                        }
                    }
                }
            }
            answer.set_completion_tokens(completion_tokens);
            answer.set_prompt_tokens(prompt_tokens);
        }
    }
    test_gen_info
}

fn merge_common_in_code(test_gen_infos: &mut Vec<TestGenInfo>) {
    for test_gen_info in test_gen_infos.iter_mut() {
        for fn_test in test_gen_info.get_tests_mut() {
            for answer in fn_test.get_answers_mut() {
                let common = answer.get_common().clone();
                if common.len() > 0 {
                    for chain_test in answer.get_tests_mut() {
                        for code in chain_test.codes.iter_mut() {
                            code.splice(1..1, common.clone());
                        }
                    }
                }
                answer.clear_common();
            }
        }
    }
}

pub async fn llm_fix(project_path: PathBuf, work_path: PathBuf) {
    let mut test_gen_infos;
    let pre_dir = project_path.join("utgen/generation/pre_fix");
    let parent_dir = project_path.join("utgen/generation/llm_fix");
    let map_path = work_path.join("brinfo/name_map.json");
    let nmap: HashMap<String, String> =
        serde_json::from_str(&fs::read_to_string(&map_path).unwrap()).unwrap();

    if exists(&parent_dir).unwrap() {
        for entry in fs::read_dir(pre_dir).unwrap() {
            let entry = entry.unwrap();
            let source_path = entry.path();
            if source_path.is_file() {
                if let Some(file_name) = source_path.file_name() {
                    let target_path = parent_dir.join(file_name);
                    if !target_path.exists() {
                        fs::copy(&source_path, &target_path).unwrap();
                    }
                }
            }
        }
        test_gen_infos = get_test_gen_infos(&project_path, false);
        merge_common_in_code(&mut test_gen_infos);
        for test_gen_info in test_gen_infos.iter() {
            let file_rela = test_gen_info.get_file();
            let file_path = project_path.join(&file_rela);
            if (project_path == work_path && !file_rela.starts_with("src"))
                || (project_path != work_path && !file_path.starts_with(&work_path))
            {
                continue;
            }
            let json_path =
                parent_dir.join(nmap.get(test_gen_info.get_name()).unwrap().to_owned() + ".json");
            test_gen_info.dump_json(&json_path);
        }
    } else {
        create_dir_all(&parent_dir).unwrap();
        test_gen_infos = get_test_gen_infos(&project_path, true);
        merge_common_in_code(&mut test_gen_infos);
        for test_gen_info in test_gen_infos.iter() {
            let file_rela = test_gen_info.get_file();
            let file_path = project_path.join(&file_rela);
            if (project_path == work_path && !file_rela.starts_with("src"))
                || (project_path != work_path && !file_path.starts_with(&work_path))
            {
                continue;
            }
            let json_path =
                parent_dir.join(nmap.get(test_gen_info.get_name()).unwrap().to_owned() + ".json");
            test_gen_info.dump_json(&json_path);
        }
    }

    let counter = Arc::new(AtomicUsize::new(0));
    let length = test_gen_infos.len();
    let mut handles = Vec::new();
    for test_gen_info in test_gen_infos.into_iter() {
        let project_path_clone = project_path.clone();
        let work_path = work_path.clone();
        let file_rela = test_gen_info.get_file();
        let file_path = project_path.join(&file_rela);
        if (project_path == work_path && !file_rela.starts_with("src"))
            || (project_path != work_path && !file_path.starts_with(&work_path))
        {
            continue;
        }
        let parent_dir_clone = parent_dir.clone();
        let encoded_name = nmap.get(test_gen_info.get_name()).unwrap().to_owned();
        let counter_clone = Arc::clone(&counter);

        let handle = tokio::spawn(async move {
            let test_gen_info =
                compilation_fix_assistant_for_one_fn(project_path_clone, work_path, test_gen_info)
                    .await;
            let lock = FIX_LOCK.get_or_init(|| async { Mutex::new(()) }).await;
            let guard = lock.lock().await;
            let json_path = parent_dir_clone.join(encoded_name + ".json");
            test_gen_info.dump_json(&json_path);
            drop(guard);
            counter_clone.fetch_add(1, Ordering::Relaxed);
            info!(
                "Fix progress: {}/{}",
                counter_clone.load(Ordering::Relaxed),
                length
            );
        });
        handles.push(handle);
    }
    futures::future::join_all(handles).await;
    delete_all_backups(&work_path);
}
