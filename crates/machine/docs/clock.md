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

Devices that count their own cycles take native observations directly:

```rust,ignore
pit.advance_to(clock.now());
if let Some(deadline) = pit.next_deadline() {
    clock.sleep_until(deadline).await;
}
```

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
Host sources progress independently of execution; guest sources advance with
published CPU retirement or controlled idle advancement. Host pacing uses
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

`GuestTimeDriver::new(instructions_per_second)` owns emulated guest time,
pending sleeps and the corresponding CPU instruction target. Its timeline starts
at zero. One instruction advances one tick. While bound, `now()` reads the
published instruction count and adds an offset; it performs no duration conversion
or clock-state update. The offset changes only for binding, pausing and idle
advancement. Pending deadlines stay in the same guest coordinate system, and the
CPU target is obtained by subtracting the offset. While the CPU is idle, the
embedding may advance time to the next registered sleep without changing retired
instruction accounting. The driver is synchronous; the embedding owns execution
and executor turns.

```rust,ignore
let driver = GuestTimeDriver::new(instructions_per_second);
let guest_clock = driver.clock();
// Connect devices using guest_clock, then construct the CPU.
let binding = driver.bind(&cpu, &execution_waker);
driver.synchronize(None); // No additional yield deadline.
// Run the CPU. At each stopped boundary:
driver.synchronize(None);
// Poll awakened activities before continuing or deciding that execution is idle.
// Only when idle, driver.advance_to_next_deadline() services the next guest deadline.
drop(binding);
```

`bind` retains CPU progress and target handles without borrowing the CPU. The
returned guard scopes target ownership and wait-change notifications. Dropping
it records final retirement, clears the target and releases the execution waker.
The clock pauses between bindings, and pending device waits remain registered.
Each binding starts from the CPU's current count without charging earlier work.

`synchronize(yield_deadline)` observes retirement, completes due sleeps and arms
the next CPU target. `yield_deadline` is an optional absolute guest time at
which the embedding wants execution to yield. The driver does not choose yield
intervals. Registration and cancellation update the target synchronously,
including during IO/MMIO callbacks, without extending the supplied yield
deadline. With neither device waits nor a yield deadline, the target is unarmed.
Changed waits wake the embedding so it can reconsider idle scheduling while a
CPU service is pending.

`advance_to_next_deadline()` advances guest time to the earliest registered sleep
and completes all due sleeps. The embedding calls it only after polling ready
activities and finding execution still idle. It returns false if there is no
registered sleep. The yield deadline does not cause this advance. Completing
sleeps wakes device activities; those activities deliver IRQs through the existing
interrupt path. Advancing guest time does not add retired CPU instructions.

Reads preserve fractional nanoseconds and counter wrap. Synchronous callbacks
can observe a count that lags execution; targets use that same published count
and retain soft checkpoint overshoot. Reading time does not dispatch device work.
`CpuStateAccess::restore` preserves cumulative retirement when restoring guest
registers, including across program loads. Initial counts belong in `CpuConfig`.

## Execution with guest time

`cpu.spawn_with_guest_time(make_time, session)` constructs a
`GuestTimeConfig` on the execution owner. `GuestTimeConfig::new(rate)` selects
unpaced guest time and device deadlines only. `with_slice_duration(duration)`
adds execution slices measured in guest time. At a slice boundary, ready activities
get a turn before execution continues. `with_host_clock(clock)` also waits for the
corresponding host time and defaults to 4 ms slices unless a duration was supplied.
The slice duration must be positive; either builder-call order uses the supplied
duration. The host clock is constructed in the factory.
The lower-level `Runner::with_guest_time` takes the same configuration directly.

```rust
let cpu = machine.create_cpu(cpu_config, None)?;
let worker = cpu.spawn_with_guest_time(
    move || GuestTimeConfig::new(instructions_per_second)
        .with_slice_duration(Duration::from_millis(4))
        .with_host_clock(host_timers.clock()),
    async |runner| {
        // Devices receive runner.clock(); the host timer service runs alongside
        // this worker and binds its wake peer.
    },
);
```

`Runner::run_cpu` uses one execution loop for both host and guest clocks. Its
private `ExecutionSchedule::poll_cpu` owns each turn: poll host work, wait for
pacing, prepare instruction targets, drive guest execution and service due waits.
Continuing execution gives ready activities a turn. Terminal exits yield only
when an execution notification remains pending, including notifications from
timers made due by the final instructions. The schedule's binding scopes the
guest driver's target and notifications to one run; completion and cancellation
release it.

