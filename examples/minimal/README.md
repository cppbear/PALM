# Minimal PALM Target

This standalone library exercises five focal functions: a branch, a struct method, an inline module, and a two-level cross-module call chain. It also includes existing unit/integration tests and a test sharing a line with production code, and test-only helpers. A helper can record a test execution for the coverage regression, which checks that XML and JSON do not cause duplicate runs. Its test dependencies are locked for the pinned nightly.

Run the [minimal pipeline check](../../docs/minimal-pipeline.md) from the PALM repository root. The script copies this directory before preprocessing or analysis; generated data is not written into this fixture.
