# utgen

utgen generates Rust tests from condition chains and focal context, checks compilation, repairs compilation errors, and collects coverage and execution statistics.

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
| `fix` | Attempt to repair candidates with compilation errors and collect post-repair statistics using unit-test insertion. |

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

The preprocessor removes modules whose `cfg` predicates are provably disabled without `test`, functions with `test` or namespaced `test` attributes, test-only impls/methods, and the contents of files with an inner `#![cfg(test)]` attribute. It preserves production predicates such as `cfg(not(test))` and treats unknown feature/target predicates conservatively. Source files retain their line breaks and byte offsets. UTF-8 character columns can change inside blanked ranges; perform analysis after preprocessing. Existing `tests.bak` is never overwritten. Non-entry files that parse as expressions (for example, `include!("value.rs")` fragments) are left unchanged; other parse failures remain errors. Cargo metadata identifies target entry files, which must parse as complete Rust files. There is no general reverse-preprocessing command.

### Analysis

```sh
utgen analyze -p <target-crate-path>
```

The target must have `Cargo.toml` and `src/`, with no existing `brinfo/` or `focxt/` directory. Use a fresh prepared copy when repeating analysis. Tool failures return a nonzero exit status. No model configuration is needed.

For manual analysis, run `cargo clean`, `cargo brinfo`, then `focxt -c <target-crate-path>` in the prepared crate. A prior `cargo check` can otherwise prevent the compiler wrapper from running.

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
| `-i, --integration` | Generate integration tests under `tests/`, using the analysis visibility flag to select functions and compilation checks to filter candidates. Default: off. |
| `-t, --tasks` | Default: 4. Maximum active focal-function generation jobs; must be positive. |

Generation validates the branch index, context index, context files, and source paths before modifying the target. Missing or inconsistent artifacts are errors. A failed generation task is reported instead of being silently lost before statistics. `--tasks 0` is rejected.

Each generation job handles one focal function, including its condition chains and input/prefix/oracle stages sequentially. It holds a slot until its result enters the bounded queue, which also has capacity N. A single consumer validates candidates and their imports while other jobs can await model responses. Thus N limits active generation jobs, not the number of compiler processes or Cargo's internal build jobs; buffered results and the candidate currently being validated are separate.

Generation may append an `ntest` dependency to the target's Cargo.toml. Existing `utgen/generation/pre_fix/<encoded>.json` results are skipped, so use a fresh target copy for a different model, prompt, or generation mode.

### Repair

```sh
utgen fix -p <target-crate-path>
```

Repair reads the generated candidates and uses compiler diagnostics to revise those that fail compilation. It does not target runtime assertion failures. `--tasks N` defaults to 4 and limits active focal-function repair jobs through result saving. Each job processes its candidates and repair rounds sequentially.

Source insertion, target cleanup, compilation/test execution, diagnostic reading, and source restoration are serialized within each command. Model requests may overlap. Cargo retains its own dependency-build parallelism. Concurrent PALM commands or experiments require separate working copies and separate target directories.

All workers are joined before cleanup. A task panic or infrastructure error returns a nonzero status and skips subsequent coverage statistics; candidate compilation errors remain ordinary repair outcomes. Repair restores its source backups before returning, deletes only backups created by that invocation on success, and retains them after a worker failure. Existing source backups are rejected without overwriting them. Temporary import/candidate files are restored or removed after validation, including unwinding. This is not recovery from forced process termination, and request/subprocess deadlines remain unchanged.

There is no integration-mode option for repair. Candidates are inserted into source files as unit tests, and post-repair statistics use that same mode. Do not interpret `gen --integration` followed by `fix` as preserving an integration-only evaluation.

## Output

Paths under `utgen/` below are relative to `--project-dir`:

| Path | Contents |
| --- | --- |
| `utgen/generation/prompt/` and `answer/` | Prompts and model responses. |
| `utgen/generation/pre_fix/` | Candidate tests and their compilation status before repair. |
| `utgen/generation/llm_fix/` | Candidates and their compilation status after repair. |
| `utgen/result/` | Pre-repair coverage and execution statistics per focal function. |
| `utgen/fixed_result/` | Post-repair coverage and execution statistics per focal function. |
| `utgen/original_result.json` | Comparison statistics when the original integration-test backup is available. |

`coverage.xml`, `coverage.json`, and `error_output.json` are intermediate files in the work directory and may be deleted after generation/repair parses them. The standalone `coverage` command retains both reports. The current implementation does not produce an HTML report.

See the [deterministic minimal pipeline](../docs/minimal-pipeline.md) and the [bytes example](../examples/README.md) for a working-copy workflow and the [Chinese technical guide](../docs/palm-rust-unit-test-generation.md) for implementation details.
