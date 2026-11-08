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
an interval. Host clocks use the host's absolute monotonic timebase.

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
