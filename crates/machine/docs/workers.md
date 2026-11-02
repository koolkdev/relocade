# Worker tasks and messages

## Compiling mapped guest code

`CompilerWorker::start(&memory, target).await` starts one persistent compiler loop
for shared guest backing, its physical map and a fixed execution target. The
worker attaches the backing once. Subsequent `BlockRequest` messages carry only
addresses, byte bounds and instruction limits through a Rust channel. The worker
reads RAM/ROM through the configured map, captures its own temporary instruction
bytes, builds x86 IR and compiles the Wasm module. It retains no CPU or device bus.

```rust,ignore
let mut compiler = CompilerWorker::start(&memory, target).await?;
let compiled = compiler.compile_blocks(vec![BlockRequest {
    address: cs_base + start_eip,
    start_eip,
    byte_len: available_code_bytes,
    instruction_limit: 64,
}]).await?;
compiler.shutdown().await?;
```

Requests use guest physical addresses before A20/physical mapping. Exported
entries retain CS-relative EIPs; dispatch addresses include CS.base. RAM/ROM
captures may cross mapping spans. MMIO and unmapped ranges are rejected without
device accesses. Keep every requested code range and its mapping unchanged from
submission through receipt; shared memory does not make a range read atomic.
No invalidation or automatic installation is provided by this lifecycle layer.

Typed errors and entry metadata use Rust responses; modules use the resource
channel below. Canceling a request waiter leaves the worker alive. Request IDs
prevent a late module from an abandoned request being used for its successor.
`shutdown().await` closes submissions and drains accepted work before releasing
the worker. Await startup and shutdown through completion; abrupt browser worker
termination has the existing `Thread` cleanup limitation.

The returned module can outlive the compiler worker. Each compatible CPU uses
`install_compiled_module` to bind it to its own state, memory, dispatch and hooks.
The caller maintains instruction validity and execution assumptions for as long
as those entries remain installed. Active or parked services must finish before
installation, as with `install_module`.

## Session-owned compiler coordination

`CompilerService` owns the compiler worker and relays results on the main thread.
Create its connections before running the service. Each connection has a Send
Rust request half and a paired resource endpoint posted to its execution owner:

```rust,ignore
let worker = CompilerWorker::start(&memory, target).await?;
let mut service = CompilerService::new(worker);
let (connection, endpoint) = service.connect()?;
let execution = ExecutionWorker::run_until(stop, (memory.backing().clone(), endpoint),
    async move |runner, (backing, endpoint)| {
        let memory = Memory::with_backing(machine_config, backing)?;
        let mut compiler = connection.bind(endpoint);
        let posted = compiler.request(ranges).await?;
        let compiled = runner.wait_for_host(posted.receive()).await?;
        // Bind compiled.module to the local CPU at an installation boundary.
        Ok::<_, Error>(())
    });
let (shutdown, result) = futures_lite::future::zip(service.run(), execution).await;
shutdown?;
```

The request phase uses Rust communication. Its completion confirms the module
has been posted to that execution owner; `receive` pairs it with entry metadata
and can still need a browser host turn. Run compilation alongside CPU work in the
runner's main activity. `wait_for_host` controls the complete runnable group, so a
busy CPU or device cannot starve delivery. The compiler protocol has no browser
activity flags or scheduling branches.

Dropping a posted result abandons only that result. Request IDs drain late modules
before a subsequent result. Dropping a connection leaves other connections and
the compiler alive. Once all connections are dropped, `service.run()` drains the
queue and awaits compiler shutdown; await it through completion. The service
owns neither CPUs nor installation, and retains no code cache. The compiler can
outlive individual execution sessions. This does not expand the guest-memory
concurrency contract to multiple writers or provide code invalidation.

## General worker tasks

Enable the machine's `worker` feature for `worker::Thread`, reexported from
`web-thread-select`. The Wasm runtime exposes the same worker API and resource
channels through its own `worker` feature. The application creates workers and
keeps them alive until their tasks finish.

Pass the shared allocation as the task context. Capture configuration and Rust
channel endpoints in the closure, then construct the CPU inside the worker:

```rust,ignore
use relocade_machine::{worker::Thread, Cpu, Memory};

let memory = Memory::new(machine_config.clone())?;
memory.view().write(0x100, &program)?;
let worker = Thread::new();
let task = worker.run_send(memory.backing().clone(), move |backing| async move {
    let memory = Memory::with_backing(machine_config, backing)?;
    let mut cpu = Cpu::new(memory, cpu_config, Default::default()).await?;
    Ok::<_, relocade_machine::Error>(cpu.run(budget).await)
});

// The caller can retain mapped RAM access while the worker executes.
let view = memory.view();
let exit = task.await??;
```

`SharedMemory` and compiled `Module` handles support the library's `Post`
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

## Persistent resource delivery

`worker::channel::<Module>()` creates two transferable resource endpoints. Pass
an endpoint as a worker context and capture an ordinary Rust request receiver in
its closure. Call `endpoint.bind()` on the receiving worker to create its local
port. A persistent worker can then accept Rust requests and call
`port.send(module)` repeatedly, without starting another `Thread::run` task for
every result. The other port receives through `recv().await` or `poll_recv`.
The same channel can carry other `Post` resources, including shared memories.

Bound browser ports retain a local listener and Rust inbox and cannot be
transferred. Dropping a receive waiter preserves the port and queued messages.
Dropping the bound peer normally ends reception after its last message. Keep the
worker alive through its task completion so its ports and other Rust resources are dropped.
A successful send queues a message; it does not acknowledge receipt.

Browser ports post resource handles through `MessagePort`; native ports move
values through Rust channels. Browser delivery still needs an event-loop turn
on the receiving owner. Request scheduling and readiness notifications remain
ordinary Rust communication, separate from resource delivery.

Host services are ordinary async functions capturing CPU state, memory, I/O and
service resources. They await Rust channel input without retaining a backend
borrow. `Cpu::run` drives execution; dropping a run waiter preserves the pending
service for a later run. The
[CPU worker example](../../../tests/browser/src/cpus.rs) exercises this complete
flow on native and browser workers: it captures configuration and channels,
constructs local state, waits for input while the caller accesses mapped memory,
resumes guest execution, and reuses the allocation on a later worker.

## Browser build and lifetime

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
