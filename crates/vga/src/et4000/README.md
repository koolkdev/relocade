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
CRTC 36h = 40h and CRTC 37h = 0Bh. These choices retain VGA address mapping and
describe the installed DRAM. SC11483 command state starts at zero (indexed mode),
with no pending command-register access. Registers whose power-on contents are
unspecified use zero until firmware programs them.

The current profile definition supplies capacity. The ET4000 register extensions
and firmware are added separately from the shared VGA composition.

Sources: [Tseng ET4000 data book](https://ardent-tool.com/datasheets/Tseng_ET4000_Databook_1990.pdf),
EV8913 schematic sheets 1–3 (printed pp. 14–16), clock selection (p. 32),
configuration registers (pp. 136–138) and Revision E addendum (pp. 184–185);
[SC11483 application documentation](https://vgamuseum.info/images/doc/chips/64200.pdf#page=104).
