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

The main thread retains current physical size and visibility independently of
demand. One pending redraw bit coalesces requests; updates during slow drawing
replace dimensions instead of queuing old sizes. Hidden and zero-sized outputs
stop drawing without stopping execution or its timer service. Resume/expose can
draw the same source. Presentation acknowledgement means submission and handing
the frame to the surface, not GPU completion.

On the browser graphics worker, each serviced redraw yields through the timer API
to an event-loop task so canvas presentation and GPU callbacks can progress under
sustained demand. This is separate from CPU cooperation and VGA refresh timing.

The private surface owner reconfigures outdated/suboptimal output and recreates a
lost surface on its platform owner: through main on native, on the graphics worker
using the existing OffscreenCanvas on web. Incompatible recovered surfaces and
GPU device loss/errors close the session through normal cleanup. Device recreation
and application-resource rebuilding are outside this one-window harness. A surface
reporting occlusion waits for another host expose/resume.

`web::control` exposes `stop()` and a `finished` Promise to browser callers. Its
output-conversion closure maps a completed Rust value into JavaScript.
