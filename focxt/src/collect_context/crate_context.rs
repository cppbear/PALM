use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    fs::{self, read_to_string, File},
    io::Write,
    path::PathBuf,
    process,
    rc::Rc,
};

use call_chain::analysis::hirvisitor::ImplInformation;
use syn::parse_file;
use toml::Value;

use super::{
    items_context::MyVisibility,
    mod_context::{ModContext, ModInfo, ModModInfo},
    result::{FnData, StructData},
    syntax_context::SyntaxContext,
};

#[derive(Debug, Clone)]
pub struct CrateContext {
    crate_name: String,
    crate_path: PathBuf,
    entry_file_paths: Vec<PathBuf>,
    main_mod_contexts: Vec<Rc<RefCell<ModContext>>>,
}

impl CrateContext {
    pub fn new(crate_path: &PathBuf) -> Self {
        let mut crate_context = CrateContext {
            crate_name: String::new(),
            crate_path: PathBuf::new(),
            entry_file_paths: Vec::new(),
            main_mod_contexts: Vec::new(),
        };
        let toml_path = crate_path.join("Cargo.toml");
        if fs::exists(&toml_path).unwrap() {
            let toml_content =
                read_to_string(toml_path).expect("Can not read the Cargo.toml file of the crate!");
            let toml_value: Value = toml_content
                .parse()
                .expect("Failed to parse the Cargo.toml file of the crate!");
            if let Some(package) = toml_value.get("package") {
                if let Some(name) = package.get("name") {
                    crate_context.crate_name = name.as_str().unwrap().to_string().replace("-", "_");
                } else {
                    eprintln!("Can not get the crate name of the crate!");
                    process::exit(2);
                }
            } else {
                eprintln!("Can not get the package infomation of the crate!");
                process::exit(3);
            }
        } else {
            eprintln!("Can not find the Cargo.toml file of the crate!");
            process::exit(4);
        }
        crate_context.crate_path = crate_path.clone();
        let main_path = crate_path.join("src/main.rs");
        let lib_path = crate_path.join("src/lib.rs");
        let mut has_entry = false;
        if fs::exists(&main_path).unwrap() {
            crate_context.entry_file_paths.push(main_path);
            has_entry = true;
        }
        if fs::exists(&lib_path).unwrap() {
            crate_context.entry_file_paths.push(lib_path);
            has_entry = true;
        }
        if has_entry == false {
            eprintln!("Can not find the entry file of the crate!");
            process::exit(5);
        }
        crate_context
    }

    pub fn parse_crate(&mut self) {
        for entry_file_path in self.entry_file_paths.iter() {
            let entry_code = read_to_string(entry_file_path).unwrap();
            let entry_syntax = parse_file(&entry_code).unwrap();
            let mut mod_mod_info = ModModInfo::new();
            mod_mod_info.insert_mod_name(&self.crate_name);
            mod_mod_info.insert_parent_mod_tree(&String::new());
            mod_mod_info.insert_file_path(entry_file_path);
            mod_mod_info
                .insert_parent_directory_path(&entry_file_path.parent().unwrap().to_path_buf());
            mod_mod_info.insert_visibility(MyVisibility::PubT);
            let mod_info = ModInfo::Mod(mod_mod_info);
            let mod_context = ModContext::new();
            mod_context.borrow_mut().insert_mod_info(&mod_info);
            ModContext::parse_from_items(
                &mod_context,
                &entry_syntax.items,
                &Some(Rc::clone(&mod_context)),
            );
            self.main_mod_contexts.push(mod_context);
        }
        if self.entry_file_paths.len() == 2 {
            self.main_mod_contexts[0]
                .borrow_mut()
                .add_use_mod(&self.main_mod_contexts[1]);
        }
    }

    pub fn change_all_names(&mut self) {
        for mod_context in self.main_mod_contexts.iter_mut() {
            mod_context
                .borrow_mut()
                .change_fn_struct_enum_union_trait_name();
        }
        for mod_context in self.main_mod_contexts.iter_mut() {
            ModContext::change_use_trees_recursively(&mod_context);
        }
        for mod_context in self.main_mod_contexts.iter_mut() {
            ModContext::change_impl_name_recursively(&mod_context);
        }
    }

