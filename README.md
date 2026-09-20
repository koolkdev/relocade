# wasm86

Rust components for executing x86 user-mode code in WebAssembly.

`wasm86-x86` generates snapshot blocks and an interpreter that share instruction
semantics. Blocks decode supplied bytes at compilation time; the generated
interpreter decodes guest memory at runtime. The embedding host supplies memory,
dispatch and segment descriptor resolution.

The current scope is a partial 16/32-bit protected-mode integer instruction set,
including segment loads, near/far transfers and string repetition. Real mode,
privilege transitions, interrupt delivery, floating point and SIMD are not implemented.
The project is under development; the crates are not published.

| Crate | Responsibility |
| --- | --- |
| [wasm86-x86](crates/x86/src/lib.rs) | x86 decoding, shared semantics, CPU state and generated execution entries. |
| [wasm86-compiler](crates/compiler/src/lib.rs) | Guest-independent typed expressions and structured control lowered to Wasm. |
| [wasm86-test-support](crates/test-support/src/lib.rs) | Internal Wasmtime and V8 test support. |

## Generate a block

```rust
use wasm86_x86::compile_block_from_bytes;

fn main() -> Result<(), wasm86_x86::BlockError> {
    // MOV EAX, 42 under flat 32-bit assumptions.
    let module = compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
    assert_eq!(module.entry, "block_1000");
    // Instantiate module.bytes with the host imports described below.
    Ok(())
}
```

For an executable embedding, follow the
[host integration contract](crates/x86/docs/host-integration.md): imported memories,
dispatch and fault exits, segment resolution, and entry validity. It is also
included in the x86 crate's generated API documentation.

## Build and test

The ordinary tests execute Wasm in Wasmtime and do not require Node.js.

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo doc --workspace --no-deps --locked
```

Open `target/doc/wasm86_x86/index.html` or
`target/doc/wasm86_compiler/index.html` for API contracts and examples.
[CONTRIBUTING.md](CONTRIBUTING.md) covers adding instructions, writing tests and
running the optional Node.js 24 / V8 lane. Exact supported encodings live with
[their instruction definitions](crates/x86/src/instruction/definitions).
