#![allow(unused)]
use super::sourceinfo::SourceInfo;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct BrData {
    pub name: String,
    pub name_with_impl: String,
    pub mod_info: ModInfo,
    pub visible: bool,
    pub loc: SourceInfo,
    pub doc: String,
    pub code: Vec<String>,
    pub size: SizeInfo,
    pub cond_chains: Vec<CondChain>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SizeInfo {
    pub chain: usize,
    pub contra: usize,
    pub min_set: usize,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModInfo {
    pub name: String,
    pub loc: SourceInfo,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CondChain {
    pub id: usize,
    pub conds: Vec<Cond>,
    pub ret: Option<String>,
    pub path: Vec<usize>,
    pub may_contra: bool,
    pub min_set: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Cond {
    pub cond: String,
    pub norm: Option<String>,
    pub value: String,
    pub line: usize,
    pub bound: Option<String>,
    pub may_panic: bool,
}
