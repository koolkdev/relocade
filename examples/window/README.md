# Main-thread window example

This application uses winit's `ApplicationHandler` directly. It opens one native
window or browser canvas, reports keys, dimensions and redraws, and closes on
Escape or the native close button. The browser embedding owns CSS layout.
The example has no pixel renderer; native status appears in the window title,
and browser status appears below the canvas.

```sh
cargo run -p relocade-window-example
```

The application shares an `Rc<Cell<u32>>` redraw count with the native caller,
which prints it after the event loop exits. The launcher accepts this non-`Send`
state because construction, callbacks and cleanup all remain on main.

The browser example uses the existing [browser toolchain](../../tests/browser/README.md):

```sh
npm --prefix tests/browser ci
node tests/browser/window.mjs --build-only
npm --prefix tests/browser run dev
```

Open `/window.html`, focus the canvas and type. Escape removes the canvas and
reports `Closed`. Run `node tests/browser/window.mjs` for the browser input,
resize, redraw and cleanup checks. On Linux, run the native fixture with
`xvfb-run -a cargo test -p relocade-window-example --test window -- --ignored`.
