# wasm86

Rust components for x86 execution in WebAssembly.

`wasm86-compiler` builds scalar WebAssembly functions from integer constants,
parameters and wrapping addition. Values such as `Val<I1>` and `Val<I32>` carry
logical integer types; function signatures use the corresponding `Type` variants.
Supported integer sizes are 1, 8, 16, 32 and 64 bits. Values support fluent
expressions such as `value.add(1)`. Calling `body.return_(&value)` completes the
function body; shared additions use reusable WebAssembly locals.

WebAssembly carries 1-, 8- and 16-bit integers in `i32`. Narrow arguments must have
their unused upper bits clear, and returned narrow values satisfy the same rule.
A one-bit argument or result is therefore `0` or `1`.

Run the Rust checks with:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The V8 execution tests require Node.js 24 on `PATH`. Run both the default and
optimizing V8 modes with:

```sh
cargo test --workspace --locked --test execution -- --ignored
```
