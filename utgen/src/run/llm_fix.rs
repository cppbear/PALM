use super::{
    TIMEOUT_DERIVE,
    integration,
    llm_fix_type::{ChangeLog, CompilerMessage, ErrorMessage, TestCode},
    prepare::get_test_gen_infos,
    run::{TestType, run_test},
};
use crate::{
    FunctionSelection,
    gene::LLM,
    types::{InsertKind, IntegrationContext, TestGenInfo},
    utils::{RestoreOnDrop, cargo_check, create_backup, insert_test, restore_file},
};
use log::{error, info, warn};
use rand::Rng;
use serde::Deserialize;
use std::{
    cmp::min,
    collections::{BTreeSet, HashMap},
    fs::{self, File, create_dir_all, read_to_string},
    i32,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::time::Duration;
use tokio::{
    sync::{Mutex, OnceCell, Semaphore},
    time::Instant,
};

static FIX_LOCK: OnceCell<Mutex<()>> = OnceCell::const_new();
static FIX_TIMEOUT: u64 = 28800;

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
                if one_json.is_error() && one_json.has_spans() {
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
    llm: &LLM,
    error_message: &ErrorMessage,
    compile_error_set: &Vec<ErrorMessage>,
    project_path: &Path,
    work_path: &Path,
    file_path: &Path,
    test_code: &TestCode,
    sig: String,
    insert_kind: InsertKind,
    common: &Vec<String>,
    integration_attrs: Option<&[String]>,
    target: Option<&crate::target::TargetInfo>,
) -> io::Result<(Vec<ErrorMessage>, TestCode, u32, u32)> {
    let mut iterative_time = 0;
    let mut min_error_set = compile_error_set.clone();
    let mut min_error_content = test_code.clone();
    let mut min_set_num = min_error_set.len();
    let mut max_time_to_iterative = 2;

    let current_error = error_message;
    let rust_assistant_prompt_json_str = include_str!("../../res/rustassistant_prompt.json");
    let rust_assistant_prompt = RustAssistantPrompt::from_json(&rust_assistant_prompt_json_str);
    let unit_command = target
        .map(|t| format!("cargo test {} --no-run", t.unit_args().join(" ")))
        .unwrap_or_default();
    let preamble_prompt = rust_assistant_prompt.rustassistant_preamble.replace(
        "{cmd}",
        if integration_attrs.is_some() {
            "cargo test --test palm_candidate --no-run"
        } else {
            &unit_command
        },
    ) + "\n";
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

    let lock = FIX_LOCK.get_or_init(|| async { Mutex::new(()) }).await;
    let mut completion_tokens = 0;
    let mut prompt_tokens = 0;

    while iterative_time < max_time_to_iterative {
        iterative_time += 1;
        let (request_choices, usage_completion, usage_prompt) =
            llm.get_answer(&final_prompt, 1, false).await?;
        completion_tokens += usage_completion;
        prompt_tokens += usage_prompt;
        let request_choices_string = request_choices.join("\n");

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

        let compile_error_set;
        if let Some(attrs) = integration_attrs {
            if integration::repaired_candidate(&new_test_code.codes, attrs).is_none() {
                continue;
            }
            let _guard = lock.lock().await;
            let (updated, errors, _) =
                check_integration_candidate(work_path, &new_test_code.codes)?;
            new_test_code = updated;
            compile_error_set = errors;
        } else {
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
            restore_file(file_path);
            let restore = RestoreOnDrop(file_path);
            insert_test(insert_kind, Path::new(&file_path), &mod_code);
            // let _ = target_clean(&work_path);

            let test_type = TestType::Error;
            run_test(project_path, work_path, test_type, false, false, target);

            let compiler_message_set = compiler_error_parser_from_json(work_path);

            let mut errors: Vec<ErrorMessage> = Vec::new();
            for compiler_message in compiler_message_set.iter() {
                let compile_error = ErrorMessage::new(work_path, compiler_message);
                errors.push(compile_error);
            }

            new_test_code = TestCode::new(&file_path, &new_test_code.codes);
            restore_file(Path::new(&file_path));
            drop(restore);
            drop(guard);
            compile_error_set = errors;
        }
        if compile_error_set.len() < min_set_num {
            min_error_set = compile_error_set;
            min_error_content = new_test_code;
            min_set_num = min_error_set.len();
        }
        if min_set_num == 0 {
            return Ok((
                min_error_set,
                min_error_content,
                completion_tokens,
                prompt_tokens,
            ));
        }
    }
    return Ok((
        min_error_set,
        min_error_content,
        completion_tokens,
        prompt_tokens,
    ));
}

async fn compilation_fix_assistant_for_one_fn(
    llm: &LLM,
    project_dir: PathBuf,
    work_path: PathBuf,
    test_gen_info: TestGenInfo,
) -> io::Result<TestGenInfo> {
    let start_time = Instant::now();
    let timeout = Duration::from_secs(FIX_TIMEOUT); // Existing between-round limit; not a subprocess deadline.

    let mut test_gen_info = test_gen_info;
    let target = crate::target::resolve_target(&work_path, test_gen_info.target.as_ref())?;
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

    let mut id = 0;
    for fn_test in test_gen_info.get_tests_mut().iter_mut() {
        for answer in fn_test.get_answers_mut().iter_mut() {
            let mut common = answer.get_common().clone();
            let mut completion_tokens = answer.get_completion_tokens();
            let mut prompt_tokens = answer.get_prompt_tokens();
            common.push("".to_string());
            for chain_test in answer.get_tests_mut().iter_mut() {
                for (num, test_code) in chain_test.codes.iter_mut().enumerate() {
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
                        restore_file(&file_path);
                        let restore = RestoreOnDrop(&file_path);
                        insert_test(insert_kind, Path::new(&file_path), &mod_code);
                        // let _ = target_clean(&work_path);

                        let test_type = TestType::Error;
                        run_test(&project_dir, &work_path, test_type, false, false, Some(&target));

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
                        drop(restore);
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
                            let (new_error_set, new_error_content, usage_completion, usage_prompt) =
                                compilation_fix_assistant_for_an_error(
                                    llm, random_error, &compile_error_set, &project_dir, &work_path,
                                    &file_path, &test_file_content, sig.clone(),
                                    insert_kind, &common, None, Some(&target),
                                ).await.map_err(|error| io::Error::other(format!(
                                    "Repair model request failed for {name}: {error}"
                                )))?;
                            compile_error_set = new_error_set;
                            test_file_content = new_error_content;
                            completion_tokens += usage_completion;
                            prompt_tokens += usage_prompt;
                            if compile_error_set.is_empty() {
                                break;
                            }
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
                        restore_file(&file_path);
                        let restore = RestoreOnDrop(&file_path);
                        insert_test(insert_kind, &file_path, &mod_code);
                        *test_code = test_file_content.codes;
                        chain_test.repaired[num] = true;
                        // let _ = target_clean(&work_path);

                        chain_test.can_compile[num] = cargo_check(&work_path, &target);
                        id += 1;
                        restore_file(Path::new(&file_path));
                        drop(restore);
                        drop(guard);
                        if start_time.elapsed() >= timeout {
                            warn!("Fix time out for {}", fn_name);
                            return Ok(test_gen_info);
                        }
                    }
                }
            }
            answer.set_completion_tokens(completion_tokens);
            answer.set_prompt_tokens(prompt_tokens);
        }
    }
    Ok(test_gen_info)
}

