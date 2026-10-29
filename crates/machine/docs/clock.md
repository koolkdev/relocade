# Shared time and waits

`timer_service()` returns a `TimerHandle` and a `TimerDriver`. Keep the driver
on a responsive host and move timer handles into execution workers. Poll the
driver's `run()` future alongside host control work. It finishes when all timer
handles and their waits are released.

`Clock::now()` reads elapsed time locally. `sleep(duration)` and
`wait_until(deadline)` send absolute deadlines to the driver and await Rust
channel notifications. Request delivery time therefore counts toward the wait.
Clones share their time origin across native threads and threaded browser workers.
Tests can implement `Clock` with manually controlled time.

The driver uses `futures_timer::Delay` directly and rechecks the deadline after
waking. Cancelling a wait cancels its host timer through the response channel;
there is no per-CPU-quantum timer or platform-specific duration conversion.
The [worker fixture](../../../tests/browser/src/clock.rs) demonstrates delivery
while the execution worker continuously polls Rust work.
