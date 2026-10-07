use std::{cell::RefCell, rc::Rc};

use quote::ToTokens;
use syn::{
    ImplItemConst, ImplItemFn, ImplItemType, Item, ItemConst, ItemEnum, ItemFn, ItemImpl,
    ItemMacro, ItemMod, ItemStatic, ItemStruct, ItemTrait, ItemTraitAlias, ItemType, ItemUnion,
    ItemUse, ReturnType, TraitItemConst, TraitItemFn, TraitItemType,
};

use super::mod_context::ModContext;

#[derive(Debug, Clone, PartialEq)]
pub enum MyVisibility {
    PubT,
    PubS,
    PubI(MyPath),
    Pri,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MyPath {
    name: String,
    next: Option<Box<MyPath>>,
}

impl MyPath {
    pub fn none() -> Self {
        MyPath {
            name: String::new(),
            next: None,
        }
    }

    pub fn new(import_path: &String) -> Self {
        let mut my_path = MyPath::none();
        let paths = import_path.split("::").collect::<Vec<&str>>();
        my_path.name = paths[0].to_string();
        if paths.len() > 1 {
            my_path.next = Some(Box::new(MyPath::new(&paths[1..].join("::"))));
        }
        my_path
    }

    pub fn new_with_copy(other_path: &MyPath) -> MyPath {
        MyPath::new(&other_path.to_string())
    }

    fn get_names_recursively(&self, names: &mut Vec<String>) {
        names.push(self.name.clone());
        if let Some(next) = &self.next {
            next.get_names_recursively(names);
        }
    }

    pub fn to_string(&self) -> String {
        let mut names: Vec<String> = Vec::new();
        self.get_names_recursively(&mut names);
        names.join("::")
    }

    fn up(&mut self) {
        if let None = self.next {
            return;
        }
        if let None = self.next.as_mut().unwrap().next {
            self.next = None;
        } else {
            self.next.as_mut().unwrap().up();
        }
    }

    fn down(&mut self, name: &String) {
        if let None = self.next {
            self.next = Some(Box::new(MyPath::new(&name)));
        } else {
            self.next.as_mut().unwrap().down(name);
        }
    }

    pub fn get_directly_use_tree(
        &mut self,
        direct_name: &String,
        original_path_string: &String,
        mod_context: &Rc<RefCell<ModContext>>,
        directly_use_tree: &mut MyPath,
        depth: usize,
    ) -> bool {
        // println!("{}", self.to_string());
        // println!("{}", mod_context.borrow().get_mod_tree().to_string());
        // println!("{}", direct_name);
        // println!();
        if depth > 50 {
            return true;
        }
        let mut crate_name: String = String::new();
        if mod_context.borrow().is_crate() {
            crate_name = mod_context.borrow().get_mod_name();
        } else {
            crate_name = mod_context.borrow().get_crate().borrow().get_mod_name();
        }
        if direct_name == "*" {
            return false;
        }
        if self.name == "crate" {
            if mod_context.borrow().is_crate() {
                *directly_use_tree = MyPath::new(&mod_context.borrow().get_mod_name());
                return self.next.as_mut().unwrap().get_directly_use_tree(
                    direct_name,
                    original_path_string,
                    mod_context,
                    directly_use_tree,
                    depth + 1,
                );
            } else {
                *directly_use_tree =
                    MyPath::new(&mod_context.borrow().get_crate().borrow().get_mod_name());
                return self.next.as_mut().unwrap().get_directly_use_tree(
                    direct_name,
                    original_path_string,
                    mod_context.borrow().get_crate(),
                    directly_use_tree,
                    depth + 1,
                );
            }
        } else if self.name == "super" {
            *directly_use_tree = MyPath::new_with_copy(
                &ModContext::get_parent_recursively(mod_context)
                    .borrow()
                    .get_mod_tree(),
            );
            return self.next.as_mut().unwrap().get_directly_use_tree(
                direct_name,
                original_path_string,
                &ModContext::get_parent_recursively(mod_context),
                directly_use_tree,
                depth + 1,
            );
        } else if self.name == "self" {
            // println!("{}", self.to_string());
            *directly_use_tree = MyPath::new_with_copy(&mod_context.borrow().get_mod_tree());
            if let None = self.next {
                return false;
            }
            return self.next.as_mut().unwrap().get_directly_use_tree(
                direct_name,
                original_path_string,
                mod_context,
                directly_use_tree,
                depth + 1,
            );
        } else {
            let pub_uses = mod_context.borrow().get_pub_use();
            for pub_use in pub_uses.iter() {
                let new_original_path_string = pub_use.get_use_tree().to_string();
                let alias = pub_use.get_alias();
                if let Some(alias) = alias {
                    if alias == direct_name {
                        if mod_context.borrow().is_crate() {
                            *directly_use_tree = MyPath::new(&mod_context.borrow().get_mod_name());
                            return pub_use.get_use_tree().clone().get_directly_use_tree(
                                pub_use.get_name(),
                                &new_original_path_string,
                                mod_context,
                                directly_use_tree,
                                depth + 1,
                            );
                        } else {
                            *directly_use_tree = MyPath::new(
                                &mod_context.borrow().get_crate().borrow().get_mod_name(),
                            );
                            return pub_use.get_use_tree().clone().get_directly_use_tree(
                                pub_use.get_name(),
                                &new_original_path_string,
                                mod_context,
                                directly_use_tree,
                                depth + 1,
                            );
                        }
                    }
                }
                if pub_use.get_name() == direct_name
                    && !new_original_path_string.eq(original_path_string)
                {
                    {
                        if mod_context.borrow().is_crate() {
                            *directly_use_tree = MyPath::new(&mod_context.borrow().get_mod_name());
                            return pub_use.get_use_tree().clone().get_directly_use_tree(
                                pub_use.get_name(),
                                &new_original_path_string,
                                mod_context,
                                directly_use_tree,
                                depth + 1,
                            );
                        } else {
                            *directly_use_tree = MyPath::new(
                                &mod_context.borrow().get_crate().borrow().get_mod_name(),
                            );
                            return pub_use.get_use_tree().clone().get_directly_use_tree(
                                pub_use.get_name(),
                                &new_original_path_string,
                                mod_context,
                                directly_use_tree,
                                depth + 1,
                            );
                        }
                    }
                }
            }
            for sub_mod in mod_context.borrow().get_sub_mods() {
                if sub_mod.borrow().is_mod_mod() && sub_mod.borrow().get_mod_name() == self.name {
                    if let None = self.next {
                        directly_use_tree.down(&self.name);
                        return true;
                    } else {
                        directly_use_tree.down(&self.name);
                        return self.next.as_mut().unwrap().get_directly_use_tree(
                            direct_name,
                            original_path_string,
                            sub_mod,
                            directly_use_tree,
                            depth + 1,
                        );
                    }
                }
            }
            if mod_context
                .borrow()
                .has_fn_struct_enum_union_trait(&self.name)
            {
                directly_use_tree.down(&self.name);
                return true;
            }
            if self.name == crate_name {
                if mod_context.borrow().is_crate() {
                    *directly_use_tree = MyPath::new(&crate_name);
                    return self.next.as_mut().unwrap().get_directly_use_tree(
                        direct_name,
                        original_path_string,
                        mod_context,
                        directly_use_tree,
                        depth + 1,
                    );
                } else {
                    *directly_use_tree = MyPath::new(&crate_name);
                    return self.next.as_mut().unwrap().get_directly_use_tree(
                        direct_name,
                        original_path_string,
                        mod_context.borrow().get_crate(),
                        directly_use_tree,
                        depth + 1,
                    );
                }
            }
            return false;
        }
    }

