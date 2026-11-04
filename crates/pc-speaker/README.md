# PC speaker

Emulates PIT channel 2 and the PC speaker control port. Counter behavior and
input-clock arithmetic are shared with [relocade-pit](../pit/README.md), which
also owns the default-rate channel-0 IRQ scheduler. BIOS keeps its IRQ handler
and time-of-day data.

`Speaker::with_clock(Rc<C>)` creates the machine `Device`. Connect it with
`runner.connect(speaker, [], relocade_pc_speaker::PORTS).await`. Mutable state and
port reads stay with execution; elapsed time uses the session clock's absolute
origin, preserving fractional PIT clocks across repeated accesses.

Only byte transfers at PIT `CHANNEL_2_PORT` (0x42, count), PIT `CONTROL_PORT`
(0x43, control/read-back), and `SPEAKER_CONTROL_PORT` (0x61) are supported.
Word/dword writes are ignored and reads return open bus. Commands
for other PIT channels and read-back commands with reserved bit 0 set are ignored.
Port 0x61 stores bits 0/1 and reads raw OUT2 in bit 5; other bits read zero and
their writes have no effect. Bit 0 is GATE2. The speaker drive is bit 1 AND OUT2,
including direct bit-1 toggling while GATE2 is low. Other system-control and
refresh/parity functions of this port are outside this crate's initial scope.

With the `audio` feature, attach CPAL's output configuration and an ordinary
bounded `async_channel::Sender<Vec<f32>>` before connecting the device. The app
opens platform output on main:

```rust,ignore
let (audio, config, samples) = relocade_app::audio::Audio::open(on_error)?;
audio.play()?; // In a browser, call from the application's user activation.
```

Move `config` and `samples` into the existing execution-worker session:

```rust,ignore
let speaker = Speaker::with_clock(Rc::new(runner.clock().clone()))
    .with_audio(config, samples)?;
let io = IoBus::new([
    runner.connect(Rc::new(RefCell::new(speaker)), [], PORTS).await,
]);
// Pass io to runner.create_cpu, then run the guest with runner.run_cpu.
```

The runner drives `Speaker::run` alongside the CPU, including while the guest
awaits input. The activity produces interleaved f32 buffers and awaits
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
machine clock. Sound-changing port writes update that phase locally. Reads,
latches and unchanged speaker-control writes leave playback alone. Guest register
readback follows the machine clock; waveform generation follows the output sample
clock between control changes.

This first implementation accepts scheduling and buffering latency. Controls
can coalesce before the next buffer; guest write timestamps are not reproduced.
It supports programmed tones, not cycle-accurate bit-banged audio or PWM.
