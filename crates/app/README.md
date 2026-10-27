# Application tasks

`relocade-app` defines worker-based applications independently of windows,
graphics, DOS, BIOS, VGA and the x86 machine. Applications supply sendable launch
configuration and define their own update and result types.

`Application::run(self, Context)` creates execution state on its worker. The
future need not be `Send`. `Context::send(update)` publishes application-specific
messages to the embedding host, and `Context::is_stopping()` exposes cooperative
cancellation. Updates and results cross the worker boundary and must be `Send`.

```rust,ignore
let task = relocade_app::spawn(application, |update| {
    // Handle the application's own update type.
});
task.stop();
let result = task.await?;
```

`Exit<T>` distinguishes `Completed(T)` from `Cancelled`; results need not implement
Default or Clone. `Task<T>` retains the existing worker Thread until accepted
work completes. Keep it alive and await it before dropping it. Cancellation wakes
pending work and drops the future on its worker. Always-ready execution must
check the context's shared cancellation flag between slices; there is no
wall-clock preemption inside a synchronous poll.

Browser consumers need the threaded build and worker bootstrap described in the
[browser guide](../../tests/browser/README.md). The crate rejects non-atomic
Wasm builds. Native tests cover worker-local Rc state across awaits, custom
updates, typed output and cancellation of pending and cooperating ready work.
