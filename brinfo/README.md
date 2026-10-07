# brinfo

This tool is used to extract the condition chains of functions or methods in a specified project.

## Build

To build brinfo, run `cargo build` in the `brinfo` directory.

## Usage

Navigate to the directory of the project or subproject you want to analyze. In the root directory of the project being analyzed, you need to add a file named `rust-toolchain.toml` :

```toml
[toolchain]
channel = "nightly-2025-03-19"
components = ["rust-src", "rustc-dev", "llvm-tools-preview"]
```

You can start by running `cargo clean` to clear the existing build cache, and then run `cargo brinfo` in that directory to start the analysis.
