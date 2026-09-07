# wasm86

Rust components for x86 execution in WebAssembly.

`wasm86-compiler` builds scalar WebAssembly functions from integer constants,
parameters and wrapping addition. Values such as `Val<I1>` and `Val<I32>` carry
logical integer types; function signatures use the corresponding `Type` variants.
Supported integer sizes are 1, 8, 16, 32 and 64 bits. Values support fluent
expressions such as `value.add(1)`. Calling `body.return_(&value)` completes the
function body; shared expressions use reusable WebAssembly locals.

Functions can also finish with `body.tail_call(target, &[value.argument()])`.
The target may be imported or defined. Use `Val::argument()` to combine different
logical types in one argument list; the call checks them against its signature.
`Program::import_function` takes a `FunctionImport` containing the module name,
field name and logical `Signature`. Unused function imports are omitted; a direct
export also retains an imported function.

WebAssembly carries 1-, 8- and 16-bit integers in `i32`. Narrow arguments must have
their unused upper bits clear, and returned narrow values satisfy the same rule.
A one-bit argument or result is therefore `0` or `1`. The compiler normalizes
narrow call arguments once per shared value without masking intermediate
arithmetic. Imported functions must return narrow results with their unused
upper bits clear too, including when the import is exported directly.

Fixed-offset memory access supports `I8`, `I16`, `I32` and `I64`, using 1, 2, 4
and 8 bytes respectively. `body.load::<I32>(memory, offset)` reads a snapshot;
`body.store(memory, offset, &value)` writes the value at that point in the body.
Stores keep their authored order, used loads preserve their value across writes,
and unused loads are omitted. Distinct memory declarations require distinct
backing memories.

Run the Rust checks with:

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
```

The V8 execution tests require Node.js 24 on `PATH`. Run both the default and
optimizing V8 modes with:

```sh
cargo test --workspace --locked -- --ignored
```
