# VGA compatibility

`Vga` owns four 64-KiB memory planes, registers and fixed timing on the execution
worker. BIOS and direct guest writes reach the same MMIO memory. Text decoding
uses captured geometry, row stride, physical address mapping, 8/9-dot characters,
font banks and heights, attribute/DAC colors, cursor shape/skew, underline,
blinking, blanking, panning and split-display settings. The BIOS text preset
loads an 8×8 CP437 font into plane two and initializes 80×25 cells.
Eight-bit graphics decoding supports chained mode 13h and byte-addressed
unchained layouts (Mode X): linear or plane-masked guest writes, all 256 DAC
colors, display pages and stride, repeated rows, panning and split display.
Mode X guests can program 320×200 or 320×240 output from the same physical planes.
The decoder follows the captured registers, including
when guest software changes CPU memory routing independently of display settings.

Planar 16-color graphics combines one bit from each physical plane and uses the
same Attribute Controller palette as text. Display start, stride, panning,
repeated rows and split display share the common CRTC address calculation.

The nominal raster is 25.175 MHz, 800 dots per line and 449 lines per frame,
with 640×400 active dots and vertical retrace on lines 412–413 (about 70.086 Hz).
Input Status 1 follows that raster, including during synchronous guest polling.
Absolute oscillator arithmetic avoids accumulated timer rounding. Timing-register
writes retain readback; arbitrary clock/retrace timing, display-state latching
and CGA shifter layouts remain later milestones.
Static display settings apply to the
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
when guest software changes modes. In chain-4, CPU A0/A1 select the plane and
A14/A15 supply its low address bits, matching CRTC doubleword addressing across
the full 64-KiB guest aperture. CRTC byte, word and doubleword addressing
translates display counters to those same physical addresses. The IBM VGA
target does not implement legacy EGA page-selection inputs or 64-KiB chaining.

Embeddings register `MEMORY_RANGE` (A0000–BFFFF) as MMIO on both the machine's
physical map and device bus. The Graphics Controller selects the active aperture;
reads outside it return FF and writes are ignored. BIOS mode 03h restores font
memory and the shared text preset. Mode 13h programs the standard 320×200 layout
and 256-color palette. Modes 0Dh (320×200) and 12h (640×480) program planar
16-color state with their respective RGBI and EGA default palette mappings.
Graphics mode setup clears the 64-KiB A0000 aperture through the selected memory
routing; in planar modes this clears all four planes. All modes use guest-visible
ports and memory accesses and honor AL bit 7 to preserve video memory. AH=0Fh
reports the BIOS mode and geometry; rendering has no BIOS-mode-number dispatch.
Graphics guests write pixels and DAC colors directly. BIOS pixel/glyph services
in graphics mode remain unsupported; DOS console bytes are still buffered but
do not draw graphics-mode text.

At each serviced refresh boundary the device captures a complete `DisplaySnapshot`:
four physical memory planes (including fonts), sequencer/CRTC/graphics/attribute
settings, DAC colors and mask, and blink phase. `CapturedFrame` adds source time
and sequence. Late service coalesces missed boundaries; guest mutations first
preserve completed boundaries using the preceding state. The fidelity contract
remains a coherent frame snapshot.

