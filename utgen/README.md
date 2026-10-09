# utgen

utgen generates Rust tests from condition chains and focal context, checks compilation, repairs compilation errors, and collects coverage and execution statistics.

For a first run, start with [PALM's offline quick start](../README.md#quick-start), then follow the [two-function model trial](../examples/README.md#prepare-a-small-model-trial). This page is the detailed command reference.

## Prerequisites

Model configuration is loaded at runtime, once per `gen` or `fix` command. Building, installing, running help, `pre-process`, `analyze`, and `coverage` do not require it.

Copy [api.example.json](res/api.example.json) to `utgen/res/api.json` from the repository root, then replace its placeholders:

```json
{
  "base": "https://xxxx/v1",
  "key": "sk-xxxxxxxxxx",
  "model": "xxx"
}
```

Select the file explicitly with `--config /absolute/path/to/api.json` (before or after the subcommand), or set `PALM_CONFIG` to its path. The existing `base`, `key`, and `model` fields remain supported. A file named `api.json` is ignored by Git but is never automatically discovered.

From the repository root, after creating the file:

```sh
export PALM_CONFIG="$(pwd)/utgen/res/api.json"
```

An absolute path continues to work after changing into the target crate. Relative configuration paths resolve against the command's current directory, independently of `--project-dir`. Configuration precedence is:

1. `--config` selects the file; otherwise `PALM_CONFIG` selects it. An explicitly selected unreadable or invalid file is an error.
2. `PALM_API_BASE`, `PALM_API_KEY`, and `PALM_MODEL` override their respective file fields. These variables can also supply all three fields without a file.
3. All three resolved fields must be nonempty. Missing configuration stops generation or repair before modifying the target.

Changing configuration takes effect on the next invocation without rebuilding. There are no built-in model credentials. Configuration values are not printed in configuration diagnostics.

Generation and repair share one non-streaming request path. `--request-timeout <SECONDS>` defaults to 180 and covers each attempt, including its response body. Transport failures, HTTP 429 (except `insufficient_quota`), and 5xx responses get at most three attempts, with 1- and 2-second delays. Other HTTP errors and malformed/empty answers are returned without network retries. SDK retries are not layered underneath this policy; the existing request/response types are retained while the existing reqwest dependency executes HTTP requests.

Requests use the selected model's default sampling settings: PALM omits `temperature` and `top_p`, since some compatible services reject these optional parameters. The request still asks for one non-streaming answer with `max_tokens=10000`.

A valid answer without `usage` remains usable. Numeric token fields contain reported usage only; missing usage contributes no known tokens, which must not be interpreted as zero actual usage. The command writes `utgen/generation/gen-requests.json` or `fix-requests.json` with attempt/failure counts, missing-usage counts, reported token totals, and `usage_complete`. That flag is false after any failed attempt or response without usage. These files also cover requests whose answers failed later parsing. They are written after workers finish, including request failures; successful cached runs record zero attempts. Validation failures before any request leave the target untouched.

Formatting retries and compiler-guided repair rounds remain separate from transport retries. A final request failure, including during oracle generation or repair, fails the command and skips subsequent statistics. The request deadline does not terminate Cargo processes or impose a whole-function deadline.

Install the toolchain and coverage tool described in the [project README](../README.md#prerequisites). On a fresh working copy, run `utgen pre-process -p <target-crate-path>` and then `utgen analyze -p <target-crate-path>` to produce:

```text
brinfo/name_map.json
brinfo/brdata/*.json
focxt/impl_informations.json
focxt/<encoded>.rs
```

These files are required even when `--context` is omitted: that flag controls whether context is included in the prompt.

## Build

From the repository root:

```sh
cargo build -p utgen --locked
cargo install --path utgen --locked
```

The `analyze` command also requires [brinfo](../brinfo/README.md), [focxt](../focxt/README.md), and [call_chain](../focxt/call_chain/README.md) on `PATH`. Use the [root installer](../README.md#installation) to install all tools. Installing utgen alone does not install these companion executables.

## Testing

```sh
cargo test --workspace --locked
```

Default tests cover configuration loading, CLI configuration errors, and both model-request paths against a local HTTP fixture. They require no real model credentials and do not contact a model service.

Local HTTP fixtures explicitly bypass system proxies. A subprocess regression checks both request paths with proxy variables set and `NO_PROXY` empty, without changing the parallel test runner's environment. Normal model requests retain system-proxy support.

To explicitly run the real-service smoke test after setting `PALM_CONFIG` or the three model environment variables:

```sh
cargo test -p utgen --locked gene::llm::tests::test_llm -- --ignored --exact
```

This opt-in test sends a real, potentially billable non-streaming request. It does not evaluate the complete generation and repair pipeline.

## Commands

The following is a command reference rather than captured help output. Use `utgen --help` or `utgen <command> --help` for the parser's options.

| Command | Current behavior |
| --- | --- |
| `pre-process` | Rename each selected crate's `tests/` to `tests.bak/` and replace test-only source ranges with whitespace while preserving line breaks and byte offsets. |
| `analyze` | Clear Cargo's check cache, run brinfo and focxt, and validate their function indices and artifacts. Currently supports one standalone crate passed with `-p`. |
| `coverage` | Run existing tests once and export XML/JSON with test code excluded. Supports one standalone crate passed with `-p`. |
| `gen` | Generate candidates, check compilation, and collect pre-repair statistics. |
| `fix` | Attempt to repair compilation errors and collect post-repair statistics, using unit-test insertion or integration-test targets with `--integration`. |

All commands take `-p, --project-dir`. Except for `coverage`, commands also accept `-w, --work-dir` for individual crates in a larger project. Both relative paths are resolved against the current shell directory; work directories do not resolve against `--project-dir`. Work directories may be repeated or comma-separated and default to the project directory. The current `analyze` command requires a single standalone crate; use that crate as `-p`, with no separate work-directory selection.

### Preprocessing

Use a working copy of the target project, as preprocessing changes source files and test directories:

```sh
utgen pre-process -p <target-crate-path>
```

For multiple crates, use explicit work-directory paths:

```sh
utgen pre-process -p <project-root> -w <crate-path-1> -w <crate-path-2>
```

The preprocessor visits `src/` and declared lib/bin entry files. It removes modules whose `cfg` predicates are provably disabled without `test`, functions with `test` or namespaced `test` attributes, test-only impls/methods, and the contents of files with an inner `#![cfg(test)]` attribute. It preserves production predicates such as `cfg(not(test))` and treats unknown feature/target predicates conservatively. Source files retain their line breaks and byte offsets. UTF-8 character columns can change inside blanked ranges; perform analysis after preprocessing. Existing `tests.bak` is never overwritten. Non-entry files that parse as expressions (for example, `include!("value.rs")` fragments) are left unchanged; other parse failures remain errors. Cargo metadata identifies target entry files, which must parse as complete Rust files. There is no general reverse-preprocessing command.

### Analysis

```sh
utgen analyze -p <target-crate-path>
```

The target must have `Cargo.toml` and `src/`, with no existing `brinfo/` or `focxt/` directory. Use a fresh prepared copy when repeating analysis. Tool failures return a nonzero exit status. No model configuration is needed.

Analysis enumerates the package's ordinary library and binary targets using Cargo metadata, including custom target names and entry paths. Raw results stay separate under `brinfo/targets/<kind>/<name>/` for mixed packages. The final branch/context indices retain every target's own definitions and prefer the library when the same source definition also occurs in a binary. Same-name definitions in different locations remain separate; definitions shared only between binaries remain associated with each binary. The selected library version does not imply that every binary variant was tested.

Mixed-target index keys include the target, for example `lib:demo::demo::shared::parse`. `--functions-file` accepts these keys, or the original Rust path when it identifies exactly one selected function. Ambiguous paths fail with the available qualified keys. Single-target keys and encoded filenames retain their existing form. Dependency contexts keep their actual origin even when a shared binary definition is omitted from the focal list.

For manual single-target analysis, run `cargo clean`, `cargo brinfo`, then `focxt -c <target-crate-path>` in the prepared crate. A prior `cargo check` can otherwise prevent the compiler wrapper from running. Use `utgen analyze` for mixed packages; unscoped low-level invocations reject multiple targets rather than overwrite their artifacts.

### Coverage of existing tests

On an original working copy, before preprocessing:

```sh
utgen coverage -p <original-crate-copy>
```

This command needs neither model configuration nor analysis artifacts. It writes `coverage.xml` and `coverage.json` in the crate directory from a single test run. Assertion failures remain visible in test output and coverage is still exported; compilation or report-export failures return a nonzero exit status. A failed command does not guarantee valid output files.

The same coverage entry point is used by generation and repair. It temporarily adds `#[coverage(off)]` to test-only modules (including generated `llmtests`), standalone test functions, and `#[cfg(test)]` helper functions/methods. Methods inside test-only impls are excluded individually; files with `#![cfg(test)]` are excluded as a whole. Module exclusion also covers nested helpers. Production functions called by tests remain instrumented; mixed production/test files are never excluded wholesale. Integration-test files use cargo-llvm-cov's default directory exclusions. Doc tests are not run by `--tests`.

Existing coverage attributes and feature gates are reused. For conditional attributes, PALM adds a complementary `cfg_attr` only where the existing setting is absent; rustc evaluates the conditions for each build. Explicit `coverage(on)` settings are respected. The nightly feature gate is inserted temporarily on the same line at crate roots, leaving compiler flags and Cargo configuration untouched. Ordinary compilation and repair need no coverage attributes. Coverage annotations are removed by restoring the bytes saved at coverage entry, after success or a returned build/report error. Temporary insertions preserve line numbers, but columns and byte offsets can change while measuring; use working copies and do not edit them concurrently. Recovery after forced process termination is still pending.

Scope is a standalone crate with sources under `src/` and Cargo target entry files. Test identification reuses preprocessing's conservative `cfg` rules. Non-entry expression fragments are kept unchanged, including any code inside them; macros are not expanded, and unmarked helpers outside test-only modules cannot be inferred as test-only. The pinned nightly and cargo-llvm-cov version are required.

### Generation

```sh
utgen gen -p <target-crate-path> --requirement --context
```

| Option | Meaning |
| --- | --- |
| `-r, --requirement` | Generate for each representative condition chain. Default: off. |
| `-c, --context` | Include the focal function's context in the prompt. Default: off. |
| `-o, --oracle` | Use separate input-range, test-prefix, and oracle generation. Default: off; otherwise generate complete tests directly. |
| `-i, --integration` | Generate integration tests under `tests/`, selecting library functions with the analysis visibility flag and checking candidate compilation. Default: off. |
| `-t, --tasks` | Default: 4. Maximum active focal-function generation jobs; must be positive. |
| `--request-timeout` | Default: 180 seconds per model request attempt; also available for `fix`. Must be positive. |
| `--functions-file` | Optional path to analysis-index keys or unambiguous Rust paths, one per line; also available for `fix`. |
| `--max-requests` | Optional positive request-attempt limit for the whole command, including retries; also available for `fix`. Default: no overall limit. |

Generation validates the branch index, context index, context files, and source paths before modifying the target. Missing or inconsistent artifacts are errors. A failed generation task is reported instead of being silently lost before statistics. `--tasks 0` is rejected.

Direct-test and prefix answers must parse as Rust and contain at least one supported `#[test]` function. Syntax errors and answers containing only helpers, imports, comments, or an empty test module use the existing format retry loop, with at most three attempts and a fixed one-second wait before each retry. The first attempt starts without a wait. Exhaustion fails that function's generation and skips subsequent command statistics. This checks candidate presence; compilation and execution still determine whether a candidate works. Prefixes are accepted without assertions because oracle generation follows separately.

Generated candidate execution uses a five-second `ntest` timeout, including unit-test statistics and integration tests. A timed-out candidate counts as failed and later candidates can still run. Statistics check expected panics inside the timeout boundary, so a timeout cannot satisfy `should_panic`; the existing any-panic expectation is retained. This limit covers the test body, not Cargo compilation, model requests, or arbitrary child processes.

Each answer returned by the request layer is saved before code-fence removal and parsing, under `utgen/generation/answer/<encoded>/<chain>/test-attempt-N.txt` or `prefix-attempt-N.txt`, with N starting at 1. Logs identify the stage, chain, attempt and rejection reason. Accepted code retains the existing `code.rs` / `prefix.rs` layout. These paths are reused across invocations, not an invocation history; use fresh copies for separate trials. Existing cached candidates, including old empty candidates, are still skipped and are not migrated by this validation change.

Each generation job handles one focal function, including its condition chains and input/prefix/oracle stages sequentially. It holds a slot until its result enters the bounded queue, which also has capacity N. A single consumer validates candidates and their imports while other jobs can await model responses. Thus N limits active generation jobs, not the number of compiler processes or Cargo's internal build jobs; buffered results and the candidate currently being validated are separate.

Generation may append an `ntest` dependency to the target's Cargo.toml. Existing `utgen/generation/pre_fix/<encoded>.json` results are skipped, so use a fresh target copy for a different model, prompt, or generation mode.

### Limited function runs

Copy names exactly from the keys of `brinfo/name_map.json` into a UTF-8 text file, one per line. `--functions-file` resolves against the current shell directory. Surrounding whitespace and blank lines are ignored, duplicate names are processed once, and there is no wildcard or comment syntax. Missing files, empty lists, and unknown names fail before configuration loading or target changes. Omitting the option preserves the normal all-function behavior; integration generation still applies its visibility filter.

The same selection applies to generation, repair preparation, and per-function coverage/execution statistics. Pass it to both `gen` and `fix`; it is not inherited from an earlier command. Repair reports a selected function with no saved candidate as an error. Unselected candidate and result files remain unchanged. Output directories can therefore contain older results outside the current selection: the invocation record identifies this run's selection. Use separate fresh copies when comparing experiments, including different integration-test selections.

`--max-requests N` counts every model request attempt across all workers, chains, generation stages, format retries, and repair rounds. Network retries consume the same limit. Checking and incrementing the count is one shared operation, so concurrent workers cannot overshoot it. Once another attempt is needed after the limit, the command fails, joins all workers, restores sources, and skips subsequent statistics; already sent requests may finish and successful candidates may be saved. Reaching exactly N attempts while completing all work is successful. A later command has its own budget. This is an attempt cap, not a token, monetary, or wall-clock budget.

The request report also records `model`, `request_timeout_seconds`, `max_requests`, `budget_exhausted`, and an `invocation` object with command, directories, options, deduplicated function names (`null` means no explicit selection), `candidate_status`, and an error when present. `candidate_status` describes generation/repair worker completion before statistics, not passing tests or completed coverage. The report does not serialize the API key or endpoint. Each command replaces its own previous report, including zero-request cached runs; preflight failures before work do not create a new report. Existing candidate and statistics JSON provide the per-function outcomes.

### Repair

```sh
utgen fix -p <target-crate-path>
```

Repair reads the generated candidates and uses compiler diagnostics to revise those that fail compilation. It does not target runtime assertion failures. `--tasks N` defaults to 4 and limits active focal-function repair jobs through result saving. Each job processes its candidates and repair rounds sequentially.

Source insertion or temporary test-file writes, compilation/test execution, diagnostic reading, and restoration are serialized within each command. Model requests may overlap. Cargo retains its own dependency-build parallelism. Concurrent PALM commands or experiments require separate working copies and separate target directories.

All workers are joined before cleanup. A task panic or infrastructure error returns a nonzero status and skips subsequent coverage statistics; candidate compilation errors remain ordinary repair outcomes. Unit-mode repair restores its source backups before returning, deletes only backups created by that invocation on success, and retains them after a worker failure. Existing source backups are rejected without overwriting them. Temporary import/candidate files are restored or removed after validation, including unwinding. This is not recovery from forced process termination. Model request deadlines are described above; Cargo subprocess deadlines remain separate work.

For integration tests, pass `--integration` to both commands:

```sh
utgen gen -p <target-crate-path> --integration
utgen fix -p <target-crate-path> --integration
```

Integration repair compiles each candidate in a temporary `tests/palm_candidate.rs` target, without inserting tests into production source. It can repair imports, module-level helpers, and the test body. Each candidate keeps its own repaired imports/helpers, so a change does not affect siblings from the same model answer. Compilation checks do not execute tests. Repair checks saved candidates again in their integration target; successful cached repairs need no model requests.

The resulting tests remain under `tests/`. Both combined and per-candidate statistics use integration targets, retain the five-second timeout, and exclude test code from coverage. Temporary compiler inputs are restored or removed on normal completion and ordinary failures. Coverage still temporarily annotates source to exclude test code and restores it afterward.

Candidate files record their generation mode. Generation and repair reject a different mode instead of converting candidates. Older cache files without a mode field are treated as unit-test candidates; regenerate old integration caches in a fresh working copy. Keep the same function selection and mode for generation and repair.

## Output

Paths under `utgen/` below are relative to `--project-dir`:

| Path | Contents |
| --- | --- |
| `utgen/generation/prompt/` and `answer/` | Prompts and model responses. |
| `utgen/generation/pre_fix/` | Candidate tests and their compilation status before repair. |
| `utgen/generation/llm_fix/` | Candidates and their compilation status after repair. |
| `utgen/generation/gen-requests.json` and `fix-requests.json` | Model/options/function selection, request limits and counts, reported token totals, usage completeness, and candidate-stage status. |
| `utgen/result/` | Pre-repair coverage and execution statistics per focal function. |
| `utgen/fixed_result/` | Post-repair coverage and execution statistics per focal function. |
| `utgen/original_result.json` | Comparison statistics when the original integration-test backup is available. |

Per-function results include `coverage_available`. When it is false, the numeric coverage fields are placeholders and must not enter coverage aggregates; report the number of unavailable functions separately. Missing mappings are not assumed to mean `cfg(test)` exclusion. Old results without this field have unconfirmed availability. Candidate/compilation statistics remain valid when no candidate compiles.

When candidate coverage has no focal-function mapping, statistics collect a zero-hit baseline for the selected Cargo target. Unit mode builds the `--lib` or `--bin` test harness with every test filtered out; it does not insert an empty test. Integration mode obtains mappings from the ordinary library artifact, using a temporary integration harness without executing tests. Thus `cfg(not(test))` code can contribute to the integration denominator while being absent in unit mode. Compilation and tool failures remain errors. Baselines add no candidate/oracle counts or `codes_*_covered` entries; their profiles stay separate from real test hits. Parsed baselines are reused only within the current statistics invocation.

Generated candidates and statistics carry the recorded target (`kind`, `name`, and entry `src_path`). Statistics use `function_name` as the analysis-index key and `rust_name` for the original Rust path. Unit compilation checks use `cargo test --lib/--bin <name> --no-run`; execution and coverage use that same target. Repair considers compiler errors, not ordinary warnings. The standalone `coverage` command and original-test comparison retain their package-wide scope.

Function-level integration mode requires a library. Automatic selection reports omitted binary functions; explicitly selecting a binary function with `--integration` fails before model requests. Cargo may still build the package's binaries for an integration test, so binary build errors can prevent integration checks. PALM does not temporarily remove binaries from the manifest.

Old single-target candidates without recorded ownership remain usable. Old mixed-target artifacts may have overwritten one another; analyze and generate in a fresh working copy instead of migrating those caches. Keep the same generation mode for repair.

`tests_*` count candidates; `oracles_*` count the existing `TestInfo` groups. A group counts once if any of its candidates compiles, is registered to run, or passes, respectively. Run counts retain the existing libtest convention, including ignored tests. Integration pass outcomes match complete test names.

`coverage.xml`, `coverage.json`, and `error_output.json` are intermediate files in the work directory and may be deleted after generation/repair parses them. The standalone `coverage` command retains both reports. The current implementation does not produce an HTML report.

See the [deterministic minimal pipeline](../docs/minimal-pipeline.md) and the [bytes example](../examples/README.md) for a working-copy workflow and the [Chinese technical guide](../docs/palm-rust-unit-test-generation.md) for implementation details.
