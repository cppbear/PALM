use super::brdata::ModInfo;
use super::sourceinfo::SourceInfo;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    io::BufReader,
    io::Write,
    path::Path,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RfocxtNameInformation {
    pub mod_name: String,
    pub fn_name: String,
    pub struct_name: String,
    pub trait_name: String,
    pub full_name: String,
    pub encoded_name: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum InsertKind {
    EOF,        // End of file
    EOM(usize), // End of module, with the line number of the end of the module
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestGenInfo {
    name: String,
    name_with_impl: String,
    mod_info: ModInfo,
    visible: bool,
    loc: SourceInfo,
    fn_tests: Vec<ChainTestInfo>,
}

impl TestGenInfo {
    pub fn new(
        name: String,
        name_with_impl: String,
        mod_info: ModInfo,
        visible: bool,
        loc: SourceInfo,
        fn_tests: Vec<ChainTestInfo>,
    ) -> TestGenInfo {
        TestGenInfo {
            name,
            name_with_impl,
            mod_info,
            visible,
            loc,
            fn_tests,
        }
    }

    pub fn get_insert_kind(&self) -> InsertKind {
        if self.mod_info.name == "" {
            return InsertKind::EOF;
        } else if !self.mod_info.loc.contains(&self.loc) {
            return InsertKind::EOF;
        } else {
            return InsertKind::EOM(self.mod_info.loc.get_endline());
        }
    }

    pub fn get_file(&self) -> String {
        self.loc.get_file()
    }

    pub fn get_name(&self) -> &str {
        &self.name
    }

    pub fn get_visibility(&self) -> bool {
        self.visible
    }

    pub fn get_tests(&self) -> &Vec<ChainTestInfo> {
        &self.fn_tests
    }

    pub fn get_tests_mut(&mut self) -> &mut Vec<ChainTestInfo> {
        &mut self.fn_tests
    }

    pub fn dump_json(&self, file_path: &Path) {
        fs::create_dir_all(file_path.parent().unwrap()).unwrap();
        let json = serde_json::to_string_pretty(&self).unwrap();
        let mut file = File::create(file_path).unwrap();
        file.write_all(json.as_bytes()).unwrap();
    }

    pub fn from_json(file_path: &Path) -> Self {
        // println!("{:?}", file_path);
        let file = File::open(file_path).unwrap();
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).unwrap()
    }

    pub fn get_mod_info(&self) -> ModInfo {
        return self.mod_info.clone();
    }

    pub fn get_loc(&self) -> SourceInfo {
        return self.loc.clone();
    }

    /// Get the use path of the function
    /// For example, if the function name is `a::b::c::fn`, the use paths are:
    /// - `use a::*;`
    /// - `use a::b::*;`
    /// - `use a::b::c::*;`
    ///
    /// If the function name is `a::b::{impl#0}::fn`, the use paths are:
    /// - `use a::*;`
    /// - `use a::b::*;`
    pub fn get_use_path(&self) -> Vec<String> {
        let parts: Vec<&str> = self.name_with_impl.split("::").collect();
        let mut paths = Vec::new();
        let mut current_path = String::new();

        for part in parts.iter().take(parts.len() - 1) {
            if part.starts_with("{impl") {
                break;
            }
            if !current_path.is_empty() {
                current_path.push_str("::");
            }
            current_path.push_str(part);
            let use_path = format!("use {}::*;", current_path);
            paths.push(use_path);
        }

        paths
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainTestInfo {
    chain_id: usize,
    prompt_conds: Vec<String>,
    input_infer: String,
    input_infer_completion_tokens: u32,
    input_infer_prompt_tokens: u32,
    answers: Vec<ChainTestAnswer>,
}

impl ChainTestInfo {
    pub fn new(
        chain_id: usize,
        prompt_conds: Vec<String>,
        input_infer: String,
        input_infer_completion_tokens: u32,
        input_infer_prompt_tokens: u32,
        answers: Vec<ChainTestAnswer>,
    ) -> ChainTestInfo {
        ChainTestInfo {
            chain_id,
            prompt_conds,
            input_infer,
            input_infer_completion_tokens,
            input_infer_prompt_tokens,
            answers,
        }
    }

    pub fn get_id(&self) -> usize {
        self.chain_id
    }

    pub fn get_prompts(&self) -> &Vec<String> {
        &self.prompt_conds
    }

    pub fn get_answers(&self) -> &Vec<ChainTestAnswer> {
        &self.answers
    }

    pub fn get_answers_mut(&mut self) -> &mut Vec<ChainTestAnswer> {
        &mut self.answers
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChainTestAnswer {
    uses: Vec<String>,
    has_test_mod: bool,
    common: Vec<String>,
    chain_tests: Vec<TestInfo>,
    completion_tokens: u32,
    prompt_tokens: u32,
}

impl ChainTestAnswer {
    pub fn new(
        uses: Vec<String>,
        has_test_mod: bool,
        common: Vec<String>,
        chain_tests: Vec<TestInfo>,
        completion_tokens: u32,
        prompt_tokens: u32,
    ) -> ChainTestAnswer {
        ChainTestAnswer {
            uses,
            has_test_mod,
            common,
            chain_tests,
            completion_tokens,
            prompt_tokens,
        }
    }

    pub fn get_uses(&self) -> &Vec<String> {
        &self.uses
    }

    pub fn has_test_mod(&self) -> bool {
        self.has_test_mod
    }

    pub fn get_common(&self) -> &Vec<String> {
        &self.common
    }

    pub fn get_tests(&self) -> &Vec<TestInfo> {
        &self.chain_tests
    }

    pub fn get_tests_mut(&mut self) -> &mut Vec<TestInfo> {
        &mut self.chain_tests
    }

    pub fn clear_common(&mut self) {
        self.common.clear();
    }

    pub fn get_completion_tokens(&self) -> u32 {
        self.completion_tokens
    }

    pub fn set_completion_tokens(&mut self, completion_tokens: u32) {
        self.completion_tokens = completion_tokens;
    }

    pub fn get_prompt_tokens(&self) -> u32 {
        self.prompt_tokens
    }

    pub fn set_prompt_tokens(&mut self, prompt_tokens: u32) {
        self.prompt_tokens = prompt_tokens;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestInfo {
    pub attrs: Vec<String>,
    pub prefix: Vec<String>,
    pub oracles: Vec<Vec<String>>,
    pub codes: Vec<Vec<String>>,
    pub can_compile: Vec<Result<(), String>>,
    pub repaired: Vec<bool>,
}

impl TestInfo {
    pub fn new(attrs: Vec<String>, prefix: Vec<String>, codes: Vec<Vec<String>>) -> TestInfo {
        let len = codes.len();
        TestInfo {
            attrs,
            prefix,
            oracles: Vec::new(),
            codes,
            can_compile: vec![Ok(()); len],
            repaired: vec![false; len],
        }
    }

    pub fn set_oracles(&mut self, oracles: Vec<Vec<String>>) {
        self.oracles = oracles;
        self.set_codes();
    }

    pub fn prefix_func(&self) -> Vec<String> {
        let mut code = self.attrs.clone();
        code.push("#[test]".to_string());
        code.push("fn test()".to_string());
        code.extend(self.prefix.clone());
        code
    }

    pub fn set_codes(&mut self) {
        let mut codes = Vec::new();
        for oracle in &self.oracles {
            let mut code = self.prefix.clone();
            let index = code.len() - 1;
            code.splice(index..index, oracle.clone());
            codes.push(code);
        }
        self.codes = codes;
        let len = self.codes.len();
        self.can_compile = vec![Ok(()); len];
        self.repaired = vec![false; len];
    }
}

pub struct TestExtract {
    pub uses: Vec<String>,
    pub has_test_mod: bool,
    pub common: Vec<String>,
    pub test_fns: Vec<(Vec<String>, Vec<String>)>, // (attrs, lines)
}