    pub fn get_depth(&self) -> i32 {
        if self.next.is_none() {
            return 1;
        } else {
            return 1 + self.next.as_ref().unwrap().get_depth();
        }
    }

    pub fn is_parent(&self, another_path: &MyPath) -> bool {
        if self.name == another_path.name {
            if let None = self.next {
                if let Some(_) = another_path.next {
                    return true;
                } else {
                    return false;
                }
            } else {
                if let None = another_path.next {
                    return false;
                } else {
                    return self
                        .next
                        .as_ref()
                        .unwrap()
                        .is_parent(&another_path.next.as_ref().unwrap());
                }
            }
        } else {
            return false;
        }
    }

    pub fn connect(&self, another_path: &MyPath) -> MyPath {
        let mut new_self = self.deep_copy();
        let mut current = &mut new_self;

        while let Some(ref mut next) = current.next {
            current = next;
        }

        let new_another = another_path.deep_copy();
        current.next = Some(Box::new(new_another));

        new_self
    }

    fn deep_copy(&self) -> MyPath {
        MyPath {
            name: self.name.clone(),
            next: self.next.as_ref().map(|next| Box::new(next.deep_copy())),
        }
    }

    // pub fn connect(&self, another_path: &MyPath) -> MyPath {
    //     let mut another_path = another_path.clone();
    //     let mut new_path = self.clone();
    //     // println!("{}", self.to_string());
    //     // println!("{}", another_path.to_string());
    //     // println!();
    //     while let Some(path) = another_path.next {
    //         new_path.down(&another_path.name);
    //         if let None = path.next {
    //             break;
    //         }
    //         let sub_path = *path.next.unwrap().clone();
    //         another_path = sub_path;
    //     }
    //     if another_path.name != "" {
    //         new_path.down(&another_path.name);
    //     }
    //     new_path
    // }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Name {
    name: String,
    complete_name: String,
    import_name: MyPath,
}

impl Name {
    pub fn none() -> Self {
        Name {
            name: String::new(),
            complete_name: String::new(),
            import_name: MyPath::none(),
        }
    }

    pub fn new(name: &String) -> Self {
        Name {
            name: name.clone(),
            complete_name: String::new(),
            import_name: MyPath::none(),
        }
    }

    pub fn insert_complete_name(&mut self, complete_name: &String) {
        self.complete_name = complete_name.clone();
    }

    pub fn insert_import_name(&mut self, import_name: &String) {
        self.import_name = MyPath::new(import_name);
    }

    pub fn insert_parent_mod_tree_for_fn_struct_enum_union_trait(
        &mut self,
        parent_mod_tree: &String,
    ) {
        let complete_name = parent_mod_tree.clone() + "::" + &self.name;
        self.complete_name = complete_name.clone();
        self.insert_import_name(&complete_name);
    }

    pub fn get_name(&self) -> String {
        self.name.clone()
    }

    pub fn get_import_name(&self) -> MyPath {
        self.import_name.clone()
    }

    pub fn get_import_name_depth(&self) -> i32 {
        self.import_name.get_depth()
    }

    pub fn change_name_for_impl_struct_name(&mut self, mod_context: &Rc<RefCell<ModContext>>) {
        let name = self.get_name();
        let depth = self.get_import_name_depth();
        if depth == 1 {
            let new_name = mod_context
                .borrow()
                .get_struct_enum_union_name_from_syntax(&name);
            self.name = new_name.name;
            self.complete_name = new_name.complete_name;
            self.import_name = new_name.import_name;
        }
        // println!("{:#?}", impl_item.get_struct_name());
        else {
            let mut directly_use_tree = MyPath::new(&mod_context.borrow().get_mod_name());
            let original_path_string = self.import_name.to_string();
            if self.import_name.get_directly_use_tree(
                &self.name,
                &original_path_string,
                mod_context,
                &mut directly_use_tree,
                0,
            ) {
                self.import_name = directly_use_tree;
            }
        }
    }

