# Build and Runtime Configuration Validation

Validation date: 2026-10-07.

This maintenance batch establishes a build and test baseline after the repository integration. It does not validate the full program-analysis, generated-test execution, and coverage pipeline.

## Baseline

The baseline is commit `51a644d9e8c0b26c352726b725f9e847d161f5fc`, exported to a separate temporary directory before editing. The pinned toolchain was installed with `rust-src`, `rustc-dev`, and `llvm-tools-preview`.

| Item | Value |
| --- | --- |
| Local platform | macOS, Apple Silicon (`aarch64-apple-darwin`) |
| Compiler | `rustc 1.87.0-nightly (75530e9f7 2025-03-18)` |
| Toolchain | `nightly-2025-03-19` |
| Dependency resolution | Existing `Cargo.lock`, with `--locked` |

| Baseline check | Result |
| --- | --- |
| `cargo build --workspace --locked`, without `utgen/res/api.json` | Failed with exit 101: `include_str!` could not read `api.json`. |
| Same build, with a non-secret placeholder configuration in the temporary copy | Passed. No request was made to the placeholder endpoint. |
| `cargo test --workspace --locked -- --skip gene::llm::tests::test_llm` | Passed: two ordinary tests; the existing real-model test was explicitly filtered out. |

The build emits existing compiler warnings. The baseline confirms that the missing configuration file was a build dependency; it does not establish that the runtime pipeline is correct.

## Runtime Configuration

`utgen gen` and `utgen fix` now load configuration once at command startup and pass it into generation and repair tasks. The file is selected by `--config`, or by `PALM_CONFIG` when the option is absent. `PALM_API_BASE`, `PALM_API_KEY`, and `PALM_MODEL` override individual file fields. No implicit configuration-file search is performed.

The legacy `base`, `key`, and `model` JSON fields remain supported. Relative configuration paths are resolved against the invoking directory, independently of the target path. Missing, empty, unreadable, or malformed configuration fails before these commands modify the target tree. This validates configuration loading, not service availability.

Prompt templates and request parameters are unchanged. Model configuration is no longer embedded in the binary. The real-service smoke test is ignored by default and covers the non-streaming request used by the production workflow.

## Checks Completed in This Batch

| Check | Result |
| --- | --- |
| Workspace build with no `api.json` | Passed on the pinned nightly. |
| Default workspace tests with no model credentials | Passed: 13 tests after the proxy-isolation fix, one real-service test ignored. |
| Configuration tests | File loading and reload, explicit-file precedence, environment-selected file, field overrides, environment-only configuration, missing/empty fields, unreadable file, and redacted diagnostics passed. |
| Local HTTP model fixtures | Generation and repair request paths use the runtime endpoint, key, and model; existing request parameters and usage parsing are preserved. |
| Proxy isolation | The new subprocess regression failed before the fix and passed after it. Both local request fixtures explicitly bypass system proxies; the full test suite also passed with upper/lowercase proxy variables set to `http://127.0.0.1:9` and both `NO_PROXY` variants empty. |
| CLI tests | Help and non-model commands do not load model configuration; missing configuration stops both `gen` and `fix` before target modification; `--config` works before or after the subcommand. |
| Installer behavior checks with a substitute Cargo executable | Other working directories, paths with spaces, tool selection, validation before installation, help, preserved diagnostics/exit status, and stopping after failure passed. |
| Actual installation | All four packages installed without model configuration into an isolated installation directory; existing user-installed tools were not replaced. |
| Installed CLI checks | `cargo-brinfo --help`, `cargo-call-chain --help`, `focxt --help`, and `utgen --help` passed from outside the repository. |
| Shell syntax | `bash -n install.sh docker/docker-build docker/docker-run` passed. |

The installer retains Cargo output, uses the repository directory to select its toolchain, and no longer runs an unconditional `cargo clean`.

The proxy regression configures only child-process environments, so parallel tests do not change one another's environment. The ordinary LLM constructor retains the SDK's default proxy behavior. The fixtures inject a separate HTTP client with proxies disabled. A test-only `reqwest` dependency reuses the existing locked version; dependency versions, checksums, and lockfile format were not changed.

The [GitHub Actions workflow](../.github/workflows/ci.yml) configures the build, default tests, CLI help checks, installer syntax/help, and standalone bytes metadata checks on Ubuntu 24.04. Its YAML was parsed locally. A hosted workflow run remains to be verified after publication; this document does not claim a Linux CI pass.

## Validation Limits and Next Batch

- No real model service was contacted. Local fixtures validate request construction and response handling, not model compatibility or generation quality.
- The tests added here do not exercise preprocessing correctness, focal-context completeness, generated-test insertion, compilation repair, or coverage measurement end to end.
- Docker was not built: the local Docker daemon was unavailable. Linux runtime validation remains pending.
- The pinned toolchain and existing dependency versions were retained; `Cargo.lock` adds only the test dependency edge described above. Compatibility of the coverage tool and target-crate dependencies needs separate verification.
- `utgen analyze` remains a logging-only command. Request concurrency, source restoration, cache validity, and coverage-baseline semantics are unchanged.

The next batch should use a small target crate to verify the relationship between prepared source files, analysis locations, function identities, focal context, and generated tests. It should establish failure propagation and coverage semantics before proceeding to a large bytes run.
