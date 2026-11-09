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

Driver storage size/save/restore (0015h/0016h/0017h) use an opaque 172-byte
Relocade snapshot at ES:DX, with 16-bit offset wrapping. `snapshot.rs`
owns the format; programs retain its bytes unchanged for a later restore. BX is
not required on save/restore. The buffer may be copied elsewhere. It saves
logical position, bounds, fractional motion, counters, buttons, callback, masks,
sensitivity and nested visibility. Restore validates the whole buffer before
changing live state; malformed buffers are ignored. The old drawn cursor is
removed and the restored cursor saves a fresh background from the current video
page. Physical background locations and the acceleration clock sample stay private;
restore starts a new speed-measurement interval. Restore the video mode before
restoring mouse state, and keep any saved callback's code valid.

Version/type query (0024h) reports Relocade driver 1.0 and PS/2 (CH=4, CL=0).
This identifies the built-in driver and the service subset listed here; it does
not advertise a complete Microsoft 6.x implementation. Separate INT 33h display
page selection, cursor exclusion regions and alternate modifier callbacks remain
unsupported; the cursor follows the programmed VGA viewport. The driver remains
built in, with no `.sys` loader or disable option.
Unknown functions leave registers and flags unchanged.

Physical motion, logical position, and cursor drawing coordinates are distinct.
Reset derives the movement bounds and center from the programmed video geometry.
Text uses eight logical Y units per row and eight X units per column (sixteen in
40-column modes). Graphics uses two logical X units per pixel at 320 pixels wide,
and one unit per pixel otherwise. Reported positions follow that grid; finer
internal motion is retained. Bounds constrain logical position,
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

Cursor drawing uses VGA's native `Framebuffer` scope, which writes the same
physical bytes accessed by the guest and captured by the renderer. It leaves VGA
register indices, read latches, access masks and VBE bank selection untouched.
`Coordinates` owns DOS logical units; VGA owns programmed geometry, viewport
addressing and pixel formats. The driver supports 40/80-column color and mono
text with the programmed font height, CGA packed pixels, planar VGA, Mode 13h,
programmed Mode X, and banked VBE indexed/RGB565/BGRX layouts.

The cursor saves the original physical pixel or character/attribute locations,
so restoration does not reinterpret old coordinates through a newly selected
page. Background storage is bounded to one text cell or 256 graphics pixels.
Graphics writes are undone in reverse order because CRTC layouts can alias pixels.

Redrawing happens after complete mouse packets and cursor-affecting INT 33h calls.
Video register changes and display captures do not redraw a stationary cursor.
The next mouse update restores its saved physical locations and draws using the
current viewport. Applications should hide around mode, page and layout changes,
including same-mode clears, and reset after changing modes to recalculate bounds.

Guest reads and screen copies include the drawn cursor. Guest drawing can overwrite
it; the next move or hide restores the background saved before drawing the cursor,
which can erase newer drawing. Applications must hide the cursor before drawing
beneath it. No video-layout observer or renderer-specific cursor state is involved.

Graphics mask bit 15 is the leftmost pixel. Drawing clips the 16×16 area after
logical-coordinate conversion and signed hotspot subtraction. A screen-mask bit
preserves color; a cursor-mask bit XORs conventional white. Indexed modes use
index 15 (limited to enabled color planes), so 256-color inversion preserves the
upper index bits. Direct-color modes use all RGB bits for white and inversion.
Black, white and inversion preserve BGRX's reserved byte. Transparent pixels are
untouched. Shapes and visibility use the same policy across all video families.

References:

- [Microsoft Mouse Programmer's Reference, second edition (1991)](https://www.bitsavers.org/pdf/microsoft/mouse/Microsoft_-_Microsoft_Mouse_Programmers_Reference_2nd_1991.pdf)
- [Microsoft KB Q70180, sensitivity and acceleration](https://helparchive.huntertur.net/document/21273)
- [INT 33h ABI reference](https://github.com/FDOS/mouse/blob/master/int33.lst)
- [CuteMouse Mode 13h mask behavior](https://github.com/FDOS/mouse/blob/master/ctmouse.asm#L1902-L1922)
- [DOSBox-X Mode 13h mask behavior](https://github.com/joncampbell123/dosbox-x/blob/master/src/ints/mouse.cpp#L795-L804)
