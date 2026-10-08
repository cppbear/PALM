mod cot;
mod generation;
mod llm;
mod prompt;
mod test_ext;

use crate::run::add_ntest_dependency;
use crate::types::{BrData, RfocxtNameInformation, TestGenInfo};
use crate::{FunctionSelection, validate_analysis};
use cot::{gen_input_range, gen_oracle, gen_prefix, gen_test};
use generation::{check_integration, check_unit, generation_tests};
use log::{error, info, warn};
use prompt::{Prompt, inputprompts, oracleprompts, prefixprompts, testprompts};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::{fs, io};
use test_ext::{extract_test_functions, try_parse};
use tokio::sync::{Semaphore, mpsc};

pub use llm::LLM;

async fn generation_task(
    llm: LLM,
    brdata: BrData,
    encoded_name: &str,
    focxt_encoded_name: &str,
    project_dir: &Path,
    work_dir: &Path,
    integration: bool,
    requirement: bool,
    context: bool,
    oracle: bool,
    tx: mpsc::Sender<TestGenInfo>,
) -> io::Result<()> {
    let name = brdata.name.clone();
    info!("Generating tests for {}", brdata.name);
    let test_gen_info = generation_tests(
        &llm,
        brdata,
        encoded_name,
        focxt_encoded_name,
        project_dir,
        work_dir,
        integration,
        requirement,
        context,
        oracle,
    )
    .await;
    let test_gen_info =
        test_gen_info.ok_or_else(|| io::Error::other(format!("Generation failed for {name}")))?;
    tx.send(test_gen_info)
        .await
        .map_err(|_| io::Error::other("Generation result channel closed"))?;
    Ok(())
}

pub async fn gen_tests_project(
    llm: &LLM,
    project_dir: &Path,
    work_dir: &Path,
    functions: &FunctionSelection,
    tasks: usize,
    integration: bool,
    requirement: bool,
    context: bool,
    oracle: bool,
) -> io::Result<()> {
    if tasks == 0 || tasks > Semaphore::MAX_PERMITS {
        return Err(io::Error::other(
            "--tasks is outside the supported positive range",
        ));
    }
    validate_analysis(project_dir, work_dir)?;
    add_ntest_dependency(work_dir);
    let brdata_dir = work_dir.join("brinfo/brdata");
    let map_path = work_dir.join("brinfo/name_map.json");
    let focxt_name_informations_path = work_dir.join("focxt/impl_informations.json");
    if brdata_dir.is_dir() {
        let nmap: HashMap<String, String> =
            serde_json::from_str(&fs::read_to_string(&map_path).unwrap()).unwrap();
        let focxt_name_informations: Vec<RfocxtNameInformation> =
            serde_json::from_str(&fs::read_to_string(&focxt_name_informations_path).unwrap())
                .unwrap();
        let (tx, mut rx) = mpsc::channel(tasks);
        let slots = Arc::new(Semaphore::new(tasks));
        let mut handles = Vec::new();
        for entry in fs::read_dir(brdata_dir).unwrap() {
            let entry = entry.unwrap();
            let brdata_path = entry.path();
            if brdata_path.is_file() {
                let brdata: BrData =
                    serde_json::from_str(&fs::read_to_string(&brdata_path).unwrap()).unwrap();
                if !functions.contains(&brdata.name) {
                    continue;
                }
                // if brdata.size.min_set < 2 {
                //     info!("{} has less than 2 condition chains in min_set", brdata.name);
                //     continue;
                // }
                if integration && !brdata.visible {
                    // info!("{} is not public", brdata.name);
                    continue;
                }
                let encoded_name = nmap.get(&brdata.name).cloned().unwrap();
                let mut focxt_encoded_name = String::new();
                for focxt_name_information in focxt_name_informations.iter() {
                    if focxt_name_information.full_name == brdata.name {
                        focxt_encoded_name = focxt_name_information.encoded_name.clone();
                        break;
                    }
                }
                if focxt_encoded_name.is_empty() {
                    error!("{} not found in focxt name map", brdata.name);
                    continue;
                }
                let gen_info_path = project_dir
                    .join("utgen/generation/pre_fix")
                    .join(&format!("{}.json", encoded_name));
                if gen_info_path.exists() {
                    warn!("Tests for {} already generated", brdata.name);
                    continue;
                }
                let tx = tx.clone();
                let project_dir = project_dir.to_path_buf();
                let work_dir = work_dir.to_path_buf();
                let llm = llm.clone();
                let slots = slots.clone();
                handles.push(tokio::spawn(async move {
                    // Wait inside the task so the producer loop cannot block
                    // the result consumer. Hold the slot through result delivery.
                    let _permit = slots.acquire_owned().await.map_err(io::Error::other)?;
                    generation_task(
                        llm,
                        brdata,
                        &encoded_name,
                        &focxt_encoded_name,
                        &project_dir,
                        &work_dir,
                        integration,
                        requirement,
                        context,
                        oracle,
                        tx,
                    )
                    .await
                }));
            }
        }
        drop(tx);
        let mut failure = None;
        while let Some(mut test_gen_info) = rx.recv().await {
            if failure.is_some() {
                continue; // Drain results so senders can finish and release slots.
            }
            let fn_name = test_gen_info.get_name().to_string();
            info!("Checking tests for {}", fn_name);
            let file_path = project_dir.join(format!(
                "utgen/generation/pre_fix/{}.json",
                nmap.get(&fn_name).unwrap()
            ));
            let project_dir = project_dir.to_owned();
            let work_dir = work_dir.to_owned();
            // Await exactly one validation at a time, without blocking model tasks.
            let checked = tokio::task::spawn_blocking(move || {
                if integration {
                    check_integration(&mut test_gen_info, &work_dir);
                } else {
                    check_unit(&mut test_gen_info, &project_dir, &work_dir);
                }
                test_gen_info.dump_json(&file_path);
            })
            .await;
            if let Err(error) = checked {
                failure = Some(io::Error::other(format!(
                    "Validation failed for {fn_name}: {error}"
                )));
            }
        }
        for handle in handles {
            let result = handle
                .await
                .map_err(|e| io::Error::other(format!("Generation task failed: {e}")))
                .and_then(|result| result);
            if let Err(error) = result {
                failure.get_or_insert(error);
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
    } else {
        return Err(io::Error::other(format!(
            "{} is not a directory",
            brdata_dir.display()
        )));
    }
    Ok(())
}
