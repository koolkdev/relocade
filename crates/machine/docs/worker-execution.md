# Worker execution

Each execution worker owns one CPU session. `Runner::run` drives the
application, device activities and machine together.
`runner.run_cpu().await` submits a request to run the CPU and waits for its exit.
Devices remain active while the application waits for input or another program.

Create the [shared timer service](clock.md) on the host, and pass its transferable handle into
the execution worker. `ExecutionWorker` uses the existing
[worker transport](workers.md) and owns runner completion and cancellation:

Pass `Some(&compiler)` to connect a CPU to a shared [compiler service](compilation.md),
or `None` to use the bundled interpreter without background compilation.

```rust,ignore
let mut machine = Machine::new(memory_config)?;
let cpu = machine.create_cpu(cpu_config, None)?;
let (handle, timers) = relocade_machine::timer_service();
let execution = cpu.spawn(handle, async move |mut runner| {
    let device = create_device(runner.clock().clone());
    runner.connect(device, |io| {
        io.bind_mmio(memory_range, &memory_read, &memory_write);
        io.bind(ports, &read, &write);
    }).await;
    runner.run_cpu().await;
    wait_for_next_program().await;
    Ok::<(), Error>(())
});
let timers = timers.spawn_with_peer(execution.wake_peer());
let result = execution.join().await;
let timers = timers.stop().await;
machine.shutdown().await?;
timers?;
```

`connect(...).await` installs the device's mappings, releases the setup context,
then calls `Device::attach`. The PIT uses this hook to install a replaceable
alarm on its clock. Reprogramming the PIT inside a port write updates the
deadline immediately. The callback runs after guest execution returns.
Dropping the alarm or ending the session cancels it.

The default hook starts `Device::run` and waits for its first poll. Devices
use this for work such as publishing video frames. For a stream of physical
input, `Runner::on_input` delivers each item to a synchronous callback.
Initialize device state before connecting and release mutable borrows before
awaiting. Ending the main activity drops these futures on their execution owner.

`spawn_activity(future).await` starts a service without bus mappings with the
same first-poll and session lifetime guarantees. `spawn_job_thread().await` returns a
sendable job executor backed by one reusable calculation thread. Jobs own their
inputs and return results through Rust memory; awaiting one suspends only its
calling activity. The thread binds the session's wake peer once, so completion
can interrupt busy guest execution without a JavaScript message per block.
In the browser, `spawn_job_thread` registers host-turn demand while waiting
for the worker startup callback. Subsequent job results wake their waiting
Rust activity directly.

Ending or cancelling the session drops local activities, then closes job
admission, drains accepted calculations and joins their threads. Retained job
handles cannot keep a stopped session alive. Calculations must finish without
waiting for another job on the same queue.

The machine initializes CPU state before spawning. Startup consumes its
non-cloneable `CpuHandle` and transfers that CPU's storage, configuration and
shared memory together. The runner constructor creates the local backend without
resetting registers, initially with an empty device bus. Startup observes cancellation
through construction and returns failures to the worker owner without entering the
session. The callback receives a complete Runner with a usable CPU.

Each `runner.connect(device, configure).await` adds its device to the session's
existing bus. `Dos::new(runner)` connects PC hardware this way. Retained bus
handles and already installed modules immediately observe the new mappings. `run_cpu()`
executes the runner's existing CPU. Construction, connection and launch do not
execute guest instructions.

The callback receives its `Runner` by value. PC/DOS retain it across programs;
`runner.cpu()` lends the stopped backend for setup. The outer session keeps
driving device activities until the callback completes, even if it releases its
runner earlier. BIOS/DOS services capture their own
application resources and execute on the same worker. The CPU selects the native
or browser engine internally and retains its initialized memory resources.
Hooks run in Rust after Wasm returns. They capture a `CpuStateAccess` handle,
memory, I/O and application resources when registered, and receive no temporary
CPU capability. The driver owns the backend throughout execution. State operations
copy values directly; no per-poll binding or dynamic borrow guard is needed.
Services can keep ordinary resource views and local references across input awaits.

