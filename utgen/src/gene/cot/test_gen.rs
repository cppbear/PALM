use super::{LLM, Prompt, extract_test_functions};
use crate::types::{ChainTestAnswer, TestInfo};
use log::error;
use std::collections::HashSet;
use std::fs;
use std::path::Path;
use tokio::time::{Duration, sleep};

pub async fn gen_test(
    llm: &LLM,
    answer_dir: &Path,
    pt_info: &Prompt,
    id: usize,
    conds: &Vec<String>,
    integration: bool,
) -> Option<Vec<ChainTestAnswer>> {
    let system_pt = &pt_info.system_pt;
    let static_pt = &pt_info.static_pt;
    let mut completion_tokens = 0;
    let mut prompt_tokens = 0;
    let mut user_pt = static_pt.clone() + &conds.join("");
    user_pt += &pt_info.depend_pt;

    for attempt in 1..=3 {
        if attempt > 1 {
            sleep(Duration::from_secs(1)).await;
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
            chain_dir.join(format!("test-attempt-{attempt}.txt")),
            raw_answer,
        )
        .unwrap();
        let answer = raw_answer.replace("```rust", "").replace("```", "");
        let codes = match extract_test_functions(&answer) {
            Ok(codes) => codes,
            Err(reason) => {
                error!("Test generation rejected chain {id}, attempt {attempt}/3: {reason}");
                continue;
            }
        };
        fs::write(
            chain_dir.join("code.rs"),
            format!("// Answer 0\n\n{answer}\n\n"),
        )
        .unwrap();
        let tests = codes
            .test_fns
            .into_iter()
            .map(|(attrs, code)| TestInfo::new(attrs, vec![], vec![code]))
            .collect();
        let mut use_set = codes.uses.into_iter().collect::<HashSet<String>>();
        if integration {
            use_set.remove("use super::*;");
        }
        return Some(vec![ChainTestAnswer::new(
            use_set.into_iter().collect(),
            codes.has_test_mod,
            codes.common,
            tests,
            completion_tokens,
            prompt_tokens,
        )]);
    }
    None
}
