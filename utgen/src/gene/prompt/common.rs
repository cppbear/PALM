use crate::types::SourceInfo;
use serde::{Deserialize, Serialize};

pub fn format_code(code: &Vec<String>, loc: &SourceInfo) -> String {
    let width = loc.get_endline().to_string().len();
    let start_line = loc.get_startline();
    code.iter()
        .enumerate()
        .map(|(i, s)| format!("{:>width$} {}", start_line + i, s, width = width))
        .collect::<Vec<String>>()
        .join("\n")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct CondPrompt {
    pub chain_id: usize,
    pub conds: Vec<String>,
}

