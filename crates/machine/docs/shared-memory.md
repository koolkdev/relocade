# Shared guest memory

Guest backing and the CPU execution context use distinct shared Wasm memories
on both backends. The Wasm physical-map table and dispatch bookkeeping stay private
to the runtime. Compiled RAM instructions still use ordinary Wasm loads and stores. MMIO uses device callbacks.

Allocate and initialize guest memory before constructing a CPU:

```rust
let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let mut runtime = Cpu::new(memory.clone(), cpu_config, Default::default()).await?;
```

One guest CPU owns execution on a PC's physical bus. A `Machine` reserves that
CPU when `create_cpu` succeeds. The reservation lasts while the machine or its
execution handle owns the CPU. A standalone `Cpu::new` or `CpuStorage::into_cpu`
over a clone of the same `Memory` uses the same reservation. Dropping the last
CPU owner releases it. Memory views and retained register handles can outlive it.
CPU construction preserves the existing guest bytes.
Registered hooks capture their own application state and service handles.

Retain memory access while the CPU executes:

```rust
let memory = runtime.memory();
let view = memory.view();
let run = runtime.run();
// A separate host task can read without borrowing runtime or run.
let value = view.read_u8(0x200)?;
```

`Memory` owns the backing handle and an immutable copy of the configured
physical address policy. Its memory views use the same RAM, ROM, A20 and hole
rules as the stopped machine. MMIO operations come from the execution worker's
live bus, including changes made after a view was created. Accessing MMIO without
a local bus is a programming error and panics.
`MemoryView` supports reads through
`&MemoryView` and writes through `&mut MemoryView`.
Borrowing the local handle does not exclude access through other shared handles.

`cpu.cpu_state()` returns a retained `CpuStateAccess` resource. It copies values
without borrowing the backend or lending references into CPU storage. Clones share
the complete state, including flags, segments and floating-point snapshots, and
remain usable after CPU destruction. Setters take `&self`; no poll-time binding
or dynamic borrow guard is needed. The driver serializes guest execution and
service polling. Use CPU state at those stopped boundaries: generated entries
may cache fields internally until they publish state on exit. Synchronous guest
device callbacks must not edit this backing. Memory safety does not make such
mid-entry architectural edits valid.

To inspect the CPU of a running `Machine`, acquire a stopped boundary from its owner:

```rust
let paused = machine.pause_cpus().await;
let state = machine.cpus()[0].snapshot();
drop(paused);
```

The guard blocks CPU execution and CPU host-service polling until released.
A running CPU stops at its next execution checkpoint. An idle or unstarted CPU
needs no acknowledgement. Cancellation also releases the pause. The execution owner
continues to acknowledge execution-interrupt requests through its normal loop.
Independent device tasks, host memory writers and standalone `Cpu::new` instances
are outside this boundary. Pausing CPUs alone is not a memory-write transaction.

`CpuContext` lends separate `cpu`, `memory` and `io` fields for stopped embedding
access. The CPU-state and guest-memory views can be borrowed
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

Host services capture these resources once at registration:

```rust
let cpu = runtime.cpu_state();
let resources = runtime.memory().with_io(io.clone());
let hook = real_mode_interrupt::hook(entry, cpu.clone(), async move || {
    let value = input.read().await;
    cpu.set_ax(value);
    resources.view().write_u16(buffer, value).unwrap();
});
```

The service retains no backend borrow. A pending future may borrow its captured
handles and keep ordinary references to local values across awaits. The driver
retains the handler until after the future is destroyed. `cpu.context()` remains
available at stopped guest boundaries, including when outer calls are parked
during an ISR, but not while a host call is actively pending. Independent state
and memory handles also work while a call waits and after runner destruction.

Start a task with `worker::Worker::spawn_with(memory.clone(), run_task)`.
The memory handle retains its configuration in Rust and transfers its shared
allocation through the typed channel. Native handles move directly;
browser shared-memory objects use structured cloning. The worker receives a
complete `Memory` and borrows a view with `memory.view()`, without extracting
backing or reconstructing the handle in application code. Configuration does not
need serialization. See the [worker guide](workers.md).

