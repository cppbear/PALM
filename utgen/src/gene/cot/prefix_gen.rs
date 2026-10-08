use super::{LLM, Prompt, extract_test_functions};
use crate::types::{ChainTestAnswer, TestInfo};
use log::error;
use rand::Rng;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tokio::time::{Duration, sleep};

pub async fn gen_prefix(
    llm: &LLM,
    answer_dir: &Path,
    pt_info: &Prompt,
    id: usize,
    conds: &Vec<String>,
    input_range: &String,
    integration: bool,
) -> Option<Vec<ChainTestAnswer>> {
    let system_pt = &pt_info.system_pt;
    let static_pt = &pt_info.static_pt;
    let mut completion_tokens = 0;
    let mut prompt_tokens = 0;

    let mut user_pt = static_pt.clone() + &conds.join("");
    user_pt += "Here are the inferred test input conditions or ranges based on the provided preconditions and return values or types, for your reference:\n";
    user_pt += input_range;
    user_pt += &pt_info.depend_pt;

    for attempt in 1..=3 {
        if attempt > 1 {
            let random_secs = {
                let mut rng = rand::rng();
                rng.random_range(10..=30)
            };
            sleep(Duration::from_secs(random_secs)).await;
        }
        let (answers, usage_completion, usage_prompt) =
            match llm.fetch_answer(Some(system_pt), &user_pt, 1, false).await {
                Ok(answer) => answer,
                Err(error) => {
                    error!("Model request failed: {error}");
                    return None;
                }
            };
        completion_tokens += usage_completion;
        prompt_tokens += usage_prompt;

        // LLM guarantees one nonempty answer. Keep it before stripping fences or parsing.
        let raw_answer = &answers[0];
        let chain_dir = answer_dir.join(format!("{id:03}"));
        fs::create_dir_all(&chain_dir).unwrap();
        fs::write(
            chain_dir.join(format!("prefix-attempt-{attempt}.txt")),
            raw_answer,
        )
        .unwrap();
        let answer = raw_answer.replace("```rust", "").replace("```", "");
        let prefixes = match extract_test_functions(&answer) {
            Ok(prefixes) => prefixes,
            Err(reason) => {
                error!("Prefix generation rejected chain {id}, attempt {attempt}/3: {reason}");
                continue;
            }
        };
        fs::write(
            chain_dir.join("prefix.rs"),
            format!("// Answer 0\n\n{answer}\n\n"),
        )
        .unwrap();
        let tests = prefixes
            .test_fns
            .into_iter()
            .map(|(attrs, prefix)| TestInfo::new(attrs, prefix, vec![]))
            .collect();
        let mut use_set = prefixes.uses.into_iter().collect::<HashSet<String>>();
        if integration {
            use_set.remove("use super::*;");
        }
        return Some(vec![ChainTestAnswer::new(
            use_set.into_iter().collect(),
            prefixes.has_test_mod,
            prefixes.common,
            tests,
            completion_tokens,
            prompt_tokens,
        )]);
    }
    None
}
