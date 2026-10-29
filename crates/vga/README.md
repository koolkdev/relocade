# VGA text display

`Vga` owns video memory, hardware registers, fixed text-mode timing, and pixel
generation on the execution worker. The machine maps `B8000..BFFFF` as MMIO and
connects the same device to `TEXT_MEMORY_RANGE` and `PORT_RANGES`. BIOS data stays
in system RAM; BIOS output, DOS output and direct guest writes share VGA memory.

The supported image is 80 by 25 cells with fixed 8-by-8 CP437 glyphs, PC colors,
attribute blinking, hardware cursor shape and skew, and display-start addressing.
The color aperture provides 32 KiB for eight BIOS pages. CPU bytes select the
character and attribute planes; display addresses wrap within their full 16-bit
cell address space. `screen()` copies live text state for inspection on the
execution worker. It is not the display-output API.

`registers/` owns port decoding, CRTC protection and register storage. Public
port addresses, indices and `text_mode` values are shared with BIOS programming.
BIOS mode setting holds sequencer reset, unlocks timing registers, restores the
preset with protection last, initializes pages/cursors, and releases reset.

`timing.rs` models the nominal text raster: 25.175 MHz, 800 dots per line,
449 lines per frame, 640 by 400 active dots, and vertical retrace on lines
412 through 413. This produces about 70.086 frames per second. Input Status 1
reports display disable and retrace from the shared clock, including during a
synchronous CPU polling loop. Port selection follows the color/monochrome bit.
Timing register writes retain readback but do not reprogram this initial raster.
Programmable clocks, totals, reset/reconfiguration timing and graphics modes are
outside this milestone. Sequencer screen-off blanks output while frames continue.

Completed boundaries are counted with integer oscillator arithmetic. Deadlines
are calculated from the device's origin and rounded up, so timer rounding does
not accumulate drift. Cursor and attribute blink toggle every 16 retraces.
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
let io = IoBus::new([runner.connect(vga, [TEXT_MEMORY_RANGE], PORT_RANGES)]);

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
output closure.

Register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26, 2-42–2-50 and 2-55–2-76.
