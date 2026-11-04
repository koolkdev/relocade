# Master interrupt controller

`Pic::initialize(runner).await` creates an execution-owned 8259A master and
registers ports 20h–21h. Add the returned connection to the CPU bus, and pass
`pic.irq(n)` from the shared controller handle to peripherals. `IrqLine::set_level`
models a level transition; `pulse` produces a timer edge. The PIC alone supplies
vectors through the existing machine `Device::poll_interrupt` API. Its initial
mask is FFh. DOS calls `pic.unmask_irq(TIMER_IRQ)` to enable the timer; the BIOS
IRQ0 handler uses `end_of_interrupt(io)` to send EOI through the device bus.

Peripherals use `IrqLine::set_initial_level` when wiring their starting output.
This establishes the physical level without latching an edge or clearing pending
edges. Later output changes use `set_level`; level-triggered requests follow the
input in either case.

The controller supports edge and level inputs, IRR/ISR/IMR, fixed and rotating
priorities, specific/non-specific EOI, automatic EOI, special mask mode, polling,
and ICW initialization. Masking retains pending edges. An acknowledged request
blocks equal/lower priorities until EOI; higher priorities may nest. IRQ1 uses
vector 09h with the DOS vector base.

Port accesses and interrupt acknowledgement update this register state directly.
The PIC has no timed activity of its own; peripherals provide the IRQ transitions.
Polling, rotation and special masking reuse the same request/service registers and
priority decision.

Behavior follows the [Intel 8259A datasheet](https://www.scs.stanford.edu/23wi-cs212/pintos/specs/8259A.pdf),
pp. 9–17. This models a single master in 8086 operation. Standard PC cascade
initialization is accepted, but a slave controller, cascaded acknowledgement,
MCS-80 vectors, electrical INTA timing and spurious-interrupt races are outside
this device. Only the master's eight physical inputs are exposed.
