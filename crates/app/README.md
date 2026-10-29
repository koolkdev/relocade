# Main-thread application launcher

`relocade_app::run` constructs a winit `ApplicationHandler` on main and starts its
event loop. The crate reexports winit so consumers use its existing event and
window APIs directly.

```rust,ignore
let exited = relocade_app::run(|events| MyApp::new(events))?;
exited.await;
shutdown_workers().await;
```

The factory receives an `EventLoopProxy` for application-defined notifications.
The application can own thread-local state such as `Rc` values. It creates windows
in `resumed`, handles input and redraws in `window_event`, and cleans up in
`exiting`. The application decides when to call `ActiveEventLoop::exit`.

On native desktops, `run` returns after the event loop exits, with an already-ready
notification. In a browser it registers callbacks and returns immediately; the
notification completes after the application's `exiting` callback returns. The
application remains owned by winit. Dropping the notification does not stop it.

The caller drives the notification with its executor, such as
`wasm_bindgen_futures::spawn_local` on web. Awaiting leaves the browser free to
deliver events. Native `run` still drives winit directly on main; returning a
future does not make native event processing asynchronous. The notification marks
event-loop exit; the caller can then await its own worker shutdown.

Workers, CPU/device execution, rendering, surfaces and application communication
are owned explicitly by consumers. The launcher adds no worker or task protocol.
