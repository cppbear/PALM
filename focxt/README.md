# focxt

focxt constructs the source context for each focal function in [PALM](../README.md). It combines compiler-derived calls and types with source-level modules, declarations, and implementations to produce context for test-generation prompts.

## Build and install

Follow the [project prerequisites](../README.md#prerequisites), including the pinned `nightly-2025-03-19` toolchain. From the PALM repository root:

```sh
cargo build -p call_chain -p focxt --locked
cargo install --path focxt/call_chain --locked
cargo install --path focxt --locked
```

focxt invokes `cargo call-chain` at runtime, so install [call_chain](call_chain/README.md) as well as focxt and keep Cargo's binary directory in `PATH`. Rebuild and install both together after changes to their shared analysis data. No model configuration is needed.

## Usage

Use a working copy of a standalone crate with the same pinned toolchain. For the test-generation workflow, [preprocess and analyze with utgen](../utgen/README.md#analysis): it runs both brinfo and focxt and validates their outputs.

For a single-target package, construct context directly from prepared source:

```sh
focxt --crate /absolute/path/to/target-crate
```

`-c` is the short form of `--crate`. focxt runs `cargo clean` and `cargo call-chain` inside the target before building context; running call-chain separately is unnecessary for this command. Start with a fresh working copy when repeating analysis, since build-cache cleanup does not remove previous context artifacts.

Mixed packages use `utgen analyze`, which selects each Cargo entry and preserves separate context data. Binary contexts keep local definitions separate from the package library; referenced library functions and types are included in a labeled dependency section. Library preference applies to the final focal list, without discarding binary dependency information.

## Output

Paths are relative to the target crate:

| Path | Contents |
| --- | --- |
| `focxt/impl_informations.json` | Function identities, source locations, and encoded artifact identifiers from call-chain. |
| `focxt/<encoded>.rs` | Per-function context used by utgen. |
| `focxt/callsandtypes/<encoded>.json` | Direct calls and type dependencies from call-chain. |
| `focxt/new_callsandtypes/<encoded>.json` | Calls and types expanded during context construction. |

Additional text files under `focxt/` describe parsed declarations and module trees for debugging. Generation also requires [brinfo](../brinfo/README.md) output from the same prepared source.

## Scope and checks

Function contexts use call-chain's compiler identities and source positions, including distinct generic trait impls and reference receivers. Recoverable macro-generated method bodies are associated with their enclosing source impl; this is not general macro expansion.

See [minimal pipeline validation](../docs/minimal-pipeline.md) for an offline fixture check and [bytes analysis validation](../docs/bytes-analysis.md) for release builds, reproducible checks, and current limits. The [technical guide](../docs/palm-rust-unit-test-generation.md) explains context selection in more detail.
