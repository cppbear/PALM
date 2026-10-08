# Examples

Start with the [minimal pipeline](../docs/minimal-pipeline.md) for an automated, no-credentials validation. The commands below describe the larger bytes target; the complete pipeline is currently regression-tested on the minimal crate.

The [bytes analysis check](../docs/bytes-analysis.md) now validates all 663 exported functions and their context artifacts on two fresh copies. Run `cargo build --workspace --release --locked` and `python3 scripts/check_bytes.py` from the repository root. Model-based generation and repair on bytes remain a separate validation stage.

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

```sh
utgen gen -p "$palm_example_dir" --requirement --context
utgen fix -p "$palm_example_dir"
```

`--requirement` enables generation for representative condition chains; `--context` includes focal context in the prompt. Both flags are off by default. In unit mode, candidates are stored in JSON and temporarily inserted into the target source for checks and execution.

### Prepare a Small Model Trial

The checked-in [two-function list](bytes-smoke-2.txt) selects `Bytes::len` and `BytesMut::len`. The [eight-function list](bytes-smoke-8.txt) adds `is_empty`, `split_off`, and `truncate` for both types. They cover 2 and 16 representative condition chains respectively. The repeated namespace segments are exact compiler-index names. The analysis regression checks both lists without calling a model.

For a future trial, agree on the model configuration and separate generation/repair attempt limits first. Set `palm_gen_limit` and `palm_fix_limit` to those positive integers, then use the prepared working copy:

```sh
utgen gen -p "$palm_example_dir" --requirement --context \
  --functions-file "$palm_repo/examples/bytes-smoke-2.txt" \
  --max-requests "$palm_gen_limit"
utgen fix -p "$palm_example_dir" \
  --functions-file "$palm_repo/examples/bytes-smoke-2.txt" \
  --max-requests "$palm_fix_limit"
```

Both commands default to four active function tasks and a 180-second deadline per request attempt. Every retry consumes the command's attempt limit. Review the request reports, candidate compilation/pass counts, coverage, and restored sources before expanding to eight functions on another fresh copy. Keep configuration and generation mode fixed when comparing these runs. No real-model result is claimed by the deterministic checks; the lists and limits prepare that later validation.

### Integration Test Mode

Use a fresh working copy and repeat extraction and preprocessing before choosing this mode, since existing generation results are reused independently of these flags:

```sh
utgen gen -p <target-crate-path> --integration --requirement --context
```

Integration mode generates files under the target's `tests/` directory after filtering functions using the analysis visibility flag and checking compilation. `utgen fix` currently uses source-inserted unit tests for repair and post-repair statistics; it does not provide a separate integration-mode repair pipeline.

### Results

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