```rust,ignore
let state = cpu.cpu_state();
cpu.install(real_mode_interrupt::hook(entry, state.clone(), async move || {
    let value = input.read().await;
    state.set_ax(value);
})).await?;
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
Capture `cpu.exit_handle()` in a service to request `HostExit::Return` after
its current poll. The driver drops pending work and consumes the request before
returning to the embedding. Dropping only the run waiter preserves both the call
and any unobserved request. In a session, requesting an exit wakes the machine
even while guest execution is blocked. To end the whole session, use `stop().await`.
The runtime owns execution and retained futures; the web and
Wasmtime backends supply Wasm engine access. Generated guest calls remain
synchronous; the execution task awaits services after those calls return.

## Task lifetime

The worker can outlive individual DOS programs. The Rust console and device
activities run without driving guest instructions. `Dos::run` requires a launched
process and returns an already completed process's result without running the CPU.
DOS termination records that result and requests an execution exit before the
driver resumes guest code. The environment remains alive for another launch.

For a session tied to a host lifetime, `cpu.run_until(stop, session)` consumes the CPU
handle, creates the timer service and drives it on a timer worker while the
session runs on execution. It returns the session result on completion, or `None` after the stop
signal and worker cleanup. The stop signal's value is ignored. Await the whole
operation; dropping it early is not orderly shutdown. The timer service ends with
the session. Use `spawn` with an externally driven clock when clock handles must
remain usable afterward or the embedding needs separate control.

The execution task has a lifetime independent of any individual UI request.
Dropping a UI waiter must not implicitly cancel the guest or its pending service.
`ExecutionWorker` owns a stop channel. `worker::Executor` polls its receiver before
dispatching runnable work. `stop().await` sends through that channel. Dropping
the sender also requests cancellation. Either wakes the session while a CPU service is pending
or requests a return from busy guest execution. The executor drops the application
and devices on their execution thread before reporting completion. `stop().await`
waits for that completion before releasing the thread.
`join().await` waits for ordinary main-activity completion. As with `Thread`, await
these operations before dropping the handle; early browser-worker termination
does not run Rust destructors. Completed guest side effects remain visible.

The CPU owns its machine directly throughout execution. Reaching a host service
constructs and polls its pinned future. A pending call retains the future and its
registered handler owner; a ready call finishes in the same poll. The driver
resumes guest dispatch or returns the requested exit after completion.

Dropping a `Cpu::run` waiter preserves active and parked calls; a later run
continues the active call or guest ISR. An IRQ-suspended outer call stays parked
until guest execution reaches a service resume entry, even when its input becomes
ready. An unpolled run performs no CPU work.

Guest HLT is a wait within active execution. It retires with IP pointing after
HLT, and `Cpu::run` stays pending until a device IRQ is accepted with live IF set.
The CPU retains that halt across replacement run waiters. Ordinary Rust wakes
and execution-interrupt requests let host work run but do not resume guest code.
`STI; HLT` executes HLT as STI's protected successor before accepting a queued IRQ;
the interrupt return frame therefore names the instruction after HLT.

Device activities and worker cancellation remain runnable while the guest is
halted. An outer host service stays parked if its ISR halts, even when that
service's input becomes ready. Terminal `HostExit::Return` and `UnhandledException`
clear the halt and release retained calls, allowing another program to use the CPU.
Stopping a worker drops the CPU and its calls through the existing cancellation path.
HLT with IF clear waits indefinitely unless the embedding ends execution;
NMI, reset and debug wake events are not modeled yet.

An idle DOS environment does not execute HLT: it has no active CPU run. Device
activities can queue IRQs, but guest delivery starts only when execution is
explicitly driven. Register edits and `CpuState` snapshot restoration do not
change the driver-owned halt or retained host calls. Public `Cpu::step` reports
`Halted` and remains halted on later steps; `run` performs IRQ acceptance.

While a host call is actively pending, complete it before using `cpu.context()`.
The context is available at stopped guest boundaries with outer calls parked.
Installing modules and hooks requires all retained calls to have completed or
been abandoned. Retained `cpu.cpu_state()`, memory and input/output handles remain
usable. Dropping the CPU releases active and parked calls innermost first, then
the backend; independently retained resources can outlive it.

Create a persistent DOS session and its CPU with `Dos::new(runner).await`. This
registers VGA with the runner and shares its timer. Mount filesystems and retain
console handles before `launch_com(...)` or `launch_mz(...)`. These synchronous
launches reuse the CPU and guest memory. `dos.run().await` returns the current
process exit code; keeping the main session alive afterwards leaves VGA active
for final captures and later launches. `dos.frames()` exposes the same
capacity-one snapshot queue across those launches. One graphics consumer receives
the snapshots; cloned receivers compete for them. Rendering and surface lifetimes
belong to the embedding and can outlive DOS.
Use `dos.process()` to retain the current program and inspect its `ProcessStatus`.
It returns `None` before the first launch; an exited process remains observable.
A launch while active returns `LaunchError::ProcessActive`, including when a
service is pending. Normal DOS termination releases process resources while the
session's mounts, directories, memory and console remain available. Launch does
not cancel a program. Guest break handling and nested EXEC are separate DOS work.

## Event-loop scheduling

The worker executor runs one session future and checks cancellation before
each poll. When input is ready, the session takes at most one item per input
source in registration order. It dispatches due hardware alarms next, followed by
ready Rust activities (including compiler receipt) and the application. A key
release at the head of the keyboard stream can cancel a typematic repeat due
at the same instant, even if the timer woke the worker first. If the application
is still waiting, the machine gets one turn. A wake during guest execution requests a return to Rust at a
completed checkpoint, independently of guest IF. A long block or `REP` can
delay that return.
[Execution checkpoints](../../x86/docs/host-integration.md#execution-interrupts)

With a foreground run request, the worker services the active Rust hook and
checks pacing before asking the CPU to run to a boundary. The CPU decides when
it can accept an IRQ, leave HLT, resume a nested guest call or install a compiler
reply. It reports a checkpoint, a blocked state or an exit. After a guest-time
checkpoint the worker publishes retirement and wakes
due sleepers. When guest execution is blocked or between programs, the clock
advances to the next device deadline only after Rust work stops requesting polls.
A device woken by a host service can cancel its timer at the current guest time.
Once the worker is quiet, a PIT deadline can raise IRQ0 and resume a halted CPU
with interrupts enabled. An activity that continually wakes itself keeps an idle
guest clock stationary.
[Guest clock and pacing](clock.md#execution-with-guest-time)

The execution worker can process successive turns entirely inside Wasm.
It parks when all work is waiting. An operation that needs its JavaScript event
loop registers that demand, for example while compiling a local Wasm module or
receiving a posted module. A session activity can wrap its own browser promise
future in `runner.host_turns().wait(future).await`. If Rust work stays runnable
while a callback is pending, the worker checks elapsed host time between turns
and yields to the browser after 1 ms. Guest instructions return at completed
checkpoints during that interval.
[Compiler reply delivery](compilation.md#compiler-replies)

`ExecutionWorker` creates a wake connection before spawning and binds it to
the transferred CPU's execution interrupt on the new worker. The host keeps
the connection alive through worker shutdown.

The browser connection initially covers the spawning host and execution worker.
Capture `execution.wake_peer()` on the host, or `runner.wake_peer()` on execution,
and post it when setting up another worker:

```rust,ignore
use relocade_machine::{worker::{self, Receiver, WakePeer, Thread}, ExecutionInterrupt};

