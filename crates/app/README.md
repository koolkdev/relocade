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

With the `graphics` feature, `window.surface_setup(event_loop)` prepares transfer
to an explicitly created `worker::Thread`. Native surface creation occurs on main
to satisfy platform requirements, including Metal. Web setup transfers the
offscreen canvas; GPU objects are initialized on the receiving worker.

```rust,ignore
let setup = window.surface_setup(event_loop)?;
let graphics = relocade_app::worker::Thread::new();
let task = graphics.run_send(setup, |setup| async move {
    let mut surface = relocade_app::graphics::Surface::new(setup).await?;
    // Acquire an attachment, record draws, submit, then surface.present(frame).
    // Use surface.device, surface.queue and surface.format() with Renderer.
    Ok::<(), String>(())
});
```

`Surface` remains on the thread that initializes it. It owns configuration,
acquisition and presentation; the caller owns image resources, command recording,
submission and redraw demand. Await the worker task before releasing its thread
and window. A browser canvas transfers once. Acquire only for nonzero output;
wgpu's acquisition status distinguishes retrying a timeout from waiting for an
occluded window. Outdated configurations are refreshed on the next request.
Surface loss is reported for application-level recovery rather than silently
reinitializing GPU resources.
