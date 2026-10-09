# Minimal Pipeline Validation

These offline checks exercise PALM with small standalone crates, real Rust tools, and fixed local model responses. They verify analysis, generation, compilation repair, execution, coverage, and cleanup. No model credentials or external model calls are needed; dependency downloads may require network access.

## Run the Check

Follow the [project prerequisites](../README.md#prerequisites), then run from the repository root:

```sh
cargo build --workspace --locked
python3 scripts/check_minimal.py
```

The script uses the [minimal fixture](../examples/minimal/README.md) in temporary copies. It prints the working directory and finishes with `Minimal analysis/generation/repair/coverage checks passed.` The checked-in fixture remains unchanged.

Run the related checks when changing coverage/preprocessing or task scheduling:

```sh
python3 scripts/check_coverage.py
cargo test -p utgen --locked coverage_baselines_match_test_build_modes -- --ignored
python3 scripts/check_tasks.py
```

All three scripts are included in the full Linux CI suite. Their options differ:

| Script | Options |
| --- | --- |
| `check_minimal.py` | `--bin-dir <directory>`; `--work-dir <new-directory>`; `--analysis-only` to stop after analysis checks. |
| `check_coverage.py` | `--bin-dir <directory>`. |
| `check_tasks.py` | `--bin-dir <directory>`. |

The default binary directory is `target/debug`. The coverage tool must be available in `PATH` at the version specified in the prerequisites. `--work-dir` must name a new directory.

## Verified Contracts

| Area | Checked behavior |
| --- | --- |
| Preprocessing | Removes test-only source ranges, preserves byte offsets and newlines, and is idempotent on the prepared fixture. |
| Analysis | Runs after a prior `cargo check`; rejects old analysis directories; brinfo and focxt agree on all five fixture functions and their source locations. |
| Context | Includes the focal method, its type, and the fixture's transitive local dependencies. |
| Generation and repair | Fixed responses produce candidates; an intentionally invalid candidate fails compilation and is repaired using compiler diagnostics. |
| Coverage | Excludes test bodies and test-only helpers, preserves production coverage, and executes tests once when exporting XML and JSON. |
| Cleanup | Restores sources after the covered success, assertion-failure, compilation-failure, and report-export-failure cases. |
| Candidate timeout | Ordinary and `should_panic` infinite loops count as failures; an expected panic and a later ordinary test pass in unit and integration statistics. |
| Statistics | Oracle groups count once across multiple candidates; pass results match complete test names; all-compilation-failure results retain a zero-hit denominator without a synthetic candidate. A passing integration candidate that does not call the focal function has zero focal coverage. |
| Coverage baselines | No-test unit/library and binary harnesses; ordinary-library integration maps; conditional compilation, generics, no-map targets, per-target reuse, separation from real profiles, and source/helper restoration after build failure. |
| Integration repair | Repairs alias/trait imports, helpers, and test bodies in integration targets; keeps sibling candidates independent; checks cached repair, mode mismatch, unselected targets, and cleanup after request failure. |
| Integration directories | Original and generated tests coexist after normal runs and cached reruns; original-test compilation failure restores both directories; an existing staging directory is preserved and reported. |

The integration-directory cases retain the original `tests.bak/` while generating `tests/`. They use one cached candidate and make no additional model requests. Separate timeout cases also check integration execution without an original-test backup.

## Outputs and Coverage Interpretation

The minimal check's temporary directory contains `logs/`, separate `original/` and `generated/` copies, `model-requests.json`, and a success summary. The coverage-compatibility check keeps logs beside each temporary case; the task check prints its own working directory and writes logs and `summary.json` there. On failure, inspect the command log named by the script.

`original/original-coverage.json` and `.xml` measure production code exercised by the original unit and integration tests through `utgen coverage`. Test bodies and identified test-only helpers contribute to neither the coverage numerator nor denominator. The check combines branch outcomes when LLVM reports the same location in multiple test binaries.

`generated/utgen/result/` and `generated/utgen/fixed_result/` contain per-function statistics for generated candidates before and after repair. The separate `utgen/original_result.json` measures only the restored original integration tests on prepared source. It is not the complete original-test baseline. Combined original-plus-generated coverage is not measured by these checks.

## Coverage Compatibility Regression

`check_coverage.py` checks existing coverage attributes, conditional attributes with Cargo features enabled and disabled, test-only functions/methods/impls, file-level test modules, and expression fragments used by `include!`. Integration tests exercise both ordinary-library and test-harness compilation. The cases verify execution outcomes, production-only coverage, source restoration, and preprocessing offsets.

Explicit `coverage(on)` is respected. Expression fragments are preserved; the checks do not provide general macro expansion or infer that arbitrary unmarked helpers are test-only. See the [coverage reference](../utgen/README.md#coverage-of-existing-tests) for supported scope.

The baseline test is normally ignored because it needs the pinned coverage and LLVM tools. The explicit command above and Linux CI run it after installing those tools. It compares ordinary-library baseline maps with a real integration run, checks that a binary's `main` and existing test bodies never run during baseline collection, and removes its temporary fixtures afterward. Per-function `coverage_available=false` values must be excluded from coverage aggregates; they do not mean zero coverage of a known denominator.

## Task Scheduling Regression

`check_tasks.py` exercises function limits N=1, N=2, and the default N=4, with more than 2N functions in each scheduling case. It checks overlapping model requests, serial compiler activity, bounded-queue backpressure, and completion after queued work resumes. Cargo's target directory is reused between scenarios.

Additional cases cover direct and input/prefix/oracle generation, integration imports and repair concurrency, function selection, request budgets, and worker/consumer failures. Checks require source restoration, preservation of unrelated backups, retention of repair backups on failure, and skipped statistics after a candidate-stage error.

## Model Request Regression

Workspace unit tests use local HTTP fixtures for missing usage, empty or malformed responses, permanent errors, transient errors, dropped connections, and request timeouts. They check attempt counts and retry exhaustion. The task regression checks request summaries and cleanup after request failures. See the [request configuration reference](../utgen/README.md#prerequisites) for the current retry policy.

## Function Selection and Request Budgets

The task checks select functions while leaving other cached candidates untouched, verify that statistics follow the same selection, and check invocation records. Concurrent generation and repair must respect the shared attempt budget, including retries. CLI and request tests cover invalid selections, invalid limits, and successful completion on the final allowed attempt.

## Answers Without Tests

Direct-test and prefix cases check recovery after a helper-only response, exhaustion of format retries, and interruption by the request budget. Raw answers remain available before normalization. Prefixes can omit assertions because oracle generation follows separately; final candidates still undergo compilation and execution checks.

## Scope

The fixture checks verify deterministic behavior, not model quality or completeness for arbitrary Rust features. General multi-crate operation, macro expansion, and recovery after forced termination are outside their coverage.

The five-second candidate timeout applies to test bodies. Request timeouts and the regression harness's subprocess deadlines are separate; passing these checks does not establish a general Cargo-process deadline. Expected-panic statistics retain the current any-panic behavior described in the [generation reference](../utgen/README.md#generation).

Use [Bytes analysis validation](bytes-analysis.md) for a larger analysis target. For model-based experiments, follow the separately scoped [two-function trial](../examples/README.md#prepare-a-small-model-trial) and inspect request reports and test outcomes.
