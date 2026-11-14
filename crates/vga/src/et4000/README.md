# ET4000AX profile

The card uses an ET4000AX Revision E, 1 MiB of DRAM and a Sierra SC11483 RAMDAC.
Its reference is Tseng's EV8913 Revision A AT-bus design, with all eight
256K×4 DRAMs fitted. The SC11483 is an explicit RAMDAC substitution for this
emulated profile. It starts in indexed VGA operation; RGB555 support is separate.

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
Attribute address/phase latch for both standard and extension registers.

The selected adapter-enable address is 3C3h or 46E8h; Rev E aliases 46E8h on
its low twelve address bits. Extension configuration registers retain their
documented bits. Translation-ROM, external genlock and 6845 compatibility
behavior are outside this profile. CPU access uses independent read/write segments
in 3CDh. Chain-4 selects linear bytes; the plane system retains VGA read latches
and graphics writes. Both address the same DRAM, with no conversion on writes.
The board exposes the segmented host aperture; CR36h's optional relocated linear
host aperture is not connected.

Full-pixel indexed scanout derives its size, pitch and 18-bit start counter from VGA and
extension registers. It uses the shared packed renderer and native drawing view,
including panning across the physical DRAM boundary. Packed output captures CRTC
blanking as pixel intervals, including blanking across counter reset. CPU banking never selects
the displayed page. The data-book 640×480×8 register sequence is exercised through
the bus and GPU renderer; no host mode table supplies that geometry. Serialized
VGA 256-color modes retain the VGA decoder's row, split-screen, serializer and
blanking rules, with the card's consecutive memory addressing. Firmware and PC
selection are separate.

Sources: [Tseng ET4000 data book](https://ardent-tool.com/datasheets/Tseng_ET4000_Databook_1990.pdf),
EV8913 schematic sheets 1–3 (printed pp. 14–16), clock selection (p. 32),
configuration registers (pp. 136–138) and Revision E addendum (pp. 184–185);
[SC11483 application documentation](https://vgamuseum.info/images/doc/chips/64200.pdf#page=104).