fn check_integration_candidate(
    work_dir: &Path,
    code: &[String],
) -> io::Result<(TestCode, Vec<ErrorMessage>, Result<(), String>)> {
    let temporary = integration::write_candidate(work_dir, code)?;
    let output = integration::compile_candidate(work_dir)?;
    let mut errors = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if value["reason"] == "compiler-message" && value["message"]["level"] == "error" {
            let message: CompilerMessage =
                serde_json::from_value(value).map_err(io::Error::other)?;
            if message.references_file(work_dir, &integration::candidate_path(work_dir)) {
                errors.push(ErrorMessage::new(work_dir, &message));
            }
        }
    }
    let status = if output.status.success() {
        Ok(())
    } else {
        let details = format!(
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        if errors.is_empty() {
            return Err(io::Error::other(format!(
                "Integration compilation failed: {details}"
            )));
        }
        Err(details)
    };
    let test_code = TestCode::new(&integration::candidate_path(work_dir), &code.to_vec());
    temporary.finish()?;
    Ok((test_code, errors, status))
}

async fn fix_integration_function(
    llm: &LLM,
    project_dir: &Path,
    work_dir: &Path,
    mut test_gen: TestGenInfo,
) -> io::Result<TestGenInfo> {
    let lock = FIX_LOCK.get_or_init(|| async { Mutex::new(()) }).await;
    let initial = {
        let _guard = lock.lock().await;
        integration::initial_uses(&test_gen, work_dir)
    };
    let start = Instant::now();
    for chain in test_gen.get_tests_mut() {
        for answer in chain.get_answers_mut() {
            let context = IntegrationContext {
                uses: initial
                    .iter()
                    .chain(answer.get_uses())
                    .cloned()
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect(),
                common: answer.get_common().clone(),
            };
            let mut completion = answer.get_completion_tokens();
            let mut prompt = answer.get_prompt_tokens();
            for test in answer.get_tests_mut() {
                if test.integration_contexts.is_empty() {
                    test.integration_contexts = vec![context.clone(); test.codes.len()];
                }
                for num in 0..test.codes.len() {
                    let code = integration::candidate_code(
                        &test.integration_contexts[num],
                        &test.attrs,
                        &test.codes[num],
                        "palm_candidate",
                        "test_candidate",
                        false,
                    );
                    // Revalidate in the actual target, including cached successful candidates.
                    let (mut current, mut errors, status) = {
                        let _guard = lock.lock().await;
                        check_integration_candidate(work_dir, &code)?
                    };
                    test.can_compile[num] = status;
                    if test.can_compile[num].is_ok() || test.repaired[num] {
                        continue;
                    }
                    let rounds = min(errors.len() + 2, 10);
                    for _ in 0..rounds {
                        if errors.is_empty() {
                            break;
                        }
                        let error = &errors[0];
                        let (next_errors, next_code, used_completion, used_prompt) =
                            compilation_fix_assistant_for_an_error(
                                llm,
                                error,
                                &errors,
                                project_dir,
                                work_dir,
                                &integration::candidate_path(work_dir),
                                &current,
                                "fn test_candidate()".to_string(),
                                InsertKind::EOF,
                                &Vec::new(),
                                Some(&test.attrs),
                                None,
                            )
                            .await?;
                        errors = next_errors;
                        current = next_code;
                        completion += used_completion;
                        prompt += used_prompt;
                    }
                    let (context, body) =
                        integration::repaired_candidate(&current.codes, &test.attrs).ok_or_else(
                            || io::Error::other("Integration repair changed the test declaration"),
                        )?;
                    test.integration_contexts[num] = context;
                    test.codes[num] = body;
                    test.repaired[num] = true;
                    test.can_compile[num] = if errors.is_empty() {
                        Ok(())
                    } else {
                        Err(errors
                            .iter()
                            .flat_map(|error| error.code_snippets.clone())
                            .collect::<Vec<_>>()
                            .join("\n"))
                    };
                    if start.elapsed() >= Duration::from_secs(FIX_TIMEOUT) {
                        return Ok(test_gen);
                    }
                }
            }
            answer.set_completion_tokens(completion);
            answer.set_prompt_tokens(prompt);
        }
    }
    Ok(test_gen)
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

pub async fn llm_fix(
    llm: &LLM,
    project_path: PathBuf,
    work_path: PathBuf,
    functions: &FunctionSelection,
    tasks: usize,
    integration: bool,
) -> io::Result<()> {
    if tasks == 0 || tasks > Semaphore::MAX_PERMITS {
        return Err(io::Error::other(
            "--tasks is outside the supported positive range",
        ));
    }
    super::validate_targets(&project_path, &work_path, functions, integration)?;
    let available = crate::target::targets(&work_path)?;
    let parent_dir = project_path.join("utgen/generation/llm_fix");
    let map_path = work_path.join("brinfo/name_map.json");
    let nmap: HashMap<String, String> =
        serde_json::from_str(&fs::read_to_string(&map_path).unwrap()).unwrap();

    // Prefer saved repair progress, then add candidates not yet copied from generation.
    let generated = get_test_gen_infos(&project_path, true);
    for info in generated
        .iter()
        .filter(|info| functions.contains(info.get_name()))
    {
        let target = crate::target::resolve_from(&available, info.target.as_ref())?;
        if !integration || target.kind == "lib" { info.check_mode(integration)?; }
    }
    let mut test_gen_infos = get_test_gen_infos(&project_path, false);
    let saved: BTreeSet<_> = test_gen_infos.iter()
        .map(|info| info.get_name().to_owned()).collect();
    test_gen_infos.extend(
        generated.into_iter()
            .filter(|info| !saved.contains(info.get_name())),
    );
    test_gen_infos.retain(|info| {
        let relative = info.get_file();
        let in_work_dir = project_path.join(relative).starts_with(&work_path);
        in_work_dir && functions.contains(info.get_name())
    });
    if let Some(names) = functions.names() {
        for name in names.iter().filter(|name| nmap.contains_key(*name)) {
            if !test_gen_infos.iter().any(|info| info.get_name() == name) {
                return Err(io::Error::other(format!(
                    "No generated candidate for selected function {name}; run gen first"
                )));
            }
        }
    }
    for info in &mut test_gen_infos {
        info.target = Some(crate::target::resolve_from(
            &available,
            info.target.as_ref(),
        )?);
        if !integration || info.target.as_ref().unwrap().kind == "lib" {
            info.check_mode(integration)?;
        }
    }
    if integration {
        test_gen_infos.retain(|info| info.target.as_ref().unwrap().kind == "lib");
    }
    if !integration {
        merge_common_in_code(&mut test_gen_infos);
    }
    create_dir_all(&parent_dir)?;
    for info in &test_gen_infos {
        let json_path = parent_dir.join(nmap.get(info.get_name()).unwrap().to_owned() + ".json");
        info.dump_json(&json_path);
    }
    // Back up each source once before starting workers. Reject stale backups
    // rather than reusing them, and only clean up files owned by this command.
    let sources: BTreeSet<_> = test_gen_infos
        .iter()
        .map(|info| project_path.join(info.get_file()))
        .collect();
    let mut backups = Vec::new();
    for source in sources.into_iter().filter(|_| !integration) {
        match create_backup(&source) {
            Ok(backup) => backups.push((source, backup)),
            Err(error) => {
                // No worker has started, so these newly created backups are unused.
                for (_, backup) in &backups {
                    fs::remove_file(backup)?;
                }
                return Err(io::Error::other(format!(
                    "Cannot back up {}: {error}",
                    source.display()
                )));
            }
        }
    }
    let slots = Arc::new(Semaphore::new(tasks));
    let counter = Arc::new(AtomicUsize::new(0));
    let length = test_gen_infos.len();
    let mut handles = Vec::new();
    for test_gen_info in test_gen_infos {
        let project_path_clone = project_path.clone();
        let work_path = work_path.clone();
        let parent_dir_clone = parent_dir.clone();
        let encoded_name = nmap.get(test_gen_info.get_name()).unwrap().to_owned();
        let counter_clone = Arc::clone(&counter);
        let slots = slots.clone();
        let llm = llm.clone();
        handles.push(tokio::spawn(async move {
            let _permit = slots.acquire_owned().await.map_err(io::Error::other)?;
            let test_gen_info = if integration {
                fix_integration_function(&llm, &project_path_clone, &work_path, test_gen_info).await?
            } else {
                compilation_fix_assistant_for_one_fn(
                    &llm, project_path_clone, work_path, test_gen_info,
                ).await?
            };
            let json_path = parent_dir_clone.join(encoded_name + ".json");
            test_gen_info.dump_json(&json_path);
            let completed = counter_clone.fetch_add(1, Ordering::Relaxed) + 1;
            info!("Fix progress: {completed}/{length}");
            Ok::<_, io::Error>(())
        }));
    }
    let mut failure = None;
    // Await every worker before restoring or deleting any shared backup.
    for handle in handles {
        let result = handle
            .await
            .map_err(|error| io::Error::other(format!("Repair task failed: {error}")))
            .and_then(|result| result);
        if let Err(error) = result {
            failure.get_or_insert(error);
        }
    }
    for (source, backup) in &backups {
        if let Err(error) = fs::copy(backup, source) {
            failure.get_or_insert_with(|| {
                io::Error::other(format!("Cannot restore {}: {error}", source.display()))
            });
        }
    }
    if let Some(error) = failure {
        return Err(error); // Retain recovery material and skip coverage statistics.
    }
    for (_, backup) in backups {
        fs::remove_file(backup)?;
    }
    Ok(())
}
