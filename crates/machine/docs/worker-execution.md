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
`CallContext` owns a CPU access handle and retains memory and I/O resources.
Its register operations borrow the backend only while accessing storage.
`CallContext::with` remains available for synchronous batches and mutable
environment state. Retained resource views and Rust locals may cross awaits.

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

The CPU keeps its machine in one shared allocation throughout execution.
Each host invocation owns a future retained separately by the CPU and borrows
machine state for each register operation or synchronous `CallContext::with` batch.
The run loop advances calls and
guest execution without moving the machine. Runs without host services allocate
no call future.

Dropping a `Cpu::run` waiter preserves the pending future and its call chain; a later
`Cpu::run` resumes it and supplies a new budget for subsequent guest execution.
Zero-budget runs do not poll pending work.

While a run awaits a host future, use owned input/output handles and `cpu.memory()`
for interactive I/O and display. A returned ISR budget or fault exit permits
CPU/environment access while the interrupted service stays parked. Keep its
service resume labels installed. `HostExit::Return` abandons parked
services; dropping the CPU releases every future and the machine together.

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

## Device interrupts

A device or interrupt controller supplies vectors through `Device::poll_interrupt`.
Its ordinary `runner.connect(device, mmio, ports)` connection carries memory,
port and interrupt traffic through the CPU's `IoBus`. Devices with no mapped
addresses use empty ranges. No additional CPU registration is needed.
`poll_interrupt(cx)` returns `Ready(vector)` to acknowledge one request, or
registers the supplied waker and returns `Pending`. The default never raises IRQs.
The bus accepts the first ready device in connection order and leaves later
requests pending. A controller device can own more detailed priority and masking.

Each nonzero `Cpu::run` checks the bus before starting its instruction quantum.
Completing a host call also checks before continuing guest dispatch, using
the remaining budget. A pending host await checks as well. The CPU checks IF
before polling, so masked requests remain with the device/controller.
`Runner::run_cpu` supplies those
quanta without another timer or scheduler. Entry uses the same generated
real-mode mechanism as software INT, but saves the current IP and retires no
instruction. The guest handler returns normally through IRET.

STI from IF=0 and MOV/POP SS end their block with `InterruptInhibited`. The
runtime consumes this exit synchronously by calling the existing interpreter
`step`, then continues normal dispatch. There is no await, IRQ check or budget
check between the inhibitor and its successor. LSS and STI from IF=1 do not
request that step. Consecutive inhibitors use the same loop. A fault or unsupported
exit from the successor returns normally with the inhibitor already retired.

Zero-budget runs do not poll or enter Wasm. Entry faults are returned as guest
exceptions after the source has acknowledged the vector.

Protected-mode delivery, nonmaskable interrupts and interrupt-controller emulation
are separate capabilities.

## Asynchronous host calls

`Hook::new` gives every invocation a `CallContext`. Its `cpu` field uses the same
`CpuView<A>` register implementation as borrowed CPU views. Each access validates
the invocation and releases its backend borrow before returning. Its `MemoryBus`
retains storage and device routing; `call.memory.view()` borrows only that handle.
`with` lends CPU, memory and mutable environment state for synchronous batches;
references from that callback cannot escape into the future. Whenever the call future returns
`Pending`, the runtime checks for IRQs. Live IF controls delivery. An await that
is immediately ready continues synchronously within the same poll.

```rust,ignore
real_mode_interrupt::hook(address, async move |mut call| {
    let value = call.with_interrupts_enabled(input.read()).await;
    call.cpu.set_ax(value);
    real_mode_interrupt::return_from_interrupt(&mut call.cpu)
})
```

Use an ordinary `input.read().await` when IF is already set as required.
`with_interrupts_enabled` only manages the flag: it enables live IF and restores
its previous value when the wait completes or is cancelled. Other live flags and
the caller's saved FLAGS stay intact. BIOS keyboard and DOS input reads use this
helper because real-mode interrupt entry clears IF. It clears live IF again
before their return path; the guest IRET restores the caller's saved flags.
Nested or overlapping helpers use scoped guards; only the last scope restores IF.
Clearing IF inside such a scope still masks IRQ delivery.

Each service owns entry, resume and completion labels in its guest stub.
`Hook::new(entry, resume, handler)` registers the pair. The real-mode helper
`real_mode_interrupt::hook` uses `NOP; NOP; IRET`: entry starts an invocation,
resume continues it, and completion selects IRET. BIOS/DOS own these existing
service stubs; there is no separate reserved continuation address.
Generated exits distinguish `HostExit::Call { address }` (a new invocation),
`HostExit::Resume { address }` (resume that service's top suspended invocation),
and `HostExit::Return` (session termination). Both address payloads identify the
service entry. An unmatched resume returns to the embedding with frames intact.

Entry automatically moves live EIP to the service's resume label before the
first future poll. Pending handlers keep that position until completion. When
an IRQ interrupts the future, the driver parks its frame and performs ordinary
interrupt entry. The saved IRQ return CS:IP naturally names the service-local
resume label. Guest IRET restores FLAGS and reaches it through ordinary dispatch.
The driver resumes the matching top frame without restoring any CPU state.
General registers, every segment, FLAGS, SS:SP and memory retain all guest effects.
Neither stack equality nor an IRET counter decides resumption.
A new call to the same service always creates a separate invocation, even if
CS:IP and SS:SP match an older one. Invocations of the same service share its
resume label; the execution stack identifies the invocation. Frames resume in
strictly nested order.

Each invocation has a weak handle to the shared machine, with CPU access enabled
only during its future poll and completion cleanup. ISR execution and nested
calls keep outer futures parked even when their awaited inputs become ready.
ISR budget exits, waiter cancellation and later runs retain the frame stack.

A guest that redirects return away from the resume entry leaves those frames
parked. The embedding must explicitly abandon them with
`Cpu::abandon_suspended_calls` when continuing after such a nonlocal transfer;
`HostExit::Return` and CPU destruction also release all suspended work. Abandonment
drops futures and resources without restoring their saved IF into the active
frame or undoing guest effects. After abandonment, guest code must not return to
the resume entry for those frames.

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
