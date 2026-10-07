use std::{fs::File, path::PathBuf, process::Command};

use call_chain::analysis::hirvisitor::ImplInformation;

pub fn cargo_clean(work_path: &PathBuf) {
    // println!("Cleaning the project...");
    let clean_output = Command::new("cargo")
        .arg("clean")
        .current_dir(work_path)
        .output()
        .expect("Failed to clean the project");

    if !clean_output.status.success() {
        eprintln!("Clean failed.");
        return;
    }
}

fn call_chain(crate_path: &PathBuf) {
    let call_chain_output = Command::new("cargo")
        .arg("call-chain")
        .current_dir(crate_path)
        .output()
        .expect("Failed to run call_chain");

    if !call_chain_output.status.success() {
        eprintln!("Call_chain failed!");
        std::process::exit(11);
    }
}

pub fn run_call_chain(crate_path: &PathBuf) {
    cargo_clean(crate_path);
    call_chain(crate_path);
}

pub fn read_impl_informations_from_json(crate_path: &PathBuf) -> Vec<ImplInformation> {
    let impl_informations_path = crate_path.join("focxt/impl_informations.json");
    let mut impl_informations: Vec<ImplInformation> = Vec::new();
    if impl_informations_path.exists() {
        let file = File::open(impl_informations_path).unwrap();
        impl_informations = serde_json::from_reader(file).unwrap();
    }
    impl_informations
}

pub fn get_encoded_name(
    impl_informations: &Vec<ImplInformation>,
    mod_name: &String,
    fn_name: &String,
    struct_name: &String,
    trait_name: &String,
) -> Option<String> {
    for impl_information in impl_informations.iter() {
        if impl_information.mod_name.eq(mod_name)
            && impl_information.fn_name.eq(fn_name)
            && impl_information.struct_name.eq(struct_name)
            && impl_information.trait_name.eq(trait_name)
        {
            return Some(impl_information.encoded_name.clone());
        }
    }
    None
}
