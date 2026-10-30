# PC timer

`Pit::with_clock` implements channel 0 at the BIOS default divisor of 65,536,
using a 1.193182 MHz input clock. It raises the standard IRQ0 vector 08h about
every 54.9254 ms (18.2 Hz).

Connect it once through `runner.connect(pit, [], [])` and put the result in the
CPU's `IoBus`. The runner drives its timer activity while guest execution runs
or awaits a host service. No device borrow spans a wait. Pending edges coalesce
until acknowledged; late host wakes preserve the timer phase without replaying
missed edges. BIOS time can therefore lose ticks while interrupt delivery is
blocked, as with a pending hardware IRQ.

DOS registers the device automatically. The BIOS IRQ0 handler advances the
32-bit count at 0040:006C and the midnight rollover byte at 0040:0070. INT 1Ah
reads those fields and consumes the rollover count.

Programmable PIT ports, additional channels and PIC routing are follow-up work.
