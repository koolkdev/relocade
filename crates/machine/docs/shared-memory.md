# Shared guest memory

Guest backing is a shared Wasm memory on both backends. The CPU execution context,
Wasm physical-map table and dispatch bookkeeping stay private to the runtime. Compiled
RAM instructions still use ordinary Wasm loads and stores. MMIO uses device callbacks.

Allocate and initialize guest memory before constructing a CPU:

```rust
let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let mut runtime = Cpu::new(memory.clone(), cpu_config, Default::default(), state).await?;
```

Each CPU imports the supplied allocation and creates private registers, mapping
storage and dispatch. CPU construction preserves the existing guest bytes.
Use `()` when no environment state is needed.

Retain memory access while the CPU executes:

```rust
let memory = runtime.memory();
let view = memory.view();
let run = runtime.run(budget);
// A separate host task can read without borrowing runtime or run.
let value = view.read_u8(0x200)?;
```

`Memory` owns the backing handle and an immutable copy of the configured
physical map. Its memory views use the same RAM, ROM, A20 and hole rules as the stopped
machine. Devices stay on the execution worker: mapped MMIO accesses require its
local bus. Accessing MMIO without one is a programming error and panics.
`MemoryView` supports reads through
`&MemoryView` and writes through `&mut MemoryView`.
Borrowing the local handle does not exclude access through other shared handles.

`CpuContext` lends separate `cpu`, `memory`, `io` and `state` fields during stopped
access and host services. The CPU-register and guest-memory views can be borrowed
together:

```rust
let registers = &mut call.cpu;
let memory = &mut call.memory;
memory.write_u16(0x200, registers.ax())?;
registers.set_ax(memory.read_u16(0x202)?);
```

Those scoped views borrow the stopped CPU. Embeddings use `cpu.with(|call| ...)`;
host hooks may use `CallContext::with` for the same synchronous batching.
Ordinary hooks use `call.cpu.ax()` and `call.cpu.set_ax(value)` directly. Each
operation validates invocation access and releases its backend borrow.

An invocation's `memory` field is a `MemoryBus`, which retains its `Memory` and
`IoBus` together. `call.memory.view()` already includes MMIO routing and borrows
only that field, leaving `call.cpu` independently usable while the view is alive.
The view may cross an await. CPU, flags and stack edits remain live; memory does
not cache a snapshot. Clone `call.memory` to retain routed access independently
of the invocation. `cpu.memory()` returns the raw `Memory` handle for sharing
backing storage without retaining local devices.

A pending run excludes `cpu.with(...)` access. Independent `Memory`
handles remain usable during execution, while a service awaits input, and after
CPU destruction.

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
host reader may see part of an update and sample the remainder later. Display
snapshots belong to the video device; see the [VGA device](../../vga/README.md).
Worker lifetime belongs to the application.

## Devices

`Device` owns synchronous MMIO and port behavior. Its methods take `&mut self`,
including reads that can change hardware state. `IoBus` holds immutable local
connections, each containing an `Rc<RefCell<dyn Device>>` and the ranges it decodes:

```rust
let io = IoBus::new([DeviceConnection {
    device,
    mmio: vec![0xb8000..=0xbffff],
    ports: vec![0x3d4..=0x3d5],
}]);
let memory = Memory::new(machine_config)?;
let mut cpu = Cpu::new(memory, cpu_config, io, ()).await?;
```

The board's physical map must mark that memory range `PhysicalMapping::Mmio`.
The device owns its storage; it does not use guest RAM backing. CPU-context memory
transfers and guest instructions route to the same device, after A20 translation.
BIOS services use `call.io` for port operations. Unconnected ranges retain
the open-bus behavior: FF reads and ignored writes. A host operation on the same
worker can bind its retained resources once with `memory.with_io(io)`, then use
the resulting `MemoryBus::view` for mapped memory access.
This also distinguishes an attached empty bus from a view without device access.

MMIO callbacks receive one through eight little-endian bytes, including partial
widths at boundaries. Port callbacks retain a complete one-, two- or four-byte
operation; the device interprets it. Each transfer borrows its device only for the
synchronous call. The bus can be cloned within a worker, but cannot cross threads.
The machine and its backend share the local bus. Wasm callbacks access it through
their backend context. Asynchronous host services use the machine's bus without
holding a device borrow. Dropping the machine releases both bus handles.

Create devices on the execution worker. Publish owned observations to other
workers instead of sending the live bus or device. VGA publishes immutable
captured text inputs; see the [display contract](../../vga/README.md). Shared RAM
remains available independently of device ownership.
