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

Use `--build` to bundle into `target/web/relocade-dos-app/dist`. The output includes
gzip-compressed Wasm and a `_headers` file declaring its encoding and COOP/COEP
headers. Use a host that honors `_headers`, or configure those headers explicitly.
The shared host owns the canvas.

The application entrypoint connects existing owners. One execution worker owns
DOS/keyboard/VGA and the Rust command loop. It sends `dos.keyboard_input()` and
`dos.frame_source()` through a startup channel. Main attaches those endpoints to
its `AppHost` while driving the application and timer service; the graphics worker
constructs `VgaDisplay` and owns the surface. Window closure can cancel startup or
a pending console read, and waits for execution and graphics cleanup. Execution
errors remain visible even when startup ends before the endpoints arrive.

## Console

`run_console(&mut dos).await` runs on the execution owner. It prints the Relocade
banner once through the same VGA console, then presents a `> ` prompt,
reads an edited line through the shared DOS console, and implements `echo` with
case-insensitive command matching. Enter, Backspace, blank lines and unknown
commands return to the next prompt. This is a small Rust command loop, not a
COMMAND.COM compatibility implementation.

The console uses `dos.console().write(bytes)` and `read_line().await`; its text,
cursor and scrolling use the same VGA state as BIOS and DOS program output.
It does not create a renderer or a worker. Existing `launch_com` and `launch_mz`
accept supplied bytes; executable lookup and storage selection are not defined here.
