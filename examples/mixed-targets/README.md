# Mixed-target fixture

This standalone package is used by `scripts/check_mixed.py`. The library and binary both include `shared.rs`, while each defines its own `helper`. Their different results expose incorrect target selection. The binary also calls the library and uses a library type.

From the PALM repository root, after building the tools and installing the pinned coverage tool:

```sh
python3 scripts/check_mixed.py
```

The script works in temporary copies with fixed local model responses. It checks analysis ownership, dependency context, unit and integration statistics, compilation repair, and source restoration. Additional temporary variants cover custom Cargo targets and definitions shared between binaries. The fixture's `main` panics so accidental execution is visible; the generated unit-test pipeline must use the test harness.
