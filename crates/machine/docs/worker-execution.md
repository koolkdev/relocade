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

The task owns one `Cpu` for its lifetime. BIOS/DOS services capture their own
application resources and execute on the same worker. The CPU selects the native
or browser engine internally and retains its initialized memory resources.
Hooks run in Rust after Wasm returns. They capture a `CpuStateAccess` handle,
memory, I/O and application resources when registered, and receive no temporary
CPU capability. The driver owns the backend throughout execution. State operations
copy values directly; no per-poll binding or dynamic borrow guard is needed.
Services can keep ordinary resource views and local references across input awaits.

```rust,ignore
let state = cpu.cpu_state();
cpu.install_hooks([real_mode_interrupt::hook(entry, state.clone(), async move || {
    let value = input.read().await;
    state.set_ax(value);
})]).await?;
```

The driver first polls the concrete pinned future. Only Pending erases its type
and retains another handler owner. Dropping a run waiter preserves that future;
dropping the CPU abandons it. CPU restoration belongs after normal completion,
not in destructors that also run when a parked invocation is abandoned. Destructors
release ordinary resources. Captured CPU handles are not revoked on abandonment.

```text
host input and timer tasks --Rust channels--> execution activity + devices
```

For a blocking console read, the CPU activity awaits input after the guest
returns. A local input activity receives bytes through a Rust channel and writes
them through the existing stdin handle. Its queue is independent of CPU state access. Browser input and I/O callbacks
stay on a responsive host and send
results through shared Rust channels. Local JavaScript message handlers are not
a control path for a continuously running CPU.

The embedding supplies host input and controls when execution stops.
Services return `()`; their adapters apply guest calling conventions.
Capture `cpu.stop_handle()` to request `HostExit::Return` at the next driver
boundary. It wakes a pending run and applies only while that run is active.
Returning from or dropping the run clears its stop request and waker. An
observed stop drops the pending call; dropping only the waiter preserves it.
The runtime owns execution and retained futures; the web and
Wasmtime backends supply Wasm engine access. Generated guest calls remain
synchronous; the execution task awaits services after those calls return.

## Task lifetime

For a session tied to a host lifetime, `ExecutionWorker::run_until(stop, session)`
creates the timer service and drives it on the caller while the session runs on
execution. It returns the session result on completion, or `None` after the stop
signal and worker cleanup. The stop signal's value is ignored. Await the whole
operation; dropping it early is not orderly shutdown. The timer service ends with
the session. Use `spawn` with an externally driven clock when clock handles must
remain usable afterward or the embedding needs separate control.

The execution task has a lifetime independent of any individual UI request.
Dropping a UI waiter must not implicitly cancel the guest or its pending service.
`ExecutionWorker` wraps the runner in `Abortable`; the runner owns CPU/device
coordination. `stop().await` sets the cancellation flag and wakes the task even
while a CPU service is pending. It drops owned CPU and device activities on their
execution thread, and waits for completion before releasing the thread.
`join().await` waits for ordinary main-activity completion. As with `Thread`, await
these operations before dropping the handle; early browser-worker termination
does not run Rust destructors. Completed guest side effects remain visible.

The CPU owns its machine directly throughout execution. Reaching a host service
constructs and polls its pinned future. A pending call retains the future and its
registered handler owner; a ready call finishes in the same poll. The driver
resumes guest dispatch or returns the requested exit after completion.

Dropping a `Cpu::run` waiter preserves the pending future and its call chain; a later
`Cpu::run` resumes it and supplies a new budget for subsequent guest execution.
Zero-budget runs do not poll pending work.

While a host service is pending, complete it before using `cpu.context()` or
installing modules and hooks. Retained `cpu.cpu_state()`, memory and input/output
handles remain usable. Dropping the CPU releases the pending call and backend;
independently retained resources can outlive it.

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

Register VGA through `runner.connect(vga, [MEMORY_RANGE], PORT_RANGES)`.
Map that complete A0000–BFFFF range as MMIO; VGA registers select which aperture
responds to each CPU access.
The runner drives its device activity throughout the main session, including CPU
service waits and gaps between programs. VGA follows fixed text-mode raster
timing and captures immutable text on this owner. Pixel conversion runs on the
worker receiving those captures, without borrowing the live device. The
[graphics fixture](../../../tests/browser/src/graphics.rs) assembles this with
`Thread::run_send` and an application-owned request channel. VGA does not create
workers or own rendering and presentation lifetimes.
Application redraws cannot advance VGA time. See the
[VGA contract](../../vga/README.md) and [renderer API](../../renderer/README.md).
