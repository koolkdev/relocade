# AT keyboard

`connect(&runner, pic.irq(KEYBOARD_IRQ)).await` registers the keyboard activity
and ports (60h and 64h), returning its host `KeyboardInput` sender and
`DeviceConnection`. Add the connection to the session bus. The activity consumes
physical frontend key positions;
characters and host layouts do not enter the hardware protocol. BIOS or a guest
interrupt handler translates the bytes into characters.

The controller starts with a ready, enabled keyboard, scan set 2, and command byte
45h: set-1 translation and IRQ1 enabled. Port 64h reports output-buffer-full,
command/data and system status. Input commands complete synchronously. Reading
60h consumes one output byte, lowers IRQ1, and loads the next buffered byte with a
new rising edge. `read_scan_code(io)` exposes this operation to firmware. The PIC
owns masking, acknowledgement and EOI.

Supported controller commands are command-byte read/write (20h/60h), self/interface
test (AAh/ABh), interface disable/enable (ADh/AEh), and output injection (D2h).
Issue output-producing controller commands with an empty output register.
Keyboard commands include LEDs, echo, scan sets 1/2, ID, typematic, enable,
default-disable, defaults, resend and reset. Commands and parameters return ACK;
unsupported keyboard commands/scan sets return RESEND. Set-2 translation also
applies to keyboard replies, including the identification and scan-set bytes.

Make/break sequences include extended keys, Print Screen and Pause. Modifier
snapshots reconcile keys already held at attachment; focus loss emits releases.
The emulated clock supplies typematic at the programmed rate and delay; host
repeat events are ignored. Delayed or inhibited transmission coalesces missed
repeats. The keyboard buffers up to sixteen scan bytes, admits complete
sequences, and reports overrun
when full. Command replies take priority over queued scan data.

The separate keyboard, scan-code and controller modules follow the
[IBM PC/AT Technical Reference, March 1986](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/at/6183355_PC_AT_Technical_Reference_Mar86.pdf),
pp. 1-42–1-54 and 4-39–4-57. This implements the keyboard path used by the DOS
session. Mouse/auxiliary commands, scan set 3, A20/reset output-pin control,
electrical serial timing and artificial Shift wrappers around navigation keys
are not implemented. Port 61h remains with the PC speaker device.
