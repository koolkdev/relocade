# Shared time and waits

`timer_service()` returns a `TimerHandle` and a `TimerDriver`. The driver starts
one dedicated timer worker. `run().await` keeps it alive until all timer handles
and waits are released. `spawn()` returns a `worker::CancellableWorker<()>` for
explicit lifetime control: `join().await` waits for normal completion, and `stop().await` cancels
remaining timers and waits for their worker to exit. Await either operation
before dropping the worker; terminating a browser worker early can skip Rust
destructors.

The shared `CancellableWorker<T>` owns cancellation and completion for both timer
and execution tasks. Each task handles its cancellation receiver: the timer loop
unparks, while the execution runner uses its existing checkpoint notifications.
Timer scheduling stays in `clock/driver.rs`.

`Clock::now()` reads elapsed time locally. `sleep(duration)` and
`wait_until(deadline)` send absolute deadlines to the driver and await Rust
channel notifications. Request delivery time therefore counts toward the wait.
Clones share their time origin across native threads and threaded browser workers.
Tests can implement `Clock` with manually controlled time.

The worker processes new requests and cancellations, completes expired timers,
then parks until the nearest absolute deadline. With no timers it parks until
another request or shutdown. Channel wakers unpark the worker; a notification
before parking retains a token, so the request cannot be lost. Dropping a wait
wakes the worker to remove its timer. The timer list is small and local to the
worker; producers share only the request channel and response channels.

The threaded browser build's standard library implements `park` and
`park_timeout` with Wasm `memory.atomic.wait32`, and `unpark` with
`memory.atomic.notify`. There are no JavaScript timeout callbacks or integer
millisecond conversions in deadline delivery. `Clock::now()` still reads the
host's monotonic clock (`performance.now()` in the browser). Every wake rechecks
that clock; requested sleep duration is never treated as elapsed time.

For a busy execution worker, bind its notification resource on the timer worker:

```rust,ignore
let (clock, driver) = relocade_machine::timer_service();
let execution = ExecutionWorker::spawn(clock, session);
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
