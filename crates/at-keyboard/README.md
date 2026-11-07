# AT keyboard

The compatibility boundary is the PC/AT software interface: scan bytes, port
status and command replies, IRQ1, repeat timing and buffer overrun. Rust async
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

The controller starts with a ready, enabled keyboard, scan set 2, and command byte
45h: set-1 translation and IRQ1 enabled. Port 64h reports input/output-buffer-full,
command/data and system status. Port callbacks access the registers synchronously;
firmware consumes writes asynchronously. Software must wait for
`INPUT_BUFFER_FULL` to clear before writing either port. Writes while busy are
ignored instead of replacing the accepted byte.
`write_controller_command` waits for input space and command acceptance through
`IoBus::wait_port`. Status subscriptions wake on input consumption, output
production, reads and command changes; polling does not drive the firmware.

Reading 60h clears `OUTPUT_BUFFER_FULL` and lowers IRQ1. The data latch retains
its value until firmware writes another byte, so another read can return the same
value with the full flag clear. This matters when a guest IRQ1 handler reads the
register and then chains to BIOS. Reads wake pending transmission, which fills the
register and raises a new edge when the device activity runs. Unread output is
preserved. `read_scan_code(io)` tests `OUTPUT_BUFFER_FULL` and consumes one available
byte. The PIC owns masking, acknowledgement and EOI. The register and flag behavior
follows Intel's [UPI-41A/41AH/42/42AH User's Manual](https://www.ceibo.com/eng/datasheets/Intel-8041-Manual.pdf),
pp. 20–21 and 56–57.

Keyboard bytes use the runner's clock with a nominal 12.5 kHz serial clock:
eleven bits take 880 microseconds. Every raw byte takes a frame, including F0
break prefixes consumed by set-2 translation. Host commands and their parameters
also take a frame before reaching the keyboard, holding off keyboard output
during that transfer. IBF clearing means the controller accepted a write; the
keyboard response follows its separate wire transfer. Reading the output register
makes space for the next frame; it does not publish the next byte immediately. The
controller retains the active byte and deadline across unrelated input and
canceled Rust waits. Interface inhibition restarts any frame not yet published
to OBF after release, and host keyboard commands take priority over that output.
This byte-boundary approximation includes elapsed but unpublished frames; it
does not implement the IBM wire protocol's tenth-clock cutoff for interruption.
The normal runner clock uses elapsed host time; device activity publishes
completed transfers when it is polled. Electrical bit sampling and CPU-cycle
timing are outside this model.

Supported controller commands are command-byte read/write (20h/60h), self/interface
test (AAh/ABh), interface disable/enable (ADh/AEh), and output injection (D2h).
Controller replies wait for an empty output register while firmware continues
accepting commands and their parameters. Interface disable/enable can therefore
complete before BIOS drains an unread scan. The model retains one pending reply;
a later reply-producing command replaces that pending byte, preserving the
unread output latch. This replacement rule is the emulator's policy for overlapping
replies; the hardware reference does not specify a reply queue.
Keyboard commands include LEDs, echo, scan sets 1/2, ID, typematic, enable,
default-disable, defaults, resend and reset. Commands and parameters return ACK;
unsupported keyboard commands/scan sets return RESEND. Set-2 translation also
applies to keyboard replies, including the identification and scan-set bytes.
Command exchanges await their parameters directly. A new command replaces an
unfinished exchange or untransmitted reply. RESEND retries the initial ACK while
the keyboard keeps waiting for the option byte. Option and ID exchanges pause
scanning, then rescan held keys so releases during the wait cannot leave
modifiers stuck in the guest.

Reset waits for its ACK to be read and the interface to remain released for
500 microseconds before applying defaults and starting the basic assurance test
(BAT). Another command can replace reset before that acceptance. During BAT,
commands are ignored and physical state is tracked without emitting scans.
The nominal test duration is 400 milliseconds, within the IBM 101/102-key
keyboard's documented 300–500 millisecond interval (Technical Reference,
pages 4-39 and 4-42). Completion sends AAh through the timed output path, then
scanning resumes with any keys still held. The session starts after keyboard
power-on testing; it does not simulate the power supply's startup interval.

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
session. Mouse/auxiliary commands, scan set 3, A20/reset output-pin control,
electrical serial timing and artificial Shift wrappers around navigation keys
are not implemented. Port 61h remains with the PC speaker device.
