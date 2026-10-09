# brinfo

brinfo extracts condition chains for Rust functions and methods using the compiler's HIR and MIR. In [PALM](../README.md), these chains supply path constraints for test generation.

## Build and install

Follow the [project prerequisites](../README.md#prerequisites), including the pinned `nightly-2025-03-19` toolchain. From the PALM repository root:

```sh
cargo build -p brinfo --locked
cargo install --path brinfo --locked
```

Installation places `cargo-brinfo` and its companion compiler driver `brinfo` in Cargo's binary directory. Ensure that directory is in `PATH`; invoke analysis through `cargo brinfo`. No model configuration is needed.

## Usage

Use a working copy of a standalone crate with the same pinned toolchain. For the test-generation workflow, [preprocess and analyze with utgen](../utgen/README.md#analysis): it runs brinfo and focxt and checks that their outputs agree.

To run only condition-chain extraction, enter the prepared target crate:

```sh
cd /absolute/path/to/target-crate
cargo clean
cargo brinfo
```

The wrapper analyzes a single ordinary `lib` or `bin` target through `cargo check`. For packages with both or with multiple binaries, use `utgen analyze`; it runs each target separately and combines the results with library preference for shared source definitions. Clearing the build cache ensures the compiler analysis runs even if the crate was previously checked. Use a fresh working copy without previous analysis output when repeating an experiment; `cargo clean` does not remove PALM's output files.

## Output

Paths are relative to the target crate:

| Path | Contents |
| --- | --- |
| `brinfo/name_map.json` | Full function names mapped to encoded artifact identifiers. |
| `brinfo/brdata/<encoded>.json` | Function source and location, condition chains, and representative-chain selection. |

Use the index to locate artifacts. Pass its full function names to `utgen gen --functions-file` or `utgen fix --functions-file` to select functions. Generation also needs [focxt](../focxt/README.md) output from the same prepared source.

For an offline check on the bundled fixture, see [minimal pipeline validation](../docs/minimal-pipeline.md). The [technical guide](../docs/palm-rust-unit-test-generation.md) explains condition-chain extraction in more detail.
