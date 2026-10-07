mod cot;
mod generation;
mod llm;
mod prompt;
mod test_ext;

use crate::run::add_ntest_dependency;
use crate::types::{BrData, RfocxtNameInformation, TestGenInfo};
use cot::{gen_input_range, gen_oracle, gen_prefix, gen_test};
use generation::{check_integration, check_unit, generation_tests};
use log::{error, info, warn};
use prompt::{inputprompts, oracleprompts, prefixprompts, testprompts, Prompt};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use test_ext::{extract_test_functions, try_parse};
use tokio::sync::mpsc;

pub use llm::LLM;

async fn generation_task(
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
) {
    info!("Generating tests for {}", brdata.name);
    let test_gen_info = generation_tests(
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
    if let Some(test_gen_info) = test_gen_info {
        tx.send(test_gen_info).await.unwrap();
    }
}

pub async fn gen_tests_project(
    project_dir: &Path,
    work_dir: &Path,
    tasks: usize,
    integration: bool,
    requirement: bool,
    context: bool,
    oracle: bool,
) {
    add_ntest_dependency(work_dir);
    let brdata_dir = work_dir.join("brinfo/brdata");
    let map_path = work_dir.join("brinfo/name_map.json");
    let focxt_name_informations_path = work_dir.join("focxt/impl_informations.json");
    if !map_path.exists() {
        error!("{} does not exist", map_path.display());
        return;
    }
    if !brdata_dir.exists() {
        error!("{} does not exist", brdata_dir.display());
        return;
    }
    if !focxt_name_informations_path.exists() {
        error!("{} does not exist", focxt_name_informations_path.display());
        return;
    }
    if brdata_dir.is_dir() {
        let nmap: HashMap<String, String> =
            serde_json::from_str(&fs::read_to_string(&map_path).unwrap()).unwrap();
        let focxt_name_informations: Vec<RfocxtNameInformation> =
            serde_json::from_str(&fs::read_to_string(&focxt_name_informations_path).unwrap())
                .unwrap();
        let (tx, mut rx) = mpsc::channel(tasks);
        for entry in fs::read_dir(brdata_dir).unwrap() {
            let entry = entry.unwrap();
            let brdata_path = entry.path();
            if brdata_path.is_file() {
                let brdata: BrData =
                    serde_json::from_str(&fs::read_to_string(&brdata_path).unwrap()).unwrap();
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
                tokio::spawn(async move {
                    generation_task(
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
                    .await;
                });
            }
        }
        drop(tx);
        while let Some(mut test_gen_info) = rx.recv().await {
            let fn_name = test_gen_info.get_name().to_string();
            info!("Checking tests for {}", fn_name);
            if integration {
                check_integration(&mut test_gen_info, work_dir);
            } else {
                check_unit(&mut test_gen_info, project_dir, work_dir);
            }
            let file_path = project_dir.join(format!(
                "utgen/generation/pre_fix/{}.json",
                nmap.get(&fn_name).unwrap()
            ));
            test_gen_info.dump_json(&file_path);
        }
    } else {
        error!("{} is not a directory", brdata_dir.display());
    }
}
