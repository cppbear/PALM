# build-utils

build-utils is an internal build dependency of [PALM](../README.md). The tool crates call `build_utils::setup_build()` from their `build.rs` scripts to configure the runtime search path for Rust compiler libraries.

`setup_build()` reads `rustc --print sysroot` and emits a linker rpath pointing to that toolchain's `lib` directory. Use the [pinned toolchain and components](../README.md#prerequisites) when building PALM.

## Build

From the PALM repository root:

```sh
cargo build -p build-utils --locked
```

This crate is built automatically with the tools. It has no CLI or separate installation step. To check its effect on the tool binaries, build the workspace with `cargo build --workspace --locked` and run the [development checks](../README.md#development-checks).
