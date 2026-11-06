# PC platform

`Pc::new(runner).await` creates a BIOS-initialized real-mode PC independently of
DOS. It owns the CPU, memory map and device wiring, while the individual crates
implement BIOS services and PIC, PIT, AT keyboard, VGA and speaker behavior.

The fixed map contains 640 KiB of conventional RAM, the VGA MMIO aperture and
identity-backed ROM at F0000–FFFFF. Initialization installs BIOS video, keyboard
and clock services and enables IRQ0/IRQ1 through the master PIC. The same PIT
drives timer interrupts and the speaker. Devices run on the supplied runner and
share its clock.

The caller supplies guest code and CPU entry state through `pc.cpu()`, then runs
it with `runner.run_cpu(pc.cpu()).await`. This starts from initialized firmware
state; reset-ROM execution, POST and disk booting are not implemented.

`keyboard_input()` feeds physical key events through the controller. `keyboard()`
retains the BIOS keyboard backed by guest RAM for translated/scripted input.
`frame_source()` provides immutable display captures; `vga()` and `speaker()`
retain devices on the execution owner. Platform windows, rendering and audio
outputs remain with the embedding application.

DOS builds on this PC through `Dos::from_pc(pc).await`. It initializes its MCB
allocator, character devices, filesystems and INT 20h/21h services while retaining
the CPU and all BIOS/device state. `Dos::new(runner).await` performs both steps.
The generic CPU runtime and device bus remain in `relocade-machine`; `relocade-pc`
composes the PC-specific crates without moving their implementations.
