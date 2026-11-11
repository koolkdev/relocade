# Background compilation

The application owns one compilation pool and shares its handle across machines.
`compilation_pool()` defaults to half the available logical parallelism, rounded
down, with at least one worker. Native builds use the OS parallelism hint;
browsers use `navigator.hardwareConcurrency`. If unavailable, the default is one.
For an explicit limit, construct `worker::Pool` with the desired threads.

Drive the pool alongside the whole machine lifecycle, including shutdown:

```rust,ignore
let (pool, compiler_jobs) = relocade_machine::compilation_pool()?;
let application = async move {
    let mut machine = Machine::new(memory_config, compiler_jobs.clone())?;
    let execution = async {
        let cpu = machine.create_cpu(cpu_config)?;
        cpu.run_until(stop, async move |mut runner| {
            let mut compiler = runner.take_compiler().unwrap();
            let target = runner.cpu().cpu_state().execution_target();
            compiler.request(target, ranges)?;
            let code = compiler.recv().await.expect("compiler running")?;
            code.install(runner.cpu()).await?;
            Ok::<_, relocade_machine::Error>(runner.run_cpu().await)
        }).await
    }.await;
    let shutdown = machine.shutdown().await;
    let result = execution?;
    shutdown?;
    Ok::<_, relocade_machine::Error>(result)
};
let ((), result) = futures_util::future::join(pool.run(), application).await;
let result = result?;
```

Each `Machine` owns a compilation coordinator worker and transfers shared guest
memory once. The coordinator captures requested blocks and calls x86's
`Compiler::compile_blocks_with` with the shared pool. x86 owns decoding, program
construction and the returned bytes, entry metadata and execution target. IR
construction, placement and Wasm emission use the pool's jobs. The coordinator
compiles the emitted Wasm with the host engine and returns the module. The pool
limits concurrent Rust jobs; the host engine's internal compilation threads have
their own policy.

Construction stays synchronous; connections and requests can queue while the
coordinator starts. Every CPU gets a request/reply connection; each request
supplies its execution target. Creating a CPU does not create another coordinator
or pool. The coordinator exists even when the machine only interprets code.

The pool handle is ordinary sendable Rust state. Clones share one queue and worker
limit across machines. Keep polling the driver while machines drain accepted
requests, since they may submit further compilation jobs during shutdown. After
all coordinators and application handles are dropped, the driver drains its jobs
and exits. The `async move` above owns the original handle so it closes with the
application.

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

Dropping a connection closes its request stream. The worker drains that stream
and releases its reply sender and wake binding. Unused CPU handles require no
startup acknowledgement.
Await `Machine::shutdown` after joining execution workers, or join them separately
while shutting down. Shutdown closes the incoming connections; the worker then
closes each request inbox, drains accepted work and exits. Dropping Machine without
awaiting cleanup can terminate a browser worker before Rust destructors run.

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
