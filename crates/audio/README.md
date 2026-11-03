# Audio output

CPAL supplies the platform types, configuration, and errors. Producers submit
interleaved `f32` buffers through an ordinary bounded `async_channel::Sender<Vec<f32>>`.
Each message owns its complete buffer, so cancelling a pending send cannot publish
part of a frame. Devices choose their sample buffers and batch sizes.

```rust,no_run
use relocade_audio::Output;

let (output, config, samples) = Output::open(|error| eprintln!("audio: {error}"))?;
samples.try_send(vec![0.25; 128 * usize::from(config.channels)]).map_err(|e| e.to_string())?;
output.play()?;
// Keep output alive while the application submits buffers.
# Ok::<(), Box<dyn std::error::Error>>(())
```

`send(buffer).await` waits for channel capacity; `try_send(buffer)` submits without
waiting. The channel holds 16 buffers, and the output callback can retain one
partly consumed buffer. Producers must bound their buffer sizes. With 128-frame
buffers at 48 kHz, the channel holds about 43 ms of audio, plus callback, producer,
and platform buffering. There is no resampling or playback-clock mapping.

The callback consumes complete frames, retains remaining samples for the next
callback, clamps samples to [-1, 1], maps NaN to silence, and converts to the host
sample format. It discards an incomplete trailing frame from each buffer. An
empty or shorter-than-one-frame buffer produces one silent output frame. Missing
samples produce silence. Sending and closure use ordinary Rust channel semantics;
dropping output closes submission immediately and discards the queued tail.

Linux builds require ALSA development headers and pkg-config (`libasound2-dev`
on Debian/Ubuntu). `cargo run -p relocade-audio --example tone` plays one second
of synthetic 440 Hz audio. The ignored output test uses the supplied ALSA null
configuration in CI; it does not establish physical output or latency.

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
Opening browser output requires atomics. The application owns worker creation and refill scheduling.

The browser suite starts through a real button click. A busy Rust worker feeds
440/660 Hz tones through the public API; a Web Audio analyser checks frequency,
amplitude, underrun silence and continued output after shared-memory growth.
It also checks bounded buffering before play, immediate submission closure on
drop, and forwarding of an asynchronous CPAL module-load error. These are
Chromium checks; physical output and other browsers still need playback testing.
