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
let (pit, pit_device) = Pit::initialize(runner, pic.irq(TIMER_IRQ)).await;
let (speaker, speaker_device) = Speaker::initialize(runner, pit).await;
let io = IoBus::new([pic_device, pit_device, speaker_device]);
speaker.borrow_mut().attach_audio(config, samples)?;
// Pass io to runner.create_cpu, then run the guest with runner.run_cpu.
```

The runner drives `Speaker::run` alongside the CPU, including while the guest
awaits input. Until output is attached, the activity waits on an ordinary Rust
channel while hardware port access remains available. Attachment is allowed once;
it uses the current timer and speaker state. The activity produces interleaved f32 buffers and awaits
`samples.send(buffer)`. The full queue suspends production until output frees
space. Output must consume at its configured playback rate; an unpaced sink
needs external pacing to let CPU execution and cancellation progress. Muted
output uses the same path with zero-filled buffers. There is no
speaker-state channel, synthesis worker, refill timer or explicit yielding loop.
Dropping app output closes submission and ends the activity. Stopping execution
drops the device and its activity on their owner; platform output can drain to
silence or be dropped separately.

The shared PIT counter integrates each sample's high time; fractional input
clocks carry across buffers. Mono drive is duplicated across output channels
with amplitude 0.2. A fixed CPAL buffer hint is clamped to 1–256 frames; the default
is 256. The app's 16-buffer queue can hold about 85 ms at 48 kHz, plus a pending
submission, callback and platform buffering. Control changes, including unmuting,
follow already prepared samples.

Playback keeps its own counter phase because queued samples extend ahead of the
machine clock. Sound-changing writes retain the latest counter state in PIT; playback consumes
that state before generating its next buffer. Reads,
latches and unchanged speaker-control writes leave playback alone. Guest register
readback follows the machine clock; waveform generation follows the output sample
clock between control changes.

This first implementation accepts scheduling and buffering latency. Controls
can coalesce before the next buffer; guest write timestamps are not reproduced.
It supports programmed tones, not cycle-accurate bit-banged audio or PWM.
