# Worker execution

Each execution worker runs one CPU session and its registered device activities.
`Runner::run` drives them together until the main session finishes or is cancelled.
`run_cpu` yields on execution interrupts and retains pending
CPU services across device wakeups. Devices keep running while the main activity
awaits input or the next program.

Create the [shared timer service](clock.md) on the host, and pass its transferable handle into
the execution worker. `ExecutionWorker` uses the existing
[worker transport](workers.md) and owns runner completion and cancellation:

```rust,ignore
let mut machine = Machine::new(memory_config)?;
let cpu = machine.create_cpu(cpu_config)?;
let (handle, timers) = relocade_machine::timer_service();
let execution = cpu.spawn(handle, async move |mut runner| {
    let device = create_device(runner.clock().clone());
    let io = IoBus::new([runner.connect(device, memory_ranges, ports).await]);
    runner.attach_io(io);
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

`connect(...).await` registers the device's ordinary async `Device::run` future
and waits until its first poll before returning the bus connection. Initialize
device state before connecting; this wait establishes activity readiness, not
arbitrary asynchronous construction. Passive devices use the default empty activity. Device
activities retain their own handles, release mutable borrows before awaiting,
and use the same `Clock::sleep` or `sleep_until` API as CPU services. Ending the
main activity drops all device futures on their owner.

The machine initializes CPU state before spawning. Startup consumes its
non-cloneable `CpuHandle` and transfers that CPU's storage, configuration and
shared memory together. The runner constructor creates the local backend without
resetting registers, initially with an empty device bus. Startup observes cancellation
through construction and returns failures to the worker owner without entering the
session. The callback receives a complete Runner with a usable CPU.

`runner.attach_io(io)` synchronously installs the device bus; `Dos::new(runner)`
uses this operation after connecting the PC's devices. `Cpu::attach_io` also serves
CPU-level callers. Replacing a bus requires a stopped CPU with no pending or parked
host calls. Existing bus handles retain their original routing. `run_cpu()` executes
the runner's existing CPU. Construction, attachment and launch do not execute guest
instructions.

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
and any unobserved request. The exit flag does not wake suspended execution;
session cancellation uses the worker's existing `stop().await`.
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
the application/device future, so cancellation is registered before any guest
execution. `stop().await` sends through that channel; dropping the sender also
requests cancellation. Either wakes the session while a CPU service is pending
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

`run_cpu` cooperates with the main/device task set when a wake requests an
execution interrupt. Host-timed runners leave the instruction target unarmed.
The same runner loop handles guest time. Runner delegates each CPU turn to
`ExecutionSchedule::poll_cpu` and gives ready activities a turn afterward. The
scheduler polls host work before waiting for pacing, then prepares instruction
targets and drives guest execution. Pending services, suspended calls, guest entry
and terminal-exit cleanup remain owned by the CPU. The schedule also owns clock
selection, execution slices and idle decisions; the `GuestTimeDriver` owns
registered sleeps and instruction-target updates. Time is published before
polling activities, including after a terminal CPU exit.
A terminal result returns immediately when no execution notification is pending;
otherwise ready activities receive a turn before the result returns.

Guest-time workers receive a configuration factory, following the same owner-local
construction rule as `spawn_with_clock`. `GuestTimeConfig` selects the instruction
rate, optional slice duration and optional host pacing. `with_slice_duration`
sets the guest-time boundary shared by cooperative yields and pacing.
`with_host_clock(clock)` defaults to 4 ms slices if no duration was supplied.
Device deadlines subdivide each slice. The embedding retains and drives the host
timer service. Scheduling notifications preserve the
allowance, and ready host services remain pollable during a pacing wait. Run
cancellation drops that wait while preserving guest progress and remaining credit;
time between runs is excluded from the host/guest correspondence.

Lower-level embeddings capture `cpu.instruction_target()` before execution and use
`target.set(Some(absolute_count))` or `target.set(None)`. Updates take effect at
existing checkpoints, including when made from synchronous device callbacks.
`Cpu::run()` reports `BudgetExhausted` without changing the target; the controller
owns rearming and cleanup. Targets use signed wrapping distance, and zero is
rounded to one because the backing reserves zero for unarmed execution. Completed
instruction counts include checkpoint overshoot.
Yielding gives ready Rust activities a polling opportunity. The host timer
driver delivers deadline notifications through Rust channels even while the
execution worker stays runnable. CPU services can sleep without suspending
devices, and sleeping devices do not suspend the CPU.

`ExecutionWorker` connects the prepared CPU's `ExecutionInterrupt` handle to
the whole session's Rust wakers, including cancellation. Host access exists before
spawning. The worker captures a Send connection token alongside that same CPU
transfer, then binds its locally reconstructed interrupt handle. Binding needs no
registration channel, return acknowledgment or target replacement.
`runner.connect(...).await` starts the device activity with its own waker before
returning its bus connection. Initialize device state before connecting, including
any asynchronous construction. A new `run_cpu` future yields once so siblings in
CPU-first async composition can register their waits; it performs no device setup.
The host guard stays alive until worker completion.

`WakeConnection` owns only the fixed signal binding. `worker::Executor` owns the
stable executor waker, polling and parking. `Runner` owns that executor and composes
the application and registered devices under it. On the execution owner:

```rust,ignore
let runner = Runner::new(clock, cpu, endpoint).await?;
runner.run(stopped, session).await
```

The constructor consumes the prepared CPU and binds its wake endpoint on the
execution owner and returns after backend construction. The worker observes its stop
signal during that await, then passes it to `run` for session cancellation. Worker
and backend failures use the machine's `Error` type. `run` installs the required
executor around the whole session, so callers do
not separately wrap it. Native fixtures can supply the same resources on their
test thread; they use the same executor. Browser binding belongs on the receiving
worker after resource transfer. Direct CPU/device tests do not need a runner.

Each executor turn polls `stopped` first, then the application/device future. A
ready stop drops that future without polling it again. Runnable work is repolled
directly in Rust; only an idle session registers an outer-executor waiter and
parks. Resumption or cancellation releases that waiter. Cancellation needs no
startup yield or wrapper around the application.

Participating work belongs inside the application/device future. The runner
polls registered devices before the main activity. Device setup explicitly awaits
connection; its activity has started before it can be used for guest execution.
Neither preparation nor a device wake starts CPU execution; only an explicit CPU
run future drives it.
VGA and timers can run while the application waits for a program or after it
finishes. Guest IRQ delivery still requires an active CPU run and eligible live
x86 interrupt state.

An ordinary host channel send or timer completion makes its waiter ready, then
its waker requests an execution interrupt and marks the session ready on the
sending thread. It wakes the outer executor only when the session is parked.
Guest checkpoints return independently of IF. The interrupt remains set
through the whole session poll, so CPU-first composition such as
`join(run_cpu, input)` yields before its ready sibling runs. Acknowledgement
follows polling and preserves new wakes, including those racing with the exchange.
The runner resumes guest execution after polling that work. The ready queues
retain coalesced notifications. Requests already pending at guest reentry use
the same block/STEP checkpoints as requests arriving during execution. After
`Interrupted`, the runner yields to Rust work before resuming the CPU. There is
no additional pre-entry check or yield for an already-pending notification.
The runner's cooperative yield wakes its current
child task without requesting another interrupt from its first root notification.
Separate notifications still request an interrupt, including reentrant input
delivered while waking a parked executor.
Pending CPU services keep their original future and IRQ-suspended outer services
remain parked until guest resumption.

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

Audio synthesis remains a device activity on execution. When CPAL consumes a
buffer, the bounded channel wakes the pending sample submission. The ordinary
executor waker then interrupts busy guest execution directly on native targets,
or through the host relay from an unbound browser AudioWorklet. Neither audio nor
the relay needs a refill timer or polling loop. Use `cpu.step()` for bounded
instruction tests; service/dispatch tests can request an explicit execution interrupt
to inspect a completed guest boundary while retaining suspended work. See the
[connection contract](workers.md#browser-build-and-lifetime).

Continuously ready device loops must also cooperate with the other root futures.
When the whole session is waiting, including for Wasm compilation, the executor
parks through the underlying executor. A Rust cooperative yield does not promise
a browser event-loop turn. Compile and install modules before guest execution
or while the CPU is exclusively stopped. Concurrent compiler work on the same
busy worker needs a separate scheduling design; ordinary JavaScript callbacks
receive no periodic fairness turn. Execution checkpoints remain at completed
blocks, with [no fixed notification-latency guarantee](../../x86/docs/host-integration.md#execution-interrupts)
for long straight-line code or REP.

## Video and application rendering

Register VGA through `runner.connect(vga, [MEMORY_RANGE], PORT_RANGES).await`.
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
