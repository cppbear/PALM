# focxt

This tool is capable of analyzing the context for a specified Rust crate.

## Build

```sh
cargo build
```

## Usage

```bash
A rust program to get focal context for a crate.

Usage: focxt --crate <CRATE_PATH>

Options:
  -c, --crate <CRATE_PATH>  Sets crate path
  -h, --help                Print help
  -V, --version             Print version
```

`focxt -c crate_path` or `focxt --crate crate_path`

Function contexts use call-chain's compiler identities and source positions, including distinct generic trait impls and reference receivers. Recoverable macro-generated method bodies are associated with their enclosing source impl; this is not general macro expansion. Rebuild focxt and call-chain together. See the [bytes analysis validation](../docs/bytes-analysis.md) for release builds, reproducible checks, and current limits.