Browser worker sharing requires cross-origin isolation, typically served with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`.

Reads sample live bytes; they do not freeze the guest or provide an atomic frame.
Wasmtime host accesses use byte atomics so concurrent Rust access is safe. A
host reader may see part of an update and sample the remainder later. Display
snapshots belong to the video device; see the [VGA device](../../vga/README.md).
Worker lifetime belongs to the application.

## Compiled instruction sources

The compiler copies each backing page once, including when discovery reaches it
through an A20 alias. A compiled module retains a `CodeSource` with the
instruction bytes used by its emitted blocks. Read-ahead and gaps are excluded.

Each successful installation on a stopped CPU validates the source bytes and
registers the installed module's backing pages while holding the same lock as
memory-view writers. `Memory::installed_code_at` looks up installations by
backing byte. `InstalledCode::was_modified` records whether a managed write
changed any instruction byte during that installation. A neighboring write or
a write of identical bytes leaves its history unchanged.

Each installed module holds a `HostWritePage` for every physical page containing
its instruction bytes. Modules on the same page share one handle. The first hold
routes RAM stores through the interpreter's host callback. Dropping the last
hold restores the page's configured route.

Retiring the module or dropping the CPU removes its registration. An outstanding
compiler result can be installed again after validation. Its previous
installation's write history remains available to observers. A guest write
retires the affected installations before the interpreter's write callback
returns. External writers stop the PC before changing executable bytes. The
CPU retires code they patched through a memory view when the host next enters
the guest.
Writers using `Memory::backing` bypass this tracking. They must remain quiescent
during publication and arrange explicit retirement before executing changed code.

## Devices

`Device` owns interrupts and optional asynchronous activity. Prepare register
operations with `PortRead`/`PortWrite` and memory operations with
`MmioRead`/`MmioWrite`. `DeviceCallbacks` adapts methods that borrow the device
through a weak reference. Nested I/O forwards the supplied `IoContext`.
`IoBus` retains device connections and the installed operations:

```rust
let io = IoBus::new(device, |io| {
    io.bind_mmio(0xb8000..=0xbffff, &memory_read, &memory_write);
    io.bind(0x3d4, &index_read, &index_write);
    io.bind(0x3d5, &data_read, &data_write);
});
let memory = Memory::new(machine_config)?;
let mut cpu = Cpu::new(memory.clone(), cpu_config, io.clone()).await?;
```

The board's physical map must mark that memory range `PhysicalMapping::MMIO`.
The device owns its storage; it does not use guest RAM backing. CPU-context memory
transfers and guest instructions route to the same device, after A20 translation.
BIOS services capture a `MemoryBus` for memory and port operations. Unconnected
ranges retain
the open-bus behavior: FF reads and ignored writes. A host operation on the same
worker can retain memory and device routing together:

```rust
let resources = memory.with_io(io);
let mut view = resources.view();
view.write_u16(0xb8000, 0x0741)?;
resources.io().write_port(0x3d4, 1, 0x0f);
```

This `MemoryBus` owns both handles. Its views can cross awaits and outlive the
CPU without keeping the Wasm runtime or a device borrowed. Clones share the same
backing bytes and device connections; the last connection owner releases the devices.
Use raw `Memory` for sharing backing between workers. An attached empty bus still
differs from a raw view without device access.

MMIO callbacks receive one through eight little-endian bytes, including partial
widths at boundaries. Complete MMIO pages identify a slot in separate live read
and write function tables. Mixed pages use slot zero to resolve device boundaries
and holes. Adjacent pages combine only when their slots match and their bus
addresses are contiguous. Each callback finishes before the next part resolves.

`bind_mmio` returns a fixed `MmioBinding`. Retain it only when operations can
change, then call `set_read(context, &read)` or `set_write(context, &write)`.
Replacement is synchronous and visible to existing compiled modules, the
interpreter and retained host views. Dropping a binding leaves its operations
installed. New bindings publish complete-page indices after both functions are
ready; reads and writes can subsequently be replaced independently.

Port callbacks retain a complete one-, two- or four-byte operation; the device
interprets it. Device callbacks borrow state only for the synchronous call. The
bus can be cloned within a worker, but cannot cross threads.
The CPU and `IoBus` retain the same `WasmRuntimeHandle`. It keeps the Wasm runtime
alive and scopes exclusive access to synchronous operations. Native `WasmRuntime`
directly owns its Wasmtime store. Wasm callbacks capture weak bus routing references,
so retaining the runtime does not create a cycle through the devices. They use
the current callback context directly. Synchronous host transfers briefly borrow
the stopped runtime; `IoBus::with_context` reuses that scope for a batch.
Asynchronous host services retain the bus without holding a device borrow.
Dropping the CPU releases its handles; retained `MemoryBus` or `IoBus` handles
keep the devices and runtime alive. Each bus supports one CPU over its lifetime,
including after that CPU is dropped or its initialization fails.

Create devices on the execution worker. Publish owned observations to other
workers instead of sending the live bus or device. VGA publishes immutable
captured text inputs; see the [display contract](../../vga/README.md). Shared RAM
remains available independently of device ownership.
