# Master interrupt controller

`Pic::new(0x08)` creates an execution-owned 8259A master. Connect the cloneable
PIC at `[PORT_RANGE]` (20h–21h), and pass `pic.irq(n)` to peripherals. `IrqLine::set_level`
models a level transition; `pulse` produces a timer edge. The PIC alone supplies
vectors through the existing machine `Device::poll_interrupt` API. Its initial
mask is FFh; DOS enables IRQ0, and the BIOS IRQ0 handler sends EOI.

The controller supports edge and level inputs, IRR/ISR/IMR, fixed and rotating
priorities, specific/non-specific EOI, automatic EOI, special mask mode, polling,
and ICW initialization. Masking retains pending edges. An acknowledged request
blocks equal/lower priorities until EOI; higher priorities may nest. IRQ1 uses
vector 09h with the DOS vector base.

Behavior follows the [Intel 8259A datasheet](https://www.scs.stanford.edu/23wi-cs212/pintos/specs/8259A.pdf),
pp. 9–17. This models a single master in 8086 operation. Standard PC cascade
initialization is accepted, but a slave controller, cascaded acknowledgement,
MCS-80 vectors, electrical INTA timing and spurious-interrupt races are outside
this device. Only the master's eight physical inputs are exposed.
