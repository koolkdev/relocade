# VGA text display

`Vga` is the emulated device. The execution worker owns its ordinary mutable
video memory and hardware registers. It implements `relocade_machine::Device`.
System RAM contains BIOS bookkeeping, not the framebuffer. The board maps
`B8000..BFFFF` as MMIO and connects
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

Retain the device locally and send its read-only `Display` to the renderer:

```rust,ignore
let vga = dos.vga(); // Rc<RefCell<Vga>>, on the execution worker.
let display = vga.borrow().display(); // Send this handle to the renderer once.

// Between execution polls, at the application's chosen display cadence:
vga.borrow_mut().publish();
// Also publish before a pending wait or completion becomes externally visible.

// On the renderer worker:
let screen = display.screen(); // Arc<TextScreen>, coherent and immutable.
let rgba = text.render(&screen, elapsed);
let texture = Texture::new(&device, [TextRenderer::WIDTH, TextRenderer::HEIGHT])?;
texture.write(&queue, rgba)?;
// Retain and update this texture independently of host redraws.
// The viewer samples it when the window needs a frame.
```

`Vga::screen()` copies the current device state on its owning worker.
`Vga::publish()` copies changed state and replaces the display's latest screen
through `arc-swap`. There is no frame queue. A retained screen remains unchanged
while execution continues, and an idle renderer can skip intermediate publications.
The device has no per-byte atomics, packed register word or mutex. Synchronization
belongs to publishing and retaining the completed screen.

Publication is explicit: writes update the device immediately, while a display
sees them only after publication. With no intervening device writes, publication
reuses the current screen.
DOS publishes its initialized screen and flushes changes when its session drops;
the VGA device also flushes on destruction. A running application supplies cadence
and publishes after every pending execution poll, including input echo on later
wakes. Device borrows must end before entering guest execution or dropping DOS.

This is still the fixed text-mode implementation. Publication does not implement
VGA retrace, latch display-start registers or capture effects within a scanline.
Future VGA timing and scanout belong to the device; the renderer consumes their
completed output. Full VGA can change its display data while still producing a
texture for window presentation. Win32 drawing and desktop composition
can produce that same output after processing their own ordered surface updates.

The read-only display can be used on a browser main thread or renderer worker.
`TextRenderer` rasterizes a retained screen into its own reusable RGBA8 buffer;
blinking can continue while execution waits for input. The renderer owns its
cadence independently of host visibility. Both rendering stages share a worker,
device and queue. Browser hosts transfer an
`OffscreenCanvas` and forward canvas events; native platforms may require surface
creation on the window thread before moving it to the renderer. The host supplies
platform setup and host redraw scheduling.

The shared [worker integration check](../../tests/browser/src/vga.rs) exercises
local guest MMIO and port state, publication while a borrowed service awaits input,
and the retained display after CPU and device teardown on native and browser workers.

The text geometry, memory aperture and CRTC register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26 and 2-55–2-68. The supported subset does not yet
implement every feature of a VGA adapter.

## GPU renderer

Enable the `renderer` feature to use `VgaRenderer`. It implements the shared
`relocade_renderer::Renderer` contract and owns its display handle, rasterizer,
texture and timer. The generic application host remains independent of VGA.
The default VGA device build does not depend on GPU or application-host code.

```rust,ignore
use relocade_renderer::{Graphics, Renderer};
use relocade_vga::VgaRenderer;

let renderer = VgaRenderer::new(display, Graphics { device, queue })?;
// The embedding polls the renderer independently of window redraws.
// Its current texture is available through renderer.texture().
```

Construction uploads the current screen into a 640×200 texture. Subsequent
polling refreshes it at approximately 60 Hz, including while presentation is
paused. Its own timer advances cursor and attribute blinking. This display
cadence remains separate from future emulated VGA scanout timing.

GPU integration coverage checks initialized pixels and repeated timer-driven
updates without a window on native and browser targets. Texture scaling and
output-format coverage stay in the renderer crate.
