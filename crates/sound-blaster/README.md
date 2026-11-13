# Sound Blaster 2.0

`relocade-sound-blaster` implements an SB2 playback subset on the execution
worker. The fixed resource profile is base `0x220`, IRQ 7, DMA channel 1 and
DSP version 2.01. PC selects it with `SoundCard::SoundBlaster2`; AdLib remains
the default and the built-in speaker is independent.

The card owns one `relocade-adlib` instance. Ports `0x228`/`0x229` and
`0x388`/`0x389` share its register latch, timer state and audio source. Resetting
the DSP leaves FM untouched. SB2 has no programmable mixer chip or OPL3.

Named byte-only DSP ports implement reset, data, read-buffer status and
write-buffer status. Releasing reset starts a 100 microsecond initialization
interval on the shared clock, followed by the `0xaa` response. Command `0xe1`
returns version 2.01. Reads with no response return `0xff`; unsupported commands
are ignored. DMA playback and digital output are separate implementation parts.

Recording, ADPCM, MIDI and optional C/MS chips are outside this playback subset.
The implementation follows Creative's [Sound Blaster Hardware Programming
Guide](https://pdos.csail.mit.edu/6.828/2018/readings/hardware/SoundBlaster.pdf),
chapters 2, 3 and 6 and the SB2 port map in appendix A. The reused FM core's
license and compatibility notes are in [AdLib](../adlib/README.md).
