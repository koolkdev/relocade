# Worker tasks and messages

The machine exposes `worker::Thread` and `ExecutionWorker` without feature flags.
The Wasm runtime reexports `web-thread-select` and adds `ThreadExt::run_resource`
for typed resource handles. The application creates workers and keeps them alive
until their tasks finish. `ExecutionWorker::spawn` creates a worker and supplies
its fully initialized `Runner`; CPU-level work can use `Cpu` directly.

`worker::CancellableWorker<T>` owns a thread, its result and a cooperative stop
signal. Its `spawn` callback receives the thread and cancellation receiver, then
starts a task with `run_send` or `run_resource`. The task must observe the receiver
and finish its own cleanup. `stop().await` signals cancellation and joins;
`join().await` waits for normal completion. `finished(&mut self)` permits racing
completion against another event while retaining the owner. Dropping that wait
does not cancel the task; consume its completed result only once.
Timer drivers and `ExecutionWorker` use this shared lifecycle.

Pass the memory handle directly. Capture CPU configuration and Rust channel
endpoints in the closure, then construct the CPU inside the worker:

```rust,ignore
use relocade_machine::{worker::{Thread, ThreadExt}, Cpu, Memory};

let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let worker = Thread::new();
let task = worker.run_resource(memory.clone(), move |memory| async move {
    let mut cpu = Cpu::new(memory, cpu_config, Default::default()).await?;
    Ok::<_, relocade_machine::Error>(cpu.run().await)
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

Use `CpuStorage::new()` when the host needs an execution-interrupt handle before
CPU construction. Retain `storage.interrupt_handle()`, post the storage with
`run_resource`, then call `storage.into_cpu(memory, cpu_config, io).await` on the
execution worker. The storage is consumed by one CPU. Its signal is initialized
before handles are published; configuring CPU registers and the model preserves
requests made during construction. Neither preparation nor construction starts
guest execution. `Cpu::new` uses the same storage path when no early handle is needed.

`SharedMemory` and compiled `Module` handles implement `Transfer` and can be
passed directly to `Thread::run_resource`. The type defines its Rust state,
engine payload and reconstruction. Native handles move as Rust values. Browser
transport preserves their JavaScript objects; shared-memory bytes are shared, not copied.
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

`wake_connection(target)` binds a fixed notification resource on the host and
returns a host guard plus Send endpoint metadata. Capture the endpoint alongside
an already posted resource, then call `endpoint.bind(local_target)` on execution.
Both target handles must address the same signal. This reuses the resource's
existing transfer instead of posting it again through a registration channel.
`ExecutionWorker` does this automatically with `CpuStorage`; callers construct
its CPU through `runner.create_cpu` or `Dos::new`. `WakeConnection` retains the
binding; the separate `worker::Executor` owns polling, stop handling and idle parking.

## Browser build and lifetime

Browser runtime builds use one threaded Rust configuration throughout: atomics,
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
