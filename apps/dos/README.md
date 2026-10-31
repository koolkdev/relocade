# DOS console application

Run the console natively:

```sh
cargo run -p relocade-dos-app --bin relocade-dos
```

Or start the shared browser host (requires the threaded Wasm toolchain and
wasm-bindgen CLI described in [the browser setup](../../tests/browser/README.md)):

```sh
npm --prefix tools ci
node tools/web.mjs relocade-dos-app
```

Use `--build` to bundle into `target/web/relocade-dos-app/dist`. Serve it with
COOP/COEP headers. The shared host owns the canvas and Close control.

The application entrypoint connects existing owners: main runs the application
and timer driver, one execution worker owns DOS/keyboard/VGA and the Rust command
loop, and the graphics worker owns `VgaDisplay` and the surface. Closing the
window waits for execution and graphics cleanup, including a pending input read.

## Console

`run_console(&mut dos).await` runs on the execution owner. It prints a `> ` prompt,
reads an edited line through the shared DOS console, and implements `echo` with
case-insensitive command matching. Enter, Backspace, blank lines and unknown
commands return to the next prompt. This is a small Rust command loop, not a
COMMAND.COM compatibility implementation.

The console uses `dos.console().write(bytes)` and `read_line().await`; its text,
cursor and scrolling use the same VGA state as BIOS and DOS program output.
It does not create a renderer or a worker. Existing `launch_com` and `launch_mz`
accept supplied bytes; executable lookup and storage selection are not defined here.
