// use super::format_code;
use crate::types::BrData;
use crate::utils::get_use_path;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

pub trait PromptTemplate {
    fn from_json(json: &str) -> Self
    where
        Self: Sized;
    fn system(&self) -> &str;
    fn integration(&self) -> Option<&str>;
    fn context(&self) -> &str;
    fn focal(&self) -> &str;
    fn requirements(&self) -> Option<&str>;
    fn depend(&self) -> Option<&str>;
}

pub struct PromptBuilder<T: PromptTemplate> {
    template: T,
    context: String,
}

impl<T: PromptTemplate> PromptBuilder<T> {
    pub fn new(focxt_path: &Path, template_json: &'static str) -> Self {
        let template = T::from_json(template_json);
        let context = fs::read_to_string(focxt_path).unwrap();
        Self { template, context }
    }

    pub fn build_system(&self, integration: bool) -> String {
        if integration {
            self.template.system().to_string() + self.template.integration().unwrap_or_default()
        } else {
            self.template.system().to_string()
        }
    }

    pub fn build_static(&self, brdata: &BrData, requirement: bool, context: bool) -> String {
        let crate_name = brdata.name.split("::").next().unwrap();
        let mut prompt = String::new();
        if context {
            prompt += self.template.context();
        }
        prompt += &format!(
            "// {}\n// crate name is {}\n",
            brdata.loc.get_file(),
            crate_name
        );
        if context {
            prompt += &(self.context.clone() + "\n");
        }
        prompt += self.template.focal();
        prompt += &brdata.doc;
        // prompt += &(format_code(&brdata.code, &brdata.loc) + "\n\n");
        prompt += &brdata.code.join("\n");
        prompt += "\n";
        if requirement && self.template.requirements().is_some() {
            prompt += self.template.requirements().unwrap();
        }
        prompt
    }

    pub fn build_depend(&self, brdata: &BrData, integration: bool) -> String {
        if !integration {
            return "".to_string();
        }
        let mut prompt = self.template.depend().unwrap_or_default().to_string();
        prompt += &get_use_path(&brdata.name_with_impl).join("\n");
        prompt += "\n";
        prompt
    }

    pub fn build(
        &self,
        brdata: &BrData,
        integration: bool,
        requirement: bool,
        context: bool,
    ) -> Prompt {
        let system_pt = self.build_system(integration);
        let static_pt = self.build_static(brdata, requirement, context);
        let depend_pt = self.build_depend(brdata, integration);

        Prompt {
            system_pt,
            static_pt,
            depend_pt,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Prompt {
    pub system_pt: String,
    pub static_pt: String,
    pub depend_pt: String,
}

impl Prompt {
    pub fn dump_json(&self, path: &Path) {
        fs::write(path, serde_json::to_string_pretty(self).unwrap()).unwrap();
    }
}
