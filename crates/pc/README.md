# PC platform

`Pc::new(runner).await` creates a BIOS-initialized real-mode PC independently of
DOS. Create its machine with `configuration::memory()` and its CPU with
`configuration::cpu()` before starting execution. PC retains the CPU's runner
and assembles local device wiring; Machine owns guest memory and CPU state.
Individual crates implement BIOS services and PIC, PIT, PS/2 keyboard/mouse,
VGA, speaker and AdLib behavior.

The fixed map contains 640 KiB of conventional RAM, the VGA MMIO aperture and
identity-backed ROM at F0000–FFFFF. Initialization installs BIOS video, keyboard
and clock services and enables IRQ0/IRQ1 through the master PIC. The same PIT
drives timer interrupts and the speaker. Devices run on the supplied runner and
share its clock.

AdLib is present at address/status port `0x388` and data port `0x389`, including
when platform output is absent. Programs select AdLib/OPL2 for music and can use
the PC speaker simultaneously. Sound Blaster DSP/DMA and OPL3 extensions are not
implemented. AdLib detection and timer status require no BIOS service or PIC IRQ.

The firmware has no disk, serial or printer backend. INT 13h/AH=15h reports no
drive (AH=0, carry clear); AH=08h fails with AH=07h and DL=0. Other commands,
including EDD probes, fail with AH=01h and carry set. AH=01h reads the previous
floppy/hard-disk status from the BIOS data area. No disk transfers occur.
INT 14h/17h return unchanged, as IBM BIOS does for a zero COM/LPT base address.
DOS can replace these vectors; unrelated vectors and IVT data pointers are not
filled with a universal fallback.

The caller supplies guest code and CPU entry state through `pc.cpu()`, then runs
it with `pc.run().await`. This starts from initialized firmware
state; reset-ROM execution, POST and disk booting are not implemented.

The x86 execution entries deliver real-mode divide errors (#DE), BOUND violations
(#BR) and invalid opcodes (#UD) through the live IVT, even with IF clear. BIOS
installs ordinary default handlers that return `ExecutionExit::UnhandledException`
to the host. Guest software can replace those vectors or chain to their previous
destinations. On a default-handler exit, the CPU is in the handler and
`real_mode_interrupt::read_frame` reads the original faulting CS:IP and FLAGS
from the guest stack. DOS inherits these defaults and exposes the result through
`RunError::Execution`, leaving the process available for inspection.

JIT blocks, interpreter execution and `Cpu::step` all deliver these faults.
`Cpu::step` returns `InterruptEntered` before executing the handler; `Cpu::run`
continues at the handler. Other faults, protected-mode exception delivery and
relocated interrupt tables remain outside the automatic delivery support.
Failures during exception entry return directly to the host.

`keyboard_input()` feeds physical key events through the controller. `keyboard()`
retains the BIOS keyboard backed by guest RAM for translated/scripted input.
`frames()` provides the capacity-one snapshot receiver for a single display
consumer; `vga()`, `speaker()` and `adlib()`
retain devices on the execution owner. Platform windows, rendering and audio
outputs remain with the embedding application.

With the `audio` feature, `pc.attach_audio(config, samples).await` connects one
CPAL configuration and bounded `async_channel::Sender<Vec<f32>>` to the
[shared mixer](../mixer/README.md). PC supplies its emulation clock and registers
the speaker at gain 0.2 and AdLib at gain 0.8. Their summed range fits within
`-1..=1`; the mixer owns timing, gains, buffering and submission.
The runner owns the activity, and the application owns platform playback.
Attachment is allowed once per PC, at 8–192 kHz. Attach before guest execution to
capture the complete note timeline. See the [AdLib core and license notes](../adlib/README.md)
for compatibility limits and the dependency's distribution requirements.

`mouse_input()` accepts relative motion, three-button transitions and focus/capture
loss through the same execution-owned PS/2 activity. `Dos::mouse_input()` forwards
the handle from its PC. BIOS INT 15h/C2h configures the device, installs a guest
far callback and enables reporting through INT 74h and IRQ2/IRQ12. A raw driver
can instead own the controller and interrupt programming. A resident INT 33h
driver is not installed. Window capture and pointer-lock policy remain with the
embedding application. See the [BIOS mouse interface](../bios/src/mouse/README.md)
for the supported calls and callback stack convention.

DOS builds on this PC through `Dos::from_pc(pc).await`. It initializes its MCB
allocator, character devices, filesystems and INT 20h/21h services while retaining
the CPU and all BIOS/device state. `Dos::new(runner).await` performs both steps.
The generic CPU runtime and device bus remain in `relocade-machine`; `relocade-pc`
composes the PC-specific crates without moving their implementations.
