# PC speaker

Emulates PIT channel 2 and the PC speaker control port. Counter behavior and
input-clock arithmetic are shared with [relocade-pit](../pit/README.md), which
also owns the default-rate channel-0 IRQ scheduler. BIOS keeps its IRQ handler
and time-of-day data.

`Speaker::with_clock(Rc<C>)` implements the machine `Device` interface. Connect
it with `runner.connect(speaker, [], relocade_pc_speaker::PORTS)`. Mutable state and
port reads stay with execution; elapsed time uses the session clock's absolute
origin, preserving fractional PIT clocks across repeated accesses.

Only byte transfers at PIT `CHANNEL_2_PORT` (0x42, count), PIT `CONTROL_PORT`
(0x43, control/read-back), and `SPEAKER_CONTROL_PORT` (0x61) are supported.
Word/dword writes are ignored and reads return open bus. Commands
for other PIT channels and read-back commands with reserved bit 0 set are ignored.
Port 0x61 stores bits 0/1 and reads raw OUT2 in bit 5; other bits read zero and
their writes have no effect. Bit 0 is GATE2. The speaker drive is bit 1 AND OUT2,
including direct bit-1 toggling while GATE2 is low. Other system-control and
refresh/parity functions of this port are outside this crate's initial scope.