The device and `DisplaySnapshot::registers` use the same `Registers` type.
Capture creation clones the complete structure, including bank indices and
palette-transfer phases. Raw bank values are available through fields such as
`registers.crtc.values` and `registers.dac.colors`; port-access operations remain
inside the device. There is no separate display-register projection to maintain.

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
`memory/pages` tracks physical writes in a fixed 32-byte bitmap.
`renderer/memory` uploads those pages when their captured baseline matches the
queued GPU contents, otherwise it restores the affected plane.
`renderer/state` copies raw register banks, captured blink phase, scanline spacing
and the unchanged DAC palette into one uniform buffer. The raw Attribute
Controller address byte is preserved; the shader tests its display-enable bit.
The shader interprets memory addresses, font banks, attributes and colors directly.
`shaders/scanout` dispatches pixels to `text`, `indexed256` or `planar16`. All use
the scanline and physical-address logic in `addressing` and the DAC lookup in
`palette`. Its vertex stage decodes frame-constant settings and the 16 attribute
colors, passing them flat to fragments in the same draw. There is no preparation
pass or intermediate GPU buffer. Further graphics decoders can read the same physical
buffer without mode-specific upload tracking or application changes.
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
select, plane enable and DAC mask. Eight-bit graphics pixels index the DAC directly,
using its mask. Output dimensions follow the CRTC and character clock. Identical
repeated rows share one image row; a preset, split or partial group that breaks
their alignment retains physical rows. Pixel aspect ratio targets nominal 4:3
display proportions rather than modeling a particular monitor. The default text
preset produces 640×200; standard mode-13h registers produce 320×200 with a 5:6
pixel aspect ratio.

The renderer requires a 2D texture limit of at least 2304 pixels for the largest
nine-dot text width, 11 flat inter-stage variables and one 256-KiB fragment
storage buffer. The first upload fills all four physical planes and 896 bytes
of state: a 128-byte register/geometry header and the 768-byte DAC palette. With
the renderer feature enabled, actual memory changes mark fixed 1-KiB pages after
VGA routing and write modes have produced the final byte. Consecutive dirty pages
in each plane become one upload. Character,
attribute and font planes update independently. Hidden memory is included, so
changing the display start or font bank requires no residency bookkeeping.

Captures remain complete. Private transfer hints retain the preceding published
plane versions and a copy of the dirty bitmap; they hold no references to older
captures. Publication clears the device's bitmap, while immediate snapshots and
independent readers leave it alone. The renderer compares plane identities and
walks the bitmap, without scanning VRAM or maintaining a separate CPU shadow.
The raw register header and palette are compared and uploaded independently
when changed. Blink, cursor or geometry updates do not resend the palette;
palette-only updates do not resend the header or font memory.

If a captured plane already matches the last queued GPU version, no memory upload
is needed. If its change baseline matches, the renderer uploads dirty pages.
Otherwise it restores that entire 64-KiB plane, including when a skipped capture
changed memory that the latest bitmap no longer marks. New renderers, other
sources, immediate snapshots and reverse playback stay correct through this
fallback. No capture acknowledgements or pending-update queues are required.

The device never waits for GPU mapping or completion. Snapshot storage still
uses shared CPU planes with copy-on-write, so a changed retained plane can copy
64 KiB even for a one-byte guest write. Dirty pages limit GPU transfers; they do
not change CPU copy-on-write granularity. Device-only builds omit transfer hints
and dirty tracking along with the optional renderer.

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

Independent CPU register decoding and pixel conversion live only in
[test support](tests/support/software.rs),
with its owned CPU image type. It is not part of the VGA library API. Capture,
DOS, CPU-upload and explicit worker fixtures use that reference. The GPU suite
compares complete images against it and retains literal glyph, palette, cursor,
blink, blanking and page-selection expectations.
Graphics checks compare all 64,000 pixels against the guest's linear byte pattern,
including CPU address-bit transitions. Focused fixtures cover display start and
stride, panning across plane groups, split and partial repeated rows, address wrap,
palette changes, skipped captures and retained text/graphics captures. Planar
checks cover all 640×480 pixels, masked write-mode-two updates, attribute palette
changes and shared scrolling/split behavior. Unsupported CGA shifter settings
still blank the image. Mode X checks select all four planes, flip through the
64-KiB address space, scroll a wider virtual display and apply sparse writes
through the ordinary page uploads.
Native DOS checks exercise INT 10h mode setup, memory preservation and register
restoration. Guest-to-GPU checks run COM programs that set modes 13h, 0Dh and
12h, write pixels through the graphics aperture, change DAC colors and restore
text. Mode X guests unchain the memory and flip pages without rewriting pixels.
Retained captures still render after the DOS session ends.

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
