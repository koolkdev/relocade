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

Keyboard events travel through an ordinary Rust mpsc channel to an async input
activity on execution. Translation feeds the existing BIOS keyboard interface:
ASCII text, control/Alt combinations, arrows/navigation, function keys and shift
state. Unicode text and IME composition are not converted to DOS code pages yet.
Focus loss clears held modifiers; synthetic focus events do not type characters.

The main thread implements winit's existing `ApplicationHandler`. It owns the
window, lifecycle and redraw opportunities, and polls host timers and worker
completion through a coalesced winit wake. `ExecutionWorker` runs one DOS CPU and
its registered devices. A separate existing `worker::Thread` owns the surface,
`VgaRenderer`, and shared image `Renderer`.

`RedrawRequested` supplies bounded demand. Graphics selects the newest immutable
capture when servicing a request, decodes it only if its sequence changed, draws
the image, submits once, and presents. Resize/expose can reuse the GPU image.
The graphics owner sends a generic redraw notification when captured input changes.
Main coalesces it with resize/expose into one pending winit redraw callback (a browser
animation frame on web). It sends only the output size to graphics; capture versions
and GPU resources stay there. While graphics is busy, pending demand keeps the newest
size. Completion releases that demand without requesting another frame by itself.
Hiding or zero-sizing the window suppresses drawing while DOS/VGA continue. Guest
program exit does not close the capture source.

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
production executable, type a key into a waiting DOS program, inspect presented
guest pixels after exit, resize, and close. Run it under Xvfb when no display is available.

`node tests/browser/app-test.mjs` builds the browser application and drives the
production page in Chromium. It stalls GPU submission while sending keyboard
input, checks displayed pixels and pending resizes, hides and resumes output,
and checks normal shutdown and GPU failure while DOS awaits input. It uses the
dependencies in `tests/browser`.
