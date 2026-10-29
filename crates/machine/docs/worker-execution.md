# Worker execution

Each execution worker runs one CPU session and its registered device activities.
`Runner::run` drives them together until the main session finishes or is cancelled.
`run_cpu` executes finite instruction quanta and retains a pending
CPU service across device wakeups. Devices keep running while the main activity
awaits input or the next program.

Create the [shared timer service](clock.md) on the host, and pass its clock into
the execution worker. `ExecutionWorker` uses the existing
[worker transport](workers.md) and owns runner completion and cancellation:

```rust,ignore
let (clock, timers) = relocade_machine::timer_service();
let execution = ExecutionWorker::spawn(clock, async move |runner| {
    let device = create_device(runner.clock().clone());
    let io = IoBus::new([runner.connect(device, memory_ranges, ports)]);
    let mut cpu = create_cpu(io).await?;
    runner.run_cpu(&mut cpu).await;
    wait_for_next_program().await;
    Ok::<(), Error>(())
});
futures_util::future::join(timers.run(), execution.join()).await;
```

`connect` registers the device's ordinary async `Device::run` future and returns
its bus connection. Passive devices use the default empty activity. Device
activities retain their own handles, release mutable borrows before awaiting,
and use the same `Clock::sleep` or `wait_until` API as CPU services. Ending the
main activity drops all device futures on their owner.

The task owns one `Cpu<T>` for its lifetime. `T` is its guest environment state;
BIOS/DOS service futures execute on the same worker. The CPU selects the native
or browser engine internally and retains its initialized memory resources.
Hooks run in Rust after Wasm returns. A `CpuContext` borrows the executing machine;
its CPU views bind storage directly and keep that backend borrow until they end.
Services can hold ordinary views and local references across input awaits.
Shared guest-memory handles support independent readers.

```text
host input and timer tasks --Rust channels--> execution activity + devices
```

For a blocking console read, the CPU activity awaits input after the guest
returns. A local input activity receives bytes through a Rust channel and writes
them through the existing stdin handle. Its queue is independent of the borrowed
CPU context. Browser input and I/O callbacks stay on a responsive host and send
results through shared Rust channels. Local JavaScript message handlers are not
a control path for a continuously running CPU.

The embedding supplies host input and controls when execution stops.
The runtime owns execution and service completion; the web and
Wasmtime backends supply Wasm engine access. Generated guest calls remain
synchronous; the execution task awaits services after those calls return.

## Task lifetime

The execution task has a lifetime independent of any individual UI request.
Dropping a UI waiter must not implicitly cancel the guest or its pending service.
`ExecutionWorker` wraps the runner in `Abortable`; the runner owns CPU/device
coordination. `stop().await` sets the cancellation flag and wakes the task even
while a CPU service is pending. It drops owned CPU and device activities on their
execution thread, and waits for completion before releasing the thread.
`join().await` waits for ordinary main-activity completion. As with `Thread`, await
these operations before dropping the handle; early browser-worker termination
does not run Rust destructors. Completed guest side effects remain visible.

The CPU holds its machine directly between runs. Reaching a host service creates
an execution future that owns the machine and call chain until execution stops.
The future then returns the machine and exit result to the CPU and is released.
Runs that finish without reaching a host service allocate no execution future.

Dropping a `Cpu::run` waiter preserves the pending future and its call chain; a later
`Cpu::run` resumes it and supplies a new budget for subsequent guest execution.
Zero-budget runs do not poll pending work.

While a host service is pending, complete it before borrowing
CPU/environment state or installing modules and hooks. Use owned input/output
handles and `cpu.memory()` for interactive I/O and display. Dropping the CPU
releases the suspended call chain and its machine together.

Create a persistent DOS session and its CPU with `Dos::new(runner).await`. This
registers VGA with the runner and shares its timer. Mount filesystems and retain
console handles before `launch_com(...)` or `launch_mz(...)`. These synchronous
launches reuse the CPU and guest memory. `dos.run(runner).await` returns the current
process exit code; keeping the main session alive afterwards leaves VGA active
for final captures and later launches. `dos.frame_source()` exposes the same
immutable capture source across those launches. Rendering and surface lifetimes
belong to the embedding and can outlive DOS.
Use `dos.process()` to retain the current program and inspect its `ProcessStatus`.
It returns `None` before the first launch; an exited process remains observable.
A launch while active returns `LaunchError::ProcessActive`, including when a
service is pending. Normal DOS termination releases process resources while the
session's mounts, directories, memory and console remain available. Launch does
not cancel a program. Guest break handling and nested EXEC are separate DOS work.

## Event-loop scheduling

`run_cpu` requests a nonzero instruction quantum (10,000 by default), then
cooperates with the main/device task set through `yield_now`. This is a Rust
polling opportunity, with no per-quantum or periodic host timer. The host timer
driver delivers deadline notifications through Rust channels even while the
execution worker stays runnable. CPU services can sleep without suspending
devices, and sleeping devices do not suspend the CPU.

Continuously ready device loops must also cooperate. Genuine CPU waits, including
Wasm compilation, can return to the underlying executor. This model does not
promise periodic processing of arbitrary JavaScript callbacks on a busy worker.
Budget checks remain at block boundaries; they provide
[no exact-stop or elapsed-time guarantee](../../x86/docs/host-integration.md#instruction-budgets).

## Video and application rendering

Register VGA through `runner.connect(vga, [TEXT_MEMORY_RANGE], PORT_RANGES)`.
The runner drives its device activity throughout the main session, including CPU
service waits and gaps between programs. VGA follows fixed text-mode raster
timing and captures immutable text on this owner. Pixel conversion runs on the
worker receiving those captures, without borrowing the live device. The
[graphics fixture](../../../tests/browser/src/graphics.rs) assembles this with
`Thread::run_send` and an application-owned request channel. VGA does not create
workers or own rendering and presentation lifetimes.
Application redraws cannot advance VGA time. See the
[VGA contract](../../vga/README.md) and [renderer API](../../renderer/README.md).
