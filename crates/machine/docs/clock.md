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
Host and guest timestamps have separate origins.
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

A relative sleep captures the full duration from the native reading. At three
billion ticks per second, a one-nanosecond sleep sets its deadline three ticks
after the reading, even when it starts between nanosecond boundaries. Existing
timestamps remain valid across pauses, rebinding and CPU counter wrap.

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
Host sources progress independently of execution. The session's guest clock
uses CPU retirement to interpolate between samples of elapsed host time. Output can await
`sync_to_host(timestamp)` without moving guest time or changing device deadlines.

`Runner`, `Pc`, `Dos`, and devices accept `Clock` without an implementation type
parameter. Controllers expose the separate capabilities to drive timers or
advance time. `SessionConfig::with_host_clock` accepts a transferable
`ClockSource`. The worker constructs its local clock from that source.

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
`GuestTimeDriver` records those waits and wakes sleepers when deadlines expire.
A pending sleep registers its absolute guest tick once. The driver caches the
earliest tick. While the CPU runs, the scheduler projects that tick to an
instruction checkpoint. When the CPU is idle, the scheduler arms a host timer
for the guest deadline.

An embedding can also drive `GuestTimeDriver` directly. A standalone driver
starts at zero and advances one tick per retired instruction while bound. Its
`now()` reads the published instruction count plus an offset. Binding, pausing
and idle advancement update that offset. Deadlines stay in guest coordinates.

```rust,ignore
let driver = GuestTimeDriver::new(instructions_per_second);
let guest_clock = driver.clock();
// Connect devices using guest_clock, then construct the CPU.
let binding = driver.bind(&cpu, &execution_waker);
// Run the CPU, then service due waits at each stopped boundary.
driver.synchronize();
// Poll awakened tasks before continuing or deciding that execution is idle.
// Only when idle, driver.advance_to_next_deadline() services the next guest deadline.
drop(binding);
```

Guest time requires a CPU built with instruction budgets enabled, including for
unthrottled execution. A worker reports an incompatible target during startup.

The guard returned by `bind` lets the clock observe retirement and update the
CPU checkpoint as device waits change. Dropping it records final retirement,
clears the clock's deadline and releases the execution waker.
The clock pauses between bindings, and pending device waits remain registered.
Each binding starts from the CPU's current count without charging earlier work.

`synchronize()` reads the current guest time and wakes any device whose
deadline has arrived. Registering or cancelling a sleep updates the next
checkpoint immediately, including during an I/O or MMIO callback. A change
to the earliest deadline also wakes a parked scheduler.

`advance_to_next_deadline()` advances guest time to the earliest registered sleep
and completes all due sleeps. The embedding calls it only after polling ready
tasks and finding execution still idle. It returns false if there is no
registered sleep. When a sleep completes, the device task wakes and can
deliver an IRQ. Idle time advances independently of the CPU's instruction count.

Port instructions publish their preceding retirement before invoking a device.
Two port reads in one compiled block can therefore see different guest ticks.
Checkpoint delivery still happens at block boundaries, and a REP instruction
retires once when all its elements finish.
`CpuStateAccess::restore` preserves cumulative retirement when restoring guest
registers, including across program loads. Initial counts belong in `CpuConfig`.

## Execution with guest time

`GuestTimeConfig::new(guest_rate)` sets the number of device ticks per second.
The DOS launcher uses one million ticks per second for the PIT, VGA and PS/2.
This scale stays fixed during free execution, throughput limiting and HLT.
`with_cpu_limit(max_instructions_per_second)` optionally caps CPU retirement.

```rust,ignore
let cpu = machine.create_cpu(cpu_config, None)?;
let config = GuestTimeConfig::new(guest_rate).with_cpu_limit(max_instructions_per_second);
let worker = ExecutionWorker::spawn(cpu, SessionConfig::guest(config), async |mut runner| {
    // Configure the guest CPU and connect devices to runner.clock().
    runner.run_cpu().await
});
let result = worker.run_until(stop).await?;
```

The worker owns the host timer service and joins it when execution finishes.
To supply a host clock, pass `SessionConfig::guest(config).with_host_clock(source)`
to `spawn`. The embedding drives that source for the session's lifetime. For
example, a test can supply a `ManualClock` and advance it to a device deadline.

While the CPU runs, the timer worker requests a calibration every 4 ms. At
each sample the scheduler records elapsed host time and retired instructions.
Between samples a port read computes guest ticks from retirement using a
fixed-point multiply and shift. VGA can observe a retrace shorter than the
host timer resolution. Repeated status reads use the raster's cached next
transition and register no sleeps.

Each new slope starts at the time already reached, including the fractional
tick. It aims for the following host sample using the measured throughput.
The estimate can recover up to one extra window of lag per sample and stops
extrapolating at the window's end. The first window assumes 50 million
instructions per second until a measurement is available. Recalibration
updates the CPU target for the earliest pending wait. All sleepers retain
their original guest ticks.

At HLT, during CPU pacing or while the foreground waits, the scheduler arms
the timer worker for the earliest guest sleep. On waking it accounts for
elapsed wall time and preserves monotonic guest readings.
[Worker scheduling](worker-execution.md#event-loop-scheduling)

### CPU limits and idle device deadlines

The CPU limit accounts for retired instructions. It lets guest execution proceed
in short bursts and waits for the corresponding absolute host time before
allowing more instructions. Rust services, device tasks and cancellation can run
while CPU entry is waiting. The limit belongs to the session, so stopping and
resuming a CPU run preserves the remaining allowance.

Device deadlines keep the same host origin across CPU execution, HLT and
foreground waits. An idle machine with no deadline parks until an external wake.

### Live input and output

Live keyboard and mouse events arrive at the execution owner without timestamps.
Each device processes its events in order. Keyboard repeat and mouse sampling
use the device clock.

A device can await `clock.sync_to_host(output_time)` before publishing a frame
or audio block. If the guest has already reached `output_time`, the frame or
block is ready for immediate delivery. The session maps a guest timestamp
to its absolute host deadline. For example, if a session started at host
time 100 ms, guest time 9 ms always maps to host time 109 ms. Requesting
that frame after host time 109 ms completes immediately.

An unbound guest clock or a standalone driver with no host clock returns an
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
local consumer. `Clock::from(handle)` consumes the transport to create that clock.
Cloned handles access the same host monotonic timebase across workers.

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

An execution worker binds its timer worker to the CPU's wake peer at startup.
Timer delivery can then interrupt a busy CPU directly, even when the spawning
host's event loop is busy. Joining or stopping execution also shuts down its
timer worker.

When supplying a separately managed timer service through `with_host_clock`,
the caller can bind that service with `driver.spawn_with_peer(worker.wake_peer())`
and must keep it running until the session has finished.

Timers are deadlines, not fixed-rate ticks. Host scheduling can still delay a
wake, and CPU checkpoints determine when an interrupt is serviced. Devices retain
their late-delivery policy: the PIT preserves its phase and coalesces missed edges
through the PIC. A 500 Hz mode-3 PIT has an output transition about every 1 ms.
The [PIT worker test](../../pit/tests/timer_worker.rs) exercises guest programming,
IRQ0 entry and EOI with the host event loop busy. Tight latency thresholds belong
in measurements, not correctness assertions dependent on the host scheduler.
