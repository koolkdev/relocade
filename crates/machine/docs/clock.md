# Shared time and waits

`relocade-time` owns `Timestamp`, `TickRate` and `Schedule`. These are ordinary
values with no live clock, thread affinity or host/guest synchronization policy.
`Clock::tick_rate()` copies the immutable scale for calculations.

`Clock` is a clonable, execution-local value that owns a timeline and creates
owned deadline waits. Devices and services use the same concrete API:

```rust,ignore
let now: Timestamp = clock.now();
let deadline = clock.deadline_after(now, Duration::from_millis(2));
let elapsed = clock.duration_between(earlier, now);
if clock.now() >= deadline {
    // Update the device at its next observable boundary.
}
clock.sleep_until(deadline).await;
clock.sync_to_host(now).await;
```

`Timestamp` is an opaque, wrapping `u64` tick coordinate. It carries no origin,
rate or source identity. Compare readings only within the same clock and its
clones, at most `i64::MAX` ticks apart, and use that clock for arithmetic and waits.
Host and guest timelines are independent.
The clock owns its fixed scale; the host timer source owns its host origin.
`Instant`, re-exported from `web_time`, is used only for external host events.

`deadline_after` and `deadline_before` round toward the future to a native tick;
their checked forms reject durations beyond half the counter range. `now()`
returns native progress without any duration conversion. `duration_between(start,
end)` measures a single interval in whole nanoseconds, returning zero for reversed
readings; its checked form returns `None` instead.

The PIT advances its counters when a port access reads or reprograms them and
when its guest deadline expires. Its task waits for the next OUT0 transition
or a programming change, then computes the next deadline from the counter state.

PIT and VGA multiply elapsed native ticks by their oscillator rate, divide by the
native tick rate, and retain the division remainder in device state. They convert
back only when scheduling a wake. Device snapshots describe the observed native
timestamp and the device's own phase; there is no shared fractional position or
elapsed-consumption cursor.

Repeating duration-based timers use `Schedule`, which retains the requested phase
and exposes only integer deadlines:

```rust,ignore
let mut next = clock.schedule(clock.now());
loop {
    next.advance(period);
    clock.sleep_until(next.deadline()).await;
    device.tick();
}
```

A period of 2.5 ticks produces deadlines 3, 5, 8, 10. Fractional phase belongs to
`Schedule`; reads and armed deadlines remain integer ticks.

Audio uses absolute frame ranges and the frontend's immutable `AudioTiming`.
Frame zero starts at a native timestamp, with a fixed native tick rate and output
sample rate. `deadline_for_frame(end_frame)` rounds the block wake once; sources
place changes within the exact frame grid and retain changes beyond the block end.
A frame need not span a whole native tick. PIT waveform phase and DAC playback
periods stay with their respective sources; FM runs at the output sample rate.

A relative sleep captures the full duration from the native reading. For example,
at three billion instructions per second, a one-nanosecond sleep always requires
three further instructions, even when it starts between nanosecond boundaries.
Idle advancement reaches its exact native deadline. Existing timestamps remain
valid across pauses, rebinding, idle advancement and CPU counter wrap.

`Sleep` owns its source without retaining a device borrow. Relative sleeps capture
their deadline when constructed; both forms register on first poll. Dropping a
polled sleep cancels it. An expired sleep may complete immediately: awaiting a
clock does not promise an executor yield or a browser event-loop turn.

`ClockSource` supplies `now() -> Timestamp` and `sleep_until(Timestamp) -> Sleep`.
Its immutable, nonzero `ticks_per_second()` defaults to one billion. Sources construct
coordinates with `Timestamp::from_ticks` and interpret them with `ticks`. Devices
use native differences only with the matching `TickRate`. `Clock::new(source)` captures the scale and wraps the source
behind the common consumer API. `Sleep::new(future)` owns its wait state.

Sources preserve monotonic readings and first-poll registration/drop cancellation.
Host sources progress independently of execution. Guest sources advance with
published CPU retirement or controlled idle advancement. Output can await
`sync_to_host(timestamp)` without moving guest time or changing device deadlines.

