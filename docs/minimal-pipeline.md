# Minimal Pipeline Validation

This batch follows the [build and configuration baseline](build-validation.md), merged through [PR #5](https://github.com/cppbear/PALM/pull/5). It validates one standalone crate with fixed local model responses, the pinned nightly, and `cargo-llvm-cov 0.6.16`.

## Run the Check

From the repository root:

```sh
cargo build --workspace --locked
cargo +stable install cargo-llvm-cov --version 0.6.16 --locked
python3 scripts/check_coverage.py
python3 scripts/check_minimal.py
python3 scripts/check_tasks.py
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

The local macOS/Apple Silicon run passed using six generation responses and one repair response. The [bytes analysis check](bytes-analysis.md) separately validates the larger target without a model. Real-model behavior, full bytes generation/repair, general multi-crate support, hard process timeouts, recovery after forced termination, and explicit run/resume behavior remain later work. Git history is retained.

The coverage entry temporarily annotates sources with `#[coverage(off)]` and enables the nightly feature at crate roots without adding newlines. It preserves compiler flags, restores original bytes on returned errors, and performs no hashing or source fingerprint checks. `cargo llvm-cov report --json` reuses the first run's data; the first XML-producing command retains the tool's normal profile cleanup between candidates. This does not provide crash recovery or general macro expansion. See the [coverage command reference](../utgen/README.md#coverage-of-existing-tests) for scope.

## Coverage Compatibility Regression

`scripts/check_coverage.py` exercises 11 small standalone crates, without a model service or extra crate dependencies. It covers existing coverage attributes, `cfg_attr(test, ...)`, compound/nested conditions with Cargo features enabled and disabled, test-only methods and impls, file-level test modules, and an expression used by `include!`. Each case has an integration test so both normal-library and test-harness builds are checked. It compares execution outcomes, checks raw production-only coverage, verifies source restoration, and confirms preprocessing leaves no original tests while preserving byte offsets. A custom target entry containing an expression is rejected by both commands.

The script runs in Linux CI. Coverage and preprocessing share only source parsing and test predicates; they keep separate edit visitors. Coverage attributes and their feature gate are added under the complement of existing conditions, without a cfg evaluator or compiler-flag overrides. Explicit `coverage(on)` remains an intentional opt-in. Expression fragments are preserved, without macro expansion or automatic test exclusion inside those fragments.

## Task Scheduling Regression

`scripts/check_tasks.py` uses nine focal functions sharing one source file, a loopback model, and real Cargo commands. It measures generation and repair request peaks for N=1, N=2, and the default N=4, requires overlap for N>1, and checks that compiler invocations never overlap or observe changing source bytes. A paused compiler lets the N=1 generation queue fill: one candidate is being validated, one result is buffered, and one active job waits to send. Releasing compilation must drain every result.

The same check covers direct and input/prefix/oracle integration generation, removal of invalid imports from saved candidates, and preservation/removal of temporary compiler inputs. Integration coverage reports remain available until every function has been read, and all nine functions must have passing tests and nonzero covered lines. Valid answers without usage must complete successfully and mark request statistics incomplete. HTTP 401 during generation, oracle generation, and repair must fail without outer-layer retries. A blocked prompt directory exercises worker-panic propagation and slot release; a stale generation backup exercises queue draining after consumer failure, and blocking the diagnostic output file exercises failure while the repair lock is held. Sources must be restored, failed repair backups retained, unrelated backups untouched, existing recovery material rejected, and post-failure statistics skipped. These checks use byte comparisons without hashing and run on macOS/Linux. CLI tests separately verify the default and rejection of zero before configuration loading.


## Model Request Regression

Local HTTP tests exercise the shared generation/repair request path, including response usage omissions, empty choices or content, invalid JSON, permanent 4xx errors, transient 429/5xx errors, dropped connections, and delayed response bodies. They verify actual attempt counts and bounded retry exhaustion without calling a real service. Existing task checks also verify request summaries and source/backup handling after a final request failure. `--request-timeout` defaults to 180 seconds per attempt and rejects zero before configuration loading. Request formatting and compilation repair retries remain business-level operations, not additional transport retry layers.

## Function Selection and Request Budgets

`scripts/check_tasks.py` generates and repairs just two selected functions while other saved candidates remain in the same directories. It checks actual request counts, selected-only statistics, untouched unselected candidate files, and invocation records. Duplicate names and blank lines are accepted. Separate CLI tests reject empty/unknown lists and a zero request limit before configuration or target changes.

With four function workers and a two-attempt limit, both generation and repair must send exactly two requests, return failure when more work needs a request, restore sources, and skip statistics. Local HTTP unit tests additionally cover retries consuming the limit, concurrent clones sharing it, and success on the final allowed attempt. None of these checks uses real model credentials. The bytes analysis check validates the checked-in two- and eight-function lists against the complete analysis index for a later model trial.

## Answers Without Tests

Before this fix, a reply such as `fn helper() {}` parsed as Rust and let generation return success with zero candidate tests. Direct-test and prefix generation now require at least one extracted `#[test]` function and reuse the existing three-attempt format retry loop. This does not require assertions in a prefix or judge the semantic quality of a test.

The task regression selects one function and exercises recovery after a helper-only reply, exhaustion after three such replies, and interruption by the shared request budget, in both generation paths. It compares saved raw answers with the server's replies, including code fences, verifies the accepted code is normalized, checks request/token counts, and requires failures to leave no successful candidate or statistics. The recovered tests must compile, pass and cover production code, including a prefix with no assertion before oracle generation. Unit tests additionally cover invalid Rust, imports/comments without tests, empty test modules, and existing helpers/test attributes. All fixtures use fresh generation directories and local HTTP; no full-project model trial is involved.
