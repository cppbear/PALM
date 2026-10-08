use super::LLM;
use super::Prompt;
use log::error;

fn postprocess(inputs: &mut Vec<String>) {
    for input in inputs.iter_mut() {
        *input = input.replace("```rust", "").replace("```", "");
    }
}

pub async fn gen_input_range(
    llm: &LLM,
    pt_info: &Prompt,
    conds: &Vec<String>,
) -> Option<(String, u32, u32)> {
    let system_pt = &pt_info.system_pt;
    let static_pt = &pt_info.static_pt;

    let user_pt = static_pt.clone() + &conds.join("");
    let (mut answers, completion_tokens, prompt_tokens) =
        match llm.fetch_answer(Some(system_pt), &user_pt, 1, false).await {
            Ok(answer) => answer,
            Err(error) => {
                error!("Model request failed: {error}");
                return None;
            }
        };

    postprocess(&mut answers);
    // info!("Answers: {:?}", answers);

    let mut answer = answers.pop().unwrap();
    if !answer.ends_with('\n') {
        answer.push('\n');
    }

    Some((answer, completion_tokens, prompt_tokens))
}
