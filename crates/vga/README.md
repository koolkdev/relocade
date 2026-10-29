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
owned snapshot. The caller chooses when and where to invoke it. It cannot lock
or borrow VGA, read mutable VRAM, or advance machine time. The resulting
`VideoFrame` is an ordinary owned image containing RGBA8 pixels, dimensions,
pixel aspect ratio, source time and sequence. The logical 640×200 image has
pixel aspect ratio 5:12; the presenter displays it at 4:3 without needing duplicate
rows for scan doubling.
Blink and blanking use captured VGA state, independently of redraw cadence.

The capture also exposes `screen`, `blink_on` and `blanked` for other rendering
implementations. An implementation can consume that state directly; it does not
need CPU pixel conversion or readback. The software path remains a usable
renderer and reference for comparing GPU output.

```rust,ignore
// Called by the embedding at a redraw opportunity, off the execution worker.
if let Some(capture) = source.current_capture() {
    if cached.as_ref().is_none_or(|image| image.sequence != capture.sequence) {
        *cached = Some(capture.rasterize());
    }
}
// The embedding can draw or retain the cached image, even after VGA shuts down.
```

`Vga::run` services hardware time and captures. Rendering is explicit and has no
VGA-owned request loop, worker, surface or output subscription. The embedding
owns redraw scheduling, request coalescing, caching, dimensions and presentation.
To keep slow conversion independent of guest execution, invoke the software
rasterizer on a separate worker using the existing machine worker infrastructure.
Closing the source only ends capture production; it does not invalidate retained
captures or images, or require a surface to close.

`scanout.rs` owns software conversion; `renderer` owns GPU text decoding. They
share fixed mode data in `text.rs`. The generic `relocade-renderer` owns image
storage and viewing and knows nothing about VGA, BIOS or DOS.

The [native/browser graphics fixture](../../tests/browser/src/graphics.rs)
assembles separate execution and rendering workers explicitly. Its single-slot
request channel, image cache, GPU target and shutdown policy belong to that
fixture. It verifies that blocked or paused rendering leaves the guest running,
requests stay bounded, and presentation can outlive capture production.

DOS supplies this same capture source through `dos.frame_source()`. Construct
`Dos::new(runner)` inside the execution session; its VGA registration persists
through input waits, process exits and later launches. Keep the session alive
long enough to capture a program's final writes. An embedding may retain and
render the last capture after the DOS session ends.

Register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26, 2-42–2-50 and 2-55–2-76.

## Explicit GPU rendering

The optional `renderer` feature exposes `VgaRenderer`. Its owner supplies a wgpu
device and queue, selects a capture, and calls it directly:

```rust,ignore
let mut vga_renderer = relocade_vga::VgaRenderer::new(device.clone(), queue.clone())?;
let mut blitter = relocade_renderer::ImageBlitter::new(device, queue, output_format)?;
if let Some(capture) = source.current_capture() {
    let frame = vga_renderer.render(&capture);
    blitter.blit(&frame.image, &output_view, output_size)?;
}
```

The text shader writes a 640×200 GPU image from captured cells, cursor, blink
and blanking. Fixed font and palette data are uploaded once. `render` uploads
4 KB of text state for a different capture and reuses its image for the same
capture; it never reads live device state or converts pixels on the CPU.

`VgaImage` carries source time and sequence alongside the image's dimensions
and pixel aspect ratio. Its GPU storage is reused on the next update. The caller
orders updates and viewing; cloning the image does not preserve older pixels.
The immutable capture remains available for another renderer or software conversion.

`ImageBlitter` can draw the image at another output size without calling `render`
again. No surface, worker, timer, request channel or background task is created.
Normal viewing keeps pixels on the GPU; copies and readback are explicit caller
operations. The CPU `CapturedFrame::rasterize()` path remains available without
any GPU dependencies.
