# PC interrupt controllers

`Pic::initialize(runner).await` creates an execution-owned 8259A pair and
registers master ports 20h–21h and slave ports A0h–A1h. Add the returned connection
to the CPU bus. A peripheral creates its `IrqLine` through `pic.irq(number, initial_high)`.
Its `set_level` models a level transition; `pulse` produces a timer edge. The PIC
alone supplies vectors through the existing machine `Device::poll_interrupt` API.
Both initial masks are FFh. PC setup calls `pic.unmask_irq(TIMER_IRQ)` to enable
the timer; the BIOS IRQ0 handler uses `end_of_interrupt(io, TIMER_IRQ)` to send
EOI through the device bus. Slave IRQ completion acknowledges both controllers.

Connection establishes the physical level without latching an edge or clearing
pending requests. Level-triggered requests reflect the supplied level immediately.
The peripheral owns its starting level and PC IRQ assignment: the PIT connects
IRQ0 at OUT0's level and the keyboard connects IRQ1 low. IRQ2 belongs to the
slave INT output; peripherals use IRQ8–15 for slave inputs. The same line handle
signals subsequent transitions and pulses.

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
pp. 9–17. Each chip uses the same arbitration implementation. The pair owns fixed
PC wiring, cascade selection and CPU wakeups. The slave starts at vector 70h;
IRQ12 produces INT 74h. Guest ICW2/ICW3 can remap vectors and select cascade IDs.
Slave delivery requires unmasking both its line and master IRQ2, and software EOI
on both chips. The master cascade priority blocks other slave interrupts until
master EOI. Poll mode acknowledges only the chip whose command port is read.

A slave request withdrawn after the master latches IRQ2 produces spurious IRQ15
without setting slave ISR7; master EOI is still required. An unselected cascade
ID leaves the vector bus undriven (modeled as FFh). MCS-80 vectors, special fully
nested mode, buffered electrical operation and individual INTA bus cycles are
outside this device. Interrupt acknowledgement is atomic at the machine boundary.
