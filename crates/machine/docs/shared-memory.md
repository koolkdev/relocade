# Shared guest memory

`Machine` owns shared resources, CPU membership and runtime code management.
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
borrow the machine. Cloning a machine retains the same shared owners. CPU IDs
are unique within that machine; `cpu_count()` counts live contexts, including
bindings retained by host callbacks.

`Machine::with_backing(&mut backend, config, machine.memory().backing().clone())`
creates another machine with its own map over the same allocation. The physical
map is immutable; its Wasm representation can be private to each CPU without
losing shared policy. The runtime module shares generated dispatcher code by
target. Engine module caches remain with the backend; instantiated functions,
installed entries and host state remain private to each CPU.

`machine.memory()` and `cpu.memory()` return a cloneable `Memory` handle.
It can edit live bytes and tables, and can outlive all CPUs without replacing the
machine’s owned memory association:

```rust
let mut memory = cpu.memory();
let view = memory.view();
let run = cpu.run_async(budget);
// A display task can sample memory without borrowing cpu or run.
let character = view.read_u8(0xb8000)?;
```

Reads follow RAM, ROM, A20 and hole rules for physical maps, or live page mappings
for protected maps. `TextScreen::read(&view)` can sample while a deferred input
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

Protected page-table storage is shared by every attachment and reader. The
configuration retains only its initial seed. `machine.memory().view().map_pages(...)`
and `unmap_pages(...)` edit that storage. The embedding must coordinate edits
with execution, mapped transfers and code installation in every attached CPU.
There is currently no runtime protocol for stopping all CPUs to update a map.

Capture generated code's source and mapping version together:

```rust
let mut source = [0; 4096];
let version = machine.memory().snapshot(code_address, &mut source)?;
// Compile from source, then publish using the captured version.
cpu.install_module(&version, &wasm, &entries).await?;
```

Snapshot copying does not exclude writers. Installation checks the address-space
identity and mapping generation after asynchronous compilation; an intervening
remap returns `Error::StaleCode` before module initialization. This check does
not synchronize concurrent map edits. Before its next execution or installation,
each CPU discards generated entries from an earlier generation and reuses their
function-table slots. Host hooks survive. Versions track mappings; embeddings
still replace captured code when guest bytes change.

Naturally aligned protected-mode locked operands use Wasm atomic operations.
Unaligned locked operands currently use checked ordinary reads and writes;
concurrent writers can interleave with those updates. Shared ownership does not
add atomicity or a scheduling policy.

With the `worker` feature, `machine.on_worker(&thread, task)` gives a task the
same machine on a native or browser worker. Create the CPU's backend inside the
task. The handoff transports backing, page tables and revision
storage together, preserving live bytes and mappings. Shared-memory handles
preserve their JavaScript resources during structured cloning. Runtime state,
CPU membership and code-version identity remain shared Rust owners, so a source
version captured on the sender still belongs to the receiving machine.
See the [worker guide](workers.md) for thread setup and integration checks.

Installed Wasm follows the x86 import ABI: it may read the page table but must not
initialize or write it. Page-table edits belong to the host mapping API. Raw
backend access is also responsible for preserving this contract.

Browser sharing requires cross-origin isolation, typically with
`Cross-Origin-Opener-Policy: same-origin` and
`Cross-Origin-Embedder-Policy: require-corp`. Shared WebAssembly memories are
cloned rather than transferred. Wasmtime host transfers use byte atomics;
transferring a range is not atomic. Presentation cadence and worker lifetime
belong to the application.
