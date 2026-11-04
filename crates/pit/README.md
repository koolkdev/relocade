# Programmable interval timer

`Counter` models the binary 8254 modes used for PC speaker tones: mode 2
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
with OUT high. Power-up count/output are undefined by hardware; this model starts
with count zero and OUT low. Read counts through latches when stable multi-byte
observations are needed. Reads without latching observe the current counter at
each byte access.

`clocks_at` and `time_at` convert absolute times and input-clock indices using
the shared PC/AT frequency. Deadlines round up, and repeated periods retain
their fractional remainder.

`connect_channel0(&runner).await` registers the default-rate channel-0 timer with the
machine runner. It raises IRQ0 (vector 08h) every 65,536 input clocks and
coalesces unacknowledged edges. DOS connects this device directly; BIOS keeps
the IRQ0 handler, tick data, midnight rollover and INT 1Ah time-of-day service.
Programmable channel-0 ports are not implemented yet. Channel 2 uses `Counter`
in the PC speaker device.
