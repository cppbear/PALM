# examples

## bytes

Project URL: https://github.com/tokio-rs/bytes

### Build Tools

Navigate to the `brinfo`, `focxt`, and `utgen` directories respectively, and run:

```sh
cargo install --path .
```

### Information Extraction

Navigate to the bytes directory and run `cargo brinfo` to generate the condition chain information.

Run `focxt -c <path_to_bytes>` to generate the context information.

### Test Generation

Run `utgen pre-process -p <path_to_bytes>` for preprocessing.

Run `utgen gen -p <path_to_bytes> -i` to generate tests and execute them. The tests will be placed in the tests folder, and the execution results will be found in `bytes/result.html`.

Run `utgen fix -p <path_to_bytes>` to fix the tests and execute them. The execution results will be found in `bytes/result.html`.
