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
copies those same inputs immediately without publishing a frame or changing CPU
latches and access phases. Immediate inspection and refresh-boundary publication
share this physical representation. `TextDisplay` derives text geometry and
appearance from it. Its `TextLayout` owns address generation separately from
colors, cursor and blink; there is no separate fixed text grid.

`FrameSource` retains just the latest immutable `CapturedFrame`. The capture has
one accumulated dirty rectangle per physical memory plane. Writes mark damage
only when their final value changes a byte after VGA masks and write modes.
Rectangles use 256-byte memory rows, independently of text or graphics layout.
Pending captures union these rectangles until a consumer observes the source;
older unobserved snapshots are replaced rather than queued. Font-plane changes
therefore remain separate from character and attribute changes.

`DisplaySnapshot::memory_changes_since` returns conservative bounds relative to
an earlier snapshot. Another observer, another device, an immediate snapshot or
replaying older state can require a full comparison. Captures always contain all
bytes, so this optimization does not constrain retention or independent readers.
Register and palette settings are captured separately from memory damage.

The capture has no RGBA image. It can cross workers and survive device
destruction. Waiting for `next_capture` observes emulated refreshes, not pixel
output or presentation. The registered VGA activity keeps capturing while the CPU
waits for input and while the app is not drawing. Destroying VGA closes this capture source and
wakes waiters without creating an extra frame.

Captured memory and display settings are renderer inputs. An embedding can
read them directly or use the optional GPU renderer below. Device-only users do
not need GPU dependencies. `Vga::run` services hardware time and captures; the
embedding owns redraw scheduling, request coalescing, dimensions and presentation.
Closing the source ends capture production without invalidating retained captures
or requiring the embedding's GPU resources to close.

`display` owns register interpretation and text address generation. `renderer/memory`
owns persistent physical-plane GPU storage, dirty-rectangle selection and byte
comparison. `renderer/text_input` supplies text addressing and font-bank selection;
`renderer/upload` coalesces changed words. Future packed and planar graphics
layouts can select physical ranges through the same memory mechanism without
making the application understand VGA modes. The generic `relocade-renderer` owns
image storage and attachment drawing and knows nothing about VGA, BIOS or DOS.

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
    vga_renderer.upload(&queue, &capture);
    let frame = vga_renderer.encode(&mut encoder).unwrap();
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
nine-dot text width. It uses an integer texture array for physical planes and a
small uniform for display settings. Text addresses are selected once per layout
change. The selected font banks stay resident (8 KiB each, at most two), including
unused glyphs, so changing a character never discovers or uploads a new glyph.
Character, attribute and font changes arrive in separate physical-plane rectangles.
Each renderer compares only selected words within those rectangles against its own
queued bytes. Adjacent changed words combine into row-bounded texture writes.
Newly visible memory and newly selected font banks receive a fresh comparison,
including banks changed while hidden. Unchanged inputs queue no upload, including
when only capture time or sequence changes.

`upload(queue, capture)` synchronizes persistent input resources without decoding
or presenting. The graphics owner can service captures independently of redraw
requests and submit uploads with `queue.submit([])`. `encode(encoder)` decodes the
latest uploaded inputs and returns `None` before the first upload. It always records
a decode, including retries after an encoder was discarded. Uploading inputs does
not imply that image pixels have been generated.

The main thread can coalesce new-frame notifications and resize events into one
animation-frame redraw request. The graphics worker uploads incoming captures;
on a redraw request it decodes the latest inputs if necessary, draws and presents.
A resize with no new inputs reuses the decoded image. Scheduling remains with the
embedding; VGA creates no worker, redraw queue or presentation loop.

Submit or discard a decode encoder before uploading another capture. Queue writes
run before submitted commands, and the persistent input resources hold one version.
Neither operation reads live VRAM or converts CPU pixels; the caller creates
encoders and submits work.

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
The fixture services input uploads while presentation is idle, owns its output
pass, and submits decoding and drawing together. Resize-only requests reuse the
last decoded image. The software fixture separately verifies the CPU-image upload
path.
