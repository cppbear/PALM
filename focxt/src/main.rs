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
    let metadata = cargo_metadata::MetadataCommand::new()
        .manifest_path(crate_path.join("Cargo.toml"))
        .no_deps()
        .exec()
        .unwrap();
    let package = metadata.root_package().unwrap();
    let targets: Vec<_> = package
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "lib" || k == "bin"))
        .collect();
    let selected = std::env::var("PALM_TARGET_NAME").ok();
    let kind = std::env::var("PALM_TARGET_KIND").ok();
    let target = if let (Some(name), Some(kind)) = (&selected, &kind) {
        targets
            .iter()
            .copied()
            .find(|t| t.name == *name && t.kind.contains(kind))
            .expect("selected Cargo target not found")
    } else if targets.len() == 1 {
        targets[0]
    } else {
        eprintln!(
            "Multiple Cargo targets: use utgen analyze to collect and combine target contexts"
        );
        process::exit(2);
    };
    run_call_chain(&crate_path);
    let output_root = std::env::var_os("PALM_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate_path.clone());
    let impl_informations = read_impl_informations_from_json(&output_root);
    let mut crate_context = CrateContext::new(
        target.name.replace('-', "_"),
        target.src_path.clone().into_std_path_buf(),
        output_root.clone(),
    );
    let crate_path = output_root;

    crate_context.parse_crate();
    crate_context.change_all_names();
    crate_context.bind_compiler_functions(&impl_informations);

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
    use call_chain::analysis::exporter::CallsAndTypes;
    use collect_context::syntax_context::{collect_calls, type_context};
    if target.kind.iter().any(|k| k == "lib") && targets.len() > 1 {
        let types: std::collections::BTreeMap<_, _> = structs
            .keys()
            .map(|name| (name.clone(), type_context(name, &structs)))
            .collect();
        fs::write(
            crate_path.join("focxt/type_contexts.json"),
            serde_json::to_vec(&types).unwrap(),
        )
        .unwrap();
    }
    if target.kind.iter().any(|k| k == "bin") {
        if let Some(library_root) = std::env::var_os("PALM_LIBRARY_OUTPUT").map(PathBuf::from) {
            let library_infos = read_impl_informations_from_json(&library_root);
            let library_types: std::collections::BTreeMap<String, String> = serde_json::from_slice(
                &fs::read(library_root.join("focxt/type_contexts.json")).unwrap(),
            )
            .unwrap();
            for info in &impl_informations {
                let path = crate_path
                    .join("focxt/new_callsandtypes")
                    .join(format!("{}.json", info.encoded_name));
                let Ok(bytes) = fs::read(path) else { continue };
                let calls: CallsAndTypes = serde_json::from_slice(&bytes).unwrap();
                let calls = collect_calls(&crate_path.join("focxt"), &impl_informations, &calls);
                let mut snippets = std::collections::BTreeSet::new();
                for name in &calls.library_calls {
                    if let Some(library_info) = library_infos.iter().find(|i| &i.full_name == name)
                    {
                        let code = fs::read_to_string(
                            library_root
                                .join("focxt")
                                .join(format!("{}.rs", library_info.encoded_name)),
                        )
                        .unwrap();
                        snippets.insert(format!("// Function {name}\n{code}"));
                    }
                }
                for name in &calls.library_types {
                    if let Some(code) = library_types.get(name) {
                        snippets.insert(format!("// Type {name}\n{code}"));
                    }
                }
                if !snippets.is_empty() {
                    let mut file = fs::OpenOptions::new()
                        .append(true)
                        .open(
                            crate_path
                                .join("focxt")
                                .join(format!("{}.rs", info.encoded_name)),
                        )
                        .unwrap();
                    writeln!(
                        file,
                        "\n// Dependency context from library crate {}",
                        std::env::var("PALM_LIBRARY_NAME").unwrap()
                    )
                    .unwrap();
                    for snippet in snippets {
                        writeln!(file, "{snippet}").unwrap();
                    }
                }
            }
        }
    }
}
