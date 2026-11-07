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

## Adaptive time model

`AdaptiveTime` is the arithmetic foundation for an execution-based estimate of
real time. It is not yet a device-facing `Clock` implementation or a selectable
CPU timing mode. The runner and devices continue to use the existing timer service.

The execution owner supplies `ClockProgress` snapshots containing cumulative
work units and cumulative idle time. Work units describe execution progress, not
physical processor cycles. Idle time includes all elapsed time while guest
execution was inactive, including service waits and host/device processing.
Counting that time separately keeps it from lowering the estimated execution rate.
Individual BIOS/DOS services will not need to annotate their awaits.

`sample(host_time, progress)` learns the time per work unit from the host interval
after subtracting its idle time. Each new measurement contributes one quarter
of the smoothed rate. `estimate(progress)` advances from the latest host sample
using work at that rate plus elapsed idle time. It reads no host clock.
Until a meaningful execution sample establishes the rate, estimates requiring
new work return `None`; the consumer must take another host sample.

Every host sample anchors subsequent estimates to actual host time. The model
clamps returned timestamps monotonically if a prediction ran ahead; it does not
add that prediction error to the next anchor. Cumulative inputs avoid additional
rounding drift from requesting many small estimates. Equal host timestamps retain
their work for the next measurable calibration interval.

The next integration step needs a dedicated execution-progress source outside
the saved CPU state, including progress visible at device accesses and within
REP. It must preserve existing retirement/RDTSC semantics and agree across JIT
and interpreter execution. The browser clock handle must be constructed on the
execution worker, where transferred Wasm memory can be read through direct Wasm
bindings. Sampling frequency and its performance/accuracy tradeoff belong to that
integration, as do device waits on the same timeline.

A CPU service returning `Poll::Pending` may self-wake or leave devices runnable;
it does not establish that the entire worker is asleep. Execution accounting
belongs at the CPU driver's boundaries. The model only consumes the resulting
work and idle totals. Explicit pause behavior, freezing time during compilation,
fixed processor timing and historical VGA scanout are separate policies.
