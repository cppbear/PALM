# PALM

[![Build and test](https://github.com/cppbear/PALM/actions/workflows/ci.yml/badge.svg)](https://github.com/cppbear/PALM/actions/workflows/ci.yml)

PALM combines program analysis and large language models to generate Rust unit tests. It extracts path constraints and function context, generates candidates, repairs compilation errors, and reports test results and coverage with test code excluded.

This is the maintained implementation of ***PALM: Synergizing Program Analysis and LLMs to Enhance Rust Unit Test Coverage***, published at ASE 2025. The repository includes changes made after the paper's experiments.

[Paper](https://doi.org/10.1109/ASE63991.2025.00223) · [Preprint](https://arxiv.org/abs/2506.09002) · [中文技术指南](docs/palm-rust-unit-test-generation.md) · [CLI reference](utgen/README.md) · [Citation](#citation)

## Prerequisites

Use a native Rust build environment with Git, [rustup](https://rustup.rs/), the stable Rust toolchain, and a C linker. The validation scripts also require Python 3.9 or later. The offline pipeline is checked on Linux CI and has been exercised on macOS with Apple Silicon.

1. Install the pinned analysis toolchain and components:

   ```sh
   rustup toolchain install nightly-2025-03-19 --profile minimal \
     --component rust-src --component rustc-dev --component llvm-tools-preview --component rust-analyzer
   ```

2. Install [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov):

   ```sh
   cargo +stable install cargo-llvm-cov --version 0.6.16 --locked
   ```

Use the same nightly toolchain for the target crate. The supported end-to-end workflow operates on one standalone crate at a time; analysis handles `lib` and `bin` targets. Building, installation, and the offline checks do not require model credentials.

## Quick start

After installing the prerequisites, run the minimal pipeline without API keys:

```sh
git clone https://github.com/cppbear/PALM.git
cd PALM
cargo build --workspace --locked
python3 scripts/check_minimal.py
```

The script uses fixed responses from a local HTTP server and real analysis, compilation, repair, and coverage tools. It works in temporary copies, prints their location, and finishes with `Minimal analysis/generation/repair/coverage checks passed.` The check verifies pipeline behavior; it does not measure model quality. See [minimal pipeline validation](docs/minimal-pipeline.md) for the checked outcomes and output files.

For model-based generation, install the tools below and follow the [two-function trial](examples/README.md#prepare-a-small-model-trial). That walkthrough uses a working copy, an explicit function list, and separate generation and repair request budgets.

## Installation

No model configuration is needed to build or install the tools. `utgen gen` and `utgen fix` load configuration at runtime; changing the API address, key, or model takes effect on the next command without rebuilding.

From the repository root, run:

```sh
./install.sh
```

The script installs `brinfo`, `focxt/call_chain`, `focxt`, and `utgen`. It locates the repository from its own path, so it can also be invoked from another directory. To select tools, use `./install.sh brinfo utgen`; `./install.sh --help` lists the options. Cargo diagnostics remain visible and installation stops on the first failure. Ensure that the installation directory, typically `$HOME/.cargo/bin`, is in `PATH`.

Alternatively, install each tool from the repository root:

```sh
cargo install --path brinfo --locked
cargo install --path focxt/call_chain --locked
cargo install --path focxt --locked
cargo install --path utgen --locked
```

## Workflow

To measure existing tests before preprocessing, run `utgen coverage -p <original-crate-copy>`. It runs tests once, excludes test code from coverage, and writes `coverage.xml` and `coverage.json` without model configuration. Keep this baseline copy separate from the generation copy.

On a fresh working copy of a standalone crate, run `utgen pre-process -p <target-crate-path>` before `utgen analyze -p <target-crate-path>`. Analysis clears Cargo's check cache, runs brinfo and focxt, and verifies their outputs. It rejects existing `brinfo/` or `focxt/` directories so previous analysis results cannot be mixed into the run.

Then use `utgen gen` and `utgen fix` for generation and compilation repair. Pass `--requirement --context` to `utgen gen` to include path constraints and focal context. Preprocessing and test checks modify the target tree; use a working copy of the target project.

Start with the [minimal pipeline check](docs/minimal-pipeline.md), which uses fixed local model responses and verifies analysis, generation, repair, source restoration, and coverage. The [bytes analysis check](docs/bytes-analysis.md) validates analysis on the larger bundled target without a model service. For model-based generation and repair, follow the [small model trial](examples/README.md#prepare-a-small-model-trial) using an explicit function list and separate request limits, then inspect the request reports and test statistics. See [utgen usage](utgen/README.md) for command details and supported scope.

Generation or repair completing does not mean every candidate passes. Inspect both request reports and test statistics. Repair addresses compilation errors; runtime failures and five-second candidate timeouts remain failed test outcomes.

## Project Structure

| Directory | Purpose |
| --- | --- |
| [brinfo](brinfo/README.md) | Extract function condition chains from HIR and MIR. |
| [focxt](focxt/README.md) | Construct code context for each focal function. |
| [focxt/call_chain](focxt/call_chain/README.md) | Extract calls and type dependencies through `cargo call-chain`. |
| [utgen](utgen/README.md) | Generate tests, check compilation, repair tests, and collect coverage. |
| [build-utils](build-utils/README.md) | Configure the compiler library search path during builds. |
| [examples](examples/README.md) | The minimal fixture, bundled bytes target, and usage instructions. |
| `docker/` | Container build and run scripts. |
| [docs](docs/README.md) | Technical guide, validation guides, and ASE 2025 materials. |

## Docker

Run `docker/docker-build` from the repository root to prepare an image with the required toolchain. Mirrors can be configured in `docker/Dockerfile` if needed.

Run `docker/docker-run` from the same directory to mount the repository at `/home/palm/palm` in the container. In the container, change to that directory and follow [Installation](#installation).

## Development checks

From the repository root, using the pinned toolchain:

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

Default tests use local model-response fixtures and do not contact a model service. The real-service test is opt-in; see [utgen testing](utgen/README.md#testing). Dependency downloads may still require network access. See [build and installation checks](docs/build-validation.md) for commands and CI scope.

The [GitHub Actions workflow](.github/workflows/ci.yml) runs on pull requests and pushes to main, canceling superseded runs for the same PR or branch. Changes limited to READMEs and the selected documentation paths under `docs/` receive a lightweight patch whitespace check. Other changes and manual workflow runs execute the full offline suite on Linux.

## Contributing

Bug reports and focused pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for setup, relevant checks, and the information to include in a report.

## Documentation

- [Technical guide in Chinese](docs/palm-rust-unit-test-generation.md): architecture, data flow, and usage.
- [ASE 2025 materials](docs/ase2025/README.md): the original poster and presentation.

## Citation

If you use PALM in your research, please cite the ASE 2025 paper. [CITATION.cff](CITATION.cff) provides the machine-readable citation.

```bibtex
@inproceedings{Chu2025PALM,
  author    = {Bei Chu and Yang Feng and Kui Liu and Hange Shi and Zifan Nan and Zhaoqiang Guo and Baowen Xu},
  title     = {{PALM}: Synergizing Program Analysis and {LLMs} to Enhance {Rust} Unit Test Coverage},
  booktitle = {2025 40th IEEE/ACM International Conference on Automated Software Engineering (ASE)},
  year      = {2025},
  pages     = {2720--2732},
  doi       = {10.1109/ASE63991.2025.00223}
}
```

## License

PALM is licensed under the [MIT License](LICENSE). The bundled [bytes example](examples/bytes/LICENSE) retains its original MIT license and copyright notice. Existing third-party notices and rights in included material remain applicable.
