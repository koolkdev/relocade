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

`Window` owns one native window or attached browser canvas on main. Create it in
`resumed` using winit's `WindowAttributes`. Feed `window_event` arguments to
`handle_event`; it ignores other window IDs and updates physical size, scale,
occlusion and close requests. `state().drawable()` excludes occluded, zero-size
and closing output. Occlusion follows the information available to winit on each
platform; it is not a guarantee that a desktop window is unobscured.

```rust,ignore
let window = relocade_app::Window::new(event_loop, attributes)?;
window.winit().request_redraw();
```

The existing winit window remains available through `winit()`. Input, redraws,
application suspension and the decision to exit stay in `ApplicationHandler`.
Keep the owner alive until its graphics users have shut down, then drop it on
main. This releases the native window and removes its browser canvas, including
a canvas supplied through winit's `with_canvas`. CSS controls browser layout
unless the caller explicitly specifies dimensions in the window attributes.
