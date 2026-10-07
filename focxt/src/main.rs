#![feature(rustc_private)]

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File},
    io::Write,
    path::PathBuf,
    process,
};

use clap::Parser;
use collect_context::{
    crate_context::CrateContext,
    result::{FnData, StructData},
};
use utils::{read_impl_informations_from_json, run_call_chain};

mod collect_context;
mod utils;

#[derive(Parser)]
#[command(name = "rust focxt")]
#[command(version = "1.0")]
#[command(about="A rust program to get focal context for a crate.",long_about=None)]
struct Cli {
    ///Sets crate path
    #[arg(short = 'c', long = "crate", required = true)]
    crate_path: String,
}

fn main() {
    let cli = Cli::parse();
    let input_crate_path = PathBuf::from(cli.crate_path);
    let crate_path = fs::canonicalize(&input_crate_path).unwrap_or_else(|_err| {
        eprintln!("The crate path {:?} doesn't exisit!", &input_crate_path);
        process::exit(1)
    });
    run_call_chain(&crate_path);

    let impl_informations = read_impl_informations_from_json(&crate_path);

    let mut crate_context = CrateContext::new(&crate_path);

    crate_context.parse_crate();
    crate_context.change_all_names();

    let mut mod_trees: HashSet<String> = HashSet::new();
    crate_context.cout_all_mod_trees_in_one_file_for_test(&mut mod_trees);
    let mut mod_trees_vec: Vec<String> = Vec::new();
    for mod_tree in mod_trees.iter() {
        mod_trees_vec.push(mod_tree.clone());
    }
    let mod_trees = mod_trees_vec;

    let mut fns: HashMap<String, FnData> = HashMap::new();
    let mut structs: HashMap<String, StructData> = HashMap::new();
    crate_context.get_result(&mut fns, &mut structs);
    // println!("fns:\n{:#?}", fns);
    // println!("structs:\n{:#?}", structs);
    let output_path = crate_path.join("focxt/result.txt");
    fs::create_dir_all(output_path.parent().unwrap()).unwrap();
    let mut file = File::create(&output_path).unwrap();
    file.write_all(format!("fns:\n{:#?}\n", fns).as_bytes())
        .unwrap();
    file.write_all(format!("structs:\n{:#?}", structs).as_bytes())
        .unwrap();

    crate_context.get_all_new_calls_and_types(&impl_informations, &mod_trees, &fns, &structs);
    crate_context.parse_all_context(&impl_informations, &mod_trees, &fns, &structs);
    crate_context.cout_in_one_file_for_test();
    crate_context.cout_complete_function_name_in_on_file_for_test();
}