Pending services and suspended calls belong to `Cpu`. Its internal
`poll_host_work` polls active Rust services and observes exit requests without
entering the guest. `poll_execution` handles guest calls, IRQ entry, dispatch,
and new or resumed host calls. Both operations keep terminal-exit cleanup inside
CPU ownership. `Cpu::run` uses the same operations without pacing; Runner delegates
their ordering to its scheduler. Ready device work runs before idle time advances,
and guest execution gets an opportunity to resume before choosing an idle deadline.

The private `ExecutionSlice` owns one retained boundary for cooperative yields
and host pacing. The next CPU stop is the earlier of that boundary and the next
device deadline. Device timers and execution notifications leave the boundary
unchanged. Crossing it carries checkpoint overshoot forward on the same slice
grid. With no configured slice, the scheduler does not sample the clock to choose
a periodic execution deadline. Devices use the same `Clock` API in either mode;
the guest time driver has no CPU execution or yield callback.

Unpaced idle execution advances directly to the next device deadline, even if
it skips slice boundaries. Paced idle advancement stops at the earlier of that
deadline and the current slice boundary. A slice alone never causes idle time
to advance when there are no registered sleeps.

### Slices paced by a host clock

Host waits use a separate host `Clock` constructed from `TimerHandle` on the
execution owner. `HostTimerDriver::spawn_with_peer` binds direct notifications so
host deadlines can wake services while guest execution stays runnable.

At the slice boundary, Runner waits for the corresponding absolute host time.
Checkpoint overshoot is included in that wait and shortens the next slice.
Late host wakes preserve the slice grid and each intervening device deadline;
guest time never jumps directly to the current host time. Devices and the CPU
receive execution opportunities between deadlines; IRQ acceptance still depends
on guest interrupt state. A slice can contain several PIT deadlines without
requiring a host wait for every transition.

At stopped boundaries, the clock driver compares host time with the mapped guest
time. Lag of at most 20 ms can catch up without pacing sleeps. Beyond 20 ms, the
driver shifts the host correspondence forward by whole nanoseconds of lag,
retaining any fraction in the correspondence. This treats a long host stall as a
pause without skipping guest execution or device deadlines. The tolerance is
fixed independently of the configured slice duration. Guest time, fractional retirement, instruction
rate and the slice grid do not change. Future CPU and output waits use the new
correspondence; already-created host waits retain their deadlines.

An execution notification makes ready activities pollable while preserving the
current allowance. Ready host services and exit requests can progress during a
pacing wait, but guest calls, IRQ entry and instruction dispatch need execution
credit. Replacing a device timer does not replace the pacing wait or grant time.

Completing or cancelling `run_cpu` cancels its host wait. The next run retains
the same slice boundary and the existing lead or lag, subject to the lag limit.
Time spent between runs is excluded by shifting the host correspondence by the
exact native pause duration; repeated cancellation cannot grant another slice.
The clock driver owns this host/guest correspondence, while
`runner/schedule/slice.rs` owns the slice boundary and cancellable host wait.

### Live input and output

Live keyboard and mouse input is applied at the execution owner's current guest
time, in arrival order. Each input yields cooperatively so queued transitions
leave opportunities for guest execution and other device work. Device sample
and repeat deadlines still use the guest clock.

For output, the device's async task awaits the
ordinary local `Sleep` returned by `clock.sync_to_host(time)` before publishing
to another worker:

```rust,ignore
clock.sync_to_host(output_time).await;
```

CPU pacing uses this same operation. The wait retains the mapped deadline and
the configured host clock. It uses that clock's ordinary `sleep_until`, including
the dedicated timer service's native and browser waits. Registration happens on
first poll, cancellation releases the timer, and elapsed deadlines complete
without registering one. Clocks and waits remain on their execution owner.

The mapping is captured when `sync_to_host` is called, so later pauses or lag
recovery cannot remap that wait. Passing a timestamp to a task uses the mapping
when the task processes it. Host-clock output uses the same timestamp; unpaced guest
clocks and inactive runs produce an immediately ready wait. Keep the host timer
service alive while execution-owner tasks use it.

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
