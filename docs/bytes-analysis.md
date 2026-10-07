# bytes Analysis Validation

This check covers the complete analysis command on the bundled bytes 1.10.0 crate with its default features. PALM tools are built in release mode with the pinned `nightly-2025-03-19`; Cargo's target-analysis profile remains unchanged. No model service or coverage tool is required.

## Plan and Acceptance

The plan is to measure the existing release build, correct observed analysis blockers, and validate two fresh working copies before expanding to model experiments. Acceptance requires:

- Preprocessing and `utgen analyze` return success on both copies.
- All 663 exported branch functions have corresponding compiler identities, raw/normalized dependency files, and nonempty context files. No function is dropped to satisfy this count.
- Branch code and compiler-exported method code match their source locations; each context contains its focal function's declaration.
- A small separate crate distinguishes generic trait implementations, reference receivers, a trait default method, and a macro-generated forwarding method. Two impls share one source line, and an inactive `cfg` variant must not replace the active method.
- Analysis leaves prepared sources and the manifest unchanged. The bundled example is also unchanged.

The review identified two traps to avoid: debug-tool timings do not establish release performance, and nonempty context files alone do not establish correct method identity. The check therefore uses release tools and also verifies selected method bodies and their impl headers. Its 300-second per-command limit is an external regression-test deadline, not a new PALM timeout or a performance guarantee.

## Changes

The baseline emitted 663 function identities but only 226 contexts. Trait default contexts were constructed without being written. Matching methods by short module/type/trait names also conflated generic trait instantiations and reference receivers. Source parsing alone missed methods supplied by forwarding macros.

Call-chain now includes each function's source span, enclosing impl header span, and already extracted source code in its index. The source parser binds functions to these compiler identities before building the dependency lookup. Compiler-selected methods with parseable source are added to the matching impl when absent from the source AST. Inactive method bodies are excluded, while required trait signatures are retained. Context files use the compiler's existing encoded names, and trait default contexts are written normally.

Recursive dependency JSON reads use `BufReader`. No persistent cache or new cache invalidation scheme is introduced. `utgen analyze` reports the duration of clean, brinfo, and focxt; a missing context error identifies the function and path.

## Reproduce

From the repository root, with the pinned compiler and components installed:

```sh
cargo build --workspace --release --locked
python3 scripts/check_bytes.py
```

Use `--bin-dir <directory>` for a custom tool build directory and `--runs 1` for one bytes copy. Rebuild the tools together: focxt consumes the source metadata emitted by the matching call-chain binary. The script prints a temporary directory containing copies, per-command logs, stage timings, and `summary.json`. Its default is two copies; dependency downloads can require network access.

## Local Results

Measured on macOS/Apple Silicon with the pinned compiler. Tool compilation is excluded from the analysis times below.

| Run | Analyze | brinfo | focxt | Branch functions / contexts | Outcome |
| --- | ---: | ---: | ---: | ---: | --- |
| Previous main, release | 43.714 s | — | — | 663 / 226 | Missing context, exit 1 |
| Corrected, first copy | 34.632 s | 2.232 s | 32.297 s | 663 / 663 | Passed |
| Corrected, second copy | 34.747 s | 2.235 s | 32.396 s | 663 / 663 | Passed |

The before/after rows perform different amounts of successful work, so they are not a speedup benchmark. Both corrected copies passed the source and artifact checks; the method-identity fixture also passed. The check is part of Linux CI alongside the existing minimal pipeline, coverage compatibility, and scheduling regressions.

## Scope

The 663 functions are the functions exported by the current analyzers for this pinned crate/configuration, not a claim to cover every possible Rust function or feature combination. Recovering an already selected method from parseable source does not provide general macro expansion: a macro-generated impl or source containing unexpanded substitutions may still be unsupported. Required trait methods without bodies are context declarations rather than focal functions.

Full bytes generation, repair, model quality, and coverage experiments remain the next validation stage. This batch does not change their evaluation semantics.
