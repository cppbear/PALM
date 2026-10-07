use std::process::Command;

pub fn setup_build() {
    let sysroot = Command::new("rustc")
        .arg("--print")
        .arg("sysroot")
        .output()
        .expect("Failed to run rustc --print sysroot");
    let sysroot = String::from_utf8(sysroot.stdout).expect("Invalid UTF-8");
    let sysroot = sysroot.trim();

    println!("cargo:rustc-link-arg=-Wl,-rpath,{}/lib", sysroot);
}
