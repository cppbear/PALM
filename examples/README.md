# Examples

Start with the [minimal pipeline](../docs/minimal-pipeline.md) for an automated, no-credentials validation. The commands below use a working copy of the larger bytes target and limit model generation and repair to selected functions.

The [bytes analysis check](../docs/bytes-analysis.md) validates all 663 exported functions and their context artifacts on two fresh copies. Run `cargo build --workspace --release --locked` and `python3 scripts/check_bytes.py` from the repository root. This check does not call a model service or measure generated-test quality.

## bytes

The bundled target is [tokio-rs/bytes](https://github.com/tokio-rs/bytes), version 1.10.0. It is excluded from PALM's tool workspace and retains its own source files, tests, and license.

### Install the Tools

Follow the [root installation instructions](../README.md#installation), including the toolchain components and cargo-llvm-cov. Install `focxt/call_chain` as well as brinfo, focxt, and utgen. Before generation or repair, create `utgen/res/api.json` as described in the [runtime configuration instructions](../utgen/README.md#prerequisites).

### Prepare a Working Copy

Preprocessing blanks existing test source ranges and renames test directories. Generation can add `ntest` to the target's Cargo.toml, and compilation checks temporarily insert tests. Use a separate copy to preserve the bundled example.

Run the following from the PALM repository root, keeping the same shell for subsequent commands:

```sh
palm_repo="$(pwd)"
export PALM_CONFIG="$palm_repo/utgen/res/api.json"
palm_example_dir="$(mktemp -d "${TMPDIR:-/tmp}/palm-bytes.XXXXXX")"
cp -R "$palm_repo/examples/bytes/." "$palm_example_dir/"
cd "$palm_example_dir"
```

### Preprocess and Analyze

```sh
utgen pre-process -p "$palm_example_dir"
utgen analyze -p "$palm_example_dir"
```

Analysis runs brinfo and focxt (including call-chain) on the prepared source, reports stage timings, and validates the resulting artifacts. If a function lacks corresponding context, the command reports the function and missing path.

### Generate and Repair Unit Tests

`--requirement` enables generation for representative condition chains; `--context` includes focal context in the prompt. Both flags are off by default. In unit mode, candidates are stored in JSON and temporarily inserted into the target source for checks and execution.

Use the scoped example below to start. The [command reference](../utgen/README.md#limited-function-runs) describes other selections and all-function behavior.

### Prepare a Small Model Trial

The checked-in [two-function list](bytes-smoke-2.txt) selects `Bytes::len` and `BytesMut::len`. The [eight-function list](bytes-smoke-8.txt) adds `is_empty`, `split_off`, and `truncate` for both types. They cover 2 and 16 representative condition chains respectively. The repeated namespace segments are exact compiler-index names. The analysis regression checks both lists without calling a model.

Choose the model in your runtime configuration and set separate generation and repair attempt limits. The values below are example budgets; adjust them as needed. Omitting `--max-requests` leaves the command without an overall request limit.

```sh
palm_gen_limit=8
palm_fix_limit=8

utgen gen -p "$palm_example_dir" --requirement --context \
  --functions-file "$palm_repo/examples/bytes-smoke-2.txt" \
  --max-requests "$palm_gen_limit"
```

After generation completes, run compilation repair with the same function list. Repair handles compilation errors; runtime test failures remain in the statistics.

```sh
utgen fix -p "$palm_example_dir" \
  --functions-file "$palm_repo/examples/bytes-smoke-2.txt" \
  --max-requests "$palm_fix_limit"
```

Both commands default to four active function tasks and a 180-second deadline per request attempt. Every retry consumes the command's attempt limit. Review the [results](#results) and restored sources before expanding to eight functions on another fresh copy. Replace the function-list path in both commands for that selection. Keep configuration and generation mode fixed when comparing runs; the deterministic checks do not measure model quality.

### Integration Test Mode

Use a fresh working copy and repeat preprocessing and analysis before choosing this mode, since existing generation results are reused independently of these flags:

```sh
utgen gen -p <target-crate-path> --integration --requirement --context
utgen fix -p <target-crate-path> --integration
```

Integration mode generates files under the target's `tests/` directory after filtering functions using the analysis visibility flag and checking compilation. Pass `--integration` to `fix` as well to repair and evaluate candidates in integration-test targets. Imports and helpers stay with each candidate, and both combined and per-candidate coverage retain integration scope. Keep the same function selection for both commands; mixing cached generation modes is rejected.

### Results

Read the outputs in this order:

1. Check the request reports under `utgen/generation/` for candidate-stage completion, request counts, and budget exhaustion. A completed candidate stage does not mean every test passed or coverage statistics finished.
2. Inspect `utgen/result/` or `utgen/fixed_result/` for the selected functions' compilation, execution, and passing-test counts.
3. Read coverage from those per-function statistics alongside the test outcomes.

Paths below are relative to the working copy:

| Path | Contents |
| --- | --- |
| `brinfo/` and `focxt/` | Extracted condition chains and context. |
| `utgen/generation/prompt/` and `answer/` | Generation prompts and model responses. |
| `utgen/generation/pre_fix/` | Candidate tests and compilation results before repair. |
| `utgen/generation/llm_fix/` | Candidate tests and compilation results after repair. |
| `utgen/generation/gen-requests.json` and `fix-requests.json` | Model, invocation selection/options, attempts, limits, token reporting, and candidate-stage status. |
| `utgen/result/` and `utgen/fixed_result/` | Per-function coverage and execution statistics. |

`coverage.xml` and `coverage.json` are intermediate files and may be removed after parsing. The current implementation writes JSON statistics; it does not generate the `result.html` report mentioned in earlier instructions. See [utgen](../utgen/README.md) for CLI limitations.
