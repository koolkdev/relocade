# VGA compatibility

`Vga` owns four 64-KiB memory planes, registers and programmable timing on the execution
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
VGA shift-interleave (GC05 bit 5) instead emits high-to-low bit pairs from maps
0/2 for the first four pixels and maps 1/3 for the next four. Maps 2/3 supply
the upper two color bits, so this CGA-compatible shifter can use all 16 colors.
The Attribute Controller masks the assembled color and maps it through the same
palette. GC05 bit 6 takes precedence and selects the 256-color shifter, as in the
[IBM reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf)
(VGA Function p. 2-86). CPU odd/even access and CRTC row-scan banking remain
independent register settings. Interleaved output uses ordinary 0–7 pel panning
and eight pixels per fetch; only 256-color output halves the image width.
After line compare, the lower region starts at address zero with row scan zero.
Pixel Panning Mode resets both byte and pel shifts there, following the
[FreeVGA programming reference](https://github.com/logiclrd/QBX/blob/main/References/VGA.md#L997-L998).
With that bit clear, both regions retain the programmed shifts. This shared
behavior applies to text, serial/interleaved four-bit pixels and byte-addressed Mode X.

The default raster is 25.175 MHz, 800 dots per line and 449 lines per frame
(about 70.086 Hz). Guest clock selection, 8/9-dot width, clock division and CRTC
counters determine Input Status 1 and capture deadlines. Mode 12h uses 800×525
at about 59.94 Hz. The vertical-counter divider scales display height and split
position independently of character height and repeated scanlines.

`Timing` decodes the register fields and converts between time and oscillator
ticks. `Raster` retains the current position, fractional progress and retrace
count. `Vga::sync_to_clock()` updates that state and publishes the latest due
capture before a guest write changes registers or VRAM. Timing changes replace
the pending deadline; sequencer reset holds the raster, and the disconnected
external or reserved clock selections stop it. Screen-off continues to generate
retraces. Disabling CRTC sync suppresses the status pulse while internal sampling
continues. Integer arithmetic carries fractional progress so repeated polling
cannot accumulate drift.
The shared clock measures session elapsed time, not CPU instruction cycles.

Static display settings still apply to the whole captured image. Programmed CRTC
blanking masks pixels to opaque black independently of VRAM, palette and panning.
Horizontal edges use character clocks; vertical edges use the physical scanline
counter, including its divider. Display-enable skew determines the horizontal crop
origin. The image retains its displayed dimensions; repeated rows share storage
only when no visible blank/unblank edge separates them. Input Status 1 bit 0
continues to report inverted display enable, a separate signal.

Blanking follows an explicit original-VGA convention: output boundaries are one
count after the raw comparator values, the horizontal end compares six low bits,
and the vertical end compares seven. This follows
[Xorg's VGA boundary programming](https://github.com/mirror/xserver/blob/master/hw/xfree86/vgahw/vgaHW.c)
and [Linux's original-VGA end mask](https://github.com/torvalds/linux/blob/master/drivers/video/fbdev/vga16fb.c).
The [IBM reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf)
(VGA Function pp. 2-57–59 and 2-73) describes the comparators, but is inconsistent
about some end/offset details. These conventions are not a claim of measured
silicon fidelity. End matching resumes from zero after total-counter reset.
An unreachable start leaves blanking inactive; an unreachable end or coincident
set/end with no later end match leaves it active in steady state. Zero-boundary
quirks and reset transients of particular adapters are not modeled. Raster setups
whose displayed region exceeds the programmed total are outside this timing model.
Historical scanout requires captures that retain changes throughout the frame
and, for accurate guest event times, machine-level timing.

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
Modes 04h/05h (320×200, four colors) and 06h (640×200, two colors) use packed
B8000 storage with alternating 8-KiB scanline banks. Modes 04h/05h use the
interleaved shifter; mode 06h uses the serial shifter with plane zero enabled.
They expose one 16-KiB page and an 8-high character grid, 40 or 80 columns by
25 rows. Modes 04h/05h produce the same VGA image: CGA composite color-burst
behavior is outside this adapter target.

Graphics mode setup clears the selected aperture through its restored memory
routing: 32 KiB at B8000 for the packed modes or 64 KiB at A0000 otherwise.
Planar clears reach all four planes; packed clears preserve unused planes.
All modes use guest-visible ports and memory accesses and honor AL bit 7 to
preserve video memory. AH=0Fh reports the BIOS mode and geometry; rendering has
no BIOS-mode-number dispatch.
Graphics guests can write pixels and DAC colors directly. BIOS AH=0Ch/0Dh also
write/read individual pixels through the same memory aperture. Planar writes
use VGA write mode two and its latches to preserve neighboring pixels; AL bit 7
requests XOR in planar and packed modes, while mode 13h keeps all eight color
bits. Packed writes preserve other bit fields in the same byte. BH selects
one of eight mode-0Dh pages and is ignored for pixel calls in single-page modes.
These BIOS services assume the layout established by mode setting; they do not
infer arbitrary guest-reprogrammed layouts. Unsupported modes, out-of-range
coordinates and invalid mode-0Dh pages return AH=86h with carry set. This bounds
policy is local; the BIOS interface does not specify out-of-range results.
AH=02h/03h keep per-page graphics cursors in BIOS memory without enabling a
hardware text cursor. AH=05h selects pages 0–7 in mode 0Dh and page 0 in other
graphics modes. Mode-0Dh pages occupy 2000h bytes per plane; page selection
programs the CRTC start address in the units of the mode preset.

AH=06h/07h scroll or clear character-cell rectangles on the active page. Windows
clip to the BIOS grid and graphics bounds, using the BIOS character height to
move whole glyph rows. Newly exposed pixels take BH's low four bits in planar
modes or all eight bits in mode 13h; bit 7 never requests XOR for a scroll fill.
Packed modes repeat BH's raw byte, following IBM VGA firmware: BH=55h fills
four-color pixels with color one, while BH=1Bh repeats colors zero through three.
Planar copying uses write mode one, pairing each source read with its destination
write before another read replaces the latches. Pixel and scroll operations
share explicit planar, indexed and packed layouts in `video/graphics/layout`.
`GraphicsAccess` owns temporary read/write modes, masks and register restoration
for the whole operation. CPU aperture and chain/odd-even routing still follow
the mode preset. Other pages, pixels outside the window and the caller's access
registers remain intact.

AH=09h/0Ah draw graphics characters using BL as the foreground and black as the
background; both leave the cursor unchanged and treat control bytes as glyphs.
Planar and packed color bit 7 instead requests XOR of set glyph bits, preserving
background pixels. Mode 06h always draws the foreground bit, ignoring BL's low
color bits; pixel writes still use AL's requested bit. These packed scrolling
and glyph rules follow the
[IBM VGA ROM](https://github.com/jeffpar/pcjs/blob/master/machines/pcx86/ibm/video/vga/1986-10-27/IBM-VGA.asm)
(routines x20F3 and x27DC). Mode 13h keeps all eight color bits. BH selects a
valid page, including page zero in mode 13h; the ET4000 convention that uses BH
as a background color is outside this VGA target. Repeats beyond the current
row are unspecified by the BIOS interface: this firmware wraps through visible
cells, stops at the page bottom and never scrolls. Text repeats retain their
16-bit offset wrapping.

Graphics font bytes come from the guest's live vectors, with 16-bit source-offset
wrapping. Eight-high fonts use INT 43h for characters 00h–7Fh and INT 1Fh for
80h–FFh; taller fonts use INT 43h for all characters. The BIOS reads a complete
glyph before changing VGA access, so a source in VRAM uses the caller's read
settings. Font height and the character grid come from the BIOS data area,
bounded by the graphics layout. Native planar masks, indexed rows or packed
bytes write the same MMIO memory as pixel services, with no renderer-side
character path.

AH=0Eh uses the active page and BL foreground in graphics modes. It handles BEL
(currently silent), backspace without erasure, CR and LF, wraps at the right edge
and scrolls at the bottom with black fill. DOS console output shares this
teletype implementation with foreground 7, alongside its existing byte stream.
Graphics character recognition (AH=08h), AH=13h string writes and AH=11h graphics
font-selection subfunctions remain unsupported.

BIOS text operations read columns, rows and page allocation from the BIOS data
area. Cursor addresses, character access, teletype wrapping and scroll windows
share that geometry. Valid pages fit the 32-KiB text aperture and the eight BIOS
cursor records. This firmware bookkeeping does not control rendering; VGA
scanout still follows its captured registers and memory.

BIOS AH=11h supports guest font loading (AL=00h/10h), built-in 8×14
(01h/11h), 8×8 (02h/12h) and 8×16 (04h/14h) fonts, bank selection (03h), and
font information (30h). Fonts are the CC0 [Modern DOS family](../bios/assets/README.md).
Guest data uses ES:BP, BH bytes per glyph, CX glyphs starting at character DX,
and bank BL. The source offset wraps at 64 KiB. Uploads write only supplied rows
of each 32-byte plane-two slot, preserve other banks and restore VGA CPU access
settings. Loading a bank does not select it; AL=03h separately programs SR03.

AL=1xh also changes CRTC character height, clears double scanning, sets the
cursor at the bottom of the glyph, and updates BIOS rows, height and page size.
These calls require active page zero. Pages round up to 2-KiB blocks: 80×25 uses
4 KiB and 80×43/50 uses 8 KiB. Vertical timing and display extent remain intact,
so a 350-line display with an 8-high font has 43 complete rows plus six scanlines
of the next row. Changing back to 14-high restores 25 complete rows. AL=0xh
leaves geometry unchanged. A zero-count guest upload leaves font memory alone;
AL=10h can still change geometry. Invalid banks, heights outside 1–32, ranges
beyond 256 glyphs, and geometry that cannot fit BIOS row fields or text memory
return AH=86h with carry set before changing state. These bounds and page
allocation are firmware policy rather than a claim about every VGA BIOS.

AH=12h/BL=30h selects the next mode-03h profile and returns AL=12h. AL=0 keeps
the existing 200-line logical profile (8-high font, doubled on a 400-line raster);
AL=1 selects 350 lines with a 14-high font; AL=2 selects 400 lines with a 16-high
font. All start at 80×25 with eight-dot characters. The preference lives in the
BIOS data area, does not change current scanout, and survives graphics mode sets.
Loading an 8-high font with AL=12h then yields 43 or 50 rows on the 350/400-line
rasters. Text mode setting still resets complete font slots even when AL bit 7
preserves character and attribute memory.

AL=30h returns ES:BP to guest-readable ROM font data or the current INT 1Fh/43h
vector selected by BH. CX reports the current display's font height, independent
of the requested ROM font, and DL reports the last BIOS row. BH=5/7 return
terminated, empty ninth-column replacement tables. Initialization sets INT 1Fh
to the upper half of the 8×8 font; later mode sets preserve that vector and
update INT 43h to the mode's built-in font. Font uploads alone preserve both
vectors. Font information also works in graphics modes; other AH=11h graphics
services remain unsupported. These ABI details follow the
[font services](https://www.ctyme.com/intr/rb-0136.htm),
[font query](https://www.ctyme.com/intr/rb-0158.htm) and
[scanline selection](https://www.ctyme.com/intr/rb-0167.htm) interfaces.

BIOS font handling owns ROM data, temporary plane-two access and validated
geometry transitions in separate modules. Mode setting reuses that same font
writer. Font bytes and bank selection reach captures and GPU uploads through
the existing VRAM/register path.

BIOS AH=0Bh implements legacy background/intensity (BH=0) and palette selection
(BH=1) through VGA Attribute Controller registers. The firmware retains mode
and color shadows at BDA 465h/466h; graphics mode sets above 07h preserve them.
Background changes program overscan and, in graphics modes, palette entry zero.
Graphics calls rebuild entries one through three from the color shadow's
palette/intensity bits, including in mode 06h. Text palette selection is a
complete no-op. This follows the same IBM VGA ROM (x1E50 and x2ABD); it does not
add CGA I/O ports or change the DAC colors.

BIOS AH=10h programs those same Attribute Controller and DAC registers. Supported
subfunctions are 00h/07h (individual attribute register), 01h/08h (overscan),
02h/09h (16 colors plus overscan), 03h (blink/intensity), 10h/15h (individual DAC
color), 12h/17h (DAC blocks), 13h/1Ah (palette paging), and 18h/19h (DAC mask).
Attribute access preserves the display-enable bit and selected register while
handling either monochrome or color status ports. Block buffers use ES:DX with
16-bit offset wrapping; DAC addresses wrap independently and components retain
their low six bits. Zero-color transfers leave streaming state untouched.
Palette paging follows the [BIOS interface](https://www.ctyme.com/intr/rb-0123.htm)
and cannot be set in mode 13h. Grayscale conversion (1Bh) remains unsupported.
Palette changes use the ordinary captured register upload; they do not rewrite
VRAM or select another rendering path.

At the start of vertical retrace the device latches the programmed display start,
then captures a complete `DisplaySnapshot` for the following active display:
four physical memory planes (including fonts), sequencer/CRTC/graphics/attribute
settings, DAC colors and mask, the effective start address, and blink phase.
`CapturedFrame` adds the sampling boundary's time and sequence. Late service
coalesces missed boundaries; guest mutations first service elapsed time using
the preceding state. A write exactly at a retrace edge applies after that latch.
The capture describes selected inputs, not pixels scanned earlier in the frame.
Other registers are sampled at that same edge. In particular, a preset-row
update made after retrace starts appears in the next capture. The snapshot
approximation does not reproduce the within-blanking ordering used by
raster-synchronized smooth scrolling.

The device and `DisplaySnapshot::registers` use the same `Registers` type.
Capture creation clones the complete structure, including bank indices and
palette-transfer phases. Raw bank values are available through fields such as
`registers.crtc.values` and `registers.dac.colors`; `registers.crtc.display_start`
holds the effective latch alongside the programmed bytes. Port-access operations
remain inside the device. There is no separate display-register projection to
maintain.

Skipping captures loses no inputs required by the selected one. `Vga::snapshot()`
captures those same inputs immediately without publishing a frame, forcing a
display-start latch, or changing CPU access phases. `DisplaySnapshot::geometry()`
derives only image dimensions, scanline spacing and nominal pixel aspect ratio.
All pixel interpretation uses captured registers and VRAM directly in the shader.

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
`shaders/scanout` dispatches pixels to `text`, `indexed256` or `graphics16`. All use
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
nine-dot text width, 13 flat inter-stage variables and one 256-KiB fragment
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

`VgaDisplay` connects a `FrameSource` to a host through
`relocade_peripherals::display::DisplayRenderer`. It waits for new captures and
uploads them in `update(queue)`, then records decoding in `encode_image(encoder)`
only when the uploaded sequence has changed. Resize-only draws reuse its image.
Closing the source preserves that image and its capture metadata, available from
`current_frame()`.

Construct the adapter inside the graphics host's factory so it shares the host's
GPU environment and remains on the graphics owner:

```rust,ignore
let (graphics, finished) = relocade_app::graphics::Graphics::spawn(
    surface_setup,
    move |device, _queue| {
        relocade_vga::VgaDisplay::new(device, frame_source).map_err(|e| e.to_string())
    },
    notify_redraw_needed,
);
```

The adapter retains the `FrameSource`. `device_lost()` drops its GPU resources
and cached output without changing guest VGA state. `attach_device(device, queue)`
creates fresh resources and fully uploads the latest capture, including after the
source closes. The same adapter and source survive; the next `encode_image` records
decoding of the restored input. Automatic restoration is VGA's policy, not a
requirement of other display backends.

The host submits uploads even when presentation is idle, and submits each recorded
batch before servicing the adapter again. `VgaDisplay` relies on that submission
contract for its cache. Callers that need to discard and retry recorded commands
can use `VgaRenderer` directly. The adapter adds no worker, surface or request loop.

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
changes and shared scrolling/split behavior. Interleaved-shifter cases use
literal four-plane colors, plane-enable masks, panning across plane pairs,
256-color precedence, odd/even B8000 writes, banked rows, counting and splits.
Mode X checks select all four planes, flip through the
64-KiB address space, scroll a wider virtual display and apply sparse writes
through the ordinary page uploads.
Native DOS checks exercise INT 10h mode setup, memory preservation and register
restoration. Guest-to-GPU checks run COM programs that set modes 04h–06h, 0Dh,
12h and 13h, write pixels through their memory apertures, change palette colors
and restore text. Packed checks compare complete images against CPU-visible
banked byte patterns and retain palette-only updates without VRAM writes.
Mode X guests unchain the memory and flip pages without rewriting pixels.
Retained captures still render after the DOS session ends.
Transition checks begin with a scrolled, split Mode X page, blank and unblank it
without changing VRAM, then switch through planar BIOS modes and return to text.
Replaying retained inputs after those switches verifies that each capture keeps
its own layout, latch and memory.

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
