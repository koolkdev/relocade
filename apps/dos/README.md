# DOS window

Run one COM or MZ program in a native window:

```sh
cargo run -p relocade-dos-app -- path/to/PROGRAM.COM
```

Program exit updates the window title and retains the display. Closing the window
stops execution, finishes outstanding graphics work, and releases the window after
both owners have finished. An execution or graphics error follows the same shutdown
path. The current surface layer reports surface/device failures; automatic surface
recreation is not implemented here.

The main thread implements winit's existing `ApplicationHandler`. It owns the
window, lifecycle and redraw opportunities, and polls host timers and worker
completion through a coalesced winit wake. `ExecutionWorker` runs one DOS CPU and
its registered devices. A separate existing `worker::Thread` owns the surface,
`VgaRenderer`, and shared image `Renderer`.

`RedrawRequested` supplies bounded demand. Graphics selects the newest immutable
capture when servicing a request, decodes it only if its sequence changed, draws
the image, submits once, and presents. Resize/expose can reuse the GPU image.
Completion requests the next winit redraw opportunity; there is no extra frame
timer and no CPU pixel conversion. Hiding or zero-sizing the window suppresses
drawing while DOS/VGA continue. Guest program exit does not close the capture source.

## Browser

Install nightly-2026-10-20 with `rust-src`, and wasm-bindgen-cli 0.2.129. Build and
serve the supplied page:

```sh
node apps/dos/web/build.mjs
npx vite@8.3.5 --config apps/dos/web/vite.config.mjs --host 127.0.0.1
```

Open `/?program=/PROGRAM.COM`, serving that program from `apps/dos/web`. The page
fetches those bytes and opens one canvas, whose size follows CSS. The development
server supplies the isolation headers required by the existing threaded Wasm build.

An embedding can instead call the generated bindings directly:

```js
await init();
const dos = open(programBytes);
// Later: request shutdown. The canvas is removed after workers finish.
dos.close();
```

`open` returns after startup; `error()` reports an asynchronous startup/runtime
failure. There is no awaitable application-exit API. Browser embedding requires the
worker shim alias in `web/vite.config.mjs` and the same isolation headers.

## Validation

`cargo test -p relocade-dos-app --test window -- --ignored` uses X11 to exercise the
production executable, inspect presented guest pixels after program exit, resize,
and close. Run it under Xvfb when no display is available.

`node tests/browser/app-test.mjs` builds the browser application and drives the
production page in Chromium, checking displayed canvas pixels, sustained redraws,
resizing and worker shutdown. It uses the dependencies in `tests/browser`.
