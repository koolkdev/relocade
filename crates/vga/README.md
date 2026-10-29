# VGA text display

`Vga` owns video memory, hardware registers, programmable raster timing, and pixel
generation on the execution worker. The machine maps `A0000..BFFFF` as MMIO and
connects the same device to `MEMORY_RANGE` and `PORT_RANGES`. BIOS data stays
in system RAM; BIOS output, DOS output and direct guest writes share VGA memory.

The supported image is 80 by 25 cells with fixed 8-by-8 CP437 glyphs, PC colors,
attribute blinking, hardware cursor shape and skew, and display-start addressing.
Four 64-KiB planes share CPU read latches. The Graphics Controller selects a
128/64-KiB graphics aperture or a 32-KiB text aperture, read/compare modes,
write modes 0–3, rotation, logical operations and pixel masks. Sequencer map
masks and chain-4/odd-even addressing select the planes. Masked writes preserve
latch bits from the last CPU read, even when writing a different address.
Plane offsets remain physical across addressing-mode changes. Chain-4 clears
the two low address bits; the Graphics Controller's odd/even chain replaces
bit zero independently of CPU read/write plane selection. The 128-KiB aperture
supplies A16 for this replacement; smaller apertures use page zero. Disabling
extended memory restricts each plane to 16 KiB. Original EGA page-selection and
64-KiB chaining variations are outside this VGA model.
The initial color text aperture provides eight BIOS pages; character and
attribute bytes select planes 0 and 1. CRTC word/doubleword addressing transforms
the display counter into physical plane addresses. Scanout does not modify CPU
latches. `screen()` copies live text state for inspection on the execution worker.
It is not the display-output API.

`registers/` owns port decoding, CRTC protection and register storage. Public
port addresses, indices and `text_mode` values are shared with BIOS programming.
The register model includes the Graphics Controller, Attribute Controller,
six-bit DAC palette, all five sequencer registers and all 25 CRTC registers.
Attribute writes alternate index/data at 3C0; reading the active Input Status 1
port resets that phase. DAC RGB transfers increment their palette address after
three components and restart when software selects another address. These port
protocols are independent of frame delivery. Graphics addressing and programmable
pixel interpretation are implemented separately from register access.
BIOS mode setting holds sequencer reset, restores clock controls and the CRTC
preset with protection last, initializes pages/cursors, and releases reset.
Restoring the other register banks is part of BIOS graphics-mode support.

`timing.rs` decodes the programmed clocks, totals, display intervals and vertical
retrace. `raster.rs` owns the live dot position, fractional progress and frame
count. The default 25.175-MHz, 800-by-449 raster runs at about 70.086 Hz. The
second oscillator is 28.322 MHz; external clock selections stop the raster.
Sequencer reset holds the counters at zero. Clock division, 8/9-dot character
width and the CRTC vertical-counter divider affect timing independently of
character height and scanline repetition.

Input Status 1 reports inverted display enable and vertical retrace, including
inside a synchronous CPU polling loop. Register writes first account for elapsed
time under the old configuration, then update the live counters and notify the
async activity to replace its deadline. Sequencer screen-off blanks output while
frames continue. Integer oscillator arithmetic carries fractional progress and
rounds deadlines up; repeated polling does not accumulate timer rounding drift.
Cursor and attribute blink toggle every 16 retraces.
The current timebase is host elapsed time, shared with CPU services and devices;
it is not a cycle-timed CPU model.

The device catches up before writes, preserving completed output before later
VRAM or register changes. Its registered async activity also services deadlines
while the CPU is idle or awaiting input. No device borrow spans a timer await.
Late servicing coalesces missed boundaries into the latest completed frame.

`scanout.rs` rasterizes the text snapshot synchronously on the execution worker.
This first implementation samples the whole screen at a retrace boundary; it
does not reconstruct mid-frame changes, hardware fetch pipelines or display-start
latching. Incremental scanout can replace this implementation without changing
frame consumers.

`VideoFrame` carries dimensions, tightly packed RGBA8 pixels with sRGB colors and
opaque alpha, pixel aspect ratio, boundary time, and an increasing sequence.
The image is 640 by 200 with pixel width/height ratio 5:12, giving a 4:3 display.
Scan doubling is represented by that ratio instead of duplicating image rows.

The read-only `Display` can cross workers. It retains one immutable
`Arc<VideoFrame>` without a queue or producer backpressure. Retained images never
change. A slow consumer skips old frames; a fast consumer can reuse the latest.
Sequences identify new output independently of timestamp rounding. Gaps can
represent coalesced frames.

```rust,ignore
// Execution worker:
let vga = Rc::new(RefCell::new(Vga::with_clock(Rc::new(runner.clock().clone()))));
let display = vga.borrow().display();
let io = IoBus::new([runner.connect(vga, [MEMORY_RANGE], PORT_RANGES)]);

// App/graphics worker, on its own redraw schedule:
if let Some(frame) = display.current_frame() {
    renderer.render([frame.width, frame.height], &frame.pixels, &view)?;
}
// A consumer can instead await display.next_frame(None), then use
// display.next_frame(Some(frame.sequence)) for subsequent output.
```

The app owns viewport sizing, GPU presentation and redraw scheduling. Reading
or presenting output cannot advance VGA time. The generic `relocade-renderer`
crate accepts the image bytes directly and has no VGA state or font knowledge.

There is no frame until the first boundary is serviced. Destroying VGA closes
the output and wakes consumers; the last unseen frame can still be received and
retained images stay readable. Teardown creates no extra frame. A DOS process
exiting leaves its session alive: keep servicing video to capture its final
writes before explicitly stopping the execution worker.

The [worker fixture](../../tests/browser/src/vga.rs) checks MMIO and status polling,
frames during a borrowed input service, immutable pixels on another worker, and
output closure. The [DOS fixture](../../tests/browser/src/dos.rs) covers persistent
sessions, busy CPU execution, slow consumers and input waits.

Register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26, 2-42–2-50 and 2-55–2-76.
