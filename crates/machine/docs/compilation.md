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
prefix up to those boundaries or the code segment limit.

`CompiledCode::install` checks the execution target and guest memory identity.
Before publishing dispatch entries and call links, the stopped CPU compares
every emitted instruction span with its live backing. A mismatch returns
`Error::StaleCompilation` and preserves existing entries and hooks. An
automatically requested function can become hot again after a stale result.
Explicit installation requires pending or parked host services to finish. A guest store
to RAM containing compiled instructions runs through the interpreter. If the
store changes an installed instruction byte, its host callback withdraws the
affected module's exports and repairs incoming call slots before returning.
The interpreter then continues the instruction and fetches subsequent bytes
from guest memory. A host that patches code while the PC is stopped can write
through a memory handle. The CPU retires those modules when control next passes
from the host to the guest. This also covers a host hook that patches a callee
and resumes guest execution within the same `Cpu::run`.

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

A session task forwards posted compiler replies into a single local slot.
Transport can finish while the CPU is stopped or the application drops a
manual receive future. Once the slot is full, the task waits for the client
to consume it before receiving the next module.

Sending a reply posts the module first and then publishes its Rust state, which
wakes the task. On a busy CPU this wake also requests a checkpoint in interpreted or compiled code.
In the browser, the Rust notification can arrive before the receiving worker's
`MessagePort` callback has delivered the module.

The receiver polls for the payload immediately. A pending poll registers demand
for a browser turn. The execution worker also registers demand while local
WebAssembly compilation is pending, for example when installing BIOS hooks
after the PIT and VGA have started.

A parked worker can receive browser callbacks immediately. On a busy worker,
`WorkerWake` gives runnable Rust work a 1 ms host-time grace period before
yielding to the browser. Completing or cancelling the last operation ends
these browser turns. [Worker scheduling](worker-execution.md#event-loop-scheduling)

At the start of a guest dispatch turn, a CPU using automatic Real16 compilation
checks the local slot for its outstanding request. It fills vacant dispatch
entries with compiled code and supplies interpreter fallbacks for any requested
roots still missing. A stale result returns its roots to sampling instead. Existing entries stay in place, including handlers retained
by a parked BIOS call. An interrupt handler can therefore use newly compiled
code while the outer call is parked.

The CPU also collects its call profile when the sampling interval has elapsed
and it gets an execution turn. Profiling never wakes an idle CPU.

An application can take the client during setup and wait for a reply while
the guest runs. If the reply arrives first, the CPU borrow ends and the
application can install it once all active and parked host calls have finished.
If the guest exits first, the client keeps the reply for the next receive.

This example runs a guest loop that makes no host calls.

```rust,ignore
use futures_util::future::Either;

let mut client = runner.take_compiler().expect("configured compiler");
client.request(target, blocks)?;
match runner.run_cpu_until(client.recv()).await {
    Either::Right(Some(result)) => result?.install(runner.cpu())?,
    Either::Right(None) => handle_compiler_shutdown(),
    Either::Left(exit) => handle_guest_exit(exit),
}
```
