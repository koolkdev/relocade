# PS/2 controller and keyboard

The compatibility boundary is the PC/AT software interface: scan bytes, port
status and command replies, IRQ1/IRQ12, repeat timing and buffer overrun. Rust async
services implement waits for commands, input, output space and clock deadlines.
Correctness does not depend on a scheduler turn between keyboard events.

`connect(&runner, &pic).await` registers the keyboard activity
and ports (60h and 64h), returning its host `KeyboardInput` sender and
`DeviceConnection`. The controller connects IRQ1 low until
an output byte is ready. Add the connection to the session bus. Controller firmware
and keyboard firmware run as two async futures in the existing device activity,
on the execution owner. Physical frontend key positions feed the keyboard;
characters and host layouts do not enter the hardware protocol. BIOS or a guest
interrupt handler translates the bytes into characters.

The controller starts with a ready, enabled keyboard, a disabled auxiliary port,
scan set 2, and command byte 65h: set-1 translation and IRQ1 enabled. Port 64h
reports input/output-buffer-full,
command/data and system status. Port callbacks access the registers synchronously;
firmware consumes writes asynchronously. Software must wait for
`INPUT_BUFFER_FULL` to clear before writing either port. Writes while busy are
ignored instead of replacing the accepted byte.

Reading 60h clears `OUTPUT_BUFFER_FULL` and lowers the corresponding IRQ. The data
latch retains its value until firmware writes another byte, so another read can return the same
value with the full flag clear. This matters when a guest IRQ1 handler reads the
register and then chains to BIOS. Reads wake pending transmission, which fills the
register and raises a new edge when the device activity runs. Unread output is
preserved. `read_scan_code(io)` consumes a keyboard byte while leaving auxiliary
output for its own interrupt handler. The PIC owns masking, acknowledgement and
EOI. The register and flag behavior
follows Intel's [UPI-41A/41AH/42/42AH User's Manual](https://www.ceibo.com/eng/datasheets/Intel-8041-Manual.pdf),
pp. 20–21 and 56–57.

Supported controller commands are command-byte read/write (20h/60h), self/interface
test (AAh/ABh/A9h), interface disable/enable (ADh/AEh/A7h/A8h), output injection
(D2h/D3h), and auxiliary command routing (D4h).
Controller replies wait for an empty output register.
Keyboard commands include LEDs, echo, scan sets 1/2, ID, typematic, enable,
default-disable, defaults, resend and reset. Commands and parameters return ACK;
unsupported keyboard commands/scan sets return RESEND. Set-2 translation also
applies to keyboard replies, including the identification and scan-set bytes.
Command exchanges await their parameters directly. A new command replaces an
unfinished exchange or untransmitted reply. RESEND retries the initial ACK while
the keyboard keeps waiting for the option byte. Reset applies defaults after its ACK
is transmitted. Option and ID exchanges pause scanning, then rescan held keys so
releases during the wait cannot leave modifiers stuck in the guest.

Make/break sequences include extended keys, Print Screen and Pause. Modifier
snapshots reconcile keys already held at attachment; focus loss emits releases.
The emulated clock supplies typematic at the programmed rate and delay; host
repeat events are ignored. Delayed or inhibited transmission coalesces missed
repeats. The keyboard buffers up to sixteen scan bytes, admits complete
sequences, and reports overrun when full. Command replies take priority over queued
scan data and wait for controller output space. Closing host input releases held
keys while the firmware continues serving guest commands.

The frontend event channel transports host input independently of the emulated
keyboard buffer. Physical transitions continue while controller output is full;
an unread byte stays intact and further input can overflow the scan buffer.
Guest reads and interface commands control when transmission can resume.

The controller owns its registers, translation and command handling in one module.
The keyboard owns scanning, repeat timing and async command exchanges in another;
scan-code tables remain separate. They follow the
[IBM PC/AT Technical Reference, March 1986](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/at/6183355_PC_AT_Technical_Reference_Mar86.pdf),
pp. 1-42–1-54 and 4-39–4-57. This implements the keyboard path used by the DOS
session. Mouse firmware, scan set 3, A20/reset output-pin control,
electrical serial timing and artificial Shift wrappers around navigation keys
are not implemented. Port 61h remains with the PC speaker device.

The shared 8042 owner tags its single output register with the originating port.
Status bit 5 identifies auxiliary bytes, which bypass keyboard translation and
assert IRQ12 when command-byte bit 1 enables it. Keyboard bytes assert IRQ1 under
bit 0. Interface inhibit bits 4/5 and A7h/A8h control the ports independently.
D4h routes the next data write to the auxiliary port; D2h/D3h inject tagged output,
and A9h tests the auxiliary interface. Controller replies use the keyboard source.

Device tasks use the same `Port`-indexed command and output operations.
The controller alternates waiting, enabled sources at byte boundaries; cancelled
transmissions surrender their turn. Packet assembly remains with each device.

Only attached devices have command queues. Writes to an empty port produce no
reply and leave the controller free to service the other port.
