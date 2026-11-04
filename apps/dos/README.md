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
DOS/keyboard/VGA/speaker and the Rust command loop. It sends `dos.keyboard_input()` and
`dos.frame_source()` through a startup channel. Main attaches those endpoints to
its `AppHost` while driving the application and timer service; the graphics worker
constructs `VgaDisplay` and owns the surface. Window closure can cancel startup or
a pending console read, and waits for execution and graphics cleanup. Execution
errors remain visible even when startup ends before the endpoints arrive.

`AppHost::open_audio` opens and owns CPAL output on main. Only its stream
configuration and bounded Rust PCM sender go to the execution worker. `Dos::new`
creates the hardware; `dos.speaker()` exposes it for `attach_audio(config, samples)`.
Guest writes to PIT channel 2 and port 0x61 produce sound; the console itself
does not ring a bell yet. Speaker state persists across guest process exits.

Native playback starts immediately. The browser console appears immediately;
the app's browser platform code requests playback once the document has user
activation. CPAL retains that request through worklet setup. If opening or
starting output fails, the console still runs with silent speaker ports.
Closing the window drops output before waiting for execution
cleanup, closing any pending PCM submission. No extra synthesis worker is created.

## Console

`run_console(&mut dos).await` runs on the execution owner. It prints the Relocade
banner once through the same VGA console, then presents a `> ` prompt,
reads an edited line through the shared DOS console, and implements `echo` with
case-insensitive command matching. Enter, Backspace, blank lines and unknown
commands return to the next prompt. This is a small Rust command loop, not a
COMMAND.COM compatibility implementation.

The console uses `dos.console().write(bytes).await` and `read_line().await`; its text,
cursor and scrolling use the same VGA state as BIOS and DOS program output.
It does not create a renderer or a worker. Existing `launch_com` and `launch_mz`
accept supplied bytes; executable lookup and storage selection are not defined here.

## Audio validation

`cargo test -p relocade-dos-app --test audio` runs real COM port programming,
keyboard waits, process relaunch and execution/output shutdown against a paced
PCM receiver. The same fixture runs in the browser suite against CPAL's actual
AudioWorklet, measuring the resulting tones and silence. The bundled application
test separately checks click/key activation and console input. These automated
checks do not establish physical speaker playback or long-term clock accuracy.
