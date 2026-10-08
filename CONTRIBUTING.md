# Contributing to PALM

Bug reports, documentation improvements, and focused pull requests are welcome. Contributions to PALM are made under the project's [MIT License](LICENSE). Keep existing third-party copyright and license notices when changing bundled material.

## Set up and check a change

Follow the [prerequisites](README.md#prerequisites), then work from the repository root:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

Use the existing check relevant to your change:

| Change | Check |
| --- | --- |
| Generation, repair, result handling, or test execution | `python3 scripts/check_minimal.py` |
| Task scheduling, request budgets, or failure cleanup | `python3 scripts/check_tasks.py` |
| Coverage annotations or preprocessing | `python3 scripts/check_coverage.py` |
| Analysis, focal context, or method identity | `cargo build --workspace --release --locked` then `python3 scripts/check_bytes.py` |

The checks use temporary targets and local model fixtures. They do not need API keys or call an external model service; dependency installation may need network access. The full suite runs in CI for code changes. Documentation changes should preserve working links, command examples, and the distinction between offline validation and real-model results.

## Report a problem

Open a [GitHub issue](https://github.com/cppbear/PALM/issues) with:

- The PALM commit, operating system, `rustc --version`, and `cargo llvm-cov --version`.
- The command you ran, expected behavior, actual behavior, and relevant error output.
- A small reproducible target or function when possible, and the stage that failed: analysis, generation, repair, or coverage.
- For model-related failures, the model identifier and relevant response or request summary with private information removed. Do not include API keys or your `api.json`.

Run PALM on a working copy of a target crate: preprocessing changes its source and test layout, and generation can add a test dependency.

## Submit a pull request

Keep the change focused and explain the problem, resulting behavior, and checks you ran. Add a regression case for a reproduced behavior change when the existing checks do not cover it. Real-model calls are not required for ordinary fixes; use local fixtures or existing saved candidates where practical.

Keep credentials, target build directories, generated answers, coverage reports, and local trial logs out of commits. Preserve the pinned toolchain and dependency lockfile unless changing them is part of the task. Record the origin of imported code or material in [source integration notes](docs/source-integration.md), and retain its applicable notices.
