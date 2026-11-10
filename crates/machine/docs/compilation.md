# Background compilation

Creating a machine starts its compiler worker:

```rust,ignore
let mut machine = Machine::new(memory_config)?;
let cpu = machine.create_cpu(cpu_config)?;
let execution = cpu.run_until(stop, async move |mut runner| {
    let mut compiler = runner.take_compiler().unwrap();
    let target = runner.cpu().cpu_state().execution_target();
    compiler.request(target, ranges)?;
    let code = compiler.recv().await.expect("compiler running")?;
    code.install(runner.cpu()).await?;
    Ok::<_, relocade_machine::Error>(runner.run_cpu().await)
}).await;
let shutdown = machine.shutdown().await;
let result = execution?;
shutdown?;
```

`Machine` owns one compiler worker and transfers shared guest memory once.
Construction stays synchronous; connections and requests can queue while the
worker starts. Every CPU gets a request/reply connection; each request supplies
its execution target. Creating a CPU does not start another compiler task. The
worker exists even when the machine only interprets code.

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

Browser module receipt currently requires an execution session that can park;
continuously runnable guest or device work can otherwise delay object delivery.
