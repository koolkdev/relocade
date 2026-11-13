# relocade-adlib

AdLib OPL2 synthesis, independent of CPAL and the application's playback lifetime.
`Opl2` accepts byte register writes and produces mono `f32` samples at the selected
output rate (8–192 kHz). It uses the existing core's resampler, keeps the second
OPL3 register bank inaccessible, and implements the YM3812 waveform-enable bit.

`Adlib::initialize` connects byte-only address/status port `0x388` and data port
`0x389` to a runner. Timer 1 ticks every 80 microseconds, timer 2 every 320. Both
reload on overflow, latch their status flags until cleared, and retain their
counting phase while masked. Reads return the OPL2 identification bits (`0x06`).
The data port is write-only; neighboring OPL3 ports remain unmapped.

Timers follow the shared emulation clock and update on register accesses, even
without an audio source. There is no timer task, host sleep, PIC connection or
BIOS service. Yamaha's application manual sections 3-1-2 and 3-5 describe the
timer controls and status flags.

## Clocked output

Create `Adlib::audio_source(config.sample_rate)` before guest execution and register
`move |interval| source.sample(interval)` with the shared mixer. The source records
timestamped register writes and applies each after synthesizing the preceding
elapsed time. It retains resampler and fractional clock phase across mixer polls;
the core takes effect at output-sample boundaries. Samples already rendered are
never changed by a later write. Intervals must move forward, span at least one
nanosecond and end no later than the current emulation time.

The mixer must continue polling when muted or when its output channel is full,
as it does for the PC speaker. This consumes pending writes without tying chip
progress to platform playback. Dropping the source releases its recording queue;
the device keeps its registers and timer status without retaining further writes.
A new source replaces the previous recording subscription and starts any current
voices with fresh envelopes. Timer reads never advance the synthesis core.

The synthesis dependency is [nuked-opl3 0.1.0](https://github.com/tgies/nuked-opl3-rs),
a pure Rust port of Nuked-OPL3 1.8. Default features are disabled; no C compiler,
platform audio backend or additional thread is needed. Both native and browser
CI include this crate.

The core supports nine two-operator voices, rhythm instruments, envelopes,
waveforms, tremolo and vibrato in OPL2 compatibility mode. It emulates the YMF262;
it is not an exact model of the YM3812's analog output. YM3812 CSM speech synthesis
is not implemented.

The [Yamaha YM3812 application manual](https://archive.org/details/YamahaYM3812ApplicationManual)
documents the waveform control in sections 3-1-1 and 3-1-11.

## Dependency license

Relocade's integration code is MIT licensed. `nuked-opl3` is **LGPL-2.1-or-later**,
copyright Nuke.YKT (original core) and Tony Gies (Rust port). Its license and source
are included in the pinned [crate archive](https://crates.io/crates/nuked-opl3/0.1.0)
and [upstream repository](https://github.com/tgies/nuked-opl3-rs).

Distributing native or Wasm binaries that include this dependency must preserve
its notices, provide the corresponding library source and satisfy the LGPL's
replacement/relinking requirements. An MIT license on the surrounding project
does not remove those requirements. The core is consumed without modifications.
