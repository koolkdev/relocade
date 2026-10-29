# Main-thread application launcher

`relocade_app::run` constructs a winit `ApplicationHandler` on main and starts its
event loop. The crate reexports winit so consumers use its existing event and
window APIs directly.

```rust,ignore
relocade_app::run(|events| MyApp::new(events))?;
```

The factory receives an `EventLoopProxy` for application-defined notifications.
The application can own thread-local state such as `Rc` values. It creates windows
in `resumed`, handles input and redraws in `window_event`, and cleans up in
`exiting`. The application decides when to call `ActiveEventLoop::exit`.

On native platforms, `run` returns after the event loop exits. In a browser it
registers the callbacks and returns immediately; the application remains owned
by winit until exit. An `Ok` return on the browser does not indicate completion.

Workers, CPU/device execution, rendering, surfaces and application communication
are owned explicitly by consumers. The launcher adds no worker or task protocol.
