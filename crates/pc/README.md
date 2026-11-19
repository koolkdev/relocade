# PC platform

`Pc::new(runner).await` creates a BIOS-initialized real-mode PC independently of
DOS. Create its machine with `configuration::memory()` and its CPU with
`configuration::cpu()` before starting execution. PC retains the CPU's runner
and assembles local device wiring; Machine owns guest memory and CPU state.
Individual crates implement BIOS services and DMA, PIC, PIT, PS/2 keyboard/mouse,
VGA, speaker and AdLib behavior.

The PC supplies 640 KiB of conventional RAM and identity-backed ROM at F0000–FFFFF.
The selected video card maps its memory windows when it connects.
Initialization installs BIOS video, keyboard
and clock services and enables IRQ0/IRQ1 through the master PIC. The same PIT
drives timer interrupts and the speaker. Devices run on the supplied runner and
share its clock.

The [primary DMA controller](../dma/README.md) supplies channels 0–3 and their
PC/AT page registers. `pc.dma()` gives peripherals access to single-transfer
memory reads, including auto-initialize reloads. The controller is present with
every sound-card selection and requires no audio output. It does not yet supply
16-bit DMA, cascading, or other transfer modes.

AdLib is installed by default. To select hardware explicitly, use:

```rust,ignore
use relocade_pc::{Configuration, Pc, SoundCard, VideoCard};

let pc = Pc::with_configuration(
    runner,
    Configuration {
        sound_card: SoundCard::None,
        video_card: VideoCard::Dispi,
        ..Default::default()
    },
).await?;
```

`VideoCard::Dispi` is the default and currently the only video-card choice.
Selection connects the device and supplies its matching BIOS firmware.
`pc.video_card()` exposes frame delivery and execution-local framebuffer access;
guest I/O remains connected directly to the card's concrete handlers.

`SoundCard::None` leaves the AdLib ports unbound. The built-in PC speaker remains
available, and hardware selection does not depend on attaching host output.
When installed, AdLib is present at address/status port `0x388` and data port
`0x389`, including when platform output is absent. Programs select AdLib/OPL2
for music and can use the PC speaker simultaneously. AdLib detection and timer
status require no BIOS service or PIC IRQ.

`SoundCard::SoundBlaster2` installs the [SB2 card](../sound-blaster/README.md)
at base `0x220` and maps its single OPL2 chip at both `0x228` and `0x388`.
PC holds an optional `sound_card::SoundCard` trait object; `pc.sound_card()`
borrows that common interface. Configuration chooses and initializes the concrete
card. The Sound Blaster card owns its shared FM chip.
DSP detection, single-cycle and auto-initialize DMA playback with IRQ7, direct
DAC output, high-speed playback and timed silence are available. The guest
installs its handler and unmasks IRQ7. OPL3, recording and ADPCM are not implemented.

`Configuration::parallel_port` defaults to `None`. Set it to
`Some(ParallelPort { base: 0x378, irq: 7 })` to install an output-only SPP adapter.
`ParallelPort` is exported by `relocade_pc`; its default selects those values.
IRQ5 is also supported; use it when Sound Blaster occupies IRQ7. The IRQ starts
masked in the PIC. `Configuration::validate()` checks the parallel base/range,
IRQ selection and Sound Blaster IRQ conflict without starting a PC.
`Pc::with_configuration` performs the same validation before installing hardware
and returns `InitializationError::Validation(String)` with a diagnostic identifying
the invalid resource and its rejected value. A base of zero is reserved for absent
BIOS ports, and all three registers must fit in the 16-bit I/O address space.
Other initialization failures retain the underlying error in
`InitializationError::Machine`. `Dos::with_configuration` propagates both categories.

`pc.parallel_port()` retains the port; its `attach` callback
receives each logical output change and an input driver for status and IRQs.
For byte delivery, create a connection while execution is stopped:

```rust,ignore
let port = pc.parallel_port().expect("configured parallel port");
let (input, output) = relocade_parallel::Centronics::connect(pc.runner(), &port).await;
```

