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

The default PC includes AdLib and the built-in speaker. Rust callers can choose
the installed sound card before startup:

```rust,ignore
use relocade_dos::{Configuration, SoundCard};

relocade_dos_app::run_with_configuration(Configuration {
    sound_card: SoundCard::SoundBlaster2,
})?;
```

`SoundCard::None` keeps the speaker available and leaves AdLib's ports unbound.
`SoundCard::AdLib` is the default for both the native binary and browser `run()`
export. Embeddings that construct DOS directly use
`Dos::with_configuration(runner, configuration).await`; `Dos::new` keeps the
default. This selects emulated hardware independently of host output.

Select `SoundCard::SoundBlaster2` for sampled effects and OPL2 music. Its fixed
settings are base `0x220`, IRQ 7, DMA 1 and DSP version 2.01. On DOS, the card's
installer normally added `SET BLASTER=A220 I7 D1 T3` to `AUTOEXEC.BAT`; the shell
supplied the variable to programs. `Dos::initial_environment()` derives these
defaults from the installed card's typed hardware settings. An embedding can
initialize its environment once when preparing the session, then keep and edit it:

```rust,ignore
let environment = dos.initial_environment();

// For each launch, borrow the launcher's current entries.
let entries: Vec<_> = environment.iter().map(Vec::as_slice).collect();
dos.launch_com(&image, relocade_dos::LoadOptions {
    environment: &entries,
    ..Default::default()
})?;
```

The same option applies to `launch_mz`. The launcher owns the environment after
setup: changes and removals are preserved by passing those entries to later
launches. DOS copies exactly what the caller supplies. `BLASTER` describes the
installed settings, and changing its value does not reconfigure the card.

The application entrypoint connects existing owners. One execution worker owns
DOS, keyboard, VGA, speaker, the selected sound card and the Rust command loop.
It sends the native console's input sender, `dos.mouse_input()` and `dos.frames()` through a startup
channel. Main attaches those endpoints to its `AppHost` while driving the
application and timer service; the graphics worker constructs `VgaDisplay` and owns the surface. Window closure can cancel startup or
a pending console read, and waits for execution and graphics cleanup. Execution
errors remain visible even when startup ends before the endpoints arrive.

`AppHost::open_audio` opens and owns CPAL output on main. Only its stream
configuration and bounded Rust PCM sender go to the execution worker.
`Dos::with_configuration` creates the selected hardware;
`dos.attach_audio(config, samples).await` connects its mixer.
Guest writes to PIT channel 2 and port 0x61 drive the speaker; writes to ports
0x388/0x389 drive OPL2 music when installed. Sound Blaster DSP/DMA effects feed
the same output stream alongside FM and the speaker.
Attach audio before launching a guest to retain all note changes. Hardware state
persists across guest process exits.

For a program launched through `launch_com` or `launch_mz`, select **AdLib/OPL2**
for music in the game's configuration. If effects are configured separately,
PC speaker can play alongside it. With `SoundBlaster2` selected, choose
**Sound Blaster 2.0**, port **220**, IRQ **7**, DMA **1** for effects and
**AdLib/OPL2** for music. Unsigned 8-bit mono PCM supports single-cycle,
auto-initialize and high-speed playback, plus direct DAC and timed silence.
Recording, ADPCM, MIDI, C/MS, SB Pro stereo and SB16/OPL3 are outside the
[SB2 playback subset](../../crates/sound-blaster/README.md).
The native prompt has no sound-device command.
See the [AdLib compatibility and license notes](../../crates/adlib/README.md) for
the core's limits and LGPL distribution requirements.

Native playback starts immediately. The browser console appears immediately;
the app's browser platform code requests playback once the document has user
activation. CPAL retains that request through worklet setup. If opening or
starting output fails, the console still runs and the installed sound-device
ports remain available. Closing the window drops output before waiting for
execution cleanup, closing any pending PCM submission. No extra synthesis worker
is created.

Click the window or canvas to capture the mouse; press Escape to release it.
The capture click is consumed. Focus loss releases held buttons and restores host
interaction. Native windows use pointer locking or confinement; browsers require
pointer-lock permission. Relative motion is independent of window size and is
scaled by the DOS driver. The built-in driver stays available across launches;
programs initialize it with INT 33h and explicitly show their cursor. The native
command prompt itself does not display a mouse cursor.

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
and use the AT controller → PIC IRQ1 → guest INT 09h → BIOS buffer path.
`launch_com` and `launch_mz` accept supplied images; program lookup and input handoff during launch are future application policy.
There is no mode flag in the CPU, PIC or BIOS. Drop the native console before
launching or resuming a guest.

## Audio validation

`cargo test -p relocade-dos-app --test adlib` executes authored COM programs through
the production PC/DOS setup. One detects AdLib without attached output; another
plays AdLib, combined AdLib/speaker, speaker alone and silence across DOS keyboard
waits. A controlled emulation clock drives captured mixer PCM. This fixture also
runs in browser CI without using CPAL playback.

`cargo test -p relocade-dos-app --test audio` runs a COM program that enables the
speaker, waits for a DOS key, then disables the speaker and waits again. A paced
PCM receiver checks tone and silence while execution remains active. The browser
suite uses CPAL's actual AudioWorklet and waits for fresh analyser samples on the
audio clock, with a bounded timeout and playback diagnostics. Output lifecycle
and backpressure remain covered by the app audio and PC mixer tests. The bundled
application test separately checks click/key activation and console input. These
automated checks do not establish physical speaker playback or long-term clock accuracy.

Representative game listening remains pending. Browser playback acceptance also
depends on the shared [AudioWorklet initialization](https://github.com/koolkdev/relocade/issues/970)
and [CPAL memory-growth](https://github.com/koolkdev/relocade/issues/971) fixes; the
affected platform-output tests remain ignored until those issues are resolved.
