# Bytes Analysis Validation

This offline check exercises preprocessing and analysis on the bundled bytes 1.10.0 crate with default features. It also checks method identity and constructor context in small fixtures. No model service or coverage tool is required.

## Run the Check

Follow the [project prerequisites](../README.md#prerequisites), then run from the repository root:

```sh
cargo build --workspace --release --locked
python3 scripts/check_bytes.py
```

The script uses release tool binaries from `target/release` and analyzes two fresh Bytes copies by default. Use `--bin-dir <directory>` for another binary directory or `--runs 1` for one Bytes copy. The method-identity and constructor fixtures run in either case. Build the tools together so focxt and call-chain use matching analysis data.

The script prints its temporary working directory and finishes with `All bytes analysis checks passed.` Dependency downloads can require network access. The bundled example remains unchanged.

## Checked Behavior

| Area | Acceptance |
| --- | --- |
| Bytes artifacts | All 663 exported functions have matching branch/context indices, dependency files, and nonempty context files containing their focal declarations. |
| Source identity | Exported function code matches its source location, accounting for brinfo's indentation normalization. |
| Method identity | Distinct generic trait implementations, reference receivers, default methods, and recoverable macro-generated methods retain the correct bodies and impl headers. Inactive `cfg` alternatives do not replace active methods. |
| Constructor context | Constructors returning `Self` or the concrete impl type retain their bodies, including the checked generic and macro-generated cases. Unrelated methods remain subject to context trimming. |
| Function lists | The unit-mode `bytes-smoke-2.txt` and `bytes-smoke-8.txt` selections match the analysis index and their expected representative condition chains. This script does not check the separate integration or mixed-target trial lists. |
| Source preservation | Analysis leaves the prepared sources and manifest unchanged. |

## Logs and Timings

The working directory contains the fixture copies, per-command logs, and `summary.json`. The summary records platform/compiler information, function/context counts, preprocessing time, total analysis time, and the clean/brinfo/focxt stage times for each Bytes run.

Tool compilation is excluded from those analysis timings. Compare runs using the same tool build profile and target configuration; the figures are measurements for that environment, not a performance guarantee. The script applies an external 300-second limit to each command. That limit is part of the regression harness, not a PALM CLI timeout.

## Scope

The 663 functions are those exported for this pinned crate and configuration. They do not establish coverage of every Rust function or feature combination. Recovering a selected method from parseable source does not provide general macro expansion; macro-generated impls or unexpanded substitutions may remain unsupported. Required trait methods without bodies are context declarations rather than focal functions.

This check runs in the full Linux CI suite. It verifies analysis artifacts and selected context behavior, without generating tests, performing compilation repair, or measuring model quality. See [minimal pipeline validation](minimal-pipeline.md) for deterministic generation/repair/coverage checks and the [small model trial](../examples/README.md#prepare-a-small-model-trial) for scoped experiments.
