# relocade

Rust components for executing x86 code in WebAssembly.

`relocade-x86` generates snapshot blocks and an interpreter that share instruction
semantics. Blocks decode supplied bytes at compilation time; the generated
interpreter decodes guest memory at runtime. The embedding host supplies memory,
dispatch and segment descriptor resolution.

The project aims for Pentium 4 (P4) user-mode instruction-set parity.
Development prioritizes missing P4 instructions and architectural behavior.

The implementation currently covers part of the 16/32-bit protected-mode user
instruction set and ordinary real-mode execution. Real mode supports segment
loads, FLAGS stack transfers, near/far transfers and software interrupts with
interrupt returns. External interrupt delivery and mode transitions remain outside
that subset. The project is under development; the crates are not published.

| Crate | Responsibility |
| --- | --- |
| [relocade-x86](crates/x86/src/lib.rs) | x86 decoding, shared semantics, CPU state and generated execution entries. |
| [relocade-compiler](crates/compiler/src/lib.rs) | Guest-independent typed expressions and structured control lowered to Wasm. |
| [relocade-test-support](crates/test-support/src/lib.rs) | Internal Wasmtime test support. |

## Generate a block

```rust
use relocade_x86::compile_block_from_bytes;

fn main() -> Result<(), relocade_x86::BlockError> {
    // MOV EAX, 42 under flat 32-bit assumptions.
    let module = compile_block_from_bytes(0x1000, &[0xb8, 42, 0, 0, 0], 1)?;
    assert_eq!(module.entry, "block_1000");
    // Instantiate module.bytes with the host imports described below.
    Ok(())
}
```

For an executable embedding, follow the
[host integration contract](crates/x86/docs/host-integration.md): imported memories,
dispatch and fault exits, segment resolution, and entry validity. Real16 uses
the [physical-memory contract](crates/x86/docs/physical-memory.md) for host transfers.
Both contracts are included in the x86 crate's generated API documentation.

The [future worker design](crates/runtime/docs/worker-execution.md) describes
execution-task ownership, UI messaging and scheduling for an interactive embedding.

## Build and test

Compiler and x86 correctness tests execute Wasm in Wasmtime. Browser integration
tests exercise the web runtime in Chrome; see [CONTRIBUTING.md](CONTRIBUTING.md).

```sh
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo fmt --all -- --check
cargo doc --workspace --no-deps --locked
```

Open `target/doc/relocade_x86/index.html` or
`target/doc/relocade_compiler/index.html` for API contracts and examples.
[CONTRIBUTING.md](CONTRIBUTING.md) covers adding instructions, writing tests and
running the browser integration suite. Exact supported encodings live with
[their instruction definitions](crates/x86/src/instruction/definitions).
