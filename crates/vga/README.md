# VGA text capture

`Vga` owns text memory, registers and fixed timing on the execution worker. BIOS
and direct guest writes reach the same MMIO memory. The supported image is
80×25 cells with 8×8 CP437 glyphs, fixed PC colors, cursor shape/skew, blinking,
blanking and display-start/page selection. Named registers and the text preset
are shared with BIOS mode programming.

The nominal raster is 25.175 MHz, 800 dots per line and 449 lines per frame,
with 640×400 active dots and vertical retrace on lines 412–413 (about 70.086 Hz).
Input Status 1 follows that raster, including during synchronous guest polling.
Absolute oscillator arithmetic avoids accumulated timer rounding. Timing-register
writes retain readback; arbitrary timing, reset behavior, fonts and graphics
modes are outside this milestone.

At each serviced refresh boundary the device captures cells, cursor, display
start, blink phase, blanking, source time and sequence. Late service coalesces
missed boundaries. Guest mutations first preserve any completed boundary using
the preceding state. The fidelity contract is a coherent text-state snapshot at
a completed refresh boundary, not reconstruction of pixel history during it.

`FrameSource` retains just the latest immutable `CapturedFrame`. The capture has
no RGBA image. It can cross workers and survive device destruction. Waiting for
`next_capture` observes emulated refreshes, not pixel output or presentation.
The registered VGA activity keeps capturing while the CPU waits for input and
while the app is not drawing. Destroying VGA closes this capture source and
wakes waiters without creating an extra frame.

`CapturedFrame::rasterize()` is a separate synchronous operation using only the
owned snapshot. Call it on the graphics worker, on redraw demand. It cannot lock
or borrow VGA, read mutable VRAM, or advance machine time. The resulting
`VideoFrame` contains immutable RGBA8 pixels, dimensions, pixel aspect ratio,
source time and sequence. The logical 640×200 image has aspect ratio 5:12; the
presenter displays it at 4:3 without needing duplicate rows for scan doubling.
Blink and blanking use captured VGA state, independently of redraw cadence.

```rust,ignore
// Execution worker:
let source = vga.borrow().frame_source();
// Transfer source to the graphics worker. On an app redraw:
if let Some(capture) = source.current_capture() {
    let frame = capture.rasterize();
    renderer.render([frame.width, frame.height], &frame.pixels, &view)?;
}
```

`scanout.rs` owns glyph-to-pixel conversion. The generic `relocade-renderer` owns
GPU drawing and knows nothing about VGA, BIOS or DOS. The embedding owns surface
setup, sizing and redraw scheduling. A bounded demand consumer and graphics
session are added by the following changes.

Register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26, 2-42–2-50 and 2-55–2-76.
