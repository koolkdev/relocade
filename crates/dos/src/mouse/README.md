# DOS mouse services

DOS installs a resident INT 33h driver. Applications set their video mode and call
AX=0000h to reset/detect it. Reset initializes the BIOS three-byte PS/2 interface,
registers a Rust far procedure as its packet callback, and enables reporting. The
live INT 15h vector remains authoritative, so firmware chaining still works.
INT 33h itself is an ordinary registered interrupt; guest drivers can replace and
chain its vector too. No separate input task or private guest communication area
is needed.

`Mouse` owns session state. The interrupt adapters declare register inputs and
outputs through the existing `#[interrupt]` derive. The incoming IBM callback
reads Z/Y/X/status above the far return address; PS/2 sign bits extend X/Y to nine
bits, and Y is inverted to DOS screen direction. User callbacks use the existing
`call_far(target, &[])` with AX/BX/CX/DX/SI/DI. BIOS preserves the interrupted
registers around both callbacks. No state borrow survives a guest call.

The implemented services are reset/detection (0000h), position/buttons (0003h),
position (0004h), press/release counts (0005h/0006h), horizontal/vertical bounds
(0007h/0008h), raw motion (000Bh), event registration/exchange (000Ch/0014h),
mickey ratios (000Fh), double-speed threshold (0013h), and sensitivity
set/query (001Ah/001Bh). Cursor show/hide (0001h/0002h) is nested; software text
cursor masks (000Ah, BX=0) apply `(cell & screen_mask) ^ cursor_mask` to the guest's
character/attribute word. Graphics cursor definition (0009h) reads 16 screen-mask
words followed by 16 cursor-mask words from ES:DX, with signed BX/CX hotspots.
Hardware text cursor selection is not implemented.
Unknown functions leave registers and flags unchanged.

Physical motion, logical position, and cursor drawing coordinates are distinct.
Text positions use a 640×200 logical grid, reported at cell boundaries; Mode 13h
uses two logical X units per display pixel. Bounds constrain logical position,
not raw mickeys. Button counts wrap at 16 bits, and reading one count consumes
only that button's selected transition count. Raw motion wraps at signed 16 bits
and is consumed only by 000Bh. Callback SI/DI observe these accumulated counters
without consuming them. Movement events still fire at a coordinate limit.
There is no retained queue of input events.

Sensitivity uses the scale documented in Microsoft KB Q70180, with linear
interpolation between its five-unit settings and 50 as unity. Fractional logical
movement is retained. Acceleration doubles movement when maximum-axis speed
exceeds the configured mickeys/second threshold (64 by default). Speed uses the
PC's emulation clock between packets; the first packet, or packets with no elapsed
time, have no measured speed and remain unaccelerated. These services do not
rescale raw motion. The sensitivity curve is an explicit compatibility policy,
not a recreation of every Microsoft driver version's acceleration internals.

Driver state survives processes. User event pointers are removed before process
memory is released and before a new launch; session packet handling never
requires an active process or enters DOS's InDOS scope. Process exit restores and
hides the cursor. Reset starts with one outstanding hide and text masks
FFFFh/7700h, preserving the cell's intensity and blink bits while inverting colors.

Text drawing uses the BIOS `TextPage` geometry owner and mapped video memory,
without moving the BIOS output cursor. It saves the original cell and restores
that page before movement, hiding, mask changes or reset. Applications must hide
the cursor before changing video modes or drawing in its saved area. Comparing
layouts cannot detect a same-mode screen clear. Full mode-transition handling,
additional text geometries and arbitrary VGA programming belong to the following
video milestone.

Mode 13h uses the same visibility and restoration owner, with a separate graphics
shape/drawing module. Mask bit 15 is the leftmost pixel. Drawing clips the 16×16
area after converting logical X to display pixels and subtracting the hotspot.
The pixel policy is `(old & (screen_bit ? FFh : 00h)) ^ (cursor_bit ? 0Fh : 00h)`,
matching CuteMouse and DOSBox-X: white is palette index 15, and inversion preserves
the upper index bits. The default arrow is visible with the BIOS palette. Pixels
are read and written through BIOS's mapped VGA access; transparent pixels are
untouched, and saved background is bounded to 256 pixels. This does not yet
support CGA, planar graphics, Mode X, VBE or cursor drawing across arbitrary VGA
register programming. Hide before changing modes, including same-mode clears.

References:

- [Microsoft Mouse Programmer's Reference, second edition (1991)](https://www.bitsavers.org/pdf/microsoft/mouse/Microsoft_-_Microsoft_Mouse_Programmers_Reference_2nd_1991.pdf)
- [Microsoft KB Q70180, sensitivity and acceleration](https://helparchive.huntertur.net/document/21273)
- [INT 33h ABI reference](https://github.com/FDOS/mouse/blob/master/int33.lst)
- [CuteMouse Mode 13h mask behavior](https://github.com/FDOS/mouse/blob/master/ctmouse.asm#L1902-L1922)
- [DOSBox-X Mode 13h mask behavior](https://github.com/joncampbell123/dosbox-x/blob/master/src/ints/mouse.cpp#L795-L804)
