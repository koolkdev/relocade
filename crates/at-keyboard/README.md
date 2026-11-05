# AT keyboard

`connect(&runner, pic.irq(KEYBOARD_IRQ)).await` registers the keyboard activity
and ports (60h and 64h), returning its host `KeyboardInput` sender and
`DeviceConnection`. Add the connection to the session bus. Controller firmware
and keyboard firmware run as two async futures in the existing device activity,
on the execution owner. Physical frontend key positions feed the keyboard;
characters and host layouts do not enter the hardware protocol. BIOS or a guest
interrupt handler translates the bytes into characters.

The controller starts with a ready, enabled keyboard, scan set 2, and command byte
45h: set-1 translation and IRQ1 enabled. Port 64h reports input/output-buffer-full,
command/data and system status. Port callbacks access the registers synchronously;
firmware consumes writes asynchronously. Software must wait for
`INPUT_BUFFER_FULL` to clear before writing either port. Writes while busy are
ignored instead of replacing the accepted byte.

Reading 60h consumes one output byte and lowers IRQ1. It wakes pending transmission,
which fills the register and raises a new edge when the device activity runs.
Unread output is preserved. `read_scan_code(io)` tests `OUTPUT_BUFFER_FULL` and
consumes one available byte. The PIC owns masking, acknowledgement and EOI.

Supported controller commands are command-byte read/write (20h/60h), self/interface
test (AAh/ABh), interface disable/enable (ADh/AEh), and output injection (D2h).
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

Register/translation state, controller firmware, keyboard scanning, keyboard
firmware and scan-code tables have separate modules. They follow the
[IBM PC/AT Technical Reference, March 1986](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/at/6183355_PC_AT_Technical_Reference_Mar86.pdf),
pp. 1-42–1-54 and 4-39–4-57. This implements the keyboard path used by the DOS
session. Mouse/auxiliary commands, scan set 3, A20/reset output-pin control,
electrical serial timing and artificial Shift wrappers around navigation keys
are not implemented. Port 61h remains with the PC speaker device.
