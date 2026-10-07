use super::Prompt;
use super::LLM;
use super::{extract_test_functions, try_parse};
use crate::types::{ChainTestAnswer, TestInfo};
use crate::utils::use_check;
use log::{error, info};
use rand::Rng;
use std::collections::HashSet;
use std::fs;
use std::io::Write;
use std::path::Path;
use tokio::time::{sleep, Duration};

fn postprocess(inputs: &mut Vec<String>) {
    for input in inputs.iter_mut() {
        *input = input.replace("```rust", "").replace("```", "");
    }
}

pub async fn gen_prefix(
    // project_dir: &Path,
    work_dir: &Path,
    answer_dir: &Path,
    pt_info: &Prompt,
    id: usize,
    conds: &Vec<String>,
    input_range: &String,
    integration: bool,
) -> Option<Vec<ChainTestAnswer>> {
    let llm = LLM::new().unwrap();
    let system_pt = &pt_info.system_pt;
    let static_pt = &pt_info.static_pt;
    let mut completion_tokens = 0;
    let mut prompt_tokens = 0;

    let mut user_pt = static_pt.clone() + &conds.join("");
    user_pt += "Here are the inferred test input conditions or ranges based on the provided preconditions and return values or types, for your reference:\n";
    user_pt += input_range;
    user_pt += &pt_info.depend_pt;

    let mut parse_retry = 0;
    while parse_retry < 3 {
        if parse_retry != 0 {
            let random_secs = {
                let mut rng = rand::rng();
                rng.random_range(10..=30)
            };
            sleep(Duration::from_secs(random_secs)).await;
        }
        // Net retry
        let mut net_retry = 0;
        let mut answers = None;
        while net_retry < 3 {
            if net_retry != 0 {
                let random_secs = {
                    let mut rng = rand::rng();
                    rng.random_range(10..=30)
                };
                sleep(Duration::from_secs(random_secs)).await;
            }
            let result = llm.fetch_answer(Some(&system_pt), &user_pt, 1, false).await;
            if result.is_ok() {
                let (result_answers, usage_completion, usage_prompt) = result.unwrap();
                completion_tokens += usage_completion;
                prompt_tokens += usage_prompt;
                answers = Some(result_answers);
                break;
            }
            error!("{}. Retrying...", result.unwrap_err());
            net_retry += 1;
        }
        if answers.is_none() {
            error!("Failed to fetch answer.");
            return None;
        }
        let mut answers = answers.unwrap();

        postprocess(&mut answers);

        answers.retain(|answer| try_parse(answer).is_ok());
        if answers.is_empty() {
            error!("No valid answer found.");
            parse_retry += 1;
            continue;
        }

        // info!("Answers: {:?}", answers);
        let mut test_answer_list = vec![];
        let path = answer_dir.join(format!("{:03}/prefix.rs", id));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = fs::File::create(&path).unwrap();
        for (id, answer) in answers.into_iter().enumerate() {
            let mut test_info_list = vec![];
            file.write_all(format!("// Answer {}\n\n", id).as_bytes())
                .unwrap();
            file.write_all((answer.clone() + "\n\n").as_bytes())
                .unwrap();

            let prefixs = extract_test_functions(&answer);
            for (attrs, prefix) in prefixs.test_fns {
                let test_info = TestInfo::new(attrs, prefix, vec![]);
                test_info_list.push(test_info);
            }
            let mut use_set = prefixs.uses.into_iter().collect::<HashSet<String>>();
            if integration {
                use_set.remove("use super::*;");
                use_check(&mut use_set, work_dir);
            }
            test_answer_list.push(ChainTestAnswer::new(
                use_set.into_iter().collect(),
                prefixs.has_test_mod,
                prefixs.common,
                test_info_list,
                completion_tokens,
                prompt_tokens,
            ));
        }
        return Some(test_answer_list);
    }
    None
}
