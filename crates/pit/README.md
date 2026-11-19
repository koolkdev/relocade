# Programmable interval timer

`Counter` models the binary 8254 modes used for periodic IRQ0 and PC speaker tones: mode 2
(rate generator) and mode 3 (square wave), including aliases 6/7, byte access
formats, count/status latches, gate transitions and deferred divisor loading.
`advance` accepts whole input clocks and integrates OUT high time, so audio
sampling can share the hardware's phase arithmetic.

Timing follows the [Intel 8254 datasheet, September 1993](https://www.scs.stanford.edu/25sp-cs212/pintos/specs/8254.pdf),
pp. 5–10, 12–14 and 17. The PC wiring and nominal 1,193,182 Hz input clock are
specified in the [IBM PC/AT Technical Reference, March 1986](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/at/6183355_PC_AT_Technical_Reference_Mar86.pdf),
pp. 1-22–1-23 and 1-40.

This is a discrete input-clock model, not a model of electrical setup/hold races.
A completed initial count loads on the next input clock, without decrementing.
A running mode-2 count changes at the next period; mode 3 changes at the next
half-period. Gate low immediately forces OUT high and stops counting; a rising
gate starts a fresh high phase on the next clock. A zero divisor means 65,536.

Modes 0/1/4/5 and BCD are deferred. Programming them stops the previous oscillator
and returns OUT low. Count 1 is invalid for modes 2/3; loading it stops oscillation
with OUT high. Power-up count/output are undefined by hardware; a default `Counter`
starts with count zero and OUT low. Read counts through latches when stable multi-byte
observations are needed. Reads without latching observe the current counter at
each byte access.

`clocks_at` and `time_at` convert absolute times and input-clock indices using
the shared PC/AT frequency. Deadlines round up, and repeated periods retain
their fractional remainder.

`Pit::initialize(runner, &pic).await` creates the execution-owned
device on the runner's clock, connects its registers and starts its task.
It returns the shared PIT. It owns byte transfers at
channel 0 (0x40), channel 2 (0x42) and control/read-back (0x43). Channel 1 (0x41)
and wider transfers are unsupported: writes are ignored and reads return open bus.
Read-back can latch both supported channels at the same machine time.

Channel 0 starts in BIOS mode 3, gate high, divisor 65,536 already loaded. The PIT
connects IRQ0 at OUT0's starting level without creating an edge. The first
IRQ arrives after a full period (about 54.9254 ms). Guest writes replace that
configuration; pending divisor loads follow the same mode-specific rules as channel 2.

Channel-0 port writes advance the counter to the current clock reading before
changing its configuration. The PIT task waits for the next output transition
or a programming change. On guest time, the task updates the PIC and arms the
next edge in the same poll. With an independently advancing clock, an edge can
already be due on the first poll. The task then updates IRQ0 and yields. If a
late wake spans multiple periods, the PIC retains one pending rising edge.
Unacknowledged edges coalesce in the PIC. No IRQ is generated merely
because an obsolete wait completed. BIOS keeps its handler, tick data, midnight
rollover and INT 1Ah service: it counts delivered IRQ0 interrupts, so changing the
timer frequency also changes BIOS time unless guest software compensates.

The speaker retains the same PIT through `Speaker::initialize(runner, pit)` and owns
port 0x61. Its gate changes and OUT2 reads use the PIT's channel 2.
`observe_channel2` supplies the initial state and every programming or gate write
synchronously, with a timestamp and fractional input-clock phase on `clock()`.
The speaker retains these changes until their PCM interval is rendered. Reads
and latches do not notify or reset playback phase.
