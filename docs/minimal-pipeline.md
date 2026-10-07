# Minimal Pipeline Validation

This batch follows the [build and configuration baseline](build-validation.md), merged through [PR #5](https://github.com/cppbear/PALM/pull/5). It validates one standalone crate with fixed local model responses, the pinned nightly, and `cargo-llvm-cov 0.6.16`.

## Run the Check

From the repository root:

```sh
cargo build --workspace --locked
cargo +stable install cargo-llvm-cov --version 0.6.16 --locked
python3 scripts/check_minimal.py
```

No model credentials are required. The model endpoint is a temporary loopback HTTP server owned by the script; target commands receive only a dummy key. Dependency downloads may require network access.

The script prints the temporary directory containing source copies, command logs, model-request counts, coverage, and generated results. Use `--work-dir <new-directory>` to select it, `--bin-dir <directory>` for a custom tool build directory, or `--analysis-only` to stop before coverage/generation. Work directories must be new. The [fixture](../examples/minimal/README.md) is never modified.

## Verified Contracts

- Original tests run on a separate original copy; coverage annotations are temporary and restored. A separate prepared copy is used for generated tests.
- Preprocessing removes test-only source ranges while preserving newlines and byte offsets, including a test and production function sharing one line. Repeating preprocessing is idempotent.
- `utgen analyze` succeeds even after a prior `cargo check`, since it clears the check cache before invoking the compiler wrapper. It rejects existing analysis output directories.
- brinfo and focxt agree on five complete function names. Source locations identify the exported focal code, accounting for brinfo's indentation normalization.
- The `Gauge::label` context contains its struct, the focal method, and the declarations for `classify`, `positive`, and the transitive `threshold` dependency.
- Missing context and malformed source locations fail before generation changes the manifest or source files. Source-location parse errors no longer panic.
- Six model responses produce six candidate tests. One candidate deliberately passes a string to an integer parameter and fails compilation; a numbered ChangeLog response repairs it through the actual repair path.
- Coverage executes each test body once for XML and JSON. Test functions, module helpers, standalone test-only helpers, and generated `llmtests` records are excluded; production coverage remains.
- Sources are restored after successful coverage, assertion failures, compilation failures, and JSON export failures.
- Both branches of `classify` are covered. Before repair, the intentionally invalid `double` candidate has zero compilable tests. After repair, all candidates compile and pass.
- Direct byte comparisons verify that source files and the target manifest match their prepared state after generation and repair; no source backup remains after successful completion.

The regression script runs in Linux CI after the ordinary workspace tests. It uses the real analyzers, compiler, coverage tool, prompt builder, response parser, candidate checks, and repair machinery. Model replies are deterministic fixtures; these results do not measure LLM quality.

## Coverage Interpretation

`original/original-coverage.json` and `.xml` measure production code exercised by all original unit and integration tests through `utgen coverage`. Test bodies and helpers contribute to neither the coverage numerator nor denominator. The check requires both original `classify` branches to have executed. LLVM can emit separate records for the same branch location in different test binaries; the check unions those outcomes instead of requiring both in one record.

`generated/utgen/result/` and `generated/utgen/fixed_result/` contain the tool's per-function statistics for generated candidates. The script checks the known branch count and execution outcomes directly. The separate legacy `utgen/original_result.json` restores only the integration-test backup; it must not be presented as coverage of all original tests. Combined original-plus-generated coverage is not part of this check.

## Changes and Limits

`utgen analyze` now invokes the tools and validates their artifacts. Its supported scope is one standalone crate with `Cargo.toml` and `src/`. Generation rejects incomplete indices/context and propagates failed asynchronous generation tasks. Repair validates the analysis and pre-repair JSON before starting.

The preprocessor parses `cfg` predicates conservatively, preserves `cfg(not(test))` and unknown production features, handles nested test ranges and UTF-8 safely, and refuses to overwrite `tests.bak`. Whitespace replacement preserves bytes/newlines, not necessarily character columns inside removed UTF-8 ranges; analysis runs after preprocessing.

Recursive context lookup now resolves full function names through the compiler's encoded-name index and consumes the accumulated dependencies. External functions without local artifacts remain outside this source context.

The local macOS/Apple Silicon run passed using six generation responses and one repair response. Real-model behavior, bytes-scale execution, general multi-crate support, request concurrency limits, hard process timeouts, recovery after forced termination, and explicit run/resume behavior remain later work. Git history is retained.

The coverage entry temporarily annotates sources with `#[coverage(off)]` and enables the nightly feature at crate roots without adding newlines. It preserves compiler flags, restores original bytes on returned errors, and performs no hashing or source fingerprint checks. `cargo llvm-cov report --json` reuses the first run's data; the first XML-producing command retains the tool's normal profile cleanup between candidates. This does not provide crash recovery or general macro expansion. See the [coverage command reference](../utgen/README.md#coverage-of-existing-tests) for scope.
