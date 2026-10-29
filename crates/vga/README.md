# VGA text state and fixed timing

`Vga` owns text memory and registers on the execution worker. Its text aperture
is MMIO; BIOS bookkeeping remains in system RAM. The supported text image is
80 by 25 cells with fixed 8-by-8 CP437 glyphs, PC colors, cursor shape/skew and
display-start addressing. The public `registers` definitions and `text_mode`
preset are shared with BIOS mode programming.

`timing.rs` tracks the nominal raster from the shared machine clock: 25.175 MHz,
800 dots per line, 449 lines per frame, 640 by 400 active dots, and retrace on
lines 412 through 413. Input Status 1 reports display disable and retrace,
including during synchronous guest polling. Color/monochrome port selection
follows the Miscellaneous Output register.

Frame boundaries use absolute oscillator arithmetic and deadlines round up;
timer rounding never accumulates into the raster rate. The registered device
activity counts completed boundaries and awaits the next one. Its mutable borrow
ends before waiting. Late service coalesces elapsed boundaries, and the frame
count can supply blink phase when pixel output is added.

This milestone keeps raster timing fixed. Timing-register writes retain readback
but do not change frequency, counters or geometry. Programmable timing and reset
behavior are separate work. BIOS mode setting restores the same register preset
used for device defaults, including timing protection and sequencer controls.

`Vga::with_clock` accepts the shared timebase. Connect it through `PORT_RANGES`
and `TEXT_MEMORY_RANGE`. `screen()` copies text state on the owning worker;
`TextRenderer` currently converts that snapshot to RGBA. Pixel-frame publication
will move rasterization into the VGA device in the next part.
