# ET4000AX profile

The card uses an ET4000AX Revision E, 1 MiB of DRAM and a Sierra SC11483 RAMDAC.
Its reference is Tseng's EV8913 Revision A AT-bus design, with all eight
256K×4 DRAMs fitted. The SC11483 is an explicit RAMDAC substitution for this
emulated profile. It starts in indexed VGA operation and also supports RGB555.

The clock multiplexer follows the EV8913 schematic. Its three-bit selection is
CRTC 34h bit 1 followed by Miscellaneous Output bits 3–2:

| Selection | Pixel oscillator |
| --- | --- |
| 0, 4 | 25.175 MHz |
| 1, 5 | 28.322 MHz |
| 2, 6 | 32.514 MHz |
| 3 | 40 MHz |
| 7 | 44.9 MHz |

The board's separate memory oscillator is 40 MHz. The 44.9 MHz input permits
interlaced 1024×768 timing; there is no 65 MHz input in this profile.

The fixed configuration uses the AT interface, 32-bit DRAM organization,
256K×4 depth, segmented CPU addressing and an eight-bit ROM interface. The
translation ROM and external synchronization input are absent; the IRQ2 jumper
is open. Vendor ROM execution is not part of device initialization.

As with the existing VGA core, construction starts from a usable legacy register
preset rather than simulating power-on and POST. Extension startup values are
fixed to locked access, bank zero, no extended display-address bits, no interlace,
CRTC 34h = 08h (enable at 46E8h), CRTC 36h = 40h, CRTC 37h = 0Bh and
Sequencer 07h = BCh (VGA operation, 32 KiB ROM, recommended Rev E bits). These choices retain VGA address mapping and
describe the installed DRAM. SC11483 command state starts at zero (indexed mode),
with no pending command-register access. Registers whose power-on contents are
unspecified use zero until firmware programs them.

`Et4000Ax` composes the shared VGA core, standard port and memory bindings, and
display activity with this board's capacity and clocks. Its CRTC index retains
six bits, so extended indices cannot alias legacy registers. Extension access follows the two-port KEY sequence (3BFh=03h, then the active
3B8h/3D8h=A0h). CRTC 33h is not KEY-protected; 35h follows the VGA timing
protection bit, except its line-compare and interlace bits. Segment-register
access remains enabled after relocking, until synchronous reset. There is one
Attribute address/phase latch for both standard and extension registers. Input
Status 1 bit 7 is the complement of vertical retrace, including cached polls;
native Tseng detection uses it before probing CRTC 33h.

The selected adapter-enable address is 3C3h or 46E8h; Rev E aliases 46E8h on
its low twelve address bits. Extension configuration registers retain their
documented bits. Translation-ROM, external genlock and 6845 compatibility
behavior are outside this profile. CPU access uses independent read/write segments
in 3CDh. Chain-4 selects linear bytes; the plane system retains VGA read latches
and graphics writes. Both address the same DRAM, with no conversion on writes.
The board exposes the segmented host aperture; CR36h's optional relocated linear
host aperture is not connected.

Full-pixel scanout derives its size, pitch and 18-bit start counter from VGA and
extension registers. It uses the shared packed renderer and native drawing view,
including panning across the physical DRAM boundary. Packed output captures CRTC
blanking as pixel intervals, including blanking across counter reset. CPU banking never selects
the displayed page. The data-book 640×480×8 register sequence is exercised through
the bus and GPU renderer; no host mode table supplies that geometry. Serialized
VGA 256-color modes retain the VGA decoder's row, split-screen, serializer and
blanking rules, with the card's consecutive memory addressing.

CRTC 35h extends the vertical total, display end and retrace counters. CRTC 34h
selects the third oscillator-select bit; Sequencer 07h supplies the /2 or /4
pixel-clock division. Register fixtures cover 800×600 at 40 MHz and 1024×768
interlaced at 44.9 MHz. Interlaced retraces occur once per field, preserving the
half-line horizontal phase between fields. As elsewhere in the renderer, captures
represent a complete current image at the capture boundary; they do not weave
historical odd/even lines or simulate changes within a displayed scanline.

The full 18-bit display start is sampled together at the shared retrace capture
boundary, representing the page for the following visible frame. Native drawing
uses the programmed page immediately. Text, planar and serialized graphics can address
the full DRAM, with physical wrapping, extended line compare, and eleven-bit
vertical display/blanking counters. IBM pin rotation and Tseng's contiguous
mapping are selected by CRTC 36h. Text cursor comparison uses all eighteen counter
bits before address-pin rotation, including cursor skew. Font banks remain in
their own plane and do not move with the display start or CPU bank pointers.

Four consecutive reads of the DAC mask port (3C6h) expose the SC11483 command
register. Any write or access to another DAC port ends command access. Command
bit 7 selects RGB555: two little-endian bytes hold red, green and blue in bits
14–10, 9–5 and 4–0; bit 15 is unused. Palette contents and the pixel mask do not
affect high-color output. Mode 1 (80h) latches both clock edges with ET4000 word
output; mode 2 (A0h) pairs successive byte clocks. ET4000 word output requires
Sequencer 07h's MCLK/2 selection. These controls change prepared scanout, leaving
CPU transfers as physical byte accesses. Restoration includes the command and
partial access sequence with the card's extended state. Legacy mode preparation
returns the RAMDAC to indexed operation without clearing the palette or VRAM.

Selected ET4000 firmware boots in 80×25 text. Its named indexed mode presets live
in the BIOS crate and program these same registers: 640×480 (BIOS 2Eh/VBE 101h),
800×600 (30h/103h), and interlaced 1024×768 (38h/105h). VBE reports the installed
1 MiB and exposes separate write/read windows at A000h. The card supplies the
same frame queue and native drawing access as DISPI, including the DOS cursor.

VBE also exposes RGB555 at 640×480 (110h) and 800×600 (113h), occupying 1280
and 1600 bytes per row. These use SC11483 mode 1, ET4000 word output and MCLK/2.
Interlaced timing retains approximately 60 fields per second with the fitted
25.175/40-MHz oscillators. The renderer's complete-image capture policy above
also applies to these modes. Larger RGB555 images exceed the installed memory;
RGB565 and 24-bit output are not supported by this RAMDAC. These high-color
presets have VBE numbers only, with the same 7Fh BDA convention used for DISPI.
VBE pitch, viewport, bank and coordinated state services use their actual format.
Returning to an indexed or legacy mode resets the high-color command.

Sources: [Tseng ET4000 data book](https://ardent-tool.com/datasheets/Tseng_ET4000_Databook_1990.pdf),
EV8913 schematic sheets 1–3 (printed pp. 14–16), clock selection (p. 32),
configuration registers (pp. 136–138) and Revision E addendum (pp. 184–185);
[Sierra SC11482/SC11483/SC11484 data sheet](https://www.dosdays.co.uk/media/sierra_semi/SC11482CV.pdf),
high-color formats and command protocol (pp. 5–6).