The peripheral owns `input`, a `futures_io::AsyncRead` stream. Higher-level
services can write to `output` through `futures_io::AsyncWrite` without changing
port registers. Both paths share device readiness and one pending-byte slot.
The input end exposes `set_status` and optional INIT/control notifications through
`on_control`. Other protocols can use the port's signals directly. There is no
attached device by default, and data writes only change the latch. Firmware
advertises LPT1 in the BIOS data area
at `0040:0008` and in INT 11h's equipment count, even with nothing connected.
Changing its base to `0x278` does not change that logical LPT slot.

The firmware has no disk, serial or printer backend. INT 13h/AH=15h reports no
drive (AH=0, carry clear); AH=08h fails with AH=07h and DL=0. Other commands,
including EDD probes, fail with AH=01h and carry set. AH=01h reads the previous
floppy/hard-disk status from the BIOS data area. No disk transfers occur.
INT 14h returns unchanged. INT 17h supports send, initialize and status through
the configured LPT registers. The live BIOS table resolves each slot to a
configured adapter; a missing adapter or unsupported request returns unchanged.
Sending to an unconnected port reports a timeout without strobing.
The timeout bytes at `0040:0078`–`007A` default to 20. Firmware interprets each
unit as one emulated second; zero checks once. Firmware awaits the adapter's
readiness notification or a single timeout on the PC's clock, admitting
interrupts while waiting. There is no periodic readiness polling.
STROBE and INIT are logical control transitions. The Centronics adapter acknowledges
only after the peripheral reads the captured byte and STROBE is released,
holding ACK active for five emulated microseconds before raising the enabled IRQ.
INT 17h returns the current status after strobing; delivery may still be pending.
INIT drops an unread port transfer and resets the handshake, preserving bytes
already accepted by direct output. Dropping the reader marks the device offline
and fails subsequent direct writes. Session shutdown ends the stream after any
pending byte is drained. Electrical setup/hold timing is not enforced, and these
transport events do not define a print job.
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
consumer; `vga()` and `speaker()` retain devices on the execution owner, and
`sound_card()` borrows the card's common interface. Platform windows, rendering and
audio outputs remain with the embedding application.

With the `audio` feature, `pc.attach_audio(config, samples).await` takes a
`relocade_frontend::audio::Config` and bounded `async_channel::Sender<Vec<f32>>`.
The application translates playback settings into sample rate, channels and a
preferred block size. PC owns scheduling and submission. Devices register their
own sources through `Sources`; Sound Blaster supplies its FM and DAC outputs.
The source collection owns event capture, synthesis state, gains and reusable
mixing scratch space.

One `AudioTiming` defines frame zero, the native tick rate and the output sample
rate. The block generator advances absolute frame ranges. Its deadline is the
rounded timestamp of the block end; sample generation retains positions within
frames. Block sizes are clamped to 1–256 frames. The audio activity captures
elapsed events when a block is due, then submits synthesis and mixing to one
session-owned job thread. Only one block is in flight, returning PCM samples.
Generator state and mixing scratch space stay in shared allocations between jobs;
their locks are taken only by calculations and remain uncontended. The CPU worker
keeps device registers, DMA,
interrupts and event recording, and continues guest execution while awaiting the
result. A full output
queue replaces its oldest block; closing output releases the activity and its
recorders. An accepted job can finish with its owned events after cancellation.
Session shutdown drains accepted work before joining the job thread. The runner
owns the activity and job thread; the application owns playback.

Each source defaults independently to unity gain. Mixing retains headroom and
platform output clamps the final sample to `-1..=1`, so simultaneous peaks can
clip. Installing another source does not change existing source levels.
Attachment is allowed once per PC; AdLib supports 8–192 kHz. Attach before guest
execution to capture the complete note timeline. See the
[AdLib core and license notes](../adlib/README.md) for compatibility limits and
the dependency's distribution requirements.

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
