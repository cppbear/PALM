use std::collections::HashSet;

use super::items_context::{
    EnumItem, FnItem, ImplFnItem, ImplItem, StructItem, TraitFnItem, TraitItem, UnionItem,
};

#[derive(Debug, Clone)]
pub enum FnType {
    Fn(FnItem),
    ImplFn(ImplFnItem, ImplItem),
    TraitFn(TraitFnItem, TraitItem),
}

#[derive(Debug, Clone)]
pub struct FnData {
    pub fn_name: String,
    pub complete_fn_name: String,
    pub fn_type: FnType,
}

#[derive(Debug, Clone)]

pub enum StructType {
    Struct(StructItem),
    Enum(EnumItem),
    Union(UnionItem),
    Trait(TraitItem),
}

#[derive(Debug, Clone)]

pub struct StructData {
    pub struct_name: String,
    pub complete_struct_name: String,
    pub struct_type: StructType,
}
