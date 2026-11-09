# Shared time and waits

`Clock` is a clonable, execution-local value that reads one timebase and creates
owned deadline waits. Devices and services use the same concrete API:

```rust,ignore
let now: Instant = clock.now();
let deadline = now + Duration::from_millis(2);
clock.sleep_until(deadline).await;
clock.sleep(Duration::from_millis(2)).await;
```

`Instant` is re-exported from `web_time`, which uses the standard library type
on native targets. `Instant` is monotonic; `Duration` is
an interval. Host time follows the host's monotonic clock. Emulated guest time
starts at an explicitly supplied instant and advances independently. Sharing an
origin does not make guest time follow host time. Pacing is the execution policy
that controls their relationship.

`Sleep` owns its source without retaining a device borrow. Relative sleeps capture
their deadline when constructed; both forms register on first poll. Dropping a
polled sleep cancels it. An expired sleep may complete immediately: awaiting a
clock does not promise an executor yield or a browser event-loop turn.

`ClockSource` supplies `now() -> Instant` and `sleep_until(Instant) -> Sleep`.
`Clock::new(source)` wraps a source behind the common consumer API, and
`Sleep::new(future)` owns its wait state. Custom sources must preserve monotonic
readings and first-poll registration/drop cancellation. `Runner`, `Pc`, `Dos`, and
devices accept `Clock` without an implementation type parameter. Controllers expose
the separate capabilities to drive timers or advance time. `ExecutionWorker::spawn_with_clock`
constructs a clock on the execution owner from a transferable factory.

## Controlled time in tests

`ManualClock` lives in `tests/clock-support`, a development-only dependency of
the consuming test suites. It is not exported by `relocade-machine` or included
in application builds. Its `clock()` uses the same public `ClockSource` interface.

`advance_by(duration)` and `advance_to(instant)` wake due sleeps;
`next_deadline()` observes the earliest polled, uncancelled sleep. Advancing to
an earlier instant leaves time unchanged. Cloned controllers share time and can
move between workers; consumer clocks remain local. Fixtures share a monotonic
anchor through the test helper's `origin()`; clock progress remains independent.

## Emulated guest time

`GuestTimeDriver::new(start, instructions_per_second)` owns emulated guest time,
pending sleeps and the corresponding CPU instruction target. While the CPU runs,
retired instructions advance guest time at the configured rate. While the CPU is
idle, the embedding may advance time to the next registered sleep. The driver is
synchronous; the embedding owns execution and executor turns.

```rust,ignore
let driver = GuestTimeDriver::new(start, instructions_per_second);
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

`ExecutionWorker::spawn_with_guest_time(instructions_per_second, yield_interval,
session)` constructs a guest time driver starting at `Instant::now()` and a
`Runner` on the execution owner. `yield_interval` is an optional positive duration
in guest time between periodic CPU yields; `None` uses device deadlines alone.
The lower-level `Runner::with_guest_time` accepts prepared CPU storage and its
wake endpoint.

`Runner::run_cpu` uses one execution loop for both host and guest clocks. It
prepares the schedule, polls the CPU once and publishes guest time. Continuing
execution gives ready activities a turn. Terminal exits yield only when an
execution notification remains pending, including notifications from timers made
due by the final instructions. The private `ExecutionSchedule` owns clock selection, periodic yield deadlines and idle
scheduling. Its binding scopes the guest driver's target and notifications to
one run; completion and cancellation release it.

Pending services and suspended calls belong to `Cpu`. Both `Cpu::run` and Runner
use the same internal `poll_run` boundary, including terminal-exit cleanup. Runner
does not retain a separate CPU-run future. Ready device work runs before idle
time advances, and the CPU is polled again before choosing an idle deadline.

The execution schedule computes periodic yield deadlines from the optional
interval.
Boundaries remain anchored to the session origin, including after checkpoint
overshoot and device reprogramming. The driver receives the next absolute guest
time at which execution should yield. Devices use the same `Clock`
API in either mode, and the guest time driver has no CPU execution or yield callback.

Host waits use a separate host `Clock` constructed from `TimerHandle` on the
execution owner. `HostTimerDriver::spawn_with_peer` binds direct notifications so
host deadlines can wake services while guest execution stays runnable.

This mode advances guest time without pacing against host time. Idle advancement
to a guest deadline is distinct from waiting for a host deadline or an external
event. Mapping guest time to host time and choosing those waits belongs to Runner's
pacing policy; that policy is not implemented here. Checkpoint placement and REP
retirement accounting also remain separate work.

## Host timers

`timer_service()` returns a `TimerHandle` and a `HostTimerDriver`. The driver starts
one dedicated timer worker. `run().await` keeps it alive until all timer handles
and waits are released. `spawn()` returns a `worker::CancellableWorker<()>` for
explicit lifetime control: `join().await` waits for normal completion, and `stop().await` cancels
remaining timers and waits for their worker to exit. Await either operation
before dropping the worker; terminating a browser worker early can skip Rust
destructors.

The shared `CancellableWorker<T>` owns cancellation and completion for both timer
and execution tasks. Each task handles its cancellation receiver: the timer loop
wakes, while the execution runner uses its existing checkpoint notifications.
Timer scheduling stays in `clock/driver.rs`.

`TimerHandle` is the transferable host-timer transport. Its `clock()` creates a
local consumer; `Clock::from(handle)` consumes the transport when it is no longer
needed for setup. `ExecutionWorker::spawn(handle, session)` performs this conversion
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
let execution = ExecutionWorker::spawn(handle, session);
let timers = driver.spawn_with_peer(execution.wake_peer());
let result = execution.join().await;
timers.stop().await?;
```

`spawn_with_peer` uses the existing worker resource transport to bind direct
notifications before processing timers. This avoids relaying browser timer wakes
through the spawning host's event loop. `ExecutionWorker::run_until` owns this
setup and stops execution before stopping its timer worker. Clocks used by other
independent sessions need their own service lifetime and notification bindings.

Timers are deadlines, not fixed-rate ticks. Host scheduling can still delay a
wake, and CPU checkpoints determine when an interrupt is serviced. Devices retain
their late-delivery policy: the PIT preserves its phase and coalesces missed edges
through the PIC. A 500 Hz mode-3 PIT has an output transition about every 1 ms.
The [PIT worker test](../../pit/tests/timer_worker.rs) exercises guest programming,
IRQ0 entry and EOI with the host event loop busy. Tight latency thresholds belong
in measurements, not correctness assertions dependent on the host scheduler.
