# Audio output

`relocade-audio` supplies bounded interleaved PCM transport through the existing
`async-channel` crate. The application owns the producer, synthesis, scheduling
and output lifetime; this crate has no dependency on emulation or a worker framework.

```rust
use relocade_audio::{pcm_buffer, Format};

let format = Format::new(48_000, 2)?;
let (mut writer, mut reader) = pcm_buffer(format, 2048)?;
writer.try_write(&[0.25, -0.25])?;
let mut samples = [0.0; 4];
assert_eq!(reader.read(&mut samples)?, 1);
assert_eq!(samples, [0.25, -0.25, 0.0, 0.0]);
# Ok::<(), Box<dyn std::error::Error>>(())
```

The unique writer can move to a synthesis worker. It checks capacity before
sending a complete batch; a full channel reports backpressure. The reader takes
complete frames with `try_recv` and fills an underrun with silence.
Dropping the writer preserves queued samples; dropping the reader closes
submission. A concurrent output closure may interrupt an in-flight write.

Samples use the explicit rate and channel order. The wrapper validates frame
boundaries and normalizes samples; channel storage, synchronization and closure
belong to `async-channel`. There is no resampling, synthesis or playback clock.

## Playback

`Output::open(on_error)` opens the default device with its preferred format and
returns an `Output` plus its `PcmWriter`. Read `writer.format()` before rendering
interleaved f32 samples. The queue holds 2048 frames. CPAL converts samples into
the device's integer or floating-point representation.

```rust,no_run
use relocade_audio::Output;

let (output, mut writer) = Output::open(|error| eprintln!("audio: {error}"))?;
let channels = usize::from(writer.format().channels());
writer.try_write(&vec![0.25; 128 * channels])?;
output.play()?;
// Keep output alive while the application feeds writer, possibly from a worker.
// Dropping output stops playback and closes further submission.
# Ok::<(), Box<dyn std::error::Error>>(())
```

The output starts consuming PCM when `play()` is called. Missing samples become
silence. Dropping the writer lets already queued samples drain; dropping the
output discards the tail. Stream errors go to the supplied callback, which
should stay brief because CPAL may invoke it on an audio thread.

Linux builds require ALSA development headers and pkg-config (Debian/Ubuntu:
`apt install libasound2-dev pkg-config`). Run `cargo run -p relocade-audio
--example tone` for one second of synthetic 440 Hz audio. The ignored output test
can use a physical device or the supplied ALSA null configuration; the null sink
checks ownership and playback without establishing physical output or latency.

## Browser output

The same Rust callback runs on CPAL 0.18.2's AudioWorklet backend. It consumes the
queue from shared Wasm memory; PCM batches are not serialized through messages.
CPAL owns the worklet's Wasm instance and deinterleaves its Rust scratch buffer
into browser output channels. No separate JavaScript audio backend is needed.

Open output on the main thread and call `play()` from a user gesture. These are
synchronous requests: opening does not await processor readiness, playback does
not await the browser's resume promise, and dropping output requests asynchronous
context closure. Dropping output immediately closes further PCM submission.
The error callback forwards errors reported by CPAL; it does not detect every
browser failure, including rejected resume promises or processor traps.

Use the repository's threaded Wasm build (`tools/web-build.mjs`) and COOP/COEP
headers. It enables atomics, shared memory and the TLS exports CPAL needs.
Ordinary Wasm builds can use the PCM transport, but opening browser output
requires atomics. The application owns worker creation and refill scheduling.
