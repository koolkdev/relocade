# Shared guest memory

Guest backing is a shared Wasm memory on both backends. The CPU execution context,
Wasm physical-map table and dispatch bookkeeping stay private to the runtime. Compiled
instructions still use ordinary Wasm loads and stores.

Allocate and initialize guest memory before constructing a CPU:

```rust
let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let mut runtime = Cpu::new(memory.clone(), cpu_config, state).await?;
```

Each CPU imports the supplied allocation and creates private registers, mapping
storage and dispatch. CPU construction preserves the existing guest bytes.
Use `()` when no environment state is needed.

Retain memory access while the CPU executes:

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
screen while a service awaiting input owns the runtime context. The same
`MemoryView` supports reads through `&MemoryView` and writes through `&mut MemoryView`.
Borrowing the local handle does not exclude access through other shared handles.

`CpuContext` lends separate `cpu`, `memory` and `state` fields during stopped
access and host services. The CPU-register and guest-memory views can be borrowed
together:

```rust
let registers = &mut call.cpu;
let memory = &mut call.memory;
memory.write_u16(0x200, registers.ax())?;
registers.set_ax(memory.read_u16(0x202)?);
```

Those views and the context borrow the stopped CPU. Drop the context before
resuming execution or installing modules. `cpu.memory()` returns an owned
handle whose lifetime is independent of execution.

A suspended service keeps its machine borrow; complete it before calling
`cpu.context()` again. Independent `Memory` handles remain usable during
execution, while a service awaits input, and after CPU destruction.

Pass `memory.backing().clone()` directly as a worker task's context.
The worker transport moves native handles and preserves browser shared-memory
objects through structured cloning. The receiver pairs the handle with its machine
configuration using `relocade_machine::Memory::with_backing`, then borrows a
view with `memory.view()`. Capture that configuration in the Rust task closure;
it does not need serialization. See the [worker guide](workers.md).
The configured guest capacity is checked against the allocation's byte length.

Browser worker sharing requires cross-origin isolation, typically served with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`.

Reads sample live bytes; they do not freeze the guest or provide an atomic frame.
Wasmtime host accesses use byte atomics so concurrent Rust access is safe. A
renderer may see part of an update and sample the remainder next frame. Presentation
cadence and worker lifetime belong to the application.
