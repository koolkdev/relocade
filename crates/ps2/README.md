# PS/2 controller, keyboard and mouse

The controller models the PC/AT port registers, scan bytes, command replies,
IRQ1/IRQ12, repeat timing and buffer overrun. The session schedules firmware
when the guest writes a port, frees the output latch or reaches a device
deadline. Host keyboard and mouse input arrives over the frontend channels.

`connect(&runner, &pic).await` registers the input receivers and ports
(60h and 64h), returning `Inputs { keyboard, mouse }` and an execution-owned
`Rc<Controller>`. Firmware can await
`wait_input_ready()` or `wait_auxiliary_output()` on that controller; these
notifications do not consume bytes or replace the guest port interface.
The controller connects IRQ1 and IRQ12 low until an output byte is ready. Its
registers are already installed on `runner.io()` when connection returns.
One connection scope installs four operations: data read/write, status read and
command write. Each callback captures the shared controller directly and handles
one register operation.
The keyboard maps physical key positions to scan codes. BIOS or a guest
interrupt handler translates those codes into characters.

The controller starts with a ready, enabled keyboard, a disabled auxiliary port,
scan set 2, and command byte 65h: set-1 translation and IRQ1 enabled. Port 64h
reports input/output-buffer-full, command/data and system status. Port callbacks access the registers synchronously.
The firmware consumes each accepted write on a scheduled turn. Software must wait for
`INPUT_BUFFER_FULL` to clear before writing either port. Writes while busy are
ignored instead of replacing the accepted byte.

Reading 60h clears `OUTPUT_BUFFER_FULL` and lowers the corresponding IRQ. The data
latch retains its value until firmware writes another byte, so another read can
return the same value with the full flag clear. This matters when a guest IRQ1 handler reads the
register and then chains to BIOS. Reading a full register schedules its refill
when another byte is ready. Publishing the next byte raises a new IRQ edge. Unread output is
preserved. `read_keyboard_latch(io)` reads the retained latch for IRQ1 even after
a guest hook has cleared OBF, leaving unread auxiliary output for IRQ12. Polling
code checks `OUTPUT_BUFFER_FULL` to distinguish new output from retained data.
The PIC owns masking, acknowledgement and EOI. The register and flag behavior
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
The keyboard remembers which command is waiting for a parameter. A new command replaces an
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
The session accepts at most one event per input source on each turn, before
dispatching alarms. A backlog leaves time for guest work. On the same controller turn, an accepted port command takes precedence
over new input. A key release cancels a repeat due at that instant. The mouse
can publish a due packet header before new motion changes its counters.

The controller owns its registers, translation and command handling in one module.
The keyboard owns scanning, repeat timing and command exchanges in another;
scan-code tables remain separate. They follow the
[IBM PC/AT Technical Reference, March 1986](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/at/6183355_PC_AT_Technical_Reference_Mar86.pdf),
pp. 1-42–1-54 and 4-39–4-57. This implements the keyboard path used by the DOS
session. Scan set 3, A20/reset output-pin control,
electrical serial timing and artificial Shift wrappers around navigation keys
are not implemented. Port 61h remains with the PC speaker device.

The shared 8042 owner tags its single output register with the originating port.
Status bit 5 identifies auxiliary bytes, which bypass keyboard translation and
assert IRQ12 when command-byte bit 1 enables it. Keyboard bytes assert IRQ1 under
bit 0. Interface inhibit bits 4/5 and A7h/A8h control the ports independently.
D4h routes the next data write to the auxiliary port; D2h/D3h inject tagged output,
and A9h tests the auxiliary interface. Controller replies use the keyboard source.

The controller alternates between enabled devices with pending bytes at each
byte boundary. Each device commits a byte when the controller accepts it and
owns the rest of its packet. A command sent to an empty port leaves the
controller free to service the other port.


## Mouse

`Inputs::mouse` accepts owned relative `MouseEvent` values. Positive host Y points
down; PS/2 Y points up. One host unit becomes one count at the default resolution
of four counts/mm. Fractional counts survive packet delivery, including remote
reads. Resolution changes scale subsequent input. Non-finite movement is ignored;
large movement saturates the signed nine-bit counters and sets overflow flags.
Only left, right and middle buttons are reported. Wheel/extended device IDs are
not implemented.

The mouse starts in stream mode with reporting disabled, 100 samples/second,
1:1 scaling and device ID 00h. It implements reset/BAT, ID, enable/disable,
defaults, sample rate, resolution, status, 2:1 scaling, remote Read Data, wrap and
resend. Commands take priority over unsent report bytes; a byte already in the
controller latch remains readable. Software should disable stream reporting before
command exchanges. Reset finishes BAT/ID before accepting further commands.
Resend repeats the last completed payload, without its ACK, then resumes a
pending motion report. Untransmitted samples retain their unscaled counters until
the report completes, so Read Data can still include them. Read Data reports
current buttons and unscaled counters. Protocol behavior follows the
[IBM mouse technical reference](https://ftp.infania.net/pub/DOS%20Drivers/IBM%20PS1/Section%206.%20Mouse.pdf),
pp. 6-3–6-11, extended to the standard three-button packet.

Stream sampling follows the button latches described on page 6-9 of the IBM
reference: a press within an interval is reported even if it was released before
the sample. Current button state then supplies the release in a following report.
Multiple clicks while output is blocked coalesce into these fixed-size latches;
there is no queue of historical button transitions. Motion accumulates in the
bounded counters independently of button changes. Remote Read Data reports only
the current button state. Focus/capture loss and input-channel closure clear held
buttons. A report remains live while inhibited or waiting for controller space;
its sample freezes when the header is published. A packet whose header is already
published completes before subsequent input is sampled.

Device-side buffering consists of one pending packet, the last completed payload,
and current counters/button latches. Resend uses a cursor into the last payload.
The pending packet captures its bytes and unscaled sample when the header is
published; until then it reads the live counters directly. Each payload is at most
three bytes. This bounds firmware history; the frontend input channel retains its
existing asynchronous delivery semantics. There is no
periodic idle output. Late sampling emits one report and starts a new interval at
header publication instead of generating missed samples in a burst. Guest
commands clear accumulated motion as specified; returning from a configuration
exchange reconciles buttons already reported to the guest.

The controller schedules the next mouse sample or keyboard repeat on the session clock.
The mouse protocol module owns commands and sample deadlines. The transmission module keeps each
packet's bytes, cursor and state together. One flat packet state distinguishes
replies, errors, reset, pending stream/Read Data sampling, and captured motion.
It handles completion and preserves the raw sample for interrupted Read Data or
resend. The motion module owns fractional conversion, counters and button latches.
The shared 8042 and PIC continue to own port backpressure, byte-source IRQs and
interrupt arbitration.

Each device consumes its live input channel in order at its current device time.
The firmware yields after each event to bound the work per poll. Key transitions
enter the keyboard's 16-byte scan buffer and can overrun it while output is
blocked. Mouse motion and buttons accumulate until the programmed sample.
Closing a channel releases its held keys or buttons.
