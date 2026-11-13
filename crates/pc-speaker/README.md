# PC speaker

Emulates the PC speaker control port and synthesizes PIT channel 2. The shared
[relocade-pit](../pit/README.md) device owns the timer registers, input-clock
arithmetic and programmable channel-0 IRQ scheduler. BIOS keeps its IRQ handler and
time-of-day data.

`Speaker::initialize(runner, pit).await` connects the speaker to that PIT,
registers port 0x61. It returns the connected speaker
handle. Both devices stay on the execution worker.

The speaker supports byte transfers at `SPEAKER_CONTROL_PORT` (0x61).
Word/dword writes are ignored and reads return open bus. PIT channel-2 count
(0x42) and control/read-back (0x43) transfers go to the PIT device.
Port 0x61 stores bits 0/1 and reads raw OUT2 in bit 5; other bits read zero and
their writes have no effect. Bit 0 is GATE2. The speaker drive is bit 1 AND OUT2,
including direct bit-1 toggling while GATE2 is low. Other system-control and
refresh/parity functions of this port are outside this crate's initial scope.

With the `audio` feature, `speaker.audio_source()` records sound-changing writes
for an execution-local consumer. `source.sample(start..end)` averages the digital
drive over a nonempty elapsed interval on the PIT's clock, producing a mono value
in 0..=1. Intervals progress forward without overlap. The shared PIT counter
integrates each sample's high time, preserving fractional input clocks and phase
while muted or when the consumer skips time. Creating another source replaces
the observer; dropping the source stops recording.

The [shared mixer](../mixer/README.md) owns the common sample timeline, gain,
channel layout, buffering and submission. PC registers the speaker at gain 0.2
alongside its other sources on the execution worker. Applications attach CPAL's configuration
and bounded PCM sender through `pc.attach_audio(config, samples).await` or
`dos.attach_audio(config, samples).await`. The speaker itself owns no output
channel or asynchronous playback activity.

Sound-changing writes append timestamped counter states and the speaker enable
bit to a local queue. Synthesis consumes every change at its position within the
sample interval, including multiple changes in one sample. Reads, latches and
unchanged speaker-control writes leave playback alone. The supplied clock sets
the timing precision; this is not a CPU-cycle model.

