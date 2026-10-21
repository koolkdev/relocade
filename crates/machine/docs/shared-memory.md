# Shared guest memory

Guest backing is a shared Wasm memory on both backends. The CPU execution context,
Wasm physical-map table and dispatch bookkeeping stay private to the runtime. Compiled
instructions still use ordinary Wasm loads and stores.

Borrow memory before starting execution:

```rust
let mut memory = runtime.memory();
let view = memory.view();
let run = runtime.run_async(budget);
// A separate display task can read without borrowing runtime or run.
let character = view.read_u8(0xb8000)?;
```

`Memory` owns the backing handle and an immutable copy of the configured
physical map. Its memory views use the same RAM, ROM, A20 and hole rules as the stopped
machine. `TextScreen::read(&view)` can sample the
screen while a deferred input operation owns the runtime context. The same
`MemoryView` supports reads through `&MemoryView` and writes through `&mut MemoryView`.
Borrowing the local handle does not exclude access through other shared handles.

`CpuContext::memory()` and `HostCall::memory()` return `MemoryView` directly.
Their views borrow the stopped CPU or callback; `cpu.memory()` returns an owned
handle whose lifetime is independent of execution.

Share the existing backend handle with `worker::Message(memory.backing().clone())`.
The worker transport moves native handles and preserves browser shared-memory
objects through structured cloning. The receiver pairs the handle with its machine
configuration using `relocade_machine::Memory::with_backing`, then borrows a
view with `memory.view()`.
The configured guest capacity is checked against the allocation's byte length.

Browser worker sharing requires cross-origin isolation, typically served with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`.

Reads sample live bytes; they do not freeze the guest or provide an atomic frame.
Wasmtime host accesses use byte atomics so concurrent Rust access is safe. A
renderer may see part of an update and sample the remainder next frame. Presentation
cadence and worker lifetime belong to the application.
