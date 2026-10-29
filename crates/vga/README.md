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

## Recording GPU decoding

The optional `renderer` feature exposes `VgaRenderer`. It uses the owner's wgpu
device and queue and records its private image-generation pass into the owner's
command encoder:

```rust,ignore
let mut vga_renderer = relocade_vga::VgaRenderer::new(&device, &queue)?;
let mut renderer = relocade_renderer::Renderer::new(device.clone(), output_format)?;
if let Some(capture) = source.current_capture() {
    let mut encoder = device.create_command_encoder(&Default::default());
    let frame = vga_renderer.encode(&queue, &mut encoder, &capture);
    let destination = relocade_renderer::fit_image(&frame.image, output_bounds);
    {
        let mut pass = encoder.begin_render_pass(&output_pass);
        renderer.draw_image(&mut pass, &frame.image, destination);
    }
    queue.submit([encoder.finish()]);
}
```

The text shader writes a 640×200 GPU image from captured cells, cursor, blink
and blanking. Fixed font and palette data are uploaded once. `encode` queues
4 KB of text state and always records decoding. It never reads live device state,
converts CPU pixels, creates an encoder or submits work.

Select one capture version per `VgaRenderer` per submitted batch. Queue uploads
run before the batch's commands, so this input buffer cannot hold separate capture
versions for multiple decodes within one submission. Submit or discard the prior
update before encoding another capture; the encoder can be discarded and retried
without a cross-call cache suppressing the new decode.

`VgaImage` carries the recorded capture's time and sequence alongside the image's
dimensions and pixel aspect ratio. Metadata describes recorded work, not GPU
completion. Submit the update before consuming its contents in later batches.
The image storage is reused on the next encode; retaining it does not retain
historical pixels. Retain the image and call `Renderer::draw_image` at a different
destination for a resize-only redraw, with no new VGA decode.

VGA owns the pass that writes its private image. The caller owns the output pass,
including its attachments, placement and load/clear policy, and submits the whole
command batch. Neither renderer creates a surface, worker, timer or background
task. Production rendering records commands without waiting for GPU completion.
`CapturedFrame::rasterize()` provides the software reference without GPU dependencies.
