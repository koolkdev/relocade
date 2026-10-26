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
// On the renderer worker, including while the CPU awaits a service:
let screen = vga.screen();
let rgba = text.render(&screen, elapsed);
renderer.render([TextRenderer::WIDTH, TextRenderer::HEIGHT], rgba, &output_view)?;
```

`screen()` samples VRAM and registers together, then releases an ordinary
`std::sync::Mutex` before returning the owned `TextScreen`. Synchronous bus accesses
use the same mutex. Guarded work never awaits or calls host code, and
`TextRenderer` rasterizes the snapshot into its reusable RGBA8 pixel buffer after
the lock has been released. BIOS operations spanning several bus transfers may
be observed between those transfers.

In browsers, all device access belongs on workers because the main thread cannot
wait for a contended mutex. This includes CPU execution, host memory-view access
to the MMIO aperture, BIOS initialization and display snapshots. The CPU worker
owns execution; a renderer worker retains `Arc<Vga>`, takes snapshots and can own
the wgpu device, queue, surface and presentation loop. Browser GPU resources must
be created in that renderer worker, using a transferred `OffscreenCanvas`. The
main thread handles window/canvas events and sends resize or lifecycle messages.
Native platforms may require surface creation on the window thread before moving
the surface to the renderer worker. The host supplies that platform setup and
presentation scheduling; these crates do not create an application event loop.

The shared [worker integration check](../../tests/browser/src/vga.rs) exercises
guest MMIO and CRTC writes on a CPU worker, display progress while input is pending,
and the retained display after CPU teardown. The same check runs on native threads
and browser workers.

The text geometry, memory aperture and CRTC register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26 and 2-55–2-68. The supported subset does not yet
implement every feature of a VGA adapter.
