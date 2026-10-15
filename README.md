# wasm86

Rust components for executing x86 code in WebAssembly.

`wasm86-x86` generates snapshot blocks and an interpreter that share instruction
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
| [wasm86-x86](crates/x86/src/lib.rs) | x86 decoding, shared semantics, CPU state and generated execution entries. |
| [wasm86-compiler](crates/compiler/src/lib.rs) | Guest-independent typed expressions and structured control lowered to Wasm. |
| [wasm86-machine](crates/machine/src/lib.rs) | CPU configuration, memory mappings and environment services. |
| [wasm86-runtime](crates/runtime/src/lib.rs) | Prepared execution modules, callable dispatch and cooperative slices. |
| [wasm86-runtime-wasmtime](crates/runtime-wasmtime/src/lib.rs) | Native Wasmtime compilation, memories and import bindings. |
| [wasm86-runtime-web](crates/runtime-web/src/lib.rs) | Browser WebAssembly compilation, memories and import bindings. |
| [wasm86-dos](crates/dos/src/lib.rs) | Minimal COM process loading and DOS console, vector and termination services. |
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
dispatch and fault exits, segment resolution, and entry validity. Real16 uses
the [physical-memory contract](crates/x86/docs/physical-memory.md) for host transfers.
Both contracts are included in the x86 crate's generated API documentation.

## Execute a configured guest

`Machine` pairs a validated `CpuContext` with an address space and an `Environment`.
Real16 uses physical RAM/ROM/MMIO mappings and A20; protected profiles use
host-managed 4-KiB linear-to-backing mappings and descriptor callbacks. `Runtime`
prepares and shares code by the complete execution target, including CPU features.
`Runtime::attach(machine).await` creates a private `Session` on the chosen backend.

A session can run a cooperative work slice or install explicitly compiled
modules at guest addresses. Work allowances may be exceeded by at most 255 units;
completed work and architectural retirement are reported separately. Dispatch calls
ordinary Wasm functions and falls back to the interpreter for missing entries. Snapshot validity
remains the installer's responsibility; automatic JIT caching is not implemented.
A session's target remains fixed. Incompatible guest segment changes return
`ContextChanged`; automatic mode transitions are not implemented.

The [runtime API](crates/runtime/src/lib.rs) has a complete setup example.
To run the [native console example](crates/runtime-wasmtime/examples/real16.rs):

```sh
cargo run -p wasm86-runtime-wasmtime --example real16 --locked
```

For a browser or dedicated worker, use `Runtime::new(WebBackend)` from
`wasm86-runtime-web` with the same `Machine` configuration. Compile the embedding
for `wasm32-unknown-unknown` with wasm-bindgen. Preparation awaits browser promises;
execution and environment callbacks are synchronous. Yield to the event loop
between cooperative slices. JSPI and nested guest callbacks are not required by this
execution contract. The web dependency graph contains no Wasmtime.

Host services are complete generated functions. Configure them with
`machine.host_functions_mut().insert(address, HostReturn::Interrupt, callback)`
before attachment, or install a `HostFunctions` module on an existing session.
The generated function calls its fixed host import and lowers its return through
normal x86 semantics. `HostContext` exposes scoped CPU, environment and routed
memory access; `HostAction::Yield` returns control without replaying the callback
on resume. Real-mode INT still builds its frame and follows the live IVT, so guest
handlers can replace vectors and chain to the same service addresses. Protected
near returns use the same mechanism for host-implemented functions.

## Run a DOS COM program

`wasm86-dos` creates a single real-mode process with a PSP, command tail, empty
environment and 64-KiB allocation. Its INT 20h/21h vectors point to compiled host
functions. Guest handlers can replace these vectors and chain to the DOS services.
The same loaded machine works with either runtime backend:

```rust,ignore
let machine = wasm86_dos::load_com(&image, b" /Q")?;
let mut session = runtime.attach(machine).await?;
let outcome = session.run_slice(10_000)?;
let output = session.with_environment_mut(|dos| dos.take_output());
let stop = session.with_environment(|dos| dos.stop());
```

Run additional cooperative slices while the process is active. A DOS stop accompanies
a host yield; inspect the reason and stop executing that process. Output is raw
bytes for the embedding to present. The [DOS API](crates/dos/src/lib.rs) lists the
supported services and diagnostics. This environment has no files, MZ loader,
allocation services, BIOS/devices or DOS extenders.

The [native example](crates/dos/examples/hello.rs) runs a complete console program:

```sh
cargo run -p wasm86-dos --example hello --locked
```

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
