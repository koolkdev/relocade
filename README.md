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
| [wasm86-runtime](crates/runtime/src/lib.rs) | Portable Real16 execution, physical routing and device callbacks. |
| [wasm86-runtime-wasmtime](crates/runtime-wasmtime/src/lib.rs) | Native execution backend using Wasmtime. |
| [wasm86-runtime-web](crates/runtime-web/src/lib.rs) | Browser WebAssembly backend through wasm-bindgen. |
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

## Run a real-mode program

`wasm86-runtime` provides persistent interpreter machines through a host-engine
backend. `Runtime::new(backend).await` generates and prepares one Real16
interpreter; `runtime.machine(map, devices).await` creates independent CPU,
memory and device state. Native applications supply `WasmtimeBackend::new()?`
from `wasm86-runtime-wasmtime`. `Machine::step()` executes one
instruction, while `Machine::run(limit, should_stop)` returns on a host stop
request, an instruction limit or a typed guest exit. The predicate observes the
initial state and each successful instruction boundary, including the final one
at the limit. Handle services or pending input between runs and resume the same
machine.

The instruction limit counts complete x86 instructions. REP remains one instruction
and runs to completion or a guest fault. Use `machine.run_slice_until(work, should_stop)`
to bound work instead: an ordinary instruction or a completed REP element costs one
unit.
A slice can return with the REP count/indices updated and EIP still at REP;
resuming continues from that progress. `SliceOutcome` reports consumed work and
completed instructions separately. Its predicate also sees partial REP boundaries.
Device callbacks remain synchronous, so neither limit guarantees wall-clock
preemption. The runtime reports CPU exceptions
without delivering them, and supplies no DOS/BIOS services or hardware interrupts.

Run the small [raw x86 example](crates/runtime-wasmtime/examples/real16.rs), which writes
`Hi` through host-defined output ports and requests an exit:

```sh
cargo run -p wasm86-runtime-wasmtime --example real16 --locked
```

The [runtime API](crates/runtime/src/lib.rs) describes machine setup, physical
memory and device callbacks. Backends own compilation, instances and memory
access; the runtime owns routing, dispatch outcomes and execution limits.
`with_memory` supplies checked backing operations, and `with_devices` /
`with_devices_mut` keep device borrows scoped to the backend. Execution defaults
to live instruction bytes through the interpreter. Explicitly
installed callable modules share its dispatcher; automatic guest JIT compilation
and code-validity tracking remain future work.

Browser applications compile for `wasm32-unknown-unknown` with wasm-bindgen and
select `WebBackend` from `wasm86-runtime-web`:

```rust
let runtime = Runtime::new(WebBackend).await?;
let mut machine = runtime.machine(map, devices).await?;
```

Machine, device and memory APIs are shared with Wasmtime. Browser module
compilation and instantiation await JavaScript promises; generating interpreter
bytes and executing guest instructions remain synchronous. Use a worker and yield
to its event loop between bounded `run_slice` calls. No backend requires `Send` device state.
The browser must support WebAssembly multi-memory, tail calls, SIMD and atomics;
an unsupported engine returns a compilation error. The runtime suite runs in
Chrome both on the page and in a dedicated worker.

## Callable execution

The runtime resolves execution addresses to Wasm function-table slots. A missing
address uses the interpreter; installed modules use the same calling convention.
`run_slice(work)` keeps dispatch inside Wasm until exhaustion or an exit.
`run_slice_until(work, predicate)` returns at published boundaries for observation.
`step()` stops after one retirement even inside an installed compiled block.

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

Run the same runtime behavior tests in headless Chrome with a matching
`chromedriver` on `PATH` (or set `CHROMEDRIVER` to its executable):

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli --version 0.2.129 --locked
CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
WASM_BINDGEN_USE_BROWSER=1 WASM_BINDGEN_TEST_TIMEOUT=180 \
  cargo test -p wasm86-runtime --test runtime --target wasm32-unknown-unknown --locked
```

Set `WASM_BINDGEN_USE_DEDICATED_WORKER=1` instead of `WASM_BINDGEN_USE_BROWSER=1`
to run the worker variant. `WASM_BINDGEN_TEST_WEBDRIVER_JSON` can point to a WebDriver
capabilities file for custom Chrome paths or launch arguments.