let send_updates = move |mut peers: Receiver<WakePeer<ExecutionInterrupt>>| async move {
    let peer = peers.recv().await.unwrap()?;
    let _binding = peer.bind();
    send_device_updates(input).await;
    Ok::<_, worker::ChannelError>(())
};
let thread = Thread::new();
let (mut peers, task) = thread.listen(send_updates)?;
peers.send(execution.wake_peer())?;
task.await??;
```

Ordinary Rust senders on that thread now signal busy execution through its local
resource. Keep the binding until those sends finish. Multiple bindings share the
same local registration; dropping a peer does not terminate execution. Ending
execution or dropping its host disables all peers, including escaped wakers.
Callback threads without a posted binding, including CPAL's AudioWorklet, notify
a coalesced Rust-atomic relay on the responsive spawning host. Its local task
requests the same execution interrupt. The callback invokes no JavaScript and
needs no engine resource transfer; the host uses its existing binding. Both
original owners close and wake the relay, and escaped wakers retain no engine
resources. The host event loop must remain responsive. This fallback adds host
scheduling latency; it is not a real-time deadline. Explicit peer bindings still
signal synchronously and avoid that extra turn.

PC audio runs as a session activity. It waits for block deadlines, collects
recorded device events and submits each PCM block to a reusable session-owned job
thread. The job synthesizes and mixes owned events while guest execution continues.
One calculation is in flight; completion returns synthesis state for the next
block. Full output replaces old PCM, and closing output drops recording handles.
Accepted work finishes before the runner joins the thread. Playback callbacks
consume bounded output without driving guest time.
Use `cpu.step()` for bounded instruction tests; service/dispatch tests can request
an execution interrupt to inspect a completed guest boundary while retaining
suspended work. See the [connection contract](workers.md#browser-build-and-lifetime).

## Video and application rendering

Register VGA with one scoped connection:

```rust
runner.connect(vga.clone(), |io| DispiAdapter::configure_io(io, &vga)).await;
```

Map that complete A0000–BFFFF range as MMIO; VGA registers select which aperture
responds to each CPU access.
The runner drives its device activity throughout the main session, including CPU
service waits and gaps between programs. VGA follows fixed text-mode raster
timing and captures immutable text on this owner. Pixel conversion runs on the
worker receiving those captures, without borrowing the live device. The
[graphics fixture](../../../tests/browser/src/graphics.rs) assembles this with
`Thread::listen` and an application-owned request channel. VGA does not create
workers or own rendering and presentation lifetimes.
Application redraws cannot advance VGA time. See the
[VGA contract](../../vga/README.md) and [renderer API](../../renderer/README.md).
