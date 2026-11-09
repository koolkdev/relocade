# Worker tasks and messages

The machine exposes `worker::Thread` and `ExecutionWorker` without feature flags.
The Wasm runtime reexports `relocade-workers`, which uses `web-thread-select`
and adds typed resource transfer for task inputs and messages.
The application creates workers and keeps them alive
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
The common derive supports named and tuple structs with one `#[worker(post)]`
leaf or `#[worker(resource)]` nested resource; other fields stay in Rust as
`Send` captures. It retains the existing configuration and
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

`SharedMemory` and compiled `Module` handles still support the library's `Post`
transport directly. Native handles move as Rust values. Browser transport
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

`wake_connection(target)` binds a fixed notification resource on the host and
returns a host guard plus Send endpoint metadata. Capture the endpoint alongside
an already posted resource, then call `endpoint.bind(local_target)` on execution.
Both target handles must address the same signal. This reuses the resource's
existing transfer instead of posting it again through a registration channel.
`ExecutionWorker` does this automatically with `CpuStorage`; callers construct
its CPU through `runner.create_cpu` or `Dos::new`. `WakeConnection` retains the
binding; the separate `worker::Executor` owns polling, stop handling and idle parking.

## Resource messages

Use the same `WorkerResource` definition for task inputs and persistent
messages. A resource can combine a posted module with Rust metadata:

```rust,ignore
#[derive(worker::WorkerResource)]
struct CompiledCode {
    #[worker(post)]
    module: Module,
    entries: Vec<BlockEntry>,
}
```

For repeated delivery, create a typed resource channel:

```rust,ignore
let (sender, endpoint) = worker::channel::<CompiledCode>()?;
let compile = compiler_thread.run_resource(sender, move |mut sender| async move {
    while let Some(request) = requests.next().await {
        sender.send(compile_request(request).await?)?;
    }
    Ok::<(), Error>(())
});
```

The sender and receiver endpoint are themselves worker resources. Transfer the
endpoint to its final owner, then call `bind()` to obtain a local receiver.
Embed either handle in another resource using `#[worker(resource)]`. The channel
uses the object's transfer definition to keep its module and metadata together;
callers do not encode payloads or match two deliveries.

Sending posts the engine payload before publishing the Rust reconstruction state.
It does not acknowledge receipt. Cancelling `recv()` preserves a partially
received object. Normal sender closure drains accepted objects before EOF,
including payloads still awaiting browser delivery. Abrupt browser worker
termination cannot promise Rust cleanup or orderly closure. Await accepted tasks
before releasing their workers.

Requests use Rust channels. Resource receipt itself observes when the sender
has posted the engine object; services need no additional readiness message.
On a busy worker, bind the endpoint once with `endpoint.bind_on(&executor)`.
Its ordinary `recv().await` then arranges host turns only while a posted object
is missing. `Executor::with_host_timer` accepts an independent host timer and
defers those turns by one millisecond. Without that timer the executor yields
immediately. Ordinary async owners can continue to use `bind()`.

`Result<R, E>` is a worker resource when `R` is one and `E` is sendable. Only the
success payload or error variant marker is posted; error data stays in Rust.
Success and failure therefore occupy the same ordered channel.

For a one-time handoff to an existing session, `worker::oneshot::channel` creates
an ordinary Rust sender and endpoint. The endpoint can be captured in a `Send`
closure; it binds a local receiver on the destination worker:

```rust,ignore
let (sender, endpoint) = worker::oneshot::channel::<CompiledCode>();
let receive = execution_thread.run_send((), move |()| async move {
    let code = endpoint.bind()?.receive().await?;
    use_compiled_code(code).await
});
sender.send(&execution_thread, code).await?;
receive.await??;
```

`send` waits for binding and uses the existing `run_resource` transport. Await it
before releasing the thread. `CancellableWorker::send_resource` exposes the same
operation on an owned worker. The resource stays local after reconstruction;
only registration and completion notifications travel through Rust channels.
A failed dispatch also resolves the receive. A busy owner binds with
`endpoint.bind_on(&executor)` and awaits `receive()` directly. Receipt includes
posting readiness, host delivery and the posting task's completion so that task
can finish even when the session stays busy afterward. `Runner::receive_resource`
provides this binding for machine execution.

## Persistent compilation

Create one `CompilerWorker` for shared guest memory and an execution target.
Each client supplies one connection: a Rust request receiver and a resource
response sender. The compiler attaches that connection to its existing thread
with the shared `Memory` handle. Requests carry ranges, with no copied code bytes.

```rust,ignore
let mut worker = CompilerWorker::start(&memory, target)?;
let (mut compiler, connection) = CompilerClient::channel()?;
worker.connect(connection);
let code = compiler.compile(vec![BlockRequest {
    address: 0x1100,   // Guest physical address before mapping and A20.
    start_eip: 0x100, // CS-relative export entry.
    byte_len: 32,
    instruction_limit: 8,
}]).await;
drop(compiler);
worker.shutdown().await?;
let code = code?;
cpu.install_compiled_module(&code.module, &[DispatchEntry {
    address: 0x1100, // Linear dispatch address, CS.base + EIP.
    export: &code.entries[0].export,
}]).await?;
```

`connect` starts a persistent request loop on the compiler thread. Compilation
runs without polling a host forwarding task or the shutdown future. Connections
have independent lifetimes, so one compiler can outlive successive execution
owners. Close clients and await `shutdown` to join every connection, including
when another task fails.

A busy execution owner creates its client with `runner.compiler_connection()`.
It sends the returned `CompilerConnection` to the compiler's host once, using the
same resource channel as other worker objects. The host attaches it with
`worker.connect(connection)`. From then on the client awaits `compile(ranges)`
directly; receipt is already bound to execution. `runner.bind_resource(endpoint)`
and `runner.receive_resource(endpoint)` provide the same policy for other
repeated and one-time resources.

The worker reads mapped RAM/ROM locally; MMIO, unmapped ranges and overflowing
bounds are rejected before allocation. Writes made before a request are visible
through shared backing. Requested bytes and mappings must remain stable until
the work finishes. This is not an atomic snapshot of changing code. Code validity
after installation, invalidation and automatic discovery remain separate work.

`CompiledCode` carries the prepared module, entries and execution target as one
object. Success and compilation errors share one response stream. A client owns
its in-flight request and partial receipt across cancellation. Its next call
drains an abandoned response before submitting a new request; dropping the
client closes submission while the compiler finishes already accepted work.
There are no request IDs, separate readiness replies or client-driving tasks.

Installation reuses the CPU's existing imports, dispatch table and hook rules.
It requires the same stopped state as `install_module`. Receiving a module does
not install it or change that contract. See
[event-loop scheduling](worker-execution.md#event-loop-scheduling) for delivery
alongside busy guest execution.

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
