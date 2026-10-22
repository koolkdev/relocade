# Shared guest memory

`Machine` owns the shared address space for its CPUs.
Each `Cpu` retains its machine and owns its private architectural context, Wasm
bindings, dispatch entries, callbacks and pending host calls. `Memory` is
a property of the machine; its backing is the existing shared-memory handle
from `wasm-runtime`. `MachineConfig` supplies the usable guest capacity, checked
against the allocation length when attaching the handle.
Shared-memory access has no global lock: guest execution, host reads, host writes
and module initialization can overlap.

`MachineConfig` describes shared memory and `CpuConfig` describes a CPU:

```rust
let machine = Machine::new(&mut backend, machine_config)?;
let mut first = machine.create_cpu(backend, first_cpu_config).await?;
let mut second = machine.create_cpu(other_backend, second_cpu_config).await?;
first.run(budget)?;
second.run(budget)?;
```

CPU creation allocates fresh architectural storage and preserves existing guest
bytes. CPU handles own their resources, so running or awaiting a CPU does not
borrow the machine. Cloning a machine retains the same shared owners.

`Machine::with_backing(config, machine.memory().backing().clone())`
creates another machine with its own map over the same allocation. The physical
map is immutable; its Wasm representation can be private to each CPU without
losing shared policy. Engine module caches remain with the backend; instantiated functions,
installed entries and host state remain private to each CPU.

`machine.memory()` and `cpu.memory()` return a cloneable `Memory` handle.
It can edit live bytes, and can outlive all CPUs without replacing the
machine’s owned memory association:

```rust
let mut memory = cpu.memory();
let view = memory.view();
let run = cpu.run_async(budget);
// A display task can sample memory without borrowing cpu or run.
let character = view.read_u8(0xb8000)?;
```

Reads follow RAM, ROM, A20 and hole rules for physical maps. `TextScreen::read(&view)` can sample while a deferred input
operation owns the CPU context. Reads may overlap writes and do not provide
an atomic snapshot or frame. The same `MemoryView<'_>` supports reads through
`&MemoryView` and writes through `&mut MemoryView`. Borrowing the local memory
handle does not exclude access through other shared handles or freeze mappings
and bytes.

`CpuContext::memory()` and `HostCall::memory()` return `MemoryView` directly.
Their views borrow the stopped CPU or callback; `cpu.memory()` returns an owned
handle whose lifetime is independent of execution.

`cpu.view()` borrows published CPU registers directly. Use `cpu.context()` when
passing CPU and guest-memory access together.

`CpuView` borrows live private CPU bytes, and `MemoryView` borrows the machine's
shared allocations. Register interpretation and guest memory policy belong to
these views. `Cpu` owns its backend, bindings and dispatch resources;
`Memory` owns the shared backing and any live tables.

`CpuContext` contains a CPU view and a memory view. `HostCall` groups those same
views with a borrow of the environment state. Both lend shorter views to helpers:

```rust
fn set_result(cpu: &mut CpuView<'_>, value: u32) {
    cpu.set_eax(value);
}

set_result(&mut cpu.view(), 42);
```

The backend lends private memory bytes while execution is stopped. Shared
allocations lend bytes independently of a backend. Views retain those borrows;
constructing or reborrowing a view allocates no storage and copies no guest state.

Send the existing backend handle with `worker::Message(memory.backing().clone())`.
The worker transport preserves the shared allocation. On the receiver, attach
the handle with `Machine::with_backing` using the same configuration.

Browser sharing requires cross-origin isolation, typically with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`. Shared WebAssembly memories are
cloned rather than transferred. Wasmtime host transfers use byte atomics;
transferring a range is not atomic. Presentation cadence and worker lifetime
belong to the application.
