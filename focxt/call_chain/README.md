# call_chain

call_chain extracts function identities, direct calls, and type dependencies for [focxt](../README.md). It is a compiler-analysis component of [PALM](../../README.md), with the Cargo command `cargo call-chain` and a library shared with focxt.

## Build and install

Follow the [project prerequisites](../../README.md#prerequisites), including the pinned `nightly-2025-03-19` toolchain. From the PALM repository root:

```sh
cargo build -p call_chain --locked
cargo install --path focxt/call_chain --locked
```

Installation provides `cargo-call-chain` and its companion compiler driver `call-chain`. Keep Cargo's binary directory in `PATH` and invoke the tool through `cargo call-chain`. No model configuration is needed.

## Usage

focxt runs call-chain automatically. Use [utgen analysis](../../utgen/README.md#analysis) for the complete analysis stage, or [focxt](../README.md#usage) to construct context directly.

To inspect only compiler-derived calls and types, enter a fresh working copy of a prepared standalone crate using the same pinned toolchain:

```sh
cd /absolute/path/to/target-crate
cargo clean
cargo call-chain
```

The wrapper analyzes `lib` and `bin` targets through `cargo check`. Clearing the build cache ensures analysis executes after an earlier check; it does not remove previous `focxt/` output.

## Output

Paths are relative to the target crate:

| Path | Contents |
| --- | --- |
| `focxt/impl_informations.json` | Function and impl identities, source locations, and encoded artifact identifiers. |
| `focxt/callsandtypes/<encoded>.json` | Direct calls and type dependencies for each function. |
| `focxt/basic_blocks/<encoded>.txt` | MIR basic blocks and locals for debugging. |

call-chain supplies compiler data; focxt produces the per-function `.rs` context files used by generation. Changes to shared data should be checked with both tools. See [minimal pipeline validation](../../docs/minimal-pipeline.md) and [bytes analysis validation](../../docs/bytes-analysis.md).
