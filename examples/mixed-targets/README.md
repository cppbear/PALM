# Mixed-target fixture

This standalone package is used by `scripts/check_mixed.py`. The library and binary both include `shared.rs`, while each defines its own `helper`. Their different results expose incorrect target selection. The binary also calls the library and uses a library type.

From the PALM repository root, after building the tools and installing the pinned coverage tool:

```sh
python3 scripts/check_mixed.py
```

The script works in temporary copies with fixed local model responses. It checks analysis ownership, dependency context, unit and integration statistics, compilation repair, and source restoration. Additional temporary variants cover custom Cargo targets and definitions shared between binaries. The fixture's `main` panics so accidental execution is visible; the generated unit-test pipeline must use the test harness.

## Limited model trial

After [installing PALM](../../README.md#installation) and [configuring a model](../../utgen/README.md#prerequisites), run from the repository root:

```sh
palm_repo="$(pwd)"
export PALM_CONFIG="$palm_repo/utgen/res/api.json"
palm_mixed_dir="$(mktemp -d "${TMPDIR:-/tmp}/palm-mixed.XXXXXX")"
cp -R "$palm_repo/examples/mixed-targets/." "$palm_mixed_dir/"
utgen pre-process -p "$palm_mixed_dir"
utgen analyze -p "$palm_mixed_dir"

utgen gen -p "$palm_mixed_dir" --requirement --context \
  --functions-file "$palm_repo/examples/mixed-smoke-4.txt" --max-requests 12
utgen fix -p "$palm_mixed_dir" \
  --functions-file "$palm_repo/examples/mixed-smoke-4.txt" --max-requests 8
```

The [four-function list](../mixed-smoke-4.txt) selects the shared `classify` definition through the library, both distinct `helper` definitions, and the binary's `bin_only` function that calls the library. Its target-qualified names are exact keys from `brinfo/name_map.json`; the ambiguous name `mixed_fixture::helper` alone cannot select one helper.

Both commands use unit mode and default to four active function tasks. The request caps include retries; a new invocation starts a new budget. Check each command's status and [result files](../README.md#results) before proceeding. Repair addresses compilation errors; runtime assertion failures remain failed outcomes. Keep the working copy and model responses outside the repository.

These selections include binary functions, so they cannot be passed unchanged with `--integration`. Integration mode requires library functions; see the [two-function integration trial](../README.md#integration-test-mode). Shared lib/bin definitions are represented by their library compilation, while binary-specific functions use the binary test harness.
