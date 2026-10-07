use super::sourceinfo::SourceInfo;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize)]
pub struct BrData {
    name: String,
    mod_info: ModInfo,
    visible: bool,
    loc: SourceInfo,
    doc: String,
    code: Vec<String>,
    size: SizeInfo,
    cond_chains: Vec<CondChain>,
}

impl BrData {
    pub fn new(
        name: String,
        mod_info: ModInfo,
        visible: bool,
        loc: SourceInfo,
        doc: String,
        code: Vec<String>,
    ) -> Self {
        Self {
            name,
            mod_info,
            visible,
            loc,
            doc,
            code,
            size: SizeInfo {
                chain: 0,
                contra: 0,
                min_set: 0,
            },
            cond_chains: vec![],
        }
    }

    pub fn chain_len(&self) -> usize {
        self.cond_chains.len()
    }

    pub fn add_chain(&mut self, mut chain: CondChain) {
        chain.id = self.cond_chains.len() + 1;
        self.cond_chains.push(chain);
    }

    pub fn set_min_set(&mut self) {
        let mut uncovered: HashSet<_> = self
            .cond_chains
            .iter()
            .flat_map(|chain| chain.get_cond_set())
            .collect();
        if uncovered.len() < 2 {
            for chain in &mut self.cond_chains {
                chain.min_set = true;
            }
            return;
        }
        let (non_contra, contra): (Vec<_>, Vec<_>) =
            self.cond_chains.iter_mut().partition(|s| !s.may_contra);
        for subset in [non_contra, contra].iter_mut() {
            while !uncovered.is_empty() {
                if let Some(best) = subset
                    .iter_mut()
                    .filter(|s| !s.get_cond_set().is_disjoint(&uncovered))
                    .max_by_key(|s| s.get_cond_set().intersection(&uncovered).count())
                {
                    best.min_set = true;
                    uncovered = uncovered
                        .difference(&best.get_cond_set())
                        .cloned()
                        .collect();
                } else {
                    break;
                }
            }
        }
    }

    pub fn set_size(&mut self) {
        self.size.chain = self.cond_chains.len();
        self.size.contra = self.cond_chains.iter().filter(|s| s.may_contra).count();
        self.size.min_set = self.cond_chains.iter().filter(|s| s.min_set).count();
    }
}

#[derive(Debug, Clone, Serialize)]
struct SizeInfo {
    chain: usize,
    contra: usize,
    min_set: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModInfo {
    pub name: String,
    pub loc: SourceInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct CondChain {
    id: usize,
    conds: Vec<Cond>,
    ret: Option<String>,
    path: Vec<usize>,
    may_contra: bool,
    min_set: bool,
}

impl CondChain {
    pub fn new(conds: Vec<Cond>, path: Vec<usize>, ret: Option<String>) -> Self {
        Self {
            id: 0,
            conds,
            path,
            ret,
            may_contra: false,
            min_set: false,
        }
    }

    pub fn get_cond_set(&self) -> HashSet<(usize, String, String)> {
        self.conds
            .iter()
            .map(|c| {
                (
                    c.line,
                    c.norm.clone().unwrap_or(c.cond.clone()),
                    c.value.clone(),
                )
            })
            .collect()
    }

    pub fn set_may_contra(&mut self) {
        let mut map = HashMap::new();
        for cond in &self.conds {
            if let Some(flag) =
                map.get(&(cond.norm.clone().unwrap_or(cond.cond.clone()), cond.line))
            {
                if flag != &cond.value {
                    self.may_contra = true;
                    break;
                }
            } else {
                map.insert(
                    (cond.norm.clone().unwrap_or(cond.cond.clone()), cond.line),
                    cond.value.clone(),
                );
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Cond {
    pub cond: String,
    pub norm: Option<String>,
    pub value: String,
    pub line: usize,
    pub bound: Option<String>,
}

impl Cond {
    pub fn new(cond: String, value: String, line: usize) -> Self {
        Self {
            cond,
            norm: None,
            value,
            line,
            bound: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallsAndTypes {
    pub mod_name: String,
    pub calls: Vec<String>,
    pub types: Vec<String>,
}

impl CallsAndTypes {
    pub fn new_empty() -> Self {
        CallsAndTypes {
            mod_name: String::new(),
            calls: Vec::new(),
            types: Vec::new(),
        }
    }

    pub fn new(mod_name: &String, calls: &HashSet<String>, types: &HashSet<String>) -> Self {
        let mut calls_vec: Vec<String> = Vec::new();
        for call in calls.iter() {
            calls_vec.push(call.clone());
        }
        let mut types_vec: Vec<String> = Vec::new();
        for a_type in types.iter() {
            types_vec.push(a_type.clone());
        }
        CallsAndTypes {
            mod_name: mod_name.clone(),
            calls: calls_vec,
            types: types_vec,
        }
    }

    pub fn add_data(&mut self, data: &CallsAndTypes) {
        for other_call in data.calls.iter() {
            if !self.calls.contains(other_call) {
                self.calls.push(other_call.clone());
            }
        }
        for other_type in data.types.iter() {
            if !self.types.contains(other_type) {
                self.types.push(other_type.clone());
            }
        }
    }
}
