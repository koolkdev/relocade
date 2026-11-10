# Worker tasks and messages

The machine and application expose `worker::Worker`. Create a worker, define its
resource channels, and start tasks independently. Keep the worker alive until its
tasks finish. `ExecutionWorker::spawn(...).await?` configures a CPU worker and
supplies its initialized `Runner`; CPU-level work can use `Cpu` directly.

`Worker::run` takes a Send factory and constructs its future on the worker. The
future may retain local objects across awaits. `Worker::channel::<R>().await?`
registers resource receipt on that worker; capture the returned receiver in a
task, then send complete resources whenever they are needed:

```rust,ignore
use relocade_machine::{worker::Worker, Cpu, Memory};

let memory = Memory::new(machine_config)?;
memory.view().write(0x100, &program)?;
let worker = Worker::new();
let (mut sender, mut incoming) = worker.channel::<Memory>().await?;
let task = worker.run(move || async move {
    let memory = incoming.recv().await.unwrap().unwrap();
    let mut cpu = Cpu::new(memory, cpu_config, Default::default()).await?;
    Ok::<_, relocade_machine::Error>(cpu.run().await)
});
sender.send(memory.clone())?;

// The caller can retain mapped RAM access while the worker executes.
let view = memory.view();
let exit = task.await??;
```

`worker.run_cancellable(move |stopped| async move { ... })` consumes an already
configured worker and returns `CancellableWorker<T>`. Its task must observe the
cancellation receiver and finish its own cleanup. `stop().await` signals
cancellation and joins; `join().await` waits for normal completion.
`finished(&mut self)` permits racing completion against another event while
retaining the owner. Dropping that wait does not cancel the task; consume its
completed result only once. Timer drivers and `ExecutionWorker` use this lifecycle.

`Memory` derives `WorkerResource` and marks its backing with `#[worker(resource)]`.
Each resource type owns its transfer, so containing structs use the same marker
for engine handles and composite resources. The derive supports named and tuple
structs; unmarked fields stay in Rust as `Send` captures. For example, a handle
containing both CPU storage and guest memory can declare those fields directly:

```rust,ignore
#[derive(worker::WorkerResource)]
struct CpuResources {
    #[worker(resource)]
    storage: CpuStorage,
    #[worker(resource)]
    memory: Memory,
    config: CpuConfig,
}
```

The channel reconstructs complete resources on receipt. Callers do not extract
backing or pair it with configuration, and transfer does not expose a capability's
underlying storage. The derive uses `ResourceTransfer` composition internally.
An existing worker can register more channels and run more tasks throughout its
lifetime. Resource transfer is independent of task submission.

Use `CpuStorage::new()` when the host needs an execution-interrupt handle before
CPU construction. Retain `storage.interrupt_handle()`, send the storage through a
channel, then call `storage.into_cpu(memory, cpu_config, io).await` on the
execution worker. The storage is consumed by one CPU. Its signal is initialized
before handles are published; configuring CPU registers and the model preserves
requests made during construction. Neither preparation nor construction starts
guest execution. `Cpu::new` uses the same storage path when no early handle is needed.

`SharedMemory` and compiled `Module` handles implement `WorkerResource` and use
the same channels. Native handles move as Rust values. Browser transport
preserves their JavaScript objects; shared-memory bytes are shared, not copied.
The receiving worker prepares its own local access to the allocation. Retained
handles remain usable after the allocating worker exits.

Machine configuration, command payloads and channel endpoints remain ordinary
Rust data. They use `Send` closure captures and Rust channels without
serialization. Once running on its worker, a future can retain non-Send values
such as `Rc`, the CPU and browser engine objects. Construct those values inside
the task. `Worker::run` returns Send results through Rust memory. To return engine
resources to the calling thread, create a local `worker::channel()`, send its
sender to the worker through a resource channel, and receive the result locally.

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

## Resources delivered to running tasks

`Worker::channel::<R>().await?` returns a sender and a receiver bound to that
worker. The receiver is a Send token that a task factory can capture. Calls to
`recv()` must run on its destination; both native and browser builds enforce this.
Callers never transfer or bind browser ports. The awaited setup reports transport
failure before task submission. The destination must remain alive and responsive
to browser delivery during setup and receipt.

The free `worker::channel::<R>()` binds receipt to the calling thread. A sender is
itself a `WorkerResource`, so it can travel to another worker through an existing
resource channel. This supports compiler-to-execution and worker-to-host replies
without adding special task inputs or return transports.

```rust,ignore
let worker = Worker::new();
let (mut sender, mut incoming) = worker.channel::<Memory>().await?;
let task = worker.run(move || async move {
    while let Some(memory) = incoming.recv().await {
        let memory = memory?;
        memory.view().write_u8(0x100, 42).unwrap();
    }
    Ok::<_, worker::ResourceError>(())
});

sender.send(memory.clone())?;
drop(sender);
task.await??;
assert_eq!(memory.view().read_u8(0x100)?, 42);
```

The channel is unbounded. `send` posts synchronously and does not wait for receipt;
it requires exclusive access to preserve ordering. Dropping the sender lets the
receiver drain accepted objects before returning `None`. Dropping a pending
`recv()` future leaves its message available for the next receive.

Browser delivery preserves posted engine objects while ordinary Rust state travels
through shared Rust memory. The channel keeps both parts paired, including when
posting or decoding fails. Receiving a browser object needs an event-loop turn;
the transport does not set the execution runtime's yield policy. The
[worker channel test](../../wasm-runtime/tests/worker_channels.rs) transfers compiled
modules and shared memory between persistent workers through this API.

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
