# Shared guest memory

Guest backing is a shared Wasm memory on both backends. The CPU execution context,
Wasm physical-map table and dispatch bookkeeping stay private to the runtime. Compiled
instructions still use ordinary Wasm loads and stores.

Memory can also be created before any CPU. `Memory::new(config)` allocates zeroed
guest backing with its physical map. `Cpu::with_memory(memory.clone(), cpu_config,
state).await` creates private CPU registers and dispatch over those existing bytes.
Dropping that CPU leaves the memory available for a later CPU, without clearing or
copying it. Callers coordinate execution if multiple CPUs share the allocation.

Borrow memory before starting execution:

```rust
let memory = runtime.memory();
let view = memory.view();
let run = runtime.run(budget);
// A separate display task can read without borrowing runtime or run.
let character = view.read_u8(0xb8000)?;
```

`Memory` owns the backing handle and an immutable copy of the configured
physical map. Its memory views use the same RAM, ROM, A20 and hole rules as the stopped
machine. `TextScreen::read(&view)` can sample the
screen while a service awaits input. The same
`MemoryView` supports reads through `&MemoryView` and writes through `&mut MemoryView`.
Borrowing the local handle does not exclude access through other shared handles.

`CpuContext::memory()` returns `MemoryView` directly for embedding access and
async services. Those views borrow the executing machine. The borrow checker
prevents retaining a context beyond its handler or overlapping a stopped context
with another run. A suspended service keeps its machine borrow; the embedding
must complete it before calling `cpu.context()` again.
`cpu.memory()` returns an independent shared handle that remains usable after the
CPU is dropped and during guest execution.

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
