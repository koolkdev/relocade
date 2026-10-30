# VGA compatibility

`Vga` owns four 64-KiB memory planes, registers and fixed timing on the execution
worker. BIOS and direct guest writes reach the same MMIO memory. Text decoding
uses captured geometry, row stride, physical address mapping, 8/9-dot characters,
font banks and heights, attribute/DAC colors, cursor shape/skew, underline,
blinking, blanking, panning and split-display settings. The BIOS text preset
loads an 8×8 CP437 font into plane two and initializes 80×25 cells.

The nominal raster is 25.175 MHz, 800 dots per line and 449 lines per frame,
with 640×400 active dots and vertical retrace on lines 412–413 (about 70.086 Hz).
Input Status 1 follows that raster, including during synchronous guest polling.
Absolute oscillator arithmetic avoids accumulated timer rounding. Timing-register
writes retain readback; arbitrary clock/retrace timing, display-state latching
and graphics decoding remain later milestones. Static text settings apply to the
whole captured image. Historical scanout requires more precise device timing and
captures that retain changes throughout the frame.

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
font memory and the shared register preset through guest-visible ports and memory
accesses before clearing text pages. Rendering has no BIOS-mode-number dispatch.

At each serviced refresh boundary the device captures a complete `DisplaySnapshot`:
four physical memory planes (including fonts), sequencer/CRTC/graphics/attribute
settings, DAC colors and mask, and blink phase. `CapturedFrame` adds source time
and sequence. Late service coalesces missed boundaries; guest mutations first
preserve completed boundaries using the preceding state. The fidelity contract
remains a coherent frame snapshot.

Skipping captures loses no inputs required by the selected one. `Vga::snapshot()`
captures those same inputs immediately without publishing a frame or changing CPU
latches and access phases. `DisplaySnapshot::geometry()` derives only image
dimensions, scanline spacing and nominal pixel aspect ratio. All pixel
interpretation uses captured registers and VRAM directly in the shader.

Snapshots share four immutable 64-KiB planes with the device. Capturing or cloning
a snapshot copies references and register values, not VRAM bytes. A guest write
copies a shared plane only when its final byte changes after VGA write modes and
masks; subsequent writes reuse the device's private plane until the next capture.
Retaining many distinct captures can therefore retain many versions of a plane.

`FrameSource` retains just the latest immutable `CapturedFrame`. Readers have no
observation bookkeeping, and a slow reader can skip any number of captures. The
capture has no RGBA image. It can cross workers and survive device destruction.
Waiting for `next_capture` observes emulated refreshes, not pixel output or
presentation. The registered VGA activity keeps capturing while the CPU waits
for input and while the app is not drawing. Destroying VGA closes this source and
wakes waiters without creating an extra frame.

Captured memory and display settings are renderer inputs. An embedding can
read them directly or use the optional GPU renderer below. Device-only users do
not need GPU dependencies. `Vga::run` services hardware time and captures; the
embedding owns redraw scheduling, request coalescing, dimensions and presentation.
Closing the source ends capture production without invalidating retained captures
or requiring the embedding's GPU resources to close.

`geometry` owns output sizing. `VgaRenderer` owns the common VRAM buffer
separately from captured registers and palette.
`renderer/memory` uploads complete captured planes into that buffer.
`renderer/state` copies raw register banks, captured blink phase, scanline spacing
and the unchanged DAC palette into one uniform buffer. The shader interprets
text addresses, font banks, attributes and colors directly. Its vertex stage
decodes frame-constant settings and the 16 text colors, passing them flat to
fragments in the same draw;
there is no preparation pass or intermediate GPU buffer. Future packed and
planar graphics decoders can read the same physical buffer without mode-specific
upload tracking or application changes.
The generic `relocade-renderer` owns image storage and attachment drawing and knows
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
let mut vga_renderer = relocade_vga::VgaRenderer::new(&device)?;
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

The text shader reads physical plane bytes at stable GPU addresses. Glyphs come
from captured plane two, and colors follow the captured attribute palette, color
select, plane enable and DAC mask. Output dimensions follow the CRTC. Identical
scan-doubled rows share one image row; a split within a repeated pair retains the
physical rows. Pixel aspect ratio targets nominal 4:3 display proportions rather
than modeling a particular monitor. The default preset still produces 640×200.

The renderer requires a 2D texture limit of at least 2304 pixels for the largest
nine-dot text width and one 256-KiB fragment storage buffer. Every encode uploads
all four captured planes and 896 bytes of state into persistent buffers. State
contains a 128-byte register/geometry header and the 768-byte DAC palette.
The shader reads those bytes directly; no CPU pixel conversion, VRAM comparison,
or change-tracking metadata is required.

Complete uploads also support skipped captures, independent readers, another
device or restoring older state. The device never waits for GPU mapping or
completion. Capture storage still uses shared CPU planes with copy-on-write;
GPU storage is a separate copy.

`encode(queue, encoder, capture)` uploads the complete inputs and records one
decode into the reusable image, including retries after a discarded encoder.
It does not read live VRAM, create an encoder or submit work.

Select one capture version per renderer per submitted batch. Queue writes run
before submitted commands, and the persistent input resources hold one version.
Submit or discard the prior decode before encoding another capture. The embedding
owns redraw scheduling and can reuse a decoded image for resize-only draws.

`VgaImage` carries the recorded capture's time and sequence alongside the image's
dimensions and pixel aspect ratio. Metadata describes recorded work, not GPU
completion. Submit the update before consuming its contents in later batches.
Image storage is reused while geometry stays the same; retaining it does not
retain historical pixels. Geometry changes allocate new output storage. Retain
the image and call `Renderer::draw_image` at a different destination for a
resize-only redraw, with no new VGA decode.

VGA owns the pass that writes its private image. The caller owns the output pass,
including its attachments, placement and load/clear policy, and submits the whole
command batch. Neither renderer creates a surface, worker, timer or background
task. Production rendering records commands without waiting for GPU completion.

## Rendering checks

Independent CPU register decoding and pixel conversion live only in
[test support](tests/support/software.rs),
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
