# Worker tasks and messages

Enable the machine's `worker` feature for `worker::Thread`. The Wasm runtime
reexports `web-thread-select` and adds `ThreadExt::run_resource` for typed resource
handles. The application creates workers and keeps them alive until their tasks
finish.

Pass the memory handle directly. Capture CPU configuration and Rust channel
endpoints in the closure, then construct the CPU inside the worker:

```rust,ignore
use relocade_machine::{worker::{Thread, ThreadExt}, Cpu, Memory};

let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let worker = Thread::new();
let task = worker.run_resource(memory.clone(), move |memory| async move {
    let mut cpu = Cpu::new(memory, cpu_config, Default::default()).await?;
    Ok::<_, relocade_machine::Error>(cpu.run(budget).await)
});

// The caller can retain mapped RAM access while the worker executes.
let view = memory.view();
let exit = task.await??;
```

`Memory` derives `WorkerResource` and marks its backing with `#[worker(post)]`.
The common derive supports named and tuple structs with one posted field; other
fields stay in Rust as `Send` captures. It retains the existing configuration and
posts shared backing through an opaque `ResourceTransfer`. The
worker adapter reconstructs the complete handle before invoking the task. Callers
do not extract backing or pair it with configuration, and the transfer does not
expose a capability's underlying storage.

`worker::channel::<R>()` creates two resource endpoints for an existing worker
connection. Pass an endpoint with `run_resource`, then call `bind` on each owner.
The resulting `Port` sends and receives complete `WorkerResource` values:

```rust,ignore
let (local, remote) = worker::channel::<Memory>()?;
let mut local = local.bind();
let task = worker.run_resource(remote, |remote| async move {
    let mut remote = remote.bind();
    let memory = remote.recv().await.unwrap()?;
    memory.view().write_u8(0x100, 42)?;
    Ok::<_, relocade_machine::Error>(())
});
local.send(memory)?;
task.await??;
```

Reconstruction captures travel through Rust channels; engine resources use the
platform transport. Receipt pairs them in send order. Cancelling a receive leaves
the partial message in the port, and dropping a bound port closes the peer after
earlier messages. `send` only queues delivery; use a Rust reply when the sender
must wait for installation. Browser receipt requires an event-loop turn.

`SharedMemory` and compiled `Module` handles support both `WorkerResource` and the
library's `Post` transport directly. Native handles move as Rust values. Browser transport
preserves their JavaScript objects; shared-memory bytes are shared, not copied.
The receiving worker prepares its own local access to the allocation. Retained
handles remain usable after the allocating worker exits.

Machine configuration, command payloads and channel endpoints remain ordinary
Rust data. They use `Send` closure captures and Rust channels without
serialization. Once running on its worker, a future can retain non-Send values
such as `Rc`, the CPU and browser engine objects. Construct those values inside
the task. `Thread::run_send` returns Send results through Rust memory;
`Thread::run` posts its result and can return a shared allocation or module.

Host services are ordinary async functions capturing CPU state, memory, I/O and
service resources. They await Rust channel input without retaining a backend
borrow. `Cpu::run` drives execution; dropping a run waiter preserves the pending
service for a later run. The
[CPU worker example](../../../tests/browser/src/cpus.rs) exercises this complete
flow on native and browser workers: it passes memory handles and captures channels,
constructs local state, waits for input while the caller accesses mapped memory,
resumes guest execution, and reuses the allocation on a later worker.

## Browser build and lifetime

For a session that can spend a synchronous interval outside its executor,
`worker::wake_connection::<R>()` connects a retained `WakeTarget` to its Rust
wakers. Transfer the returned endpoint with `run_resource`, bind it there, and
poll the entire session through `connection.run(...)`. When the execution owner
has constructed its target, `connection.activate(target).await` installs the
host binding before busy execution and yields once so enclosing cancellation can
register its waker. Keep the returned `WakeHost` alive until execution finishes.

`WakeTarget::wake` requests return to the executor. The request remains set
through the complete session poll, so CPU-first composition such as
`join(run_cpu, input)` reaches a checkpoint before consuming ready input.
`acknowledge` clears it after the poll; wakes during that poll reassert the
request, including wakes racing with acknowledgement. The wrapped waker invokes
both the resource notification and the original executor waker on the sending
thread. Wrap an outer `Abortable` too, so cancellation uses the same notification
path as ordinary channel input.

Use `connection.yield_now().await` when busy execution cooperatively yields. It
wakes the current task, preserving child scheduling in combinators, without
requesting another interrupt from that wake. Other notifications, including a
combinator's own independent scheduling wakes, continue to request interruption.

Native connections retain a `Send` target directly. Browser connections post it
back to the responsive host and reconstruct local access there. Their eager host
receiver does not depend on polling the worker's completion handle. Browser
wakers carry a session ID; only the two connected threads have a local target
for it. Moving a Rust channel sender to an additional worker does not post those
engine resources. Such a sender still wakes the executor, but cannot interrupt a
busy guest through this connection. Additional direct peers need their own
resource setup before budgets can be removed. Session IDs are never reused;
late wakes cannot signal a replacement session.

The browser suite uses one threaded Rust configuration throughout: atomics,
bulk memory, mutable globals, rebuilt standard library, shared imported memory,
and wasm-bindgen's TLS exports. Shared Rust host memory is distinct from shared
guest memory. Pages must be cross-origin isolated with COOP/COEP headers.

The application resolves `web-thread:wasm-shim` to worker-safe bindings. A small
adapter passes the library's `module` argument as wasm-bindgen's `module_or_path`.
The shared build setup and browser checks live in
[tests/browser](../../../tests/browser/README.md). Backend tests exercise engine
operations; worker integration checks exercise channel wakeups, shared-memory
lifetimes, compiled-module handoff and CPU execution.

Await accepted work before dropping its thread. Browser bootstrap failure
reporting and thread allocation reclamation are tracked in
[#439](https://github.com/koolkdev/relocade/issues/439). Receiving posted messages
requires an event-loop task opportunity on the receiving browser thread.
