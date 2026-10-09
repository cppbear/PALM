# Minimal PALM Target

This standalone library exercises five focal functions: a branch, a struct method, an inline module, and a two-level cross-module call chain. It also includes existing unit/integration tests and a test sharing a line with production code, and test-only helpers. A helper can record a test execution for the coverage regression, which checks that XML and JSON do not cause duplicate runs. Its test dependencies are locked for the pinned nightly.

Run the [minimal pipeline check](../../docs/minimal-pipeline.md) from the PALM repository root. The script copies this directory before preprocessing or analysis; generated data is not written into this fixture.

For a small demonstration, start with `classify` in [src/lib.rs](src/lib.rs). It calls the [support module](src/support.rs), which checks whether the value is positive. Fixed local model responses produce assertions such as `classify(2) == 1` and `classify(-1) == 0`. The check also deliberately supplies an invalid argument to `nested::double`, then repairs it using real compiler diagnostics.

The fixture demonstrates pipeline behavior, not model quality. After running it, use the [output guide](../../docs/minimal-pipeline.md#outputs-and-coverage-interpretation) to distinguish request counts, candidate outcomes, and coverage. Its success message means the expected checks passed, including cases that intentionally fail compilation or execution.
