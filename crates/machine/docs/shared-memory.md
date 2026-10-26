# Shared guest memory

Guest backing is a shared Wasm memory on both backends. Each CPU keeps its own
registers, Wasm physical-map table, dispatch and environment state. Compiled
instructions still use ordinary Wasm loads and stores.

Allocate guest memory independently, then construct each CPU over that memory:

```rust,ignore
let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let mut cpu = Cpu::new(memory.clone(), cpu_config, state).await?;
```

`Memory` owns the shared allocation and its immutable machine configuration.
Cloning retains the same guest bytes. It provides direct access immediately,
including after receipt through a worker channel. `Cpu::new` creates private
CPU state without clearing the supplied memory. Use `()` when no application
state is needed.

`cpu.memory()` returns a clone without borrowing the CPU. A display task can
retain it while a deferred input operation owns the CPU context:

```rust,ignore
let memory = cpu.memory();
let view = memory.view();
let run = cpu.run_async(budget);
let character = view.read_u8(0xb8000)?;
```

`MemoryView` applies RAM, ROM, A20 and hole rules. `CpuContext` lends separate
`cpu`, `memory` and `state` fields in stopped access, hooks and deferred handlers.
The CPU-register and guest-memory views can be borrowed together:

```rust,ignore
let registers = &mut call.cpu;
let memory = &mut call.memory;
memory.write_u16(0x200, registers.ax())?;
registers.set_ax(memory.read_u16(0x202)?);
```

The CPU context borrows guest memory independently of the backend borrow for
private CPU registers. Drop the context before resuming execution or installing
modules. `Memory::with_backing` validates an existing shared allocation against
the configured capacity. Worker transport preserves that capacity and every
physical mapping, including pages currently hidden by A20.

At the Wasm runtime layer, `SharedMemory::new(minimum, maximum)` allocates shared
memory in 64-KiB pages. `WasmSharedMemory` supplies direct `view()`, byte access
and `byte_len()` operations. `Backend::create_memory` allocates private memory;
import shared memory through `Backend::import_shared_memory`. The backend's
memory key is separate from the shared allocation and requires that backend
for access.

Native shared memory can move through Rust channels. Browser shared memory
retains a local JavaScript object and access functions. It crosses workers
through `worker::channel` or `Thread::run`, which serialize the WebAssembly
memory object while sharing its backing bytes. The receiving worker creates its
own local access functions. Any worker may allocate memory; retained copies
remain usable after that worker exits. See the [worker guide](workers.md).

Reads sample live bytes; they do not freeze the guest or provide an atomic frame.
Wasmtime host accesses use byte atomics so concurrent Rust access is safe. A
renderer may observe part of an update and sample the remainder next frame.
Coordinate CPU execution and guest writes when needed; shared ownership does not
supply guest synchronization or automatic compiled-code invalidation.
