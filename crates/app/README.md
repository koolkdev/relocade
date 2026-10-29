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

The launcher itself creates no workers. CPU/device execution and application
policy stay with its consumer; the optional graphics host owns one graphics worker.

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
`recreate_device().await` replaces the device, queue and configuration while
retaining the platform surface target. Drop old attachments and GPU resources
first, then rebuild them with the new device and format. This never transfers a
browser canvas again. Acquisition preserves `Lost` and `Validation` statuses;
replacing the GPU device does not repair a lost platform surface.

`graphics::Graphics` supplies the complete graphics owner when the application
wants to connect a display to this surface. Its factory creates the display on
the graphics worker using the same device and queue as the surface. The display
implements `relocade_peripherals::display::DisplayRenderer`; an existing `Image`
also works directly. Shared drawing tools remain in `relocade-renderer`, while
peripheral contracts have their own crate.
The app crate has no VGA, DOS or machine dependency. The factory runs once and
can move its source into the display. The connection keeps its logical state
across GPU replacement and owns any guest-visible loss/reset behavior through
`device_lost()` and `attach_device(device, queue)`. Replacement is unsupported by
default; a GPU-only static image needs a source to regenerate its contents.

```rust,ignore
let (mut graphics, finished) = relocade_app::graphics::Graphics::spawn(
    window.surface_setup(event_loop)?,
    move |device, queue| create_display(device, queue),
    move || { let _ = events.send_event(MyEvent::RedrawNeeded); },
);

// On RedrawNeeded, resize or expose, schedule window.winit().request_redraw().
// On winit's RedrawRequested, when the output is drawable:
graphics.request_redraw(window.state().size);

// Poll `finished` alongside the main event loop to observe errors as they occur.
// When closing, request shutdown and await completion before releasing the window:
graphics.close();
finished.await?;
```

`notify_redraw_needed` runs on the graphics worker and asks the app to schedule
a redraw opportunity. On winit's `RedrawRequested`, the app calls
`graphics.request_redraw(size)` to queue drawing the latest image at that size.
While a draw is running, later requests replace one pending size. Zero size
suppresses drawing.
The application stops requesting redraws when hidden or suspended; outstanding
work can settle. Graphics handles completion internally and does not start another
frame without demand. Normal presentation does not wait for GPU completion.

The display's asynchronous `update(queue)` services inputs independently of draws.
It returns `Some(true)` when a redraw is useful, `Some(false)` for input work that
does not change visible content, and `None` when its source closes. Graphics submits
queued uploads even without redraw demand, and coalesces redraw notifications until
the next request. `encode_image(encoder)` records any GPU work needed for the current
display image, or returns an existing image for reuse. The host records drawing that
image later in the same command batch, then submits before another input update or
encode. The image uses reusable GPU storage; recording does not wait for GPU
completion. Guest execution stays independent of these host redraws.

Source closure preserves the display and its last image. Closing or dropping the
graphics connection stops the worker's activity. The separate completion future
reports startup and GPU errors and retains the worker until its Rust resources
have been released. Keep polling it through shutdown. Browser work yields to the
event loop so continuously ready updates cannot starve WebGPU callbacks; this does
not generate redraw demand. Surface loss is reported, without automatic recreation.

The worker handles source updates, redraw requests and GPU failures in one loop.
Drawing one frame reports whether presentation was requested, skipped or should
be retried.
