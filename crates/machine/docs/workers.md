# Worker tasks and messages

The machine exposes `worker::Thread` and `ExecutionWorker` without feature flags.
The Wasm runtime reexports `web-thread-select` and adds `ThreadExt::run_resource`
for typed resource handles. The application creates workers and keeps them alive
until their tasks finish. `ExecutionWorker::spawn` creates a worker and supplies
its fully initialized `Runner`; CPU-level work can use `Cpu` directly.

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

## Persistent resource channels

For repeated resource delivery from a persistent worker, create a
`worker::channel::<Module>()`. Transfer each `Endpoint` to its final owner with
`ThreadExt::run_resource`, then call `bind()` to create a local `Port`:

```rust,ignore
let (sender, receiver) = worker::channel::<Module>()?;
let compile = compiler_thread.run_resource(sender, move |sender| async move {
    let sender = sender.bind();
    while let Some(request) = requests.next().await {
        sender.send(compile_request(request).await?)?;
    }
    Ok::<(), Error>(())
});
```

Use Rust channels for requests and readiness notifications; resource ports carry
engine handles. Sending queues a resource without acknowledging receipt. Bound
ports own their local listener and cannot be transferred. Cancelling `recv()`
preserves queued values. Dropping a bound peer closes the stream after its queued
messages. An unbound endpoint has no close handshake; bind it on its final owner
before relying on orderly closure. Browser worker termination cannot promise
that handshake. Await accepted tasks before releasing their workers.

Receiving a posted resource still needs a browser event-loop turn. A Rust
readiness notification can interrupt guest execution, but cannot itself deliver
the engine handle. Resource transport does not choose execution scheduling.

## Persistent compilation

Start one `CompilerWorker` for shared guest memory and an execution target.
Startup transfers the existing `Memory` handle and a resource endpoint together
through `ThreadExt::run_resource`; it does not reconstruct memory in the caller.
Pairs of `WorkerResource` handles compose their existing transfers.

```rust,ignore
let mut compiler = CompilerWorker::start(&memory, target).await?;
let code = compiler.compile_blocks(vec![BlockRequest {
    address: 0x1100,       // Guest physical address before mapping and A20.
    start_eip: 0x100,     // CS-relative export entry.
    byte_len: 32,
    instruction_limit: 8,
}]).await?;
compiler.shutdown().await?;
cpu.install_compiled_module(&code.module, &[DispatchEntry {
    address: 0x1100,      // Linear dispatch address, CS.base + EIP.
    export: &code.entries[0].export,
}]).await?;
```

Requests carry bounded ranges, not instruction bytes. The worker reads mapped
RAM/ROM locally; MMIO, unmapped ranges and overflowing bounds are rejected before
allocation. Writes made before a request are visible through the shared backing.
The requested code and mappings must remain stable until receipt. This is not an
atomic snapshot of concurrently changing code. The caller continues to maintain
code validity after installation; invalidation and automatic discovery are separate.

`CompiledCode` contains a prepared module, its exported entries and required
execution target. Installation uses the CPU's existing imports, dispatch table
and hook replacement rules, and requires the same stopped state as
`install_module`. The caller supplies compatible execution state and addresses.
The compiler owns no CPU and can outlive individual execution owners.

Dropping a request waiter abandons that result without stopping the worker.
Request IDs keep late posted modules separate from later results. Compilation
errors fail that request and leave the worker available. `shutdown().await`
closes submission and waits for the persistent loop to release its resources.
Keep the owner alive until shutdown completes. On a browser, call this API from
a responsive owner; receiving modules alongside busy guest execution needs an
explicit host-turn policy.

## Session compiler service

`CompilerService` owns the persistent compiler on the responsive host. Create its
connections before driving `run()`. Each `CompilerConnection` is one typed worker
resource containing Rust request state and a posted module endpoint:

```rust,ignore
let mut service = CompilerService::new(CompilerWorker::start(&memory, target).await?);
let connection = service.connect()?;
let execution = ExecutionWorker::run_until(
    stop,
    (memory.clone(), connection),
    async move |runner, (memory, connection)| {
        let mut compiler = connection.bind();
        run_session(runner, memory, &mut compiler).await
    },
);
futures_lite::future::zip(service.run(), execution).await;
```

Each local `CompilerClient` requests compilation through Rust channels. Its
`request(ranges).await` returns `PostedCode` only after the service has posted the
module to that execution owner. Receive it with
`runner.wait_for_host(posted.receive()).await` while composing CPU execution and
the receive activity. Guest execution continues through compilation and between
browser delivery attempts; see [event-loop scheduling](worker-execution.md#event-loop-scheduling).
Receiving the resource does not install it or relax installation's stopped-state
contract. The service retains no CPU or automatic installation policy.

Connections have independent lifetimes. Dropping one client or abandoning a
result leaves others usable. Request IDs discard abandoned posted results on
the next receive. Once all connections close, the service drains requests and
awaits compiler shutdown. Await that completion before releasing the host task.
One service can outlive successive execution workers sharing the same guest
memory. Code stability remains the caller's responsibility.

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
