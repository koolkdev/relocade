# Worker execution

Each execution worker owns one CPU session. Its scheduler drives the
application, session tasks and CPU. A session task is a Rust future polled on
the execution worker, such as the PIT timer loop or the compiler reply receiver. The DOS console
can wait for input between programs while devices continue running on guest time.
`runner.run_cpu().await` asks the scheduler to execute guest instructions
and returns when the foreground run exits.

The PC runtime passes a guest clock to its devices. To configure an execution
worker directly, set the guest clock rate with `GuestTimeConfig`. An optional
CPU limit sets how many instructions may retire per host second.
[Clock and CPU limit](clock.md#execution-with-guest-time)

```rust,ignore
let mut machine = Machine::new(memory_config)?;
let cpu = machine.create_cpu(cpu_config, None)?;
let execution = cpu.spawn_with_guest_time(
    move || GuestTimeConfig::new(guest_rate),
    async move |mut runner| {
        let device = create_device(runner.clock().clone());
        runner.connect(device, |io| {
            io.bind_mmio(memory_range, &memory_read, &memory_write);
            io.bind(ports, &read, &write);
        }).await;
        runner.run_cpu().await;
        wait_for_next_program().await;
        Ok::<(), Error>(())
    },
);
let result = execution.join().await;
machine.shutdown().await?;
```

Pass `Some(&compiler)` to `machine.create_cpu` to enable background compilation.
[Compiler service](compilation.md)

Use `runner.io().connect(device, configure)` for a device that only handles
synchronous bus accesses. For a device with background work, `runner.connect(...).await`
installs its mappings and starts `Device::run`. When it returns, the task
has had its first poll and can already receive input or timer notifications.
PIT waits for its next OUT0 transition or a programming change. The keyboard
and mouse each await input and their own device deadlines. Each device decides
the order of events that are ready together.

`runner.spawn_activity(future).await` returns after the scheduler first polls the
future. A pending task can then sleep until its waker signals. Once queued, it
belongs to the session even if its caller drops the startup wait. Returning from the application ends
the session and drops any unfinished tasks.

`spawn_job_thread().await` returns a sendable job executor backed by one
reusable calculation thread. Jobs own their inputs and return results through
Rust memory. Awaiting one suspends only its calling task. The thread binds the session's wake peer once, so completion
can interrupt busy guest execution without a JavaScript message per block.
In the browser, `spawn_job_thread` registers host-turn demand while waiting
for the worker startup callback. Subsequent job results wake their waiting
Rust task directly.

Ending or cancelling the session drops local tasks, then closes job
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
driving device tasks until the callback completes, even if it releases its
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
host input and timers --Rust channels--> tasks on the execution worker
```

For a blocking guest console read, the CPU retains the pending DOS service
while the scheduler runs other work. An input task can receive bytes through
a Rust channel and write
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
tasks run without driving guest instructions. `Dos::run` requires a launched
process and returns an already completed process's result without running the CPU.
DOS termination records that result and requests an execution exit before the
driver resumes guest code. The environment remains alive for another launch.

For a guest-timed session tied to a host lifetime,
`cpu.run_until_with_guest_time(make_time, stop, session)` consumes the CPU
handle and runs the host timer service for the session. The factory receives
the host clock on the execution worker. It returns the session result on completion, or `None` after the stop
signal and worker cleanup. The stop signal's value is ignored. Await the whole
operation; dropping it early is not orderly shutdown. The timer service ends with
the session. Use `spawn` with an externally driven clock when clock handles must
remain usable afterward or the embedding needs separate control.

The execution task has a lifetime independent of any individual UI request.
Dropping a UI waiter must not implicitly cancel the guest or its pending service.
`ExecutionWorker` owns a stop channel. The scheduler polls its receiver before
dispatching runnable work. `stop().await` sends through that channel. Dropping
the sender also requests cancellation. Either wakes the session while a CPU service is pending
or requests a return from busy guest execution. The scheduler drops the application
and devices on their execution thread before reporting completion. `stop().await`
waits for that completion before releasing the thread.
`join().await` waits for ordinary application completion. As with `Thread`, await
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

Device tasks and worker cancellation remain runnable while the guest is
halted. An outer host service stays parked if its ISR halts, even when that
service's input becomes ready. Terminal `HostExit::Return` and `UnhandledException`
clear the halt and release retained calls, allowing another program to use the CPU.
Stopping a worker drops the CPU and its calls through the existing cancellation path.
HLT with IF clear waits indefinitely unless the embedding ends execution;
NMI, reset and debug wake events are not modeled yet.

An idle DOS environment does not execute HLT: it has no active CPU run. Device
tasks can queue IRQs, but guest delivery starts only when execution is
explicitly driven. Register edits and `CpuState` snapshot restoration do not
change the driver-owned halt or retained host calls. Public `Cpu::step` reports
`Halted` and remains halted on later steps; `run` performs IRQ acceptance.

While a host call is actively pending, complete it before using `cpu.context()`.
The context is available at stopped guest boundaries with outer calls parked.
Installing modules and hooks requires all retained calls to have completed or
been abandoned. Retained `cpu.cpu_state()`, memory and input/output handles remain
usable. Dropping the CPU releases active and parked calls innermost first, then
the backend; independently retained resources can outlive it.

The DOS launcher passes `relocade_pc::configuration::guest_time` its host clock
and optional MIPS limit. Inside the session, `Dos::new(runner).await` connects
the PC hardware to that runner and its guest clock. Mount filesystems and retain
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

Each worker turn checks cancellation, polls ready Rust tasks and the
application, then considers guest execution. A device, a completed compiler
reply or a foreground CPU request can wake the worker. A wake during busy
guest execution requests a return to Rust at a completed checkpoint.
The interpreter and compiled code both honor that request regardless of guest
IF. A long block or `REP` can delay the checkpoint.
[Execution checkpoints](../../x86/docs/host-integration.md#execution-interrupts)

With a foreground run, the scheduler polls the CPU's active Rust service and
checks its execution allowance before dispatching guest code. The CPU handles
guest IRQ delivery, HLT and resuming nested guest calls. It reports a checkpoint,
a blocked state or an exit. A checkpoint lets the scheduler publish retired
instructions and wake due timers. When a service is blocked, the CPU is halted
or the application is between programs, guest time can advance to the next
device deadline once ready Rust work has had its turn. Advancing an idle clock
retires zero instructions. A PIT expiry can then raise IRQ0 and wake a halted
CPU whose interrupt flag is set. [Guest time and CPU pacing](clock.md#execution-with-guest-time)

When the worker has no runnable work, it parks immediately. A busy worker can
execute successive turns inside Wasm. An operation that needs the JavaScript
event loop registers demand through `runner.host_turns().wait(future).await`.
Examples include receiving a posted compiler module and awaiting local Wasm
compilation. While that demand is pending, the scheduler checks elapsed host
time at CPU checkpoints and returns to the browser after 1 ms of runnable
work. [Compiler reply delivery](compilation.md#compiler-replies)

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

The PC audio task runs on the execution worker. It waits for block deadlines, collects
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
The runner drives its device task throughout the main session, including CPU
service waits and gaps between programs. VGA follows fixed text-mode raster
timing and captures immutable text on this owner. Pixel conversion runs on the
worker receiving those captures, without borrowing the live device. The
[graphics fixture](../../../tests/browser/src/graphics.rs) assembles this with
`Thread::listen` and an application-owned request channel. VGA does not create
workers or own rendering and presentation lifetimes.
Application redraws cannot advance VGA time. See the
[VGA contract](../../vga/README.md) and [renderer API](../../renderer/README.md).
