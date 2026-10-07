# PALM

Source code and documents for ***PALM: Synergizing Program Analysis and LLMs to Enhance Rust Unit Test Coverage***.

PALM combines program analysis with LLMs to generate Rust tests, repair compilation errors, and collect coverage data. The Rust workspace is located at the repository root.

## Prerequisites

1. Install the pinned Rust toolchain:

   ```sh
   rustup install nightly-2025-03-19
   rustup component add --toolchain nightly-2025-03-19 rust-src rustc-dev llvm-tools-preview
   ```

2. Install [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov):

   ```sh
   cargo +stable install cargo-llvm-cov --locked
   ```

3. Use the same nightly toolchain for the target crate. The analysis tools handle `lib` and `bin` targets.
4. For generation and repair, prepare runtime model configuration as described in [utgen](utgen/README.md#prerequisites). Building, installation, and ordinary tests do not require model credentials.

## Project Structure

| Directory | Purpose |
| --- | --- |
| [brinfo](brinfo/README.md) | Extract function condition chains from HIR and MIR. |
| [focxt](focxt/README.md) | Construct code context for each focal function. |
| `focxt/call_chain/` | Extract calls and type dependencies through `cargo call-chain`. |
| [utgen](utgen/README.md) | Generate tests, check compilation, repair tests, and collect coverage. |
| `build-utils/` | Configure the compiler library search path during builds. |
| [examples](examples/README.md) | The bundled bytes target and usage instructions. |
| `docker/` | Container build and run scripts. |
| [docs](docs/README.md) | Technical guide, ASE 2025 materials, and source integration notes. |

## Installation

No model configuration is needed to build or install the tools. `utgen gen` and `utgen fix` load configuration at runtime; changing the API address, key, or model takes effect on the next command without rebuilding.

From the repository root, run:

```sh
./install.sh
```

The script installs `brinfo`, `focxt/call_chain`, `focxt`, and `utgen`. Ensure that the installation directory, typically `$HOME/.cargo/bin`, is in `PATH`.

Alternatively, install each tool from the repository root:

```sh
cargo install --path brinfo --locked
cargo install --path focxt/call_chain --locked
cargo install --path focxt --locked
cargo install --path utgen --locked
```

## Docker

Run `docker/docker-build` from the repository root to prepare an image with the required toolchain. Mirrors can be configured in `docker/Dockerfile` if needed.

Run `docker/docker-run` from the same directory to mount the repository at `/home/palm/palm` in the container. In the container, change to that directory and follow [Installation](#installation).

## Workflow

Run `cargo brinfo` in the target crate, then run `focxt -c <target-crate-path>` to extract condition chains and context. The current `utgen analyze` command only logs its arguments; these analysis steps must be run explicitly.

Use `utgen pre-process`, `utgen gen`, and `utgen fix` for preprocessing, generation, and compilation repair. Pass `--requirement --context` to `utgen gen` to include path constraints and focal context. Preprocessing and test checks modify the target tree; use a working copy of the target project.

See the [bytes example](examples/README.md) and [utgen usage](utgen/README.md) for the commands and result locations.

## Documentation

- [Technical guide in Chinese](docs/palm-rust-unit-test-generation.md): architecture, data flow, and usage.
- [ASE 2025 materials](docs/ase2025/README.md): the original poster and presentation.
- [Source integration notes](docs/source-integration.md): source repositories, revisions, and branch decisions.
