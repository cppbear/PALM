# Source Integration

This repository brings together PALM's implementation, the technical guide developed in `rust-utgen`, and the ASE 2025 presentation materials. The implementation baseline is [`cppbear/PALM`, commit `60572e3`](https://github.com/cppbear/PALM/tree/60572e367faad7603878d61fcc1d02344f810774).

## Source Revisions

| Repository and branch | Revision | Treatment |
| --- | --- | --- |
| `cppbear/PALM`, `main` | `60572e367faad7603878d61fcc1d02344f810774` | Preserve the implementation and promote the contents of `palm/` to the repository root. |
| `SSCT-Lab/rust-utgen`, `main` | `3807fbe8b69bdb0c7206bdf7b4f538d252be3b4e` | Its main generation updates are already represented in the PALM baseline. |
| `SSCT-Lab/rust-utgen`, `1.87` | `4de8a5994052eb680beb6f773c9a80299ce7be58` | Import the Chinese guide and eight images from `docs/`; adjust the guide to describe PALM's current behavior. |
| `SSCT-Lab/rust-utgen`, `comment_out_test_module` | `29b8377ec9af251b4cb3795c7582c6d8673c6c22` | Retain PALM's existing handling of both test modules and test functions. |
| `SSCT-Lab/rust-utgen`, `focxt_rustc_api` | `0354152adf631987f8fa0a68d80567a6522f669f` | Leave the experimental context extractor in its source branch. |
| `cppbear/ase2025`, `main` | `d24c7d5cda1604894d3b2cecfaddeecd3ef46b73` | Import both original PDFs into `docs/ase2025/`. |

The source repositories have separate Git histories. Selected files are imported with their origins recorded here; this integration does not merge those histories or remove the source repositories.

## Licensing and Attribution

PALM's source code, scripts, and project documentation use the root [MIT License](../LICENSE), with collective attribution to PALM contributors. The five tool crates declare the same license through workspace metadata. The selected source revisions above did not contain a license for the PALM implementation; this repository now explicitly adopts MIT rather than inheriting the example crate's license.

The bundled `examples/bytes` crate retains its [original MIT license and copyright notice](../examples/bytes/LICENSE). The technical guide and images originate from `rust-utgen`; the conference PDFs originate from `ase2025` and are preserved as described in [ASE 2025 materials](ase2025/README.md). Existing third-party notices and rights in included material remain applicable; the root license does not replace them.

## Layout and Compatibility

The Rust workspace, installation script, and Docker scripts now reside at the repository root. The two original README files are consolidated, and the root `.gitignore` retains all original rules, including `api.json`.

The initial integration preserved Rust source files, prompt templates, executable permissions, the pinned `nightly-2025-03-19` toolchain, and `Cargo.lock` from the PALM baseline. All five tool crates remain workspace members. The root workspace explicitly excludes `examples/bytes`, allowing that target to be used as a separate Cargo workspace.

The existing container mount point remains `/home/palm/palm`. Commands in the project documentation start from the new repository root or an explicitly identified target directory.

## Branch Decisions

The `1.87` branch's compiler API changes and Rust 2024 edition migration are already present in the PALM baseline. Its guide and eight images are the added documentation assets. The images are preserved byte for byte; the guide is revised for portable example paths, current preprocessing behavior, CLI limitations, and output locations.

The `comment_out_test_module` branch only handles test modules. PALM already handles modules as well as standalone test functions and namespaced test attributes, so replacing its preprocessor with that branch would remove existing behavior. Further preprocessor corrections are a separate change.

The `focxt_rustc_api` branch changes the entry point to `cargo focxt` and produces `focxt/name_map.json`, while its generation code still requires `focxt/impl_informations.json`. It also uses the older compiler API. Integration of that extractor requires a separate interface migration and validation.

The integration commits preserved the existing generation and repair behavior. Subsequent maintenance adds runtime API configuration, isolates real-model tests, and validates installation and basic CI; see [build validation](build-validation.md). Subsequent checks cover [function-level task limits](minimal-pipeline.md#task-scheduling-regression) and [bytes analysis](bytes-analysis.md). The unfinished batch test injection path remains outside the active CLI.