    pub fn get_all_new_calls_and_types(
        &self,
        impl_informations: &Vec<ImplInformation>,
        mod_trees: &Vec<String>,
        fns: &HashMap<String, FnData>,
        structs: &HashMap<String, StructData>,
    ) {
        for mod_context in self.main_mod_contexts.iter() {
            mod_context.borrow().get_all_new_calls_and_types(
                &self.crate_path.join("focxt"),
                mod_trees,
                impl_informations,
                fns,
                structs,
                self,
            );
        }
    }

    pub fn parse_all_context(
        &self,
        impl_informations: &Vec<ImplInformation>,
        mod_trees: &Vec<String>,
        fns: &HashMap<String, FnData>,
        structs: &HashMap<String, StructData>,
    ) {
        for mod_context in self.main_mod_contexts.iter() {
            mod_context.borrow().get_all_context(
                &self.crate_path.join("focxt"),
                mod_trees,
                impl_informations,
                fns,
                structs,
                self,
            );
        }
    }

    pub fn cout_in_one_file_for_test(&self) {
        let output_path = self.crate_path.join("focxt/context.txt");
        fs::create_dir_all(output_path.parent().unwrap()).unwrap();
        let mut file = File::create(&output_path).unwrap();
        file.write_all(format!("{:#?}", self).as_bytes()).unwrap();
    }

    pub fn cout_all_mod_trees_in_one_file_for_test(&self, out_mod_trees: &mut HashSet<String>) {
        let output_path = self.crate_path.join("focxt/mod_trees");
        fs::create_dir_all(&output_path).unwrap();
        let mut num = 0;
        for mod_context in self.main_mod_contexts.iter() {
            let mut mod_trees: Vec<String> = Vec::new();
            mod_context.borrow().get_all_mod_trees(&mut mod_trees);
            for mod_tree in mod_trees.iter() {
                out_mod_trees.insert(mod_tree.clone());
            }
            let output_file_path = output_path.join(format!("mod_tree{}.txt", num));
            let mut file = File::create(&output_file_path).unwrap();
            file.write_all(format!("{:#?}", mod_trees).as_bytes())
                .unwrap();
            num += 1;
        }
    }

    pub fn cout_complete_function_name_in_on_file_for_test(&self) {
        let output_path = self.crate_path.join("focxt/functions");
        fs::create_dir_all(&output_path).unwrap();
        let mut num = 0;
        for mod_context in self.main_mod_contexts.iter() {
            let mut function_names: Vec<String> = Vec::new();
            mod_context
                .borrow()
                .get_complete_function_names(&mut function_names);
            function_names.sort();
            let output_file_path = output_path.join(format!("function{}.txt", num));
            let mut file = File::create(&output_file_path).unwrap();
            file.write_all(format!("{:#?}", function_names).as_bytes())
                .unwrap();
            num += 1;
        }
    }

    pub fn get_result(
        &self,
        fns: &mut HashMap<String, FnData>,
        structs: &mut HashMap<String, StructData>,
    ) {
        for main_mod_context in self.main_mod_contexts.iter() {
            main_mod_context.borrow().get_result(fns, structs);
        }
    }

    pub fn get_relative_types_for_struct(&self, name: &String, relative_types: &mut Vec<String>) {
        for main_mod_context in self.main_mod_contexts.iter() {
            main_mod_context
                .borrow()
                .get_relative_types_for_struct(name, relative_types);
        }
    }

    pub fn get_relative_types_for_trait(&self, name: &String, relative_types: &mut Vec<String>) {
        for main_mod_context in self.main_mod_contexts.iter() {
            main_mod_context
                .borrow()
                .get_relative_types_for_trait(name, relative_types);
        }
    }

    pub fn get_useful_information_for_function(
        &self,
        function_name: &String,
        syntax_context: &mut SyntaxContext,
    ) {
        for main_mod_context in self.main_mod_contexts.iter() {
            main_mod_context
                .borrow()
                .get_useful_information_for_function(function_name, syntax_context);
        }
    }
}
