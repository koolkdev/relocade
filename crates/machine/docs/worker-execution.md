# Worker execution

`Runner` executes finite CPU quanta and yields to the host between them. It
returns the first non-budget exit and retains the CPU future while services await
input. The embedding owns its session task and composes independent device work
with ordinary async futures.

```rust,ignore
let runner = relocade_machine::Runner::default();
let exit = runner.run(&mut cpu).await;
handle_execution_exit(exit);
```

`Clock::now()` supplies shared elapsed session time; `wait_until(deadline)` waits
for a deadline while giving the host an opportunity to process work. The default
`MonotonicClock` uses `web_time::Instant` and `futures_timer::Delay`. Its clones
preserve the time origin. Tests can supply a manually advanced clock through
`Runner::with_clock`.

The task owns one `Cpu<T>` for its lifetime. `T` is its guest environment state;
BIOS/DOS service futures execute on the same worker. The CPU selects the native
or browser engine internally and retains its initialized memory resources.
Hooks run in Rust after Wasm returns. A `CpuContext` borrows the executing machine;
its CPU views bind storage directly and keep that backend borrow until they end.
Services can hold ordinary views and local references across input awaits.
Shared guest-memory handles support independent readers.

```text
UI --input/control messages--> worker message handlers
                                      |
                               queues and wakeups
                                      |
                               execution task
                               owns DOS + runtime
```

For a blocking console read, the execution task awaits input after the guest
returns. A worker message handler receives input bytes and writes them through an
owned stdin handle, waking the same task. The handle accesses an independent
queue, so delivering input does not borrow the runtime. Browser stream handles
stay inside the worker; messages carry bytes across the thread boundary. A native
embedding can use channels and a task on the runtime's owning thread.

Worker creation, UI messaging and task lifetime belong to the embedding
application. The runtime owns execution and service completion; the web and
Wasmtime backends supply Wasm engine access. Generated guest calls remain
synchronous; the execution task awaits services after those calls return.

## Task lifetime

The execution task has a lifetime independent of any individual UI request.
Dropping a UI waiter must not implicitly cancel the guest or its pending service.
An explicit stop command needs a control path that can wake the task while a
service is pending; merely checking commands after a read completes is insufficient.
The owning task stops by dropping the CPU and its pending service. Completed
side effects, including writes visible through shared memory or file handles, remain.

The CPU holds its machine directly between runs. Reaching a host service creates
an execution future that owns the machine and call chain until execution stops.
The future then returns the machine and exit result to the CPU and is released.
Runs that finish without reaching a host service allocate no execution future.

Dropping a `run` waiter preserves the pending future and its call chain; a later
`run` resumes it and supplies a new budget for subsequent guest execution.
Zero-budget runs do not poll pending work.

While a host service is pending, complete it before borrowing
CPU/environment state or installing modules and hooks. Use owned input/output
handles and `cpu.memory()` for interactive I/O and display. Dropping the CPU
releases the suspended call chain and its machine together.

Create a persistent DOS session and its CPU with `Dos::new().await`, then
mount filesystems and retain console handles before `launch_com(...)` or
`launch_mz(...)`. These synchronous launches reuse the CPU and guest memory.
Use `dos.process()` to retain the current program and inspect its `ProcessStatus`.
It returns `None` before the first launch; an exited process remains observable.
A launch while active returns `LaunchError::ProcessActive`, including when a
service is pending. Normal DOS termination releases process resources while the
session's mounts, directories, memory and console remain available. Launch does
not cancel a program. Guest break handling and nested EXEC are separate DOS work.

## Event-loop scheduling

`Runner::run` requests a nonzero instruction quantum (10,000 by default) and
awaits a host timer between exhausted quanta. On the browser this is a
`setTimeout` event-loop task, so posted messages and timers can progress. Awaiting
an already-ready future does not yield, and repeatedly scheduling microtasks can starve message delivery. Budget
checks remain at block boundaries; they provide
[no exact-stop or elapsed-time guarantee](../../x86/docs/host-integration.md#instruction-budgets).
