# Examples

Start with the [minimal pipeline](../docs/minimal-pipeline.md) for an automated, no-credentials validation. The commands below describe the larger bytes target; the complete pipeline is currently regression-tested on the minimal crate.

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

Analysis runs brinfo and focxt (including call-chain) on the prepared source. If a function lacks corresponding context, generation reports the missing artifact; support for larger targets is still being validated.

### Generate and Repair Unit Tests

```sh
utgen gen -p "$palm_example_dir" --requirement --context
utgen fix -p "$palm_example_dir"
```

`--requirement` enables generation for representative condition chains; `--context` includes focal context in the prompt. Both flags are off by default. In unit mode, candidates are stored in JSON and temporarily inserted into the target source for checks and execution.

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
| `utgen/result/` and `utgen/fixed_result/` | Per-function coverage and execution statistics. |

`coverage.xml` and `coverage.json` are intermediate files and may be removed after parsing. The current implementation writes JSON statistics; it does not generate the `result.html` report mentioned in earlier instructions. See [utgen](../utgen/README.md) for CLI limitations.
