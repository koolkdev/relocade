# Worker tasks and messages

The machine exposes `worker::Worker` and `ExecutionWorker` without feature flags.
`Worker::run` starts an ordinary Rust task. `Worker::listen` starts a task with a
local inbox and returns its sender; resources arrive through `sender.send(value)`.
The application keeps the worker alive until its tasks finish.
`ExecutionWorker::spawn` creates a worker and supplies its fully initialized
`Runner`; CPU-level work can use `Cpu` directly.

`worker::CancellableWorker<T>` owns a worker, its completion future and a cooperative
stop signal. Its `spawn` callback starts one task through the owner's `run` or
`listen` method, then sends any initial messages. These methods retain the task
before returning, so a setup or send failure can request cancellation and wait
for cleanup before being reported. The task must observe the cancellation receiver,
including while waiting for its first message, and finish its own cleanup.
`request_stop()` signals cancellation without releasing the owner;
`stop().await` signals cancellation and joins;
`join().await` waits for normal completion. `finished(&mut self)` permits racing
completion against another event while retaining the owner. Dropping that wait
does not cancel the task; consume its completed result only once.
Timer drivers, `ExecutionWorker` and graphics use this shared lifecycle.
`WorkerError` reports channel setup or send errors and worker-task failures.

Pass the memory handle directly. Capture CPU configuration and Rust channel
endpoints in the closure, then construct the CPU inside the worker:

```rust,ignore
use relocade_machine::{worker::{Receiver, Worker}, Cpu, Memory};

type Error = Box<dyn std::error::Error + Send + Sync>;

let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let worker = Worker::new();
let (mut input, task) = worker.listen(move |mut input: Receiver<Memory>| async move {
    let memory = input.recv().await.ok_or("memory sender closed")??;
    let mut cpu = Cpu::new(memory, cpu_config, Default::default()).await?;
    Ok::<_, Error>(cpu.run().await)
})?;
input.send(memory.clone())?;

// The caller can retain mapped RAM access while the worker executes.
let view = memory.view();
let exit = task.await??;
```

`Memory` derives `Transfer`. Each field's type defines its transfer behavior:
`SharedMemory` posts its engine handle, and `Arc<MachineConfig>` retains its Rust
allocation. Named, tuple and unit structs compose those definitions without field
annotations. Receiving a message reconstructs the complete value on that worker.

```rust,ignore
#[derive(worker::Transfer)]
struct CpuResources {
    storage: CpuStorage,
    memory: Memory,
}
```

The generated implementation carries typed Rust state and a platform payload,
then decodes their fields on the receiving worker. Callers do not extract backing
or supply reconstruction closures. Every field needs `Transfer`; being `Send`
alone does not select an encoding. A shared `Arc<T>` retains its allocation when
`T: Send + Sync`, without requiring transfer behavior on its referent.

`Worker::listen` accepts messages before startup finishes and throughout the task's
life. For another direction, create `worker::channel::<T>()` on the receiving
thread and transfer its `Sender<T>` to the producer. The receiver stays local;
the sender can itself be sent through another channel to establish a reverse
direction. Each channel has one sender, preserves order, and uses an unbounded
queue. `send` posts immediately and `recv().await` receives a complete value.

```rust,ignore
use relocade_machine::worker::{self, Receiver, Sender, Worker};

let (sender, mut compiled) = worker::channel::<CompiledUnit>()?;
let worker = Worker::new();
let (mut input, task) = worker.listen(|mut input: Receiver<Sender<CompiledUnit>>| async move {
    let mut sender = input.recv().await.unwrap()?;
    sender.send(compile_unit().await?)
})?;
input.send(sender)?;
let unit = compiled.recv().await.transpose()?;
task.await??;
```

Dropping a receive wait preserves its message. Dropping the sender lets the
receiver drain accepted messages before returning `None`; dropping the receiver
closes further sends. Browser object delivery still requires an event-loop turn
on the receiver. The channel does not own worker lifetime or CPU scheduling.

Use `CpuStorage::new()` when the host needs an execution-interrupt handle before
CPU construction. Retain `storage.interrupt_handle()`, send the storage through
the worker's inbox, then call `storage.into_cpu(memory, cpu_config, io).await` on the
execution worker. The storage is consumed by one CPU. Its signal is initialized
before handles are published; configuring CPU registers and the model preserves
requests made during construction. Neither preparation nor construction starts
guest execution. `Cpu::new` uses the same storage path when no early handle is needed.

`SharedMemory` and compiled `Module` handles implement `Transfer` and can be
sent directly through typed channels. The type defines its Rust state,
engine payload and reconstruction. Native handles move as Rust values. Browser
transport preserves their JavaScript objects; shared-memory bytes are shared, not copied.
The receiving worker prepares its own local access to the allocation. Retained
handles remain usable after the allocating worker exits.

Machine configuration, command payloads and channel endpoints remain ordinary
Rust data. They use `Send` closure captures and Rust channels without
serialization. Once running on its worker, a future can retain non-Send values
such as `Rc`, the CPU and browser engine objects. Construct those values inside
the task. `Worker::run` and `Worker::listen` return Send results through Rust memory;
send resources back through a channel when the result contains an engine object.
The low-level `Thread` API remains available for backend and pool integration.

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
