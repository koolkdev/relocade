# Shared guest memory

Guest backing is a shared Wasm memory on both backends. The CPU execution context,
Wasm physical-map table and dispatch bookkeeping stay private to the runtime. Compiled
instructions still use ordinary Wasm loads and stores.

Create a reader before starting execution:

```rust
use relocade_machine::ReadMemory;

let memory = runtime.shared_memory();
let run = runtime.run_async(budget);
// A separate display task can read without borrowing runtime or run.
let character = memory.read_u8(0xb8000)?;
```

`SharedMemory` owns the backing handle and an immutable copy of the configured
physical map. Its reads use the same RAM, ROM, A20 and hole rules as the stopped
machine. It implements `ReadMemory`, so `TextScreen::read(&memory)` can sample the
screen while a deferred input operation owns the runtime context.

A native reader can move to the rendering thread. In the browser, send
`memory.backing().memory()` through `postMessage`; shared WebAssembly memories
are cloned, not transferred. The receiving thread can wrap it with
`relocade_wasm_runtime::SharedMemory::from_memory` and associate its machine
configuration with `relocade_machine::SharedMemory::new`.

Browser worker sharing requires cross-origin isolation, typically served with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`.

Reads sample live bytes; they do not freeze the guest or provide an atomic frame.
Wasmtime host accesses use byte atomics so concurrent Rust access is safe. A
renderer may see part of an update and sample the remainder next frame. Presentation
cadence and worker lifetime belong to the application.
