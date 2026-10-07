use super::sourceinfo::SourceInfo;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::{self, Display, Formatter},
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum Condition {
    Bool(BoolCond),
    For(ForCond),
    Match(MatchCond),
    IfLet(MatchCond),
    Try(String),
    MayPanic(String, Option<String>),
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum BoolCond {
    Binary(BinaryCond),
    Other(String),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum BinKind {
    Eq,
    Lt,
    Le,
    Ne,
    Ge,
    Gt,
    Other,
}

impl Display for BinKind {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        match self {
            BinKind::Eq => write!(f, "=="),
            BinKind::Lt => write!(f, "<"),
            BinKind::Le => write!(f, "<="),
            BinKind::Ne => write!(f, "!="),
            BinKind::Ge => write!(f, ">="),
            BinKind::Gt => write!(f, ">"),
            BinKind::Other => write!(f, "other"),
        }
    }
}

impl BoolCond {
    pub fn get_cond_str(&self) -> String {
        match self {
            BoolCond::Binary(b) => b.get_cond_str(),
            BoolCond::Other(s) => s.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct BinaryCond {
    pub kind: BinKind,
    pub expr: String,
    pub lhs: String,
    pub rhs: String,
    pub cmp_with_int: bool,
}

impl BinaryCond {
    pub fn get_bound(&self, cond: bool) -> Option<String> {
        match self.kind {
            BinKind::Lt | BinKind::Gt => {
                if !cond {
                    return Some(format!("{} == {}", self.lhs, self.rhs));
                }
            }
            BinKind::Le | BinKind::Ge => {
                if cond {
                    return Some(format!("{} == {}", self.lhs, self.rhs));
                }
            }
            _ => return None,
        }
        None
    }

    pub fn eq_with_int(&self) -> bool {
        self.cmp_with_int && self.kind == BinKind::Eq
    }

    pub fn ne_with_int(&self) -> bool {
        self.cmp_with_int && self.kind == BinKind::Ne
    }

    pub fn get_norm_str(&self) -> Option<String> {
        if !matches!(self.kind, BinKind::Other) && self.lhs > self.rhs {
            let kind = match self.kind {
                BinKind::Lt => BinKind::Gt,
                BinKind::Le => BinKind::Ge,
                BinKind::Gt => BinKind::Lt,
                BinKind::Ge => BinKind::Le,
                _ => self.kind,
            };
            return Some(format!("{} {} {}", self.rhs, kind, self.lhs));
        }
        None
    }

    pub fn get_cond_str(&self) -> String {
        self.expr.clone()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct ForCond {
    pub iter_var: String,
    pub iter_range: String,
}

impl ForCond {
    pub fn get_cond_str(&self) -> String {
        format!("{} in {}", self.iter_var, self.iter_range)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum PattKind {
    Enum(u128),
    StructLike(BTreeMap<usize, (Option<u128>, SourceInfo)>),
    Other(Option<u128>),
    Wild,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Patt {
    pub pat_str: String,
    pub kind: PattKind,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct Arm {
    pub pat: Patt,
    pub guard: Option<BTreeMap<SourceInfo, BTreeSet<Condition>>>,
    pub body_source: Option<SourceInfo>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub enum MatchKind {
    Enum(Vec<String>),
    StructLike(Option<Vec<String>>),
    Other,
}

impl MatchKind {
    pub fn get_field_name(&self, idx: usize) -> String {
        match self {
            MatchKind::Enum(variants) => variants[idx].clone(),
            MatchKind::StructLike(Some(fields)) => fields[idx].clone(),
            _ => idx.to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct MatchCond {
    pub match_source: SourceInfo,
    pub match_str: String,
    pub match_kind: MatchKind,
    pub arms: BTreeMap<SourceInfo, Arm>,
}

impl MatchCond {
    pub fn new(match_source: SourceInfo, match_str: String, match_kind: MatchKind) -> Self {
        Self {
            match_source,
            match_str,
            match_kind,
            arms: BTreeMap::new(),
        }
    }
}

impl MatchCond {
    pub fn get_cond_str(&self, pat_source: SourceInfo) -> String {
        format!(
            "{} is {}",
            self.match_str,
            self.arms.get(&pat_source).unwrap().pat.pat_str
        )
    }
}
