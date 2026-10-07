use super::{Prompt, PromptBuilder, PromptTemplate};
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct TestTemplate {
    system: String,
    integration: String,
    context: String,
    focal: String,
    requirements: String,
    depend: String,
}

impl PromptTemplate for TestTemplate {
    fn from_json(json: &str) -> Self {
        serde_json::from_str(json).unwrap()
    }

    fn system(&self) -> &str {
        &self.system
    }

    fn integration(&self) -> Option<&str> {
        Some(&self.integration)
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
        Some(&self.depend)
    }
}

pub fn testprompts(
    brdata: &crate::types::BrData,
    focxt_path: &std::path::Path,
    integration: bool,
    requirement: bool,
    context: bool,
) -> Prompt {
    let pb: PromptBuilder<TestTemplate> =
        PromptBuilder::new(focxt_path, include_str!("../../../res/test_prompt.json"));
    pb.build(brdata, integration, requirement, context)
}
