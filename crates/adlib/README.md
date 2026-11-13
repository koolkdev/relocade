# relocade-adlib

AdLib OPL2 synthesis, independent of CPAL and the application's playback lifetime.
`Opl2` accepts byte register writes and produces mono `f32` samples at the selected
output rate (8–192 kHz). It uses the existing core's resampler, keeps the second
OPL3 register bank inaccessible, and implements the YM3812 waveform-enable bit.

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
