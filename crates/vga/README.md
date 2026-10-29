# VGA text display

Register storage and port access live in `src/registers/`. CRTC indices, write
protection and cursor fields belong to `crtc.rs`; sequencer reset and clocking
registers belong to `sequencer.rs`. Miscellaneous Output selects color/monochrome
CRTC ports and enables CPU access to video RAM. Unsupported registers read FF
and ignore writes. The public `registers` module gives firmware the same port
addresses and register names used by the device.

The shared preset in `registers/text_mode.rs` documents the initial register
bytes alongside their decoded values: 100 eight-dot character clocks per line,
449 lines per frame, and 640 by 400 active dots. The fixed eight-row text font is double-scanned.

`Vga` owns mutable video memory and hardware registers on the execution worker
and implements `relocade_machine::Device`. System RAM contains BIOS
bookkeeping, not the framebuffer. The board maps `B8000..BFFFF` as MMIO and connects
the same device to that aperture and CRTC ports `3D4/3D5`.

The initial supported mode is 80 by 25 characters with fixed 8-by-8 CP437 glyphs,
PC colors, attribute blinking and a hardware cursor. The color text aperture
provides 32 KiB, enough for eight BIOS pages. Odd/even CPU bytes select character
and attribute planes. Scanout uses the full 16-bit CRTC cell address into two
64-KiB logical planes, including starts within a page and wraparound at FFFF.

CRTC indices 0A/0B control cursor shape, visibility and skew; 0C/0D control display
start; 0E/0F control cursor location. Byte and paired index/data port writes work.
The timing and clock-control registers retain their programmed values. Raster
progression, programmable graphics modes, font loading and palette programming
are not implemented yet.

BIOS services maintain their BDA mode, page and per-page cursor data and program
these registers. Direct guest port writes affect the device without updating the
BDA. Direct guest writes, BIOS output and DOS output all reach the same VRAM.

Retain an `Rc<RefCell<Vga>>` on the execution worker, independently of the CPU's
current run. `screen()` copies VRAM and registers into an owned `TextScreen`.
Release the device borrow before polling execution or awaiting other work.

```rust,ignore
use relocade_vga::TextRenderer;

// On the execution worker, including while the CPU awaits input:
let screen = dos.screen();
// Transfer this owned screen to the renderer worker.
let mut text = TextRenderer::new();
let rgba = text.render(&screen, elapsed);
renderer.render([TextRenderer::WIDTH, TextRenderer::HEIGHT], rgba, &output_view)?;
```

The snapshot stays stable across later device writes. Device callbacks borrow
mutable state only for the synchronous transfer; BIOS operations spanning several
transfers may be observed between them. The live device cannot cross threads.

The renderer worker can own the wgpu device, queue, surface and presentation loop.
The embedding supplies platform surface setup, transfers owned screen snapshots,
and forwards resize and lifecycle messages. These crates do not create an
application event loop.

The shared [worker integration check](../../tests/browser/src/vga.rs) exercises
guest MMIO and CRTC writes, snapshot delivery while input is pending, and snapshots
that remain readable after CPU teardown on native threads and browser workers.

The text geometry, memory aperture and CRTC register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26 and 2-55–2-68. The supported subset does not yet
implement every feature of a VGA adapter.