    pub fn change_name_for_impl_trait_name(&mut self, mod_context: &Rc<RefCell<ModContext>>) {
        let name = self.get_name();
        let depth = self.get_import_name_depth();
        if depth == 1 {
            let new_name = mod_context.borrow().get_trait_name_from_syntax(&name);
            self.name = new_name.name;
            self.complete_name = new_name.complete_name;
            self.import_name = new_name.import_name;
        }
        // println!("{:#?}", impl_item.get_struct_name());
        else {
            let mut directly_use_tree = MyPath::new(&mod_context.borrow().get_mod_name());
            let original_path_string = self.import_name.to_string();
            if self.import_name.get_directly_use_tree(
                &self.name,
                &original_path_string,
                mod_context,
                &mut directly_use_tree,
                0,
            ) {
                self.import_name = directly_use_tree;
            }
        }
    }
}

// #[derive(Debug, Clone, PartialEq)]
// pub struct Applications {
//     applications: Vec<String>,
// }

// impl Applications {
//     pub fn new() -> Self {
//         Applications {
//             applications: Vec::new(),
//         }
//     }

//     pub fn insert_application(&mut self, application: &String) {
//         self.applications.push(application.clone());
//     }

//     pub fn insert_applications(&mut self, applications: &Vec<String>) {
//         self.applications = applications.clone()
//     }

//     pub fn get_applications(&self) -> Vec<String> {
//         return self.applications.clone();
//     }
// }

#[derive(Debug, Clone, PartialEq)]
pub enum FunctionItem {
    FnItem(FnItem),
    ImplFnItem(ImplFnItem),
    TraitFnItem(TraitFnItem),
}

impl FunctionItem {
    pub fn get_complete_function_name_in_file(&self) -> String {
        match self {
            FunctionItem::FnItem(fn_item) => fn_item.get_complete_function_name_in_file(),
            FunctionItem::ImplFnItem(impl_fn_item) => {
                impl_fn_item.get_complete_function_name_in_file()
            }
            FunctionItem::TraitFnItem(trait_fn_item) => {
                trait_fn_item.get_complete_function_name_in_file()
            }
        }
    }

