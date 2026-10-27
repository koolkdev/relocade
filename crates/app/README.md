# Application execution, rendering and window hosting

`relocade-app` defines worker-based applications and rendering independently
of windows, DOS, BIOS, VGA and the x86 machine. Applications supply sendable launch
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

Browser consumers use the threaded build and worker bootstrap in the shared
[web tooling](../../tools/web/README.md). Importing bindings on a worker must not
launch the application. The crate rejects non-atomic Wasm builds. Native tests
cover worker-local Rc state across awaits, custom updates, typed output and
cancellation of pending and cooperating ready work.

## Rendering

The [renderer crate](../renderer/README.md) defines the stateful `Renderer`
contract, shared `Graphics` resources and `Texture`/`Viewer` operations.
`Renderer`, `Graphics` and `Texture` are reexported here for application consumers.
Renderers own their state and can progress without a window; the embedding drives their futures and
samples their current textures independently.

## Window host

The host provides one winit window on desktop or one canvas in a browser. It
starts an execution worker and a graphics worker. The graphics worker drives
both the application's renderer and the shared viewer on one device
and queue. For a redraw, the worker acquires a surface frame, samples the renderer's current
texture, draws it through the viewer and presents the frame.

```rust,ignore
let control = relocade_app::run::<MyRenderer, _>(application, options)?;
let result = control.await?;
```

The application's `Update` type is the renderer's `Source`. Sending a source
constructs or replaces the renderer on the graphics worker; replacing it drops
the previous renderer on that worker. Application-specific rendering requests
can travel through a channel included in the source.

`WindowOptions` chooses title, initial logical size and `OnCompletion::Close` or
`OnCompletion::KeepOpen`. With `KeepOpen`, final queued source updates remain
valid after execution completes. The session retains the result until the host
closes; a later Stop preserves it. A worker error takes precedence over a retained
result and initiates shutdown. Normal renderer completion retains its final
texture and releases the renderer's remaining resources. It is never polled again.

`Control::stop()` requests shutdown; `stop_handle()` produces a clonable
`StopHandle` for callers that move the control into a future or JavaScript Promise.
Completion waits for both worker tasks and window detachment. Native winit returns
the control after shutdown; browser winit returns it after event-loop registration.
Page or process termination can prevent orderly shutdown; the host does not report
successful completion while workers remain active.

Only host redraws stop when a window is zero-sized, occluded or suspended. After
initial GPU setup, rendering keeps progressing, including during asynchronous
surface recreation. Rendering cadence and texture dimensions do not depend on the
window. At most one host presentation request is outstanding, and resize events
retain the latest size. Acknowledging a consumed request permits another host frame; it does
not mean that GPU work has completed.

## Browser lifecycle bindings

A browser export calls `web::launch(control, map_exit)` and returns its
`BrowserControl`. The shared adapter exposes `stop()` and a `finished` Promise;
the application maps its typed `Exit<T>` into a JavaScript value. Launch errors
and rejected completion promises use JavaScript Error objects. Importing the
bindings does not start an application.

```rust,ignore
#[wasm_bindgen]
pub fn open(bytes: Vec<u8>, name: String) -> Result<BrowserControl, JsValue> {
    relocade_app::web::launch(MyApplication::new(bytes, name).open(), |exit| {
        // Convert the application's result or cancellation for its frontend.
        map_result(exit)
    })
}
```

## Ownership

| Module | Responsibility |
| --- | --- |
| `application.rs` | Execution, typed updates and results. |
| `task.rs` | Cancellation and retirement of completed workers. |
| `launch/web.rs` | Browser shutdown requests and completion conversion. |
| `session.rs` | Worker startup/shutdown, completion policy, retained results and errors. |
| `control.rs` | External shutdown requests and typed completion. |
| `window.rs` | Winit event adapter and window-host entrypoint. |
| `window/state.rs` | Window creation, live size/visibility and detachment. |
| `window/scheduler.rs` | Host redraw deadlines and one outstanding presentation request. |
| `display.rs` | GPU initialization, renderer lifetime and concurrent window presentation. |
| `display/output.rs` | Host surface, output format, acquisition and presentation. |
| `platform/native.rs`, `platform/web.rs` | Event-loop execution and surface handoff. |

The platform module is selected once. Native targets ask main to create a
surface; browsers transfer an OffscreenCanvas once and create the surface on
the graphics worker. Winit observes CSS size, while the graphics worker owns
bitmap size. The device and host output format remain stable across resize and
surface recreation. Display textures have their own fixed color format.

macOS uses the same native graphics-worker path. wgpu 30.0.1 has an AppKit
thread-access issue during Metal frame acquisition, tracked in
[wgpu #10627](https://github.com/gfx-rs/wgpu/issues/10627). The upstream report
shows Main Thread Checker diagnostics; macOS rendering and lifecycle still
need runtime validation.

The window check verifies typed automatic completion after rendering work
and while GPU startup may still be pending. Winit requires main and one event
loop per process, so it uses a standalone entrypoint with separate processes.
It only runs when explicitly selected under a display server:

```sh
WINIT_UNIX_BACKEND=x11 xvfb-run -a cargo test -p relocade-app --test window --locked -- --ignored
```
