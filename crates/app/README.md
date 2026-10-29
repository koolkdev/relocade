# Application tasks

`Application` supplies launch data and a worker-local `run` future. Its input,
published source and result are application-defined types; no window or GPU is required.
`Context` receives input, uses an embedding-supplied timer handle and publishes
latest-only sources without scheduling rendering.

```rust,ignore
let mut task = relocade_app::spawn(application, clock);
task.send_input(input);
let source = task.current_source();
task.stop();
let result = task.await?;
```

The embedding drives the corresponding `TimerDriver` while execution is alive.
Completion returns `Some(output)` or an error. Cancellation returns `None` after
worker-local state is destroyed. Retain and await the task through shutdown;
dropping it early can terminate a browser worker before Rust cleanup completes.
The last published source remains readable after execution completes.

This uses the existing machine worker and timer infrastructure. It adds no executor
or CPU loop; CPU consumers continue using the machine's `Runner`.

## Window hosting

Enable the `window` feature and implement `WindowApplication` to display a source.
It adds graphics construction and drawing to `Application`; window-independent
tasks do not depend on winit or wgpu.

```rust,ignore
let control = relocade_app::open(application, WindowOptions::default())?;
let result = control.await?;
```

The main thread owns winit, input and window/canvas lifetime. A separate graphics
worker owns GPU resources, surface configuration, acquisition and presentation.
`create_graphics` and `draw` run there; the application records and submits drawing
commands into the supplied attachment. Native surfaces are created on the main
thread, including the macOS/Metal requirement. The browser transfers its canvas
once and creates the surface on the graphics worker.

Native `open` runs the event loop until close; web `open` registers it and returns.
The main event loop drives the shared timer service. Execution completion preserves
its source and result while the window remains open. Close or `Control::stop`
cancels workers, waits for Rust cleanup and then removes the window/canvas.
Browser dimensions come from embedding CSS; `WindowOptions::size` sets the initial
native logical size.
