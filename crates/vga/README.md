# VGA compatibility

`Vga` owns four 64-KiB memory planes, registers and fixed timing on the execution
worker. BIOS and direct guest writes reach the same MMIO memory. The supported image is
80×25 cells with 8×8 CP437 glyphs, fixed PC colors, cursor shape/skew, blinking,
blanking and display-start/page selection. Named registers and the text preset
are shared with BIOS mode programming.

The nominal raster is 25.175 MHz, 800 dots per line and 449 lines per frame,
with 640×400 active dots and vertical retrace on lines 412–413 (about 70.086 Hz).
Input Status 1 follows that raster, including during synchronous guest polling.
Absolute oscillator arithmetic avoids accumulated timer rounding. Timing-register
writes retain readback; arbitrary timing, reset behavior, fonts and graphics
modes are outside this milestone.

The sequencer, CRTC, Graphics Controller, Attribute Controller and six-bit DAC
own their register storage and CPU access rules. Attribute writes alternate
address and data phases; reading the active Input Status 1 port resets that
phase. DAC reads and writes step through RGB channels and wrap palette addresses.
CPU reads load all four latches; writes preserve those latched bytes. Memory
transfers apply VGA read modes 0–1, write modes 0–3, rotation, logical operations,
plane masks and pixel masks.
Chain-4 and odd/even routing select physical planes without repacking memory
when guest software changes modes. CRTC byte, word and doubleword addressing
translates display counters to those same physical addresses. The IBM VGA
target does not implement legacy EGA page-selection inputs or 64-KiB chaining.

Embeddings register `MEMORY_RANGE` (A0000–BFFFF) as MMIO on both the machine's
physical map and device bus. The Graphics Controller selects the active aperture;
reads outside it return FF and writes are ignored. BIOS text-mode setup restores
the shared preset through guest-visible ports before clearing text memory.
Font and palette registers retain guest programming; programmable captured
display state and graphics decoding follow in later stages.

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

Captured cells, cursor, blink and blanking are renderer inputs. An embedding can
read them directly or use the optional GPU renderer below. Device-only users do
not need GPU dependencies. `Vga::run` services hardware time and captures; the
embedding owns redraw scheduling, request coalescing, dimensions and presentation.
Closing the source ends capture production without invalidating retained captures
or requiring the embedding's GPU resources to close.

`renderer` owns GPU text decoding and `text.rs` owns fixed mode resources. The
generic `relocade-renderer` owns image storage and attachment drawing and knows
nothing about VGA, BIOS or DOS.

DOS supplies this same capture source through `dos.frame_source()`. Construct
`Dos::new(runner)` inside the execution session; its VGA registration persists
through input waits, process exits and later launches. Keep the session alive
long enough to capture a program's final writes. An embedding may retain and
render the last capture after the DOS session ends.

Register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26, 2-42–2-96 and 2-104–2-106.

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

## Rendering checks

Software conversion lives only in [test support](tests/support/software.rs),
with its owned CPU image type. It is not part of the VGA library API. Capture,
DOS, CPU-upload and explicit worker fixtures use that reference. The GPU suite
compares complete images against it and retains literal glyph, palette, cursor,
blink, blanking and page-selection expectations.

Recording checks combine decoding and drawing in one caller submission, discard
and retry an update, and resize a retained GPU image after dropping its decoder.
The shared renderer's pixel checks draw two images into one loaded attachment,
with explicit placement and a caller scissor, preserving untouched pixels.

The [GPU worker fixture](../../tests/browser/src/graphics/gpu.rs) explicitly
assembles execution and graphics workers with a bounded request channel. It
creates no surface and checks that paused or slow rendering leaves the guest
running, demand stays bounded, and captured images remain usable after VGA ends.
The fixture owns its output pass and submits decoding and drawing together.
The software fixture separately verifies the CPU-image upload path.
