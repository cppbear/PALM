# utgen

utgen generates Rust tests from condition chains and focal context, checks compilation, repairs compilation errors, and collects coverage and execution statistics.

## Prerequisites

Create `utgen/res/api.json` from the repository root, or `res/api.json` from this directory:

```json
{
  "base": "https://xxxx/v1",
  "key": "sk-xxxxxxxxxx",
  "model": "xxx"
}
```

Replace the placeholders with your LLM service configuration. The file is ignored by Git and embedded into the binary with `include_str!`; changing the address, key, or model requires rebuilding utgen. No API configuration is included in the repository.

Install the toolchain and coverage tool described in the [project README](../README.md#prerequisites). Before generation, run `cargo brinfo` in the target crate and `focxt -c <target-crate-path>` to produce:

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
cargo build -p utgen
cargo install --path utgen --locked
```

## Commands

The following is a command reference rather than captured help output. Use `utgen --help` or `utgen <command> --help` for the parser's options.

| Command | Current behavior |
| --- | --- |
| `pre-process` | Rename existing `tests` directories to `tests.bak` and comment out test modules and test functions under the selected `src` directories. |
| `analyze` | Log the selected directories. Run brinfo and focxt explicitly to perform analysis. |
| `gen` | Generate candidates, check compilation, and collect pre-repair statistics. |
| `fix` | Attempt to repair candidates with compilation errors and collect post-repair statistics using unit-test insertion. |

All four commands take `-p, --project-dir`. Use `-w, --work-dir` for individual crates in a larger project. Both relative paths are resolved against the current shell directory; work directories do not resolve against `--project-dir`. Work directories may be repeated or comma-separated and default to the project directory.

### Preprocessing

Use a working copy of the target project, as preprocessing changes source files and test directories:

```sh
utgen pre-process -p <target-crate-path>
```

For multiple crates, use explicit work-directory paths:

```sh
utgen pre-process -p <project-root> -w <crate-path-1> -w <crate-path-2>
```

The current preprocessor recognizes modules whose `cfg` attribute text contains `test`, and functions with `test` or namespaced `test` attributes. It does not provide a general reverse-preprocessing command.

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
| `-t, --tasks` | Default: 128. Currently sizes the result channel and does not enforce a strict limit on concurrent LLM requests. |

Generation may append an `ntest` dependency to the target's Cargo.toml. Existing `utgen/generation/pre_fix/<encoded>.json` results are skipped, so use a fresh target copy for a different model, prompt, or generation mode.

### Repair

```sh
utgen fix -p <target-crate-path>
```

Repair reads the generated candidates and uses compiler diagnostics to revise those that fail compilation. It does not target runtime assertion failures. The command currently accepts `--tasks`, but that value is not passed to the repair scheduler.

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

`coverage.xml`, `coverage.json`, and `error_output.json` are intermediate files in the work directory and may be deleted after parsing. The current implementation does not produce an HTML report.

See the [bytes example](../examples/README.md) for a working-copy workflow and the [Chinese technical guide](../docs/palm-rust-unit-test-generation.md) for implementation details.
