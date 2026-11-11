# Background compilation

Start one `CompilerService` and connect each CPU to it. The service owns
its coordinator worker, backend compiler and Rust compilation pool. It progresses
independently; the application does not poll a separate pool driver.

```rust,ignore
let compiler = relocade_machine::CompilerService::start()?;
let result = run_machine(&compiler).await;
let shutdown = compiler.shutdown().await;
let result = result?;
shutdown?;
```

Here `run_machine` creates `Machine::new(memory_config)` and calls
`machine.create_cpu(cpu_config, compiler)` for each CPU. CPU creation borrows the
service only to open that CPU's connection. Its request and result endpoints
transfer with the CPU to its execution worker; the worker communicates directly
with the service. The Machine retains no service borrow or routing task.
Await `Machine::shutdown` after execution finishes. Several Machines can use
the same service.

`CompilerService::start()` defaults to half the available logical parallelism,
rounded down, with at least one pool worker. Native builds use the OS parallelism
hint; browsers use `navigator.hardwareConcurrency`. If unavailable, the default
is one. `start_with_workers(NonZeroUsize)` supplies an explicit pool size.

Each CPU connection transfers a handle to its Machine's shared guest memory once;
requests carry addresses and bounds rather than memory bytes. The service
captures requested blocks and calls x86's
`Compiler::compile_blocks_with` with the internal pool. x86 owns decoding, program
construction and the returned bytes, entry metadata and execution target. IR
construction, placement and Wasm emission use pool jobs. The service compiles the
emitted Wasm with its backend compiler and returns the module. The pool limits
concurrent Rust jobs; backend engine parallelism remains separate.

Each connection has an async request loop on the service worker. Connections keep
their memory sources and ordered request streams independent while sharing
compilation capacity. CPU creation stays synchronous; connections and requests can
queue during startup. Every request supplies its execution target. Creating
another CPU or Machine does not create another pool.

The service owns compilation and delivery. Hotness, candidate selection, code
validity, installation, and future module caching or sharing between CPUs belong
to the Machine runtime and its execution owners.

`CompilerClient` is a single-owner request/result stream. `request` immediately
queues the execution target, addresses and compilation bounds. `recv` returns
results in that connection's request order, including compilation errors.
Dropping a receive wait preserves the next result.
The runtime can queue several requests and receive their results as it runs.

The worker uses `Memory::read_code` to copy mapped RAM/ROM directly from shared
backing, including A20 aliases. It shares address resolution with ordinary memory
views and requires no CPU or device bus. MMIO, holes and overflowing ranges are
rejected before capture allocation. `CompiledCode::install` checks the full
execution target and uses the existing CPU imports, dispatch and hook replacement
rules. Each CPU gets its own instance. Pending or parked host services must finish
before installation. Code discovery, hotness and invalidation remain separate;
callers keep code and mappings stable through compilation and installed use.

Dropping a CPU's `CompilerClient` closes its request stream. The worker drains
that stream and releases its reply sender and wake binding. Unused CPU handles
require no startup acknowledgement.
Await `Machine::shutdown` after joining execution workers, or join them separately
while shutting down. The Machine closes all its CPUs' request streams and waits
for every connection to drain accepted work, including when one reports an error.
Dropping the Machine also closes those streams; awaiting shutdown additionally
waits for completion. Other Machines continue using the service.

After all Machines finish, await `CompilerService::shutdown`. It closes admission,
drains the pool and joins its workers. Dropping the service
early can terminate browser workers before Rust destructors run.

Compiler clients bind receipt to the Runner's executor during setup. Poll their
receive futures inside that session, independently of `runner.run_cpu()`.
Guest and device work continue while compilation is in progress. Once the
compiler posts a reply, the channel's Rust state confirms that its host payload
is on the way. If that payload is not yet available, the receiver registers a
pending delivery. The executor shares one 1 ms interval on the independent host
clock across pending deliveries before requesting a zero-delay event-loop turn.

The executor suspends the whole activity set for that turn, including busy
devices, then resumes it even when delivery is still pending. Each retry gives
work another 1 ms interval. Empty channels, compilation that has not posted a
reply, and already available payloads request no timer. Completion or cancellation
clears the receive's registration; the last one cancels the shared timer and yield.
Actual intervals can overshoot
because guest checkpoints and host callbacks do not have fixed latency bounds.

`Runner::new` uses its supplied independently progressing clock. Guest-time
execution uses the explicitly configured host clock; without one, it requests
the host turn immediately instead of waiting on guest time. The generic worker
API exposes this through `Receiver::with_executor` and `Executor::with_host_timer`.
Compiler code contains no platform-specific delivery flags.
The internal host policy reports application readiness: ready during the grace
interval, pending during the yield, then ready again even if delivery is late.
The executor polls stop before readiness and uses the same parking path for idle
work and host yields. The Runner's execution schedule continues to own CPU budgets,
guest deadlines and guest-time pacing.

Receiving code does not install it or stop the guest automatically. Drop or finish
the current run waiter at an allowed CPU boundary, install explicitly, and resume
execution. Local module instantiation is synchronous on both current backends.
