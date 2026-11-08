# BIOS pointing-device services

INT 15h/AH=C2h exposes the PC's existing three-button PS/2 device. The equipment
word at 0040:0010 has bit 2 set. A guest can use this BIOS interface or own the
controller ports and IRQ12 directly. This is a packet interface; INT 33h position,
button counters and cursor drawing belong to a DOS mouse driver.

| AL | Input | Result on success |
| --- | --- | --- |
| 00h | BH=0 disable, BH=1 enable | Reporting changes; enable requires a callback and unmasks IRQ2/IRQ12 |
| 01h | Reset | BH=device ID 00h, BL=AAh; disabled, 100 samples/s, resolution 2, scaling 1:1 |
| 02h | BH=0..6 | Sample rate 10, 20, 40, 60, 80, 100 or 200 samples/s |
| 03h | BH=0..3 | Resolution 1, 2, 4 or 8 counts/mm |
| 04h | Identify | BH=device ID 00h |
| 05h | BH=3 | Initialize the three-byte interface, reset device and remove the callback |
| 06h | BH=0 status, 1 scaling 1:1, 2 scaling 2:1 | Status returns BL=device status, CL=resolution, DL=sample rate |
| 07h | ES:BX=far procedure | Register or replace the callback; 0000:0000 disables reporting and removes it |

Success returns AH=0 and CF clear. Errors set CF and return AH=1 for an invalid
function, 2 for invalid input, 3 for an interface error, 4 for a device resend,
or 5 when enable has no callback. Initialization accepts only the standard
three-byte packet: other sizes 1..8 return interface error; 0 and values above
8 return invalid input. Other INT 15h functions return AH=86h with carry set.
Only documented results and saved CF change; the caller's saved IF is retained.

## Callback convention

IRQ12 at the default INT 74h consumes one auxiliary byte per interrupt. Three
bytes form a packet. At callback entry, the guest stack contains:

| Offset from SP | Word |
| --- | --- |
| +0 | Far return IP |
| +2 | Far return CS |
| +4 | Z, always zero |
| +6 | Y byte, zero-extended |
| +8 | X byte, zero-extended |
| +10 | Status byte, zero-extended |

Status carries signs, overflow and button bits, including the middle button.
X/Y are raw bytes, not sign-extended deltas. The handler must return with `RETF`
without popping arguments and must balance its own stack. BIOS removes the four
argument words and preserves general-purpose registers and DS/ES/FS/GS. The
ordinary IRQ IRET restores interrupted CS:IP and FLAGS. Live IF is enabled while
the callback runs; normal CPU IF and PIC masks govern device interrupt entry.

The BIOS can await callbacks during BIOS INT 16h and DOS console reads. A callback
can itself call BIOS services. An outer suspended read cannot consume a ready key
until the callback returns. IRQ12 stays in service until that return, after which
BIOS sends slave then master EOI. Cancellation or session teardown does not restore
saved registers or acknowledge interrupts from a destructor.

## Ownership and implementation

`Mouse` owns BIOS state and the callback address. Its services use the existing
`#[interrupt]` adapters and `interrupt_dispatch!`. BIOS errors stay typed until
these adapters write AH. The shared `relocade_ps2::protocol` constants name wire
commands, replies and controller flags for both firmware and devices.
`Interface` owns the bounded three-byte packet assembly and controller exchanges.
Commands discard a partial
packet, briefly inhibit auxiliary output, and await controller readiness while IRQ1
continues to service keyboard bytes. Cascade and IRQ12 are masked together during
the exchange and their prior masks are restored afterward. A blocked exchange
returns interface error after 100 ms per input/output wait on the session clock.
The wait races controller notifications with the deadline; it never self-wakes
while idle. This timeout does not simulate electrical PS/2 timing.

BIOS owns the normal controller policy: translated keyboard with IRQ1 enabled,
and auxiliary output/IRQ enable consistent with BIOS reporting state. Device
resolution, rate, scaling and status are shared with raw device commands. BIOS
calls do not preserve arbitrary raw controller programming or a raw driver's
reporting ownership; such a driver should also own IRQ delivery.

The callback calls `call.call_far(target, &[status, x, y, 0]).await` directly,
inside the existing `with_saved_registers` scope. The shared far-call entry checks
the complete frame and saves the return address. Its Rust adapter writes arguments
directly to the guest stack before execution and discards them after normal RETF.
Calls reuse interrupt dispatch's suspended-call stack and resume entries.
BIOS needs no scratch-register convention or ROM trampoline. `Mouse::new` retains
the connected PS/2 controller and session clock; `register_interrupts` accepts
that resource optionally for machines without a pointing device.

The BIOS ABI follows the [IBM PS/2 and PC BIOS Interface Technical Reference,
April 1987](https://ftpmirror.your.org/pub/misc/bitsavers/pdf/ibm/pc/ps2/PS2_and_PC_BIOS_Interface_Technical_Reference_Apr87.pdf),
INT 15h/C2h, printed pages 2-97–2-101. The three-button extension follows the
existing PS/2 device. `dos/tests/mouse_bios.rs` covers the production PC/DOS wiring,
including host samples, raw configuration, callback arguments, keyboard waits,
PIC/IF gates, timeout recovery and abandonment. Machine runtime-call tests own
far-call fault/retry, cancellation, nesting and browser-backend coverage.