    pub fn get_items(&self) -> Vec<Item> {
        match self {
            FunctionItem::FnItem(fn_item) => fn_item.get_items(),
            FunctionItem::ImplFnItem(impl_fn_item) => impl_fn_item.get_items(),
            FunctionItem::TraitFnItem(trait_fn_item) => trait_fn_item.get_items(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConstItem {
    item: Option<ItemConst>,
    visibility: MyVisibility,
}

impl ConstItem {
    pub fn new() -> Self {
        ConstItem {
            item: None,
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_item(&mut self, item: &ItemConst) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ItemConst {
        return self.item.clone().unwrap();
    }

    pub fn to_item(&self) -> Item {
        Item::Const(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitAliasItem {
    item: Option<ItemTraitAlias>,
    visibility: MyVisibility,
}

impl TraitAliasItem {
    pub fn new() -> Self {
        TraitAliasItem {
            item: None,
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_item(&mut self, item: &ItemTraitAlias) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ItemTraitAlias {
        return self.item.clone().unwrap();
    }

    pub fn to_item(&self) -> Item {
        Item::TraitAlias(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UseItem {
    item: Option<ItemUse>,
    visibility: MyVisibility,
}

impl UseItem {
    pub fn new() -> Self {
        UseItem {
            item: None,
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_item(&mut self, item: &ItemUse) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ItemUse {
        return self.item.clone().unwrap();
    }

    pub fn to_item(&self) -> Item {
        Item::Use(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModItem {
    mod_name: String,
    file_name: Option<String>,
    item: Option<ItemMod>,
    // inline: bool,
    inside_items: Vec<Item>,
    visibility: MyVisibility,
}

impl ModItem {
    pub fn new() -> Self {
        ModItem {
            mod_name: String::new(),
            file_name: None,
            item: None,
            inside_items: Vec::new(),
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_mod_name(&mut self, mod_name: &String) {
        self.mod_name = mod_name.clone();
    }

    pub fn insert_file_name(&mut self, file_name: &String) {
        self.file_name = Some(file_name.clone());
    }

    pub fn insert_item(&mut self, item: &ItemMod) {
        self.item = Some(item.clone());
    }

    pub fn insert_items(&mut self, items: &Vec<Item>) {
        self.inside_items = items.clone();
    }

    pub fn get_mod_name(&self) -> String {
        return self.mod_name.clone();
    }

    pub fn get_file_name(&self) -> Option<String> {
        return self.file_name.clone();
    }

    pub fn get_items(&self) -> Vec<Item> {
        return self.inside_items.clone();
    }

    pub fn get_item(&self) -> ItemMod {
        return self.item.clone().unwrap();
    }

    pub fn has_inside(&self) -> bool {
        if self.inside_items.len() > 0 {
            return true;
        } else {
            return false;
        }
    }

    pub fn to_item(&self) -> Item {
        let mut item = self.item.clone().unwrap();
        if self.has_inside() {
            if let Some(items) = &mut item.content {
                items.1.extend(self.inside_items.clone());
            }
        }
        Item::Mod(item.clone())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }

    pub fn get_visibility(&self) -> MyVisibility {
        self.visibility.clone()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StaticItem {
    item: Option<ItemStatic>,
    visibility: MyVisibility,
}

#[derive(Debug, Clone, PartialEq)]

pub struct MacroItem {
    item: Option<ItemMacro>,
}

impl MacroItem {
    pub fn new() -> Self {
        MacroItem { item: None }
    }

    pub fn insert_item(&mut self, item: &ItemMacro) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ItemMacro {
        return self.item.clone().unwrap();
    }

    pub fn to_item(&self) -> Item {
        Item::Macro(self.item.clone().unwrap())
    }
}

impl StaticItem {
    pub fn new() -> Self {
        StaticItem {
            item: None,
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_item(&mut self, item: &ItemStatic) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ItemStatic {
        return self.item.clone().unwrap();
    }

    pub fn to_item(&self) -> Item {
        Item::Static(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeItem {
    item: Option<ItemType>,
    visibility: MyVisibility,
}

impl TypeItem {
    pub fn new() -> Self {
        TypeItem {
            item: None,
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_item(&mut self, item: &ItemType) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ItemType {
        return self.item.clone().unwrap();
    }

    pub fn to_item(&self) -> Item {
        Item::Type(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

#[derive(Debug, Clone)]
pub struct FnItem {
    fn_name: Name,
    complete_name_in_file: String,
    item: Option<ItemFn>,
    // has_items: bool,
    inside_items: Vec<Item>,
    // application: Applications,
    visibility: MyVisibility,
}

impl FnItem {
    pub fn new() -> Self {
        FnItem {
            fn_name: Name::none(),
            complete_name_in_file: String::new(),
            // complete_function_name_in_file: String::new(),
            item: None,
            inside_items: Vec::new(),
            // application: Applications::new(),
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_function_name(&mut self, fn_name: &String) {
        self.fn_name = Name::new(fn_name);
    }

    pub fn insert_complete_name_in_file(&mut self, prefix: &String) {
        if prefix.eq("") {
            self.complete_name_in_file = self.fn_name.get_name();
        } else {
            self.complete_name_in_file = prefix.clone() + "::" + &self.fn_name.get_name();
        }
    }

    pub fn insert_item(&mut self, item: &ItemFn) {
        self.item = Some(item.clone());
    }

    pub fn insert_items(&mut self, items: &Vec<Item>) {
        self.inside_items = items.clone();
    }

    pub fn has_inside(&self) -> bool {
        if self.inside_items.len() > 0 {
            return true;
        } else {
            return false;
        }
    }

    pub fn get_complete_function_name_in_file(&self) -> String {
        self.complete_name_in_file.clone()
    }

    // pub fn get_function_name(&self) -> String {
    //     return self.function_name.clone();
    // }

    pub fn get_items(&self) -> Vec<Item> {
        return self.inside_items.clone();
    }

    pub fn insert_parent_mod_tree(&mut self, mod_tree: &String) {
        self.fn_name
            .insert_parent_mod_tree_for_fn_struct_enum_union_trait(mod_tree);
    }

    pub fn get_name(&self) -> String {
        self.fn_name.get_name()
    }

    pub fn get_complete_name(&self) -> String {
        self.fn_name.get_import_name().to_string()
    }

    pub fn clear(&mut self) {
        self.item.as_mut().unwrap().block.stmts.clear();
    }

    // pub fn get_complete_function_name_in_file(&self) -> String {
    //     return self.complete_function_name_in_file.clone();
    // }

    // pub fn insert_applications(&mut self, applications: &Vec<String>) {
    //     self.application.insert_applications(applications);
    // }

    // pub fn get_applications(&self) -> Vec<String> {
    //     return self.application.get_applications();
    // }

    // pub fn get_item(&self) -> ItemFn {
    //     if let MyItemFn::Fn(item_function) = self.item.clone().unwrap() {
    //         return item_function;
    //     } else {
    //         eprintln!("Failed to get a fn item!");
    //         process::exit(12);
    //     }
    // }

    pub fn to_item(&self) -> Item {
        Item::Fn(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

impl PartialEq for FnItem {
    fn eq(&self, other: &Self) -> bool {
        self.fn_name == other.fn_name
    }
}

#[derive(Debug, Clone)]
pub struct ImplTypeItem {
    item: Option<ImplItemType>,
    visibility: MyVisibility,
}

impl ImplTypeItem {
    pub fn new() -> Self {
        ImplTypeItem {
            item: None,
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_item(&mut self, item: &ImplItemType) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ImplItemType {
        self.item.clone().unwrap()
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

#[derive(Debug, Clone)]
pub struct ImplConstItem {
    item: Option<ImplItemConst>,
    visibility: MyVisibility,
}

impl ImplConstItem {
    pub fn new() -> Self {
        ImplConstItem {
            item: None,
            visibility: MyVisibility::Pri,
        }
    }

    pub fn insert_item(&mut self, item: &ImplItemConst) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> ImplItemConst {
        self.item.clone().unwrap()
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }
}

#[derive(Debug, Clone)]
pub struct ImplFnItem {
    fn_name: Name,
    complete_name_in_file: String,
    item: Option<ImplItemFn>,
    // has_items: bool,
    inside_items: Vec<Item>,
    visibility: MyVisibility,
    is_constructor: bool,
}

impl ImplFnItem {
    pub fn new() -> Self {
        ImplFnItem {
            fn_name: Name::none(),
            complete_name_in_file: String::new(),
            item: None,
            inside_items: Vec::new(),
            visibility: MyVisibility::Pri,
            is_constructor: false,
        }
    }

    pub fn insert_item(&mut self, item: &ImplItemFn) {
        self.item = Some(item.clone());
        if let ReturnType::Type(_, r_type) = &item.sig.output {
            match r_type {
                _ => {
                    let r_type_string = r_type.to_token_stream().to_string();
                    if r_type_string.contains("Self")
                        || r_type_string.contains(self.get_name().as_str())
                    {
                        self.is_constructor = true;
                    }
                }
            }
        }
    }

    pub fn insert_fn_name(&mut self, fn_name: &String) {
        self.fn_name = Name::new(fn_name);
    }

    pub fn insert_complete_name_in_file(&mut self, prefix: &String) {
        if prefix.eq("") {
            self.complete_name_in_file = self.fn_name.get_name();
        } else {
            self.complete_name_in_file = prefix.clone() + "::" + &self.fn_name.get_name();
        }
    }

    pub fn insert_items(&mut self, items: &Vec<Item>) {
        self.inside_items = items.clone();
    }

    pub fn has_inside(&self) -> bool {
        if self.inside_items.len() > 0 {
            return true;
        } else {
            return false;
        }
    }

    pub fn get_complete_function_name_in_file(&self) -> String {
        self.complete_name_in_file.clone()
    }

    pub fn get_items(&self) -> Vec<Item> {
        return self.inside_items.clone();
    }

    pub fn get_item(&self) -> ImplItemFn {
        self.item.clone().unwrap()
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }

    pub fn change_name(
        &mut self,
        mod_context: &Rc<RefCell<ModContext>>,
        struct_name: &Name,
        trait_name: &Option<Name>,
    ) {
        if let None = trait_name {
            let mod_path = mod_context.borrow().get_mod_tree();
            let struct_path = struct_name.get_import_name();
            let mut up_struct_path = struct_path.clone();
            up_struct_path.up();
            if mod_path == up_struct_path {
                let mut fn_path = struct_path.clone();
                fn_path.down(&self.fn_name.get_name());
                let complete_name = fn_path.to_string();
                self.fn_name.insert_complete_name(&complete_name);
                self.fn_name.insert_import_name(&complete_name);
            } else {
                let mut fn_path = mod_path.clone();
                let struct_path_string = struct_path.to_string();
                fn_path.down(&format!("<impl {}>", struct_path_string));
                fn_path.down(&self.fn_name.get_name());
                let complete_name = fn_path.to_string();
                self.fn_name.insert_complete_name(&complete_name);
                self.fn_name.insert_import_name(&complete_name);
            }
        } else {
            let struct_path = struct_name.get_import_name();
            let trait_path = trait_name.as_ref().unwrap().get_import_name();
            let fn_path_string = format!(
                "<{} as {}>",
                struct_path.to_string(),
                trait_path.to_string()
            ) + "::"
                + &self.fn_name.get_name();
            self.fn_name.insert_complete_name(&fn_path_string);
            self.fn_name.insert_import_name(&fn_path_string);
        }
    }

    pub fn get_name(&self) -> String {
        self.fn_name.get_name()
    }

    pub fn get_complete_name(&self) -> String {
        self.fn_name.get_import_name().to_string()
    }

    pub fn clear(&mut self) {
        if !self.is_constructor {
            self.item.as_mut().unwrap().block.stmts.clear();
        }
    }
}

impl PartialEq for ImplFnItem {
    fn eq(&self, other: &Self) -> bool {
        self.fn_name == other.fn_name
    }
}

#[derive(Debug, Clone)]
pub struct ImplItem {
    impl_num: i32,
    struct_name: Name,
    trait_name: Option<Name>,
    item: Option<ItemImpl>,
    types: Vec<ImplTypeItem>,
    consts: Vec<ImplConstItem>,
    functions: Vec<ImplFnItem>,
    // applications: Applications,
    relative_types: Vec<String>,
}

impl ImplItem {
    pub fn new() -> Self {
        ImplItem {
            impl_num: 0,
            struct_name: Name::none(),
            trait_name: None,
            item: None,
            types: Vec::new(),
            consts: Vec::new(),
            functions: Vec::new(),
            relative_types: Vec::new(),
            // applications: Applications::new(),
        }
    }

    pub fn insert_impl_num(&mut self, impl_num: i32) {
        self.impl_num = impl_num;
    }

    pub fn insert_struct_name(&mut self, struct_name: &String) {
        self.struct_name = Name::new(struct_name);
    }

    pub fn insert_struct_import_name(&mut self, import_name: &String) {
        self.struct_name.insert_import_name(import_name);
    }

    pub fn change_struct_name(&mut self, name: &Name) {
        self.struct_name = name.clone();
    }

    pub fn insert_trait_name(&mut self, trait_name: &String) {
        self.trait_name = Some(Name::new(trait_name));
    }

    pub fn insert_trait_import_name(&mut self, import_name: &String) {
        self.trait_name
            .as_mut()
            .unwrap()
            .insert_import_name(import_name);
    }

    pub fn change_trait_name(&mut self, name: &Name) {
        self.trait_name = Some(name.clone());
    }

    pub fn insert_item(&mut self, item: &ItemImpl) {
        self.item = Some(item.clone());
    }

    pub fn insert_type(&mut self, item: &ImplTypeItem) {
        self.types.push(item.clone());
    }

    pub fn insert_const(&mut self, item: &ImplConstItem) {
        self.consts.push(item.clone());
    }

    pub fn insert_function(&mut self, item: &ImplFnItem) {
        self.functions.push(item.clone());
    }

    pub fn get_impl_num(&self) -> i32 {
        return self.impl_num;
    }

    pub fn get_item(&self) -> &ItemImpl {
        self.item.as_ref().unwrap()
    }

    pub fn get_function_with_inside(&self) -> Vec<ImplFnItem> {
        let mut functions_with_inside: Vec<ImplFnItem> = Vec::new();
        for impl_fn_item in self.functions.iter() {
            if impl_fn_item.has_inside() {
                functions_with_inside.push(impl_fn_item.clone());
            }
        }
        functions_with_inside
    }

    pub fn to_item(&self) -> Item {
        let mut item_impl = self.item.clone().unwrap();
        for impl_type_item in self.types.iter() {
            item_impl
                .items
                .push(syn::ImplItem::Type(impl_type_item.get_item()));
        }
        for impl_const_item in self.consts.iter() {
            item_impl
                .items
                .push(syn::ImplItem::Const(impl_const_item.get_item()));
        }
        for impl_fn_item in self.functions.iter() {
            item_impl
                .items
                .push(syn::ImplItem::Fn(impl_fn_item.get_item()));
        }
        Item::Impl(item_impl)
    }

    pub fn get_fns(&self) -> &Vec<ImplFnItem> {
        &self.functions
    }

    pub fn get_struct_name(&self) -> &Name {
        &self.struct_name
    }

    pub fn get_trait_name(&self) -> &Option<Name> {
        &self.trait_name
    }

    pub fn change_function_name(&mut self, mod_context: &Rc<RefCell<ModContext>>) {
        for function in self.functions.iter_mut() {
            function.change_name(mod_context, &self.struct_name, &self.trait_name);
        }
    }

    pub fn clear(&mut self) {
        for function in self.functions.iter_mut() {
            function.clear();
        }
    }

    pub fn insert_relative_types(&mut self, relative_types: Vec<String>) {
        self.relative_types.extend(relative_types);
    }

    pub fn get_relative_types(&self) -> Vec<String> {
        self.relative_types.clone()
    }

    pub fn change_function(&mut self, function: &ImplFnItem) {
        for f in self.functions.iter_mut() {
            if f.get_name() == function.get_name() {
                *f = function.clone();
                break;
            }
        }
    }

    pub fn has_function(&self, function_name: &String) -> bool {
        for function_item in self.functions.iter() {
            if function_item.get_complete_name().eq(function_name) {
                return true;
            }
        }
        return false;
    }

    // pub fn insert_applications(&mut self, applications: &Vec<String>) {
    //     self.applications.insert_applications(applications);
    // }

    // pub fn get_struct_name(&self) -> String {
    //     return self.struct_name.clone();
    // }

    // pub fn get_trait_name(&self) -> Option<String> {
    //     return self.trait_name.clone();
    // }

    // pub fn get_all_applications(&self) -> Vec<String> {
    //     let mut all_applications = self.applications.get_applications();
    //     for function_item in self.functions.iter() {
    //         all_applications.extend(function_item.get_applications());
    //     }
    //     all_applications
    // }

    // pub fn get_item(&self) -> ItemImpl {
    //     return self.item.clone().unwrap();
    // }
}

#[derive(Debug, Clone, PartialEq)]
pub struct StructItem {
    struct_name: Name,
    item: Option<ItemStruct>,
    // applications: Applications,
    visibility: MyVisibility,
    relative_types: Vec<String>,
}

impl StructItem {
    pub fn new() -> Self {
        StructItem {
            struct_name: Name::none(),
            item: None,
            // applications: Applications::new(),
            visibility: MyVisibility::Pri,
            relative_types: Vec::new(),
        }
    }

    pub fn insert_struct_name(&mut self, struct_name: &String) {
        self.struct_name = Name::new(struct_name);
    }

    pub fn insert_item(&mut self, item: &ItemStruct) {
        self.item = Some(item.clone());
    }

    pub fn to_item(&self) -> Item {
        Item::Struct(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }

    pub fn insert_parent_mod_tree(&mut self, mod_tree: &String) {
        self.struct_name
            .insert_parent_mod_tree_for_fn_struct_enum_union_trait(mod_tree);
    }

    pub fn get_struct_name(&self) -> &Name {
        &self.struct_name
    }

    pub fn get_name(&self) -> String {
        self.struct_name.get_name()
    }

    pub fn insert_relative_types(&mut self, relative_types: Vec<String>) {
        self.relative_types.extend(relative_types);
    }

    pub fn get_relative_types(&self) -> Vec<String> {
        self.relative_types.clone()
    }

    // pub fn insert_applications(&mut self, applications: &Vec<String>) {
    //     self.applications.insert_applications(applications);
    // }

    // pub fn get_struct_name(&self) -> String {
    //     return self.struct_name.clone();
    // }

    // pub fn get_applications(&self) -> Vec<String> {
    //     return self.applications.get_applications();
    // }

    // pub fn get_item(&self) -> ItemStruct {
    //     return self.item.clone().unwrap();
    // }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EnumItem {
    enum_name: Name,
    item: Option<ItemEnum>,
    // applications: Applications,
    visibility: MyVisibility,
    relative_types: Vec<String>,
}

impl EnumItem {
    pub fn new() -> Self {
        EnumItem {
            enum_name: Name::none(),
            item: None,
            // applications: Applications::new(),
            visibility: MyVisibility::Pri,
            relative_types: Vec::new(),
        }
    }

    pub fn insert_enum_name(&mut self, enum_name: &String) {
        self.enum_name = Name::new(enum_name);
    }

    pub fn insert_item(&mut self, item: &ItemEnum) {
        self.item = Some(item.clone());
    }

    pub fn to_item(&self) -> Item {
        Item::Enum(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }

    pub fn insert_parent_mod_tree(&mut self, mod_tree: &String) {
        self.enum_name
            .insert_parent_mod_tree_for_fn_struct_enum_union_trait(mod_tree);
    }

    pub fn get_name(&self) -> String {
        self.enum_name.get_name()
    }

    pub fn get_enum_name(&self) -> &Name {
        &self.enum_name
    }

    pub fn insert_relative_types(&mut self, relative_types: Vec<String>) {
        self.relative_types.extend(relative_types);
    }

    pub fn get_relative_types(&self) -> Vec<String> {
        self.relative_types.clone()
    }

    // pub fn insert_applications(&mut self, applications: &Vec<String>) {
    //     self.applications.insert_applications(applications);
    // }

    // pub fn get_enum_name(&self) -> String {
    //     return self.enum_name.clone();
    // }

    // pub fn get_applications(&self) -> Vec<String> {
    //     return self.applications.get_applications();
    // }

    // pub fn get_item(&self) -> ItemEnum {
    //     return self.item.clone().unwrap();
    // }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UnionItem {
    union_name: Name,
    item: Option<ItemUnion>,
    // applications: Applications,
    visibility: MyVisibility,
    relative_types: Vec<String>,
}

impl UnionItem {
    pub fn new() -> Self {
        UnionItem {
            union_name: Name::none(),
            item: None,
            // applications: Applications::new(),
            visibility: MyVisibility::Pri,
            relative_types: Vec::new(),
        }
    }

    pub fn insert_union_name(&mut self, union_name: &String) {
        self.union_name = Name::new(union_name);
    }

    pub fn insert_item(&mut self, item: &ItemUnion) {
        self.item = Some(item.clone());
    }

    pub fn to_item(&self) -> Item {
        Item::Union(self.item.clone().unwrap())
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }

    pub fn insert_parent_mod_tree(&mut self, mod_tree: &String) {
        self.union_name
            .insert_parent_mod_tree_for_fn_struct_enum_union_trait(mod_tree);
    }

    pub fn get_name(&self) -> String {
        self.union_name.get_name()
    }

    pub fn get_union_name(&self) -> &Name {
        &self.union_name
    }

    pub fn insert_relative_types(&mut self, relative_types: Vec<String>) {
        self.relative_types.extend(relative_types);
    }

    pub fn get_relative_types(&self) -> Vec<String> {
        self.relative_types.clone()
    }

    // pub fn insert_applications(&mut self, applications: &Vec<String>) {
    //     self.applications.insert_applications(applications);
    // }

    // pub fn get_union_name(&self) -> String {
    //     return self.union_name.clone();
    // }

    // pub fn get_applications(&self) -> Vec<String> {
    //     return self.applications.get_applications();
    // }

    // pub fn get_item(&self) -> ItemUnion {
    //     return self.item.clone().unwrap();
    // }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitTypeItem {
    item: Option<TraitItemType>,
}

impl TraitTypeItem {
    pub fn new() -> Self {
        TraitTypeItem { item: None }
    }

    pub fn insert_item(&mut self, item: &TraitItemType) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> TraitItemType {
        self.item.clone().unwrap()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitConstItem {
    item: Option<TraitItemConst>,
}

impl TraitConstItem {
    pub fn new() -> Self {
        TraitConstItem { item: None }
    }

    pub fn insert_item(&mut self, item: &TraitItemConst) {
        self.item = Some(item.clone());
    }

    pub fn get_item(&self) -> TraitItemConst {
        self.item.clone().unwrap()
    }
}

#[derive(Debug, Clone)]
pub struct TraitFnItem {
    fn_name: Name,
    complete_name_in_file: String,
    item: Option<TraitItemFn>,
    // has_items: bool,
    inside_items: Vec<Item>,
    is_constructor: bool,
}

impl TraitFnItem {
    pub fn new() -> Self {
        TraitFnItem {
            fn_name: Name::none(),
            complete_name_in_file: String::new(),
            item: None,
            inside_items: Vec::new(),
            is_constructor: false,
        }
    }

    pub fn insert_item(&mut self, item: &TraitItemFn) {
        self.item = Some(item.clone());
        if let ReturnType::Type(_, r_type) = &item.sig.output {
            match r_type {
                _ => {
                    let r_type_string = r_type.to_token_stream().to_string();
                    if r_type_string.contains("Self")
                        || r_type_string.contains(self.get_name().as_str())
                    {
                        self.is_constructor = true;
                    }
                }
            }
        }
    }

    pub fn insert_fn_name(&mut self, fn_name: &String) {
        self.fn_name = Name::new(fn_name);
    }

    pub fn insert_complete_name_in_file(&mut self, prefix: &String) {
        if prefix.eq("") {
            self.complete_name_in_file = self.fn_name.get_name();
        } else {
            self.complete_name_in_file = prefix.clone() + "::" + &self.fn_name.get_name();
        }
    }

    pub fn insert_items(&mut self, items: &Vec<Item>) {
        self.inside_items = items.clone();
    }

    pub fn has_inside(&self) -> bool {
        if self.inside_items.len() > 0 {
            return true;
        } else {
            return false;
        }
    }

    pub fn get_complete_function_name_in_file(&self) -> String {
        self.complete_name_in_file.clone()
    }

    pub fn get_items(&self) -> Vec<Item> {
        return self.inside_items.clone();
    }

    pub fn get_item(&self) -> TraitItemFn {
        self.item.clone().unwrap()
    }

    pub fn get_name(&self) -> String {
        self.fn_name.get_name()
    }

    pub fn get_complete_name(&self) -> String {
        self.fn_name.get_import_name().to_string()
    }

    pub fn change_name(&mut self, trait_name: &Name) {
        let struct_path = " ";
        let trait_path = trait_name.get_import_name();
        let fn_path_string = format!("<{} as {}>", struct_path, trait_path.to_string())
            + "::"
            + &self.fn_name.get_name();
        self.fn_name.insert_complete_name(&fn_path_string);
        self.fn_name.insert_import_name(&fn_path_string);
    }

    pub fn clear(&mut self) {
        if !self.is_constructor {
            if let Some(function_item) = &mut self.item {
                function_item.default = None;
            }
        }
    }
}

impl PartialEq for TraitFnItem {
    fn eq(&self, other: &Self) -> bool {
        self.fn_name == other.fn_name
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TraitItem {
    trait_name: Name,
    item: Option<ItemTrait>,
    types: Vec<TraitTypeItem>,
    consts: Vec<TraitConstItem>,
    functions: Vec<TraitFnItem>,
    // applications: Applications,
    visibility: MyVisibility,
    relative_types: Vec<String>,
}

impl TraitItem {
    pub fn new() -> Self {
        TraitItem {
            trait_name: Name::none(),
            item: None,
            types: Vec::new(),
            consts: Vec::new(),
            functions: Vec::new(),
            // applications: Applications::new(),
            visibility: MyVisibility::Pri,
            relative_types: Vec::new(),
        }
    }

    pub fn insert_trait_name(&mut self, trait_name: &String) {
        self.trait_name = Name::new(trait_name);
    }

    pub fn insert_item(&mut self, item: &ItemTrait) {
        self.item = Some(item.clone());
    }

    pub fn insert_type(&mut self, item: &TraitTypeItem) {
        self.types.push(item.clone());
    }

    pub fn insert_const(&mut self, item: &TraitConstItem) {
        self.consts.push(item.clone());
    }

    pub fn insert_function(&mut self, item: &TraitFnItem) {
        self.functions.push(item.clone());
    }

    pub fn get_trait_name(&self) -> &Name {
        &self.trait_name
    }

    pub fn get_trait_name_str(&self) -> String {
        return self.trait_name.get_name();
    }

    pub fn get_function_with_inside(&self) -> Vec<TraitFnItem> {
        let mut functions_with_inside: Vec<TraitFnItem> = Vec::new();
        for trait_fn_item in self.functions.iter() {
            if trait_fn_item.has_inside() {
                functions_with_inside.push(trait_fn_item.clone());
            }
        }
        functions_with_inside
    }

    pub fn to_item(&self) -> Item {
        let mut item_trait = self.item.clone().unwrap();
        for trait_type_item in self.types.iter() {
            item_trait
                .items
                .push(syn::TraitItem::Type(trait_type_item.get_item()));
        }
        for trait_const_item in self.consts.iter() {
            item_trait
                .items
                .push(syn::TraitItem::Const(trait_const_item.get_item()));
        }
        for trait_fn_item in self.functions.iter() {
            item_trait
                .items
                .push(syn::TraitItem::Fn(trait_fn_item.get_item()));
        }
        Item::Trait(item_trait)
    }

    pub fn get_fns(&self) -> &Vec<TraitFnItem> {
        &self.functions
    }

    pub fn insert_visibility(&mut self, visibility: MyVisibility) {
        self.visibility = visibility;
    }

    pub fn insert_parent_mod_tree(&mut self, mod_tree: &String) {
        self.trait_name
            .insert_parent_mod_tree_for_fn_struct_enum_union_trait(mod_tree);
    }

    pub fn get_name(&self) -> String {
        self.trait_name.get_name()
    }

    pub fn get_item(&self) -> &ItemTrait {
        self.item.as_ref().unwrap()
    }

    pub fn change_function_name(&mut self) {
        for function in self.functions.iter_mut() {
            function.change_name(&self.trait_name);
        }
    }

    pub fn insert_relative_types(&mut self, relative_types: Vec<String>) {
        self.relative_types.extend(relative_types);
    }

    pub fn get_relative_types(&self) -> Vec<String> {
        self.relative_types.clone()
    }

    pub fn clear(&mut self) {
        for function in self.functions.iter_mut() {
            function.clear();
        }
    }

    pub fn change_function(&mut self, function: &TraitFnItem) {
        for f in self.functions.iter_mut() {
            if f.get_name() == function.get_name() {
                *f = function.clone();
                break;
            }
        }
    }

    pub fn has_function(&self, function_name: &String) -> bool {
        for function_item in self.functions.iter() {
            if function_item.get_complete_name().eq(function_name) {
                return true;
            }
        }
        return false;
    }

    // pub fn insert_applications(&mut self, applications: &Vec<String>) {
    //     self.applications.insert_applications(applications);
    // }

    // pub fn get_all_applications(&self) -> Vec<String> {
    //     let mut all_aplications = self.applications.get_applications();
    //     for function_item in self.functions.iter() {
    //         all_aplications.extend(function_item.get_applications());
    //     }
    //     all_aplications
    // }

    // pub fn get_item(&self) -> ItemTrait {
    //     return self.item.clone().unwrap();
    // }
}

#[derive(Debug, Clone)]
pub struct UseTree {
    use_name: String,
    alias_name: Option<String>,
    use_tree: MyPath,
    visibility: MyVisibility,
}

impl UseTree {
    pub fn new(
        use_name: String,
        use_tree: String,
        alias_name: Option<String>,
        visibility: MyVisibility,
    ) -> Self {
        UseTree {
            use_name: use_name,
            alias_name: alias_name,
            use_tree: MyPath::new(&use_tree),
            visibility: visibility,
        }
    }

    pub fn insert_visibility(&mut self, visibility: &MyVisibility) {
        self.visibility = visibility.clone();
    }

    pub fn get_visibility(&self) -> &MyVisibility {
        &self.visibility
    }

    pub fn get_alias(&self) -> &Option<String> {
        &self.alias_name
    }

    pub fn get_use_tree(&self) -> &MyPath {
        &self.use_tree
    }

    pub fn get_name(&self) -> &String {
        &self.use_name
    }

    pub fn change_use_tree(&mut self, mod_context: &Rc<RefCell<ModContext>>) {
        let mut directly_use_tree = MyPath::new(&mod_context.borrow().get_mod_name());
        let original_path_string = self.use_tree.to_string();
        if self.use_tree.get_directly_use_tree(
            &self.use_name,
            &original_path_string,
            mod_context,
            &mut directly_use_tree,
            0,
        ) {
            self.use_tree = directly_use_tree;
        }
    }
}
