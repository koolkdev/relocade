# PC speaker

Emulates the PC speaker control port and synthesizes PIT channel 2. The shared
[relocade-pit](../pit/README.md) device owns the timer registers, input-clock
arithmetic and programmable channel-0 IRQ scheduler. BIOS keeps its IRQ handler and
time-of-day data.

`Speaker::initialize(runner, pit).await` connects the speaker to that PIT,
registers port 0x61 and starts its activity. It returns the speaker handle and
its connection for the CPU bus. Both devices stay on the execution worker.

The speaker supports byte transfers at `SPEAKER_CONTROL_PORT` (0x61).
Word/dword writes are ignored and reads return open bus. PIT channel-2 count
(0x42) and control/read-back (0x43) transfers go to the PIT device.
Port 0x61 stores bits 0/1 and reads raw OUT2 in bit 5; other bits read zero and
their writes have no effect. Bit 0 is GATE2. The speaker drive is bit 1 AND OUT2,
including direct bit-1 toggling while GATE2 is low. Other system-control and
refresh/parity functions of this port are outside this crate's initial scope.

With the `audio` feature, attach CPAL's output configuration and an ordinary
bounded `async_channel::Sender<Vec<f32>>` when output becomes available. The app
opens platform output on main:

```rust,ignore
let (audio, config, samples) = relocade_app::audio::Audio::open(on_error)?;
audio.play()?; // In a browser, call from the application's user activation.
```

Move `config` and `samples` into the existing execution-worker session:

```rust,ignore
let (pic, pic_device) = Pic::initialize(runner).await;
let (pit, pit_device) = Pit::initialize(runner, &pic).await;
let (speaker, speaker_device) = Speaker::initialize(runner, pit).await;
runner.cpu().io().connect([pic_device, pit_device, speaker_device]);
speaker.borrow_mut().attach_audio(config, samples)?;
// Run the guest with runner.run_cpu().await.
```

The runner drives `Speaker::run` alongside the CPU, including while the guest
awaits input. Until output is attached, the activity waits on an ordinary Rust
channel while hardware port access remains available. Attachment is allowed once;
it uses the current timer and speaker state. The activity waits on the PIT's
supplied clock until an entire sample interval has elapsed, renders interleaved
f32 PCM, and submits it through the bounded channel. Host and guest clocks use
the same path. Muted output submits silence while retaining the timer's phase.
When output is stopped or its queue fills, the oldest queued buffer is replaced;
register changes keep being consumed instead of accumulating behind a stalled sink.
Dropping app output closes submission and ends the activity. Stopping execution
drops the device and its activity on their owner; platform output can drain to
silence or be dropped separately.

The shared PIT counter integrates each sample's high time; fractional input
clocks carry across buffers. Mono drive is duplicated across output channels
with amplitude 0.2. A fixed CPAL buffer hint is clamped to 1–256 frames; the default
is 256: one interval is about 5.33 ms at 48 kHz. The app's existing 16-buffer
queue can hold about 85 ms at that rate, plus callback and platform buffering.
CPAL consumes the PCM at its configured rate; buffers need no separate host wait.

Sound-changing writes append timestamped counter states and the speaker enable
bit to a local queue. Synthesis consumes every change at its position within the
sample interval, including multiple changes in one sample. Reads, latches and
unchanged speaker-control writes leave playback alone. The supplied clock sets
the timing precision; this is not a CPU-cycle model.

The PCM transport is independent of PIT and speaker state. A future AdLib
synthesizer can use the same elapsed-time synthesis and ordinary PCM handoff.
