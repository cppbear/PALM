use super::{Prompt, PromptBuilder, PromptTemplate};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct InputTemplate {
    system: String,
    context: String,
    focal: String,
    requirements: String,
}

impl PromptTemplate for InputTemplate {
    fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap()
    }

    fn system(&self) -> &str {
        &self.system
    }

    fn integration(&self) -> Option<&str> {
        None
    }

    fn context(&self) -> &str {
        &self.context
    }

    fn focal(&self) -> &str {
        &self.focal
    }

    fn requirements(&self) -> Option<&str> {
        Some(&self.requirements)
    }

    fn depend(&self) -> Option<&str> {
        None
    }
}

pub fn inputprompts(
    brdata: &crate::types::BrData,
    focxt_path: &std::path::Path,
    requirement: bool,
    context: bool,
) -> Prompt {
    let pb: PromptBuilder<InputTemplate> =
        PromptBuilder::new(focxt_path, include_str!("../../../res/input_prompt.json"));
    pb.build(brdata, false, requirement, context)
}