`Runner`, `Pc`, `Dos`, and devices accept `Clock` without an implementation type
parameter. Controllers expose the separate capabilities to drive timers or
advance time. `CpuHandle::spawn_with_clock` constructs a clock on the
execution owner from a transferable factory.

## Controlled time in tests

`ManualClock` lives in `tests/clock-support`, a development-only dependency of
the consuming test suites. It is not exported by `relocade-machine` or included
in application builds. Its `clock()` uses the same public `ClockSource` interface.

`advance_by(duration)` and `advance_to(timestamp)` wake due sleeps;
`next_deadline()` observes the earliest polled, uncancelled sleep. Advancing to
an earlier timestamp leaves time unchanged. Cloned controllers share time and can
move between workers; consumer clocks remain local. Each controller starts at zero on its own timeline. `at(duration)` constructs a
fixture deadline.

## Emulated guest time

The machine scheduler owns guest time for the session. Each device receives a
`Clock` for reading the current tick and waiting for a deadline. The scheduler's
`GuestTimeDriver` records those waits, arms the CPU's instruction target and
wakes sleepers when deadlines expire. The scheduler decides when to run ready
work or advance idle time.

The guest timeline starts at zero. One retired instruction advances it by one
tick. While bound, `now()` reads the published instruction count plus an offset.
Binding, pausing and idle advancement update that offset. Pending deadlines
stay in guest coordinates. Subtracting the offset gives their CPU targets.

A lower-level embedding can drive guest time directly.

```rust,ignore
let driver = GuestTimeDriver::new(instructions_per_second);
let guest_clock = driver.clock();
// Connect devices using guest_clock, then construct the CPU.
let binding = driver.bind(&cpu, &execution_waker);
driver.synchronize(None); // No additional yield deadline.
// Run the CPU. At each stopped boundary:
driver.synchronize(None);
// Poll awakened tasks before continuing or deciding that execution is idle.
// Only when idle, driver.advance_to_next_deadline() services the next guest deadline.
drop(binding);
```

`bind` retains CPU progress and target handles without borrowing the CPU. The
returned guard scopes target ownership and wait-change notifications. Dropping
it records final retirement, clears the clock's target and releases the execution waker.
The clock pauses between bindings, and pending device waits remain registered.
Each binding starts from the CPU's current count without charging earlier work.

`synchronize(yield_deadline)` observes retirement, completes due sleeps and arms
the next CPU target. `yield_deadline` is an optional absolute guest time at
which the embedding wants execution to yield. The driver does not choose yield
intervals. Registration and cancellation update the target synchronously,
including during IO/MMIO callbacks, without extending the supplied yield
deadline. With neither device waits nor a yield deadline, the clock releases its target.
Changed waits wake the embedding so it can reconsider idle scheduling while a
CPU service is pending.

`advance_to_next_deadline()` advances guest time to the earliest registered sleep
and completes all due sleeps. The embedding calls it only after polling ready
tasks and finding execution still idle. It returns false if there is no
registered sleep. The yield deadline does not cause this advance. Completing
sleeps wakes device tasks; those tasks deliver IRQs through the existing
interrupt path. Advancing guest time does not add retired CPU instructions.

Reads preserve fractional nanoseconds and counter wrap. Synchronous callbacks
can observe a count that lags execution; targets use that same published count
and retain soft checkpoint overshoot. Reading time does not dispatch device work.
`CpuStateAccess::restore` preserves cumulative retirement when restoring guest
registers, including across program loads. Initial counts belong in `CpuConfig`.

Lower-level embeddings can set `cpu.instruction_target()` directly on CPUs with
instruction budgets enabled. `set(Some(count))` arms an absolute retirement
target, and `set(None)` disarms it. Keep the target within signed wrapping
distance of the published count. A target of zero rounds to one because the
backing reserves zero for disarmed execution. `Cpu::run` reports
`BudgetExhausted` at a completed checkpoint and leaves rearming to its caller.
The machine programs the backend with the earlier of the caller's limit and its
own clock deadline. Reaching the caller's limit ends the foreground run even
when both are due at the same checkpoint.

## Execution with guest time

