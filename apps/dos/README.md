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
DOS/keyboard/VGA/speaker and the Rust command loop. It sends the native console's
input sender and `dos.frame_source()` through a startup channel. Main attaches those endpoints to
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

The application owns a physical-event channel and runs the prompt on the execution
owner:

```rust,ignore
let (input, events) = relocade_frontend::keyboard::channel();
// Send `input` to AppHost::attach_keyboard on the UI owner.
run_console(dos.native_console(events)).await;
```

`NativeConsole::read_line().await` consumes physical events using the BIOS US
keyboard layout, independent of the host language. It reuses DOS's line editor
for Enter, Backspace and control characters. `write(bytes)` is synchronous: it
uses cooked tab stops and the shared BIOS/VGA text, cursor and scrolling code.
The prompt implements `echo`, with case-insensitive matching, and reports unknown
commands. It creates no DOS process and writes no duplicate stdout capture.

Native input belongs to the prompt. It does not enter the controller or the BIOS
keyboard buffer; native output bypasses DOS handles, redirection and installed
CON drivers. No guest code, IRQ handlers or resident utilities execute at the
prompt. The runner continues its device activities, including display/audio;
BIOS ticks advance only when guest execution services timer interrupts.

Guest programs instead receive physical events through `dos.keyboard_input()`
and use the PC's guest I/O path. `launch_com` and `launch_mz` accept supplied images;
program lookup and input handoff during launch are future application policy.
There is no mode flag in the CPU, PIC or BIOS. Drop the native console before
launching or resuming a guest.

## Audio validation

`cargo test -p relocade-dos-app --test audio` runs a COM program that enables the
speaker, waits for a DOS key, then disables the speaker and waits again. A paced
PCM receiver checks tone and silence while execution remains active. The browser
suite uses CPAL's actual AudioWorklet and waits for fresh analyser samples on the
audio clock, with a bounded timeout and playback diagnostics. Output lifecycle
and backpressure remain covered by the app audio and speaker tests. The bundled
application test separately checks click/key activation and console input. These
automated checks do not establish physical speaker playback or long-term clock accuracy.
