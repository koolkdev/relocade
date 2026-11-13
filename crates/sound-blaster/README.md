# Sound Blaster 2.0

`relocade-sound-blaster` implements an SB2 playback subset on the execution
worker. The fixed resource profile is base `0x220`, IRQ 7, DMA channel 1 and
DSP version 2.01. PC selects it with `SoundCard::SoundBlaster2`; AdLib remains
the default and the built-in speaker is independent.

The card owns one `relocade-adlib` instance. Ports `0x228`/`0x229` and
`0x388`/`0x389` share its register latch, timer state and audio source. Resetting
the DSP leaves FM untouched. SB2 has no programmable mixer chip or OPL3.
The card composes that FM device with a DSP. The DSP owns its command protocol,
timed playback and DAC. Playback owns DMA progress, sample phase and completion
IRQs; the DAC owns the held level, mute state and sample capture.

Named byte-only DSP ports implement reset, data, read-buffer status and
write-buffer status. Releasing reset starts a 100 microsecond initialization
interval on the shared clock, followed by the `0xaa` response. Command `0xe1`
returns version 2.01. Reads with no response return `0xff`; unsupported commands
are ignored.

Command `0x40` programs a sample period of `256 - time_constant` microseconds.
Command `0x14` consumes its little-endian length plus one through DMA channel 1.
The activity services elapsed samples in roughly 1 ms batches, shortening the
last batch for completion. DSP and DMA register accesses also synchronize elapsed
transfers. DMA applies pending transfers before a guest changes its masks or
buffer registers, so those changes cannot affect earlier sample slots.
Fractional sample time survives polling; changing the time constant starts a new
period. Muting through `0xd3` leaves DMA running; `0xd1` enables the DAC and `0xd8`
reports its state. Reset cancels playback, pending command bytes and the IRQ line.

DSP completion raises IRQ7 until a read of `0x22e` acknowledges it. PIC EOI is a
separate guest operation. DSP length and DMA count are independent: exhausted or
masked DMA pauses consumption without completing the DSP command. Unmasking
resumes future sample slots. Transfer progress and IRQs require no audio sink.
Command `0x10` writes one unsigned byte directly to the DAC at guest-selected
times. Both direct writes and consumed DMA buffers feed `AudioSource`, with
`0x80` as zero, `0x00` as -1 and `0xff` as 127/128. The DAC holds its last value
until another sample, mute or reset. The source averages these levels over the
mixer's intervals, preserving duration when the output sample rate differs.

Create a source with `card.audio_source()` before guest execution. It retains
owned consumed bytes rather than rereading guest buffers during rendering.
Source reads first synchronize hardware through the shared clock so mixer/device
wake order cannot omit samples;
the same transfer cursor prevents double consumption. Dropping the source stops
recording without stopping DMA or IRQs. PC defaults PCM, FM and the speaker to
unity gain. These independent levels belong to PC audio policy;
device emulation supplies unscaled samples. The mixer preserves headroom, with
clamping at platform output, and keeps its bounded, nonblocking submission.

Auto-initialize DSP playback, pause/continue, high-speed commands and timed
silence are the next playback milestone. DMA's own auto-initialize register
behavior remains independent of these DSP commands.

Recording, ADPCM, MIDI and optional C/MS chips are outside this playback subset.
The implementation follows Creative's [Sound Blaster Hardware Programming
Guide](https://pdos.csail.mit.edu/6.828/2018/readings/hardware/SoundBlaster.pdf),
chapters 2, 3 and 6 and the SB2 port map in appendix A. The reused FM core's
license and compatibility notes are in [AdLib](../adlib/README.md).