`GuestTimeConfig::new(guest_rate)` sets the number of retired instructions per
guest second. The DOS launcher sets the rate to one million and supplies this
clock to the PIT, VGA and PS/2. Each thousand instructions represent one
millisecond on their timeline. `with_cpu_limit(max_instructions_per_second,
host_clock)` optionally caps how many instructions retire per *host* second.

```rust,ignore
let cpu = machine.create_cpu(cpu_config, None)?;
let result = cpu.run_until_with_guest_time(
    move |host_clock| GuestTimeConfig::new(guest_rate)
        .with_cpu_limit(max_instructions_per_second, host_clock),
    stop,
    async |mut runner| {
        // Configure the guest CPU and connect devices to runner.clock().
        runner.run_cpu().await
    },
).await?;
```

`run_until_with_guest_time` owns the host timer service and calls
`configure_guest_time(host_clock)` once on the execution worker.
When the embedding owns its timers, it can use
`cpu.spawn_with_guest_time(configure_guest_time, session)` and construct
its configuration on that worker. `Runner::with_guest_time` accepts
the configuration directly. `with_host_clock(host_clock)` supplies a host clock
for idle device deadlines and output presentation without setting a CPU limit.

`run_cpu().await` makes the CPU runnable and waits for its exit. The scheduler
sets a CPU checkpoint at the earliest of the next device wake, the optional
guest slice boundary and the end of the current CPU allowance. Device sleeps register a
task to wake. A slice returns control to the scheduler while instructions are
running. Only device sleeps determine where idle guest time advances.
`with_slice_duration(duration)` sets a positive slice interval in guest time.
Instruction blocks can overshoot a checkpoint. Both the interpreter and
compiled code publish retirement when execution returns to Rust.
[Worker scheduling](worker-execution.md#event-loop-scheduling)

### CPU limits and idle device deadlines

The CPU limit accounts for retired instructions. It lets guest execution proceed
in short bursts and waits for the corresponding absolute host time before
allowing more instructions. Rust services, device tasks and cancellation can run
while CPU entry is waiting. Sending a new CPU request leaves the current
retirement allowance in effect.

When the CPU is halted, waiting for a Rust service, or between programs, the
scheduler lets ready Rust work run before advancing to the next device
deadline. A deterministic session can advance guest time directly to that
deadline. A session with a host clock waits for the remaining duration at the
device clock's nominal rate. If Rust work becomes ready, the scheduler can
reconsider the deadline. A late host wake advances guest time to the device
deadline that woke it. The next idle interval starts at the current host time.
Idle advancement retires zero CPU instructions. An idle machine with no guest
deadline parks until an external wake.

### Live input and output

Live keyboard and mouse events arrive at the execution owner without timestamps.
Each device processes its events in order. Keyboard repeat and mouse sampling
use the device clock.

A device can await `clock.sync_to_host(output_time)` before publishing a frame
or audio block. If the guest has already reached `output_time`, the frame or
block is ready for immediate delivery. For a future timestamp, a bound guest
clock measures the remaining interval at the nominal guest rate and adds it
to the current host time. For example, at guest time 7 ms and host time 100 ms,
requesting presentation for guest time 9 ms creates a host deadline of 102 ms.
That sleep keeps its deadline if the guest later runs ahead or pauses.

An unbound guest clock or one configured without a host clock returns an
immediately ready wait. A host clock uses the supplied timestamp directly.
The host timer service registers each pending sleep on first poll and releases
it on cancellation. Keep the service alive while presentation waits use it.

## Host timers

`timer_service()` returns a `TimerHandle` and a `HostTimerDriver`. The driver starts
one dedicated timer worker. `run().await` keeps it alive until all timer handles
and waits are released. `spawn()` returns a `worker::Worker<()>` for
explicit lifetime control: `join().await` waits for normal completion, and `stop().await` cancels
remaining timers and waits for their worker to exit. Await either operation
before dropping the worker; terminating a browser worker early can skip Rust
destructors.

The shared `Worker<T>` owns startup, cancellation and completion for both timer
and execution tasks. It receives initial resources and releases the startup inbox.
Once running, each task handles its cancellation receiver: the timer loop
wakes, while the execution runner uses its existing checkpoint notifications.
Timer scheduling stays in `clock/driver.rs`.

`TimerHandle` is the transferable host-timer transport. Its `clock()` creates a
local consumer; `Clock::from(handle)` consumes the transport when it is no longer
needed for setup. `cpu.spawn(handle, session)` performs this conversion
on the execution owner. Clones access the same host monotonic timebase across workers.

The host clock reads `Instant::now()` locally. Sleeps send absolute deadlines to the
driver and await Rust channel notifications. Request delivery time therefore counts
toward the wait. Keep the driver alive until consumers and their sleeps finish.

The worker processes new requests and cancellations, completes expired timers,
then waits until the nearest absolute deadline. With no timers it waits until
another request or shutdown. Channel wakers notify the worker; a notification
before waiting is retained, so the request cannot be lost. Dropping a wait
wakes the worker to remove its timer. The timer list is small and local to the
worker; producers share only the request channel and response channels.

On native Windows, the worker waits on a high-resolution waitable timer and an
auto-reset event. The timer supplies the deadline; the event interrupts it for new requests,
cancellation and shutdown. Idle waits use only the event. This avoids the coarse
`WaitOnAddress` timeout used by Rust's timed parking without requesting a process
timer-resolution change. If these handles cannot be created, including on Windows
versions before 10 version 1803, the worker falls back to ordinary Rust parking.
Other native platforms also use Rust parking. The platform wait mechanism is private
to `clock/driver/wait`; timer handles and worker lifecycle APIs are unchanged.

By default, the threaded browser build's standard library implements `park` and
`park_timeout` with Wasm `memory.atomic.wait32`, and `unpark` with
`memory.atomic.notify`. This remains the path on macOS, Linux and unrecognized
browser environments. Browsers on Windows use a worker-local `MessageChannel`
task to arm each `setTimeout` instead: measured atomic waits there can round a
1–2 ms deadline up to about 15 ms. A fresh message task breaks the nested-timer
chain that otherwise clamps repeated timeouts to at least 4 ms. Platform selection
is isolated in `wait/browser/environment.rs` and requires no embedding options.

The browser backend yields to the timer worker's own event loop. Its message
handler recomputes remaining time from an absolute deadline; queueing the message
does not extend the wait. Requests, cancellation and shutdown use a retained Rust
notification and an `AtomicWaker`; they carry no JavaScript handles across threads.
Cancelling a deadline invalidates queued message work, clears any armed timeout
and settles its promise so Rust callbacks can be released. Worker shutdown closes
both message ports. The peer binding stays alive throughout the async driver.

`Clock::now()` reads the host's monotonic clock (`performance.now()` in the browser).
Every wake rechecks that clock; requested sleep duration is never treated as
elapsed time. All backends remain subject to host scheduling and browser throttling;
none guarantees millisecond deadlines or replays missed interrupts.

For a busy execution worker, bind its notification resource on the timer worker:

```rust,ignore
let (handle, driver) = relocade_machine::timer_service();
let cpu = machine.create_cpu(cpu_config, None)?;
let execution = cpu.spawn(handle, session);
let timers = driver.spawn_with_peer(execution.wake_peer());
let result = execution.join().await;
timers.stop().await?;
```

`spawn_with_peer` uses the existing worker resource transport to bind direct
notifications before processing timers. This avoids relaying browser timer wakes
through the spawning host's event loop. `CpuHandle::run_until` owns this
setup and stops execution before stopping its timer worker. Clocks used by other
independent sessions need their own service lifetime and notification bindings.

Timers are deadlines, not fixed-rate ticks. Host scheduling can still delay a
wake, and CPU checkpoints determine when an interrupt is serviced. Devices retain
their late-delivery policy: the PIT preserves its phase and coalesces missed edges
through the PIC. A 500 Hz mode-3 PIT has an output transition about every 1 ms.
The [PIT worker test](../../pit/tests/timer_worker.rs) exercises guest programming,
IRQ0 entry and EOI with the host event loop busy. Tight latency thresholds belong
in measurements, not correctness assertions dependent on the host scheduler.
