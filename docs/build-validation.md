# Build and Installation Checks

Use these checks to verify PALM's build, command entry points, and runtime configuration. Follow the [project prerequisites](../README.md#prerequisites) first. Commands below start in the repository root and use the pinned toolchain and `Cargo.lock`.

## Build and Test

```sh
cargo build --workspace --locked
cargo test --workspace --locked
```

Neither command needs an `api.json` file or model credentials. Ordinary tests use local HTTP fixtures to check configuration selection, CLI validation, request handling, retries, and budgets. The real-service test is ignored by default; see [utgen testing](../utgen/README.md#testing) for its explicit opt-in command.

Dependency downloads can require network access. Passing these checks confirms the tested build and request behavior, not compatibility with a particular model service.

## Command Entry Points

After the workspace build:

```sh
bash -n install.sh docker/docker-build docker/docker-run
./install.sh --help
./target/debug/cargo-brinfo --help
./target/debug/cargo-call-chain --help
./target/debug/focxt --help
./target/debug/utgen --help
cargo metadata --manifest-path examples/bytes/Cargo.toml --no-deps --format-version 1
```

These commands check shell syntax, CLI startup, and the bundled example's separate workspace. If using a custom Cargo target directory, adjust the binary paths. Shell syntax checks do not build or run the Docker image.

## Installation

To check an actual installation, run:

```sh
./install.sh
cargo brinfo --help
cargo call-chain --help
focxt --help
utgen --help
```

The installer installs all four tool packages using the repository's toolchain and stops on the first Cargo failure. Keep the installation directory in `PATH`. The [installation guide](../README.md#installation) also describes selecting individual tools.

Model configuration is loaded only when generation or repair starts. See the [runtime configuration reference](../utgen/README.md#prerequisites) for file selection and environment overrides.

## CI and Further Checks

The [native CI workflow](../.github/workflows/ci.yml) checks builds, ordinary tests, CLI entry points, installer syntax/help, and example metadata on Linux. It also runs the [minimal pipeline and related regressions](minimal-pipeline.md) and [Bytes analysis checks](bytes-analysis.md).

Changes limited to READMEs and the documentation paths selected in the workflow receive a patch whitespace check. Other changes and manual workflow runs execute the full offline suite. A documentation-only CI result does not imply that Rust tests ran.

The independent [Docker checks workflow](../.github/workflows/docker.yml) builds a Linux AMD64 image, checks its Python, compiler, and coverage tools, installs all four PALM tool packages, and checks their command entry points. It runs for changes to Docker files, toolchain files, Cargo manifests/lockfiles, build scripts, build-utils, root Cargo configuration, the installer, or its own workflow. Ordinary Rust logic and documentation changes do not trigger it. The image uses BuildKit's GitHub Actions layer cache and is loaded only into the runner's local Docker engine.

To run the minimal offline pipeline in the container as well, select **Docker checks → Run workflow** in GitHub Actions and enable `full_pipeline`. This uses the installed tool binaries and fixed local responses. Docker checks run independently of native CI, have a 30-minute job limit, and cancel superseded runs for the same PR or branch.

The offline checks use temporary target copies and local model responses. They cover the scenarios documented in each guide; they do not measure model quality, provide general crash recovery, or establish support for arbitrary Rust projects.
