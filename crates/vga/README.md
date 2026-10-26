# VGA text display

`Vga` is the emulated device. It owns video memory and hardware registers behind
a short lock and implements `relocade_machine::Device`. System RAM contains BIOS
bookkeeping, not the framebuffer. The board maps `B8000..BFFFF` as MMIO and connects
the same device to that aperture and CRTC ports `3D4/3D5`.

The initial supported mode is 80 by 25 characters with fixed 8-by-8 CP437 glyphs,
PC colors, attribute blinking and a hardware cursor. The color text aperture
provides 32 KiB, enough for eight BIOS pages. Odd/even CPU bytes select character
and attribute planes. Scanout uses the full 16-bit CRTC cell address into two
64-KiB logical planes, including starts within a page and wraparound at FFFF.

CRTC indices 0A/0B control cursor shape, visibility and skew; 0C/0D control display
start; 0E/0F control cursor location. Byte and paired index/data port writes work.
Other registers currently read FF and ignore writes. Programmable graphics modes,
font loading, palette programming and display timing are future device work.

BIOS services maintain their BDA mode, page and per-page cursor data and program
these registers. Direct guest port writes affect the device without updating the
BDA. Direct guest writes, BIOS output and DOS output all reach the same VRAM.

Retain an `Arc<Vga>` for a display, independently of CPU execution:

```rust,ignore
use relocade_vga::TextRenderer;

let vga = dos.vga();
let mut text = TextRenderer::new();
// On the display thread, including while the CPU awaits a service:
if let Some(screen) = vga.screen() {
    // Acquire the output texture only after obtaining a snapshot.
    let rgba = text.render(&screen, elapsed);
    renderer.render([TextRenderer::WIDTH, TextRenderer::HEIGHT], rgba, &output_view)?;
}
```

`screen()` samples VRAM and registers together, then releases the lock. It returns
`None` on contention, allowing a display to keep its previous frame without
blocking its event loop. Synchronous bus operations use a native mutex wait or
a Wasm spin loop; guarded work never awaits or calls host code.
`TextRenderer` rasterizes that owned snapshot into its reusable RGBA8 pixel buffer.
GPU access, presentation cadence and window lifetime belong to the host. BIOS
operations spanning several bus transfers may be observed between those transfers.

The text geometry, memory aperture and CRTC register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26 and 2-55–2-68. The supported subset does not yet
implement every feature of a VGA adapter.
