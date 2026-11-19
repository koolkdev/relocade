# Background compilation

Independent PCs can share one `CompilerService`. The service owns
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
`machine.create_cpu(cpu_config, Some(compiler))` once for its guest CPU.
CPU creation borrows the service only to open that connection. Its request and result endpoints
transfer with the CPU to its execution worker; the worker communicates directly
with the service. The Machine retains no service borrow or routing task.
Await `Machine::shutdown` after execution finishes. Several Machines can use
the same service.

Pass `None` to `create_cpu` when a CPU does not need background compilation.
It uses the bundled interpreter and opens no compiler connection, so tests can
run without starting a service. `Runner::take_compiler` then returns `None`.

`CompilerService::start()` defaults to half the available logical parallelism,
rounded down, with at least one pool worker. Native builds use the OS parallelism
hint; browsers use `navigator.hardwareConcurrency`. If unavailable, the default
is one. `start_with_workers(NonZeroUsize)` supplies an explicit pool size.

Each CPU connection transfers a handle to its Machine's shared guest memory once.
Requests carry guest addresses and compilation bounds. For explicit block requests,
the service captures the bytes and calls `Compiler::compile_blocks_with`. For
function requests, the service asks x86 to decode each instruction and uses its
control flow to discover reachable blocks. It passes those decoded blocks to
`Compiler::compile_decoded_blocks_with`. IR construction, placement and Wasm
emission use pool jobs. The service compiles the emitted Wasm
with its backend compiler and returns the module. The pool limits concurrent
Rust jobs. The backend engine controls its own parallelism.

The compiler's optional `workers` feature adapts the generic `JobExecutor` to
`CompileExecutor`. Machine compilation and interpreter asset generation enable
that integration; `relocade-workers` has no dependency on the compiler.

Each connection has an async request loop on the service worker. Connections keep
their memory sources and ordered request streams independent while sharing
compilation capacity. CPU creation stays synchronous; connections and requests can
queue during startup. Every request supplies its execution target. All connected
PCs use the same pool.

The service owns compilation and delivery. Hotness, candidate selection, code
validity, installation, and future module caching or sharing between CPUs belong
to the Machine runtime and its execution owners.

`CompilerClient` is a single-owner request/result stream. Each `BlockRequest`
supplies a real-mode `FarPointer` and compilation bounds. The worker reads the
bytes at that CS:IP and the runtime installs the result under the same CS:IP,
even if the CPU changes segments before delivery. Two far pointers can share
instruction bytes and have separate dispatch entries. `recv` returns results in
that connection's request order, including compilation errors.
Dropping a receive wait preserves the next result.
The runtime can queue several requests and receive their results as it runs.

`CompilerClient::request_functions` queues guest functions for compilation. Each
`FunctionRequest` supplies an entry CS:IP and its discovery limits. The machine
uses the entries as roots and follows branches and call continuations. Entries
in the same code segment and decode width can share blocks. Request a callee
by including its entry. Installing the result publishes the compiled function
entries and post-call continuations to CPU dispatch. Other blocks stay inside
the module. Its source registration covers the bytes of every compiled block.

The worker's code reader copies mapped RAM/ROM directly from shared backing,
including A20 aliases. It shares address resolution with ordinary memory views
and requires no CPU or device bus. An explicit capture rejects holes, MMIO and
overflowing ranges before allocating its buffer. Discovery reads the available
prefix up to those boundaries or the code segment limit. `CompiledCode::install`
checks the full execution target and uses the existing CPU imports, dispatch
and hook replacement rules. Each CPU gets its own instance. Pending or parked
host services must finish before installation. Code discovery, hotness and invalidation remain separate;
callers keep code and mappings stable through compilation and installed use.

Dropping a CPU's `CompilerClient` closes its request stream. The worker drains
that stream and releases its reply sender and wake binding. Unused CPU handles
require no startup acknowledgement.
Await `Machine::shutdown` after joining execution workers, or join them separately
while shutting down. The machine closes its CPU's request stream. Shutdown waits
for accepted work to drain and returns any connection error. Dropping the machine
also closes the stream. Other PCs continue using the service.

After all Machines finish, await `CompilerService::shutdown`. It closes admission,
drains the pool and joins its workers. Dropping the service
early can terminate browser workers before Rust destructors run.

## Compiler replies

A session task receives posted compiler replies. It can receive a module
while the CPU is stopped or a manual reader is between calls. A local queue
holds one reply for the client. Once that slot is full, the task waits for
the client to consume it before receiving the next module.

Sending a reply posts the module first and then publishes its Rust state, which
wakes the task. On a busy CPU this wake also requests a checkpoint in interpreted or compiled code.
In the browser, the Rust notification can arrive before the receiving worker's
`MessagePort` callback has delivered the module.

The receiver polls for the payload immediately. A pending poll registers demand
for a browser turn. The execution worker also registers demand while local
WebAssembly compilation is pending, for example when installing BIOS hooks
after the PIT and VGA have started.

If the worker is idle, it returns to the browser immediately. While Rust work
is runnable and a callback is pending, the machine scheduler gives that work
1 ms of elapsed host time before yielding to the browser. The CPU returns at completed
checkpoints so the worker can check the deadline between turns. A long guest
block can overshoot it. Pending callbacks share this interval. After a browser
turn, the worker resumes runnable work and retries if necessary. Completing
or cancelling the last operation ends the retries.

For automatic Real16 compilation, the CPU checks for a delivered reply after
polling its active host service. A parked BIOS call can retain dispatch entries
while its interrupt handler runs. The reply stays in the client slot until
the last such call returns and releases those entries. The resumed call goes
through the same CPU check before guest execution continues.
The CPU installs compiled code for missing entries and provides interpreter fallbacks for
remaining roots. An application that takes ownership through
`Runner::take_compiler` can instead await `CompilerClient::recv` as a session
task, stop its CPU run at an allowed boundary, and install the returned code
explicitly. Dropping that receive wait preserves the result for another wait.
