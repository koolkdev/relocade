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
embedding access. Its CPU-register and guest-memory views can be borrowed together:

```rust
let registers = &mut call.cpu;
let memory = &mut call.memory;
memory.write_u16(0x200, registers.ax())?;
registers.set_ax(memory.read_u16(0x202)?);
```

Those views and the context borrow the stopped CPU. Drop the context before
resuming execution or installing modules. `cpu.memory()` returns an owned
handle whose lifetime is independent of execution.

Host services receive an owned `CallContext`. Its CPU access is valid only during
that invocation's future poll and completion cleanup. Its memory and I/O handles
are independent resources, so this view can cross an await:

```rust
let mut memory = call.memory.view();
let value = input.read().await;
memory.write_u16(0x200, value)?;
call.cpu.set_ax(value);
```

A pending service excludes embedding CPU access until it completes, while lending
no backend borrow between polls. Independent `Memory` and `MemoryBus` handles
remain usable during execution, while a service awaits input, and after CPU
destruction.

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
let mut cpu = Cpu::new(memory.clone(), cpu_config, io.clone(), ()).await?;
```

The board's physical map must mark that memory range `PhysicalMapping::Mmio`.
The device owns its storage; it does not use guest RAM backing. CPU-context memory
transfers and guest instructions route to the same device, after A20 translation.
BIOS services retain the same `IoBus` for port operations. Unconnected ranges retain
the open-bus behavior: FF reads and ignored writes. A host operation on the same
worker can retain memory and device routing together:

```rust
let resources = memory.with_io(io);
let mut view = resources.view();
view.write_u16(0xb8000, 0x0741)?;
resources.io().write_port(0x3d4, 1, 0x0f);
```

This `MemoryBus` owns both handles. Its views can cross awaits and outlive the
CPU without retaining a backend or a device borrow. Clones share the same backing
bytes and device connections; the last connection owner releases the devices.
Use raw `Memory` for sharing backing between workers. An attached empty bus still
differs from a raw view without device access.

MMIO callbacks receive one through eight little-endian bytes, including partial
widths at boundaries. Port callbacks retain a complete one-, two- or four-byte
operation; the device interprets it. Each transfer borrows its device only for the
synchronous call. The bus can be cloned within a worker, but cannot cross threads.
The machine and its backend share the local bus. Wasm callbacks access it through
their backend context. Asynchronous host services use the machine's bus without
holding a device borrow. Dropping the machine releases its bus handles; retained
`MemoryBus` or `IoBus` handles can keep the devices alive.

Create devices on the execution worker. Publish owned observations to other
workers instead of sending the live bus or device. VGA publishes immutable
captured text inputs; see the [display contract](../../vga/README.md). Shared RAM
remains available independently of device ownership.
