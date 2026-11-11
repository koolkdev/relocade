# VGA compatibility

`Vga` owns 4 MiB of shared video memory, registers and programmable timing on the
execution worker. The first 256 KiB supplies the four 64-KiB legacy VGA planes.
BIOS and direct guest writes reach the same MMIO memory. Text decoding
uses captured geometry, row stride, physical address mapping, 8/9-dot characters,
font banks and heights, attribute/DAC colors, cursor shape/skew, underline,
blinking, blanking, panning and split-display settings. Startup BIOS mode 03h
loads an 8×16 CP437 font into plane two and initializes 80×25 nine-dot cells:
720×400 active dots at about 70 Hz. Later mode sets honor the guest's scanline
preference; startup does not impose a separate renderer mode.
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

Sequencer Shift Load (SR01 bit 2) loads the graphics serializers every other
character clock; Shift 4 (bit 4) takes precedence and loads every fourth clock.
With both clear they load every clock. The CRTC address counter advances
independently, so a delayed load samples its current address rather than repeating
the last eight pixels. Graphics byte panning advances character clocks before
load alignment and address counting; combined with pel panning it can select any
of the 32 dots in a four-clock load. These controls follow the
[IBM reference](https://www.mcamafia.de/pdf/ibm_vgaxga_trm2.pdf), pp. 2-49–50,
2-63 and 2-99. Text uses its separate Attribute Controller serializer.

Between loads, four-bit outputs rotate through the chained serializers before
attribute masking and color lookup. In 256-color graphics this applies to each
nibble before they form a DAC index. Chaining follows the
[82C451 data sheet](https://www.ardent-tool.com/datasheets/CHIPs_82C451_alt.pdf),
p. 55, after the Graphics Controller's bit packing. Circular feedback from output
0 into serializer 3 and a fresh load at the unpanned start of each scanline are
explicit emulation conventions, not measured original-IBM behavior. The Type 2
implementation that ignores these controls is not selected. Nonstandard nine-dot
graphics serialization and within-line changes remain outside this model.

BIOS startup uses 28.322 MHz, 900 dots per line and 449 lines per frame
(about 70.087 Hz), matching IBM VGA's color-monitor mode 03h default
([VGA Technical Reference, p. 2-12](https://bitsavers.org/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf)).
The bare device's legacy register defaults remain 25.175 MHz and 800 dots per line.
Guest clock selection, 8/9-dot width, clock division and CRTC
counters determine Input Status 1 and capture deadlines. Mode 12h uses 800×525
at about 59.94 Hz. The vertical-counter divider scales display height and split
position independently of character height and repeated scanlines.

`timing/vga` decodes programmable register fields, including the four-bit retrace
end comparison and its wrap across the vertical counter reset. `timing/svga`
supplies the fixed preset. Both produce common `Timing` counts, explicit retrace
intervals and clock rates. `Raster` retains position, fractional progress and
the lifetime retrace count; it knows no CRTC encoding or VGA blink divider.
Status reads reuse cached bits until the next possible display-enable, retrace,
or internal capture edge. Repeated reads before that deadline only record the
observed time and compare it with the deadline. Due edges enter a separate cold
update routine for raster arithmetic, display-start latching and capture, keeping
capture temporaries out of ordinary polls. Timing changes integrate whole elapsed
nanoseconds under the old timing before remapping counters, retaining fractional
time and partial dots. Stopped clocks consume time without advancing the beam.
Horizontal status edges are skipped outside vertical
display. Late observations catch up all elapsed retraces without a per-frame loop.

Each component under `registers/` owns its `State`, retained port bindings,
handlers and mode selection. Simple reads show their bank operation directly;
index writes select the cached data-write effect for the masked register index.
`ports.rs` connects the components and owns the shared byte window, Misc Output, Feature Control and status aliases. Setup, Misc Output, DISPI
mode changes and state restoration select cached handlers before returning to
the guest, including when no new index write follows. Ordinary byte accesses
have no port decoding or adapter-enable/alias checks. In packed SVGA, CRTC, Sequencer, Graphics and Attribute writes retain VGA
programming without capture or timing work; leaving packed mode applies the
retained programming again. DAC, setup, Misc Output and DISPI retain their
adapter effects in both modes.
Input Status 1 services raster edges and resets the Attribute access phase;
inactive aliases return FFh without affecting that phase.

Standalone registers use `io.port(address, policy, read, write)`. The connection
claims the address and configures both functions with its scoped context. The
policy is explicit: `PortPolicy::BYTE` and `WORD` accept only that width;
`BYTE_SEQUENCE` visits successive byte addresses within the owning window. On a
single port it handles the low byte and returns FF or ignores the trailing bytes.

VGA claims shared windows with `io.ports(range, policy)` and gives components
`window.port(address)` endpoints or `window.indexed(index, data)` pairs. Each
endpoint keeps its parent's transfer boundary. Components prepare concrete
callbacks once; `port.configure(context, read, write)` publishes the final pair
directly. Its retained `read` and `write` fields are reusable definitions, not a
second copy of the active map. `IndexedPorts<T>` publishes its fixed selector
operations and the chosen data pair. Index changes replace only the data writer.

`window.configure(context, enabled, ...)` configures undeclared holes and asks
components for their final handlers. It does not reset implemented ports first.
Disabled windows ignore complete transfers, including those starting in holes;
re-enabling configures the holes and components together. Hole handlers retain
the full parent boundary so a wide access can reach a following register.
Setup controls and Input Status 1 use standalone registration. Setup stays
accessible independently of adapter decoding; each status alias selects its
cached active or disabled operations at decoding boundaries. VGA wires all
components before its configuration pass. Initial configuration uses the
connection's context; later changes use the callback's current context.

VGA word/dword accesses visit successive bytes within the parent window, even
across component boundaries. Each device borrow ends before the next operation
is looked up. Holes and read-only starting ports still process their tails.
DISPI accepts one complete word at each port. `DeviceCallbacks` adapts concrete
device methods without owning registration; independently shared state can use
plain callbacks. `programming.rs` owns the three write effects: register mutation
only, capture before mutation, and raster advancement followed by mutation and
timing reconciliation. `RegisterWrites` prepares those concrete handlers once;
index/mode changes choose one, so data writes do not classify their effect.
Attribute and DAC use the same display wrapper with their own access protocols.
DISPI retains its value-dependent VRAM clearing and layout completion. Each
component's `State` owns indices, masks, protection and access phases.
`state.rs` owns save/restore encodings; restoration replaces component state,
preserves its endpoints and reapplies display configuration.

`LegacyRegisters` groups the live VGA-compatible components, including the DAC
shared with packed modes. Memory can borrow this owner separately from mutable
VRAM. `Vga::registers()` explicitly copies an independent `Registers` inspection
snapshot; ordinary port access, MMIO and display capture read component state
directly and never construct this snapshot.

`device.rs` owns MMIO and the output activity. Ordinary reads, index writes and CPU bank
changes do not advance the raster. Display writes compare the cached retrace
deadline and preserve overdue inputs before mutation; timing writes additionally
reconcile the old phase. Timing changes replace the pending deadline. In VGA,
sequencer reset holds the raster, and disconnected external or reserved clock
selections stop it. Screen-off continues to generate retraces. Disabling CRTC
sync suppresses the status pulse while internal sampling
continues. Integer arithmetic carries fractional progress so repeated polling
cannot accumulate drift.
The shared clock uses the session's configured timebase, including native
instruction progress when a guest clock is selected.

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

CPU address decoding follows the PC/AT adapter convention: write-only port
46E8h bit 3 enables normal VGA I/O and VRAM access, and bit 4 selects setup.
In setup, only the adapter control and option byte at 0102h are accessible.
Option bit 0 must also be set for normal access; it can be changed only in setup.
Blocked reads return FF and writes do nothing, preserving memory latches,
register indices and transfer phases. Miscellaneous Output bit 1 remains an
independent CPU RAM gate. Address decoding does not blank output or stop timing;
its controls stay on the device rather than invalidating display captures.
The device starts enabled, as it does for its other post-initialization defaults.

This uses the [IBM adapter ROM's setup sequence and BL=32h service](https://github.com/jeffpar/pcjs/blob/master/machines/pcx86/ibm/video/vga/1986-10-27/IBM-VGA.asm)
(x95–xAA and x96B–x97E). The PC/AT control bits and write-only 46E8h port are
also described in the [Video Seven VGA reference](https://www.minuszerodegrees.net/manuals/Video%20Seven/Video%20Seven%20-%20V7%20VGA%20chip%20-%20Technical%20reference%20manual%20-%20JUN88.pdf)
(pp. 3-16–17). Setup access with bit 3 clear is an explicit emulation convention,
consistent with IBM's sequence, not a hardware-validated priority rule. Video
Seven documents disable taking priority instead. Motherboard controls at 0094h/03C3h,
option-ROM banking, additional adapters and clone-specific aliases are not modeled.

Embeddings register `MEMORY_RANGE` (A0000–BFFFF) as MMIO on both the machine's
physical map and device bus. The Graphics Controller selects the active aperture;
reads outside it return FF and writes are ignored. BIOS modes 00h/01h use 40
text columns and modes 02h/03h use 80, all with 25 rows and restored font memory.
The even/odd pairs produce identical VGA color output. Their eight pages occupy
0800h or 1000h bytes each. The 200/350-line color profiles use eight-dot cells.
The 400-line profiles use nine-dot cells at 28.322 MHz: 360×400 in forty columns
and 720×400 in eighty columns. Their CRTC timing, cursor shape and unshifted
panning value follow the [IBM VGA ROM register tables](https://www.pcjs.org/blog/2015/06/01/).
Mode 07h uses 80×25 nine-dot monochrome cells, B0000 memory and the 3B4/3BA
CRTC/status ports. It selects 350 lines with a 14-high font, or 400 lines with
a 16-high font when requested through AH=12h/BL=30h. Its eight 1000h-byte pages
share the same character and attribute planes as color text: a no-clear mode
change moves the CPU aperture without copying or replacing those planes.
The ordinary text shader renders monochrome intensity, reverse video, underline
and line graphics from the programmed palette and registers.
Mode 13h programs the standard 320×200 layout
and 256-color palette. Planar 16-color modes 0Dh (320×200) and 0Eh (640×200)
use the RGBI default palette; modes 10h (640×350) and 12h (640×480) use EGA
palette mappings. Modes 0Eh and 10h expose an 80×25 character grid with 8-high
and 14-high fonts respectively for BIOS graphics character services.

Mode 11h uses the 640×480 raster with an 80×30 BIOS grid and a 16-high font.
Plane zero holds the conventional one-bit bitmap, but the IBM preset retains
four-plane CPU access and maps every nonzero attribute color to DAC entry 63 (white).
Read-mode-one comparisons use plane zero only. The shared planar pixel and
scroll services retain four-bit colors, so guest palette programming can expose
the other planes. AH=09h forces white glyphs in this mode, retaining XOR;
AH=0Ah and teletype use the supplied color. These details follow the
[IBM VGA ROM](https://github.com/jeffpar/pcjs/blob/master/machines/pcx86/ibm/video/vga/1986-10-27/IBM-VGA.asm)
(mode table and routine x272B).

Mode 0Fh uses 640×350 with two 32-KiB pages per plane, an 80×25 BIOS grid and
a 14-high font. Its monochrome CRTC and status ports coexist with the A0000
aperture. Display planes zero and two select black, white, blinking white and
bright white through the IBM attribute/DAC palettes and shared graphics blink.
BIOS drawing still reads and writes all four color bits; planes one and three
remain stored but are masked out of the default display.

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
the requested graphics page and is ignored for pixel calls in single-page modes.
These BIOS services assume the layout established by mode setting; they do not
infer arbitrary guest-reprogrammed layouts. Unsupported modes, out-of-range
coordinates and invalid graphics pages return AH=86h with carry set. This bounds
policy is local; the BIOS interface does not specify out-of-range results.
AH=02h/03h keep per-page graphics cursors in BIOS memory without enabling a
hardware text cursor. AH=05h selects pages 0–7 in mode 0Dh, 0–3 in mode 0Eh,
0–1 in modes 0Fh/10h, and page 0 in other graphics modes. Those planar pages occupy
2000h, 4000h and 8000h bytes per plane respectively; page selection programs
the CRTC start address in the units of the mode preset.

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
wrapping. CGA-compatible modes 04h–06h use INT 43h for characters 00h–7Fh and
INT 1Fh for 80h–FFh, with fixed eight-high cells. Planar and indexed modes use
INT 43h for all characters, including with eight-high fonts. Character writes read a
complete glyph before changing VGA access, so a source in VRAM uses the caller's read
settings. The character grid comes from the BIOS data area and is bounded by
the graphics layout. Font height also follows BIOS data except for the fixed
eight-high packed modes. Native planar masks, indexed rows or packed
bytes write the same MMIO memory as pixel services, with no renderer-side
character path.

AH=0Eh uses the active page and BL foreground in graphics modes. It handles BEL
(currently silent), backspace without erasure, CR and LF, wraps at the right edge
and scrolls at the bottom with black fill. DOS console output shares this
teletype implementation with foreground 7, alongside its existing byte stream.

AH=13h writes CX characters from ES:BP to BH's page, starting at DH:DL.
AL=00h/01h use BL for every attribute; AL=02h/03h read alternating character and
attribute bytes. Source offsets wrap within the segment and reads stay live if
the source overlaps video memory. AL bit 0 publishes the final cursor; otherwise
the stored cursor is unchanged. Zero characters is a complete no-op. Strings
share teletype controls, wrapping and scrolling, but ordinary characters use
AH=09h drawing, including mode 11h's white/XOR rule. Text scrolling takes its fill
attribute from the cursor position after advance; graphics scrolling fills black.
The local output cursor and explicit scroll page let inactive-page strings render
without switching the visible page or moving its hardware cursor.

These string formats follow the
[documented BIOS interface](https://fd.lod.bz/rbil/interrup/video/1013.html):
inline attributes accompany control characters too, and scrolling targets BH's
page. This deliberately differs from early IBM ROM quirks that consume attributes
only for printable characters or scroll the active page. AL values above three
and invalid pages for nonempty strings return AH=86h with carry set.

AH=08h reads a text cell or recognizes a character in graphics modes 04h–06h
and 0Dh–13h. Graphics reads sample the cell at the requested page's stored BIOS
cursor, then search the live fonts for the first exact match. Modes 04h/05h treat
every nonzero two-bit pixel as set; mode 06h uses its bitmap directly. These
packed modes sample eight rows and search INT 43h characters 00h–7Fh, followed
by INT 1Fh characters 80h–FFh unless that vector is null.

Planar modes sample inverted VGA read-mode-one comparisons, retaining the live
Color Compare and Color Don't Care registers. With the default compare color
zero, this recognizes pixels with a nonzero bit in any cared plane. Mode 13h
treats every nonzero pixel byte as set. Planar and indexed modes use the BIOS
font height and search all 256 characters at INT 43h. DAC colors do not affect
recognition. Font offsets wrap within their segment, using the shared memory
transfer API. The BIOS restores the caller's access registers before reading
the font, including when the font resides in VRAM.

Graphics returns AH=0 and AL=the matched character, or AX=0 when no glyph matches.
Duplicate and blank glyphs return the lowest matching code. This follows the
[IBM VGA ROM](https://github.com/jeffpar/pcjs/blob/master/machines/pcx86/ibm/video/vga/1986-10-27/IBM-VGA.asm)
(x2579–x2724 and x2A04–x2A1E). Cursor/page state and VGA access registers are
preserved; memory reads still load the hardware latches. Out-of-grid cursors
and planar/indexed font heights outside 1–32 return zero; invalid pages return AH=86h
with carry set. Rust callers use the same service as
`read_character(memory, io, page) -> Option<CharacterCell>`, with attribute zero
for graphics. Display captures and rendering are unaffected.

BIOS text operations read columns, rows and page allocation from the BIOS data
area. Cursor addresses, character access, teletype wrapping and scroll windows
share that geometry. Valid pages fit the 32-KiB text aperture and the eight BIOS
cursor records. This firmware bookkeeping does not control rendering; VGA
scanout still follows its captured registers and memory.

AH=12h/BL=34h enables legacy cursor emulation with AL=0 or disables it with
AL=1, returning AL=12h. Other selections return AL=0 without changing state.
The policy lives in BDA 487h bit 0, survives mode changes and affects subsequent
AH=01h calls; changing the policy does not rewrite the current hardware cursor.
AH=01h always stores the requested CX for AH=03h. With emulation enabled it
translates legacy shapes to the BIOS font height before programming the CRTC:
for example, 06h/07h becomes 0Dh/0Eh with a 16-high font. Font layout and mode
setup program native shapes directly, bypassing legacy emulation even for custom
font heights. The cursor module owns this policy, translation and CRTC writes.

The enabled translation follows the IBM VGA ROM (x1EA9–x1F2E), including its
range checks, reversed ranges and hidden-cursor representation. BDA 487h bit 3
bypasses translation for an inactive adapter. Emulation remains disabled at
startup, preserving raw VGA shapes. Disabled emulation also preserves the raw
hide bits; this deliberately differs from IBM's unconditional normalization of
CH bits 6:5 = 01b to the empty range 1Eh/00h. Rendering uses only the resulting
CRTC registers, with no BIOS cursor policy in the renderer.

BIOS AH=11h supports guest font loading (AL=00h/10h), built-in 8×14
(01h/11h), 8×8 (02h/12h) and 8×16 (04h/14h) fonts, bank selection (03h), and
font information (30h). Fonts are the CC0 [Modern DOS family](../bios/assets/README.md).
Guest data uses ES:BP, BH bytes per glyph, CX glyphs starting at character DX,
and bank BL. The source offset wraps at 64 KiB. Uploads write only supplied rows
of each 32-byte plane-two slot, preserve other banks and restore VGA CPU access
settings. Loading a bank does not select it; AL=03h separately programs SR03.

AL=1xh also changes CRTC character height, clears double scanning, sets the
cursor at the bottom of the glyph, and updates BIOS rows, height and page size.
In mode 07h it also moves the underline to the last glyph row; color-text
underline settings remain unchanged.
These calls require active page zero. Pages round up to 2-KiB blocks: 80×25 uses
4 KiB and 80×43/50 uses 8 KiB. Vertical timing and display extent remain intact,
so a 350-line display with an 8-high font has 43 complete rows plus six scanlines
of the next row. Changing back to 14-high restores 25 complete rows. AL=0xh
leaves geometry unchanged. A zero-count guest upload leaves font memory alone;
AL=10h can still change geometry. Invalid banks, heights outside 1–32, ranges
beyond 256 glyphs, and geometry that cannot fit BIOS row fields or text memory
return AH=86h with carry set before changing state. These bounds and page
allocation are firmware policy rather than a claim about every VGA BIOS.

AH=12h/BL=30h selects the next text profile and returns AL=12h. For color text,
AL=0 keeps the existing 200-line logical profile (8-high font, doubled on a 400-line raster);
AL=1 selects 350 lines with a 14-high font; AL=2 selects 400 lines with a 16-high
font. Color profiles start at 40×25 or 80×25 according to the mode, with eight-dot
cells for 200/350 lines and nine-dot cells for 400 lines.
Mode 07h uses 350 lines for AL=0/1 and 400 lines for AL=2, with nine-dot characters.
The preference lives in the BIOS data area, does not change current scanout,
and survives graphics mode sets.
Loading an 8-high font with AL=12h then yields 43 or 50 rows on the 350/400-line
rasters. Text mode setting still resets complete font slots even when AL bit 7
preserves character and attribute memory.

AL=30h returns ES:BP to guest-readable ROM font data or the current INT 1Fh/43h
vector selected by BH. CX reports the current display's font height, independent
of the requested ROM font, and DL reports the last BIOS row. BH=5/7 return
terminated, empty ninth-column replacement tables. Initialization sets INT 1Fh
to the upper half of the 8×8 font; later mode sets preserve that vector and
update INT 43h to the mode's built-in font. Font uploads alone preserve both
vectors. Font information also works in graphics modes. These ABI details follow the
[font services](https://www.ctyme.com/intr/rb-0136.htm),
[font query](https://www.ctyme.com/intr/rb-0158.htm) and
[scanline selection](https://www.ctyme.com/intr/rb-0167.htm) interfaces.

AH=11h/AL=20h sets INT 1Fh to ES:BP. AL=21h sets INT 43h to ES:BP and the
BIOS character height to CX; this firmware supports heights 1–32 and rejects
other heights before modifying state. AL=22h/23h/24h select the ROM 8×14/8×8/8×16
graphics fonts. For AL=21h–24h, BL selects 14, 25 or 43 rows (1, 2 or 3), or
uses DL (BL=0). IBM's fallback for other BL values is 25 rows; DL=0 records
last-row index FFh. These calls only change BIOS vectors and geometry, including
when called in text mode. They preserve VRAM, VGA registers, pages and cursors.
Subsequent graphics character operations read the live font source and clip the
BIOS grid to the graphics layout. Packed modes keep eight-high drawing and
scrolling even if the reported BIOS font height changes. These behaviors follow
the IBM VGA ROM (x3639–x36A1, x27BE, x28CB and x294F).

BIOS font handling owns ROM data, temporary plane-two access and validated
geometry transitions in separate modules. Mode setting reuses that same font
writer. Font bytes and bank selection reach captures and GPU uploads through
the existing VRAM/register path.

BIOS AH=0Bh implements legacy background/intensity (BH=0) and palette selection
(BH=1) through VGA Attribute Controller registers. The firmware retains mode
and color shadows at BDA 465h/466h; graphics mode sets above 07h preserve them.
With the monochrome CRTC port recorded in the BIOS data area, AH=0Bh is a
complete no-op, including the shadows. VGA AH=10h palette services remain available.
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
and cannot be set in mode 13h. AX=101Bh converts CX colors starting at BL to
grayscale through the DAC ports. The index wraps at 256; BH is ignored, as in
IBM's port-based implementation. Conversion uses six-bit inputs and IBM's
15-bit weights (9830 red, 19333 green, 3604 blue), rounded by adding 16384
before shifting. This preserves the rounding of the
[IBM VGA ROM](https://github.com/jeffpar/pcjs/blob/master/machines/pcx86/ibm/video/vga/1986-10-27/IBM-VGA.asm)
(x2DB9–x2E33), including boundary colors that differ from an eight-bit approximation.
Zero colors is a complete no-op, matching this firmware's other block transfers;
it deliberately does not reproduce the ROM's zero-count LOOP underflow.
Palette changes use the ordinary captured register upload; they do not rewrite
VRAM or select another rendering path.

AH=12h/BL=31h controls default palette loading: AL=0 enables it and AL=1
disables it. With loading disabled, mode setting preserves Attribute Controller
entries 00h–0Fh and overscan 11h, plus DAC colors, mask and transfer position.
Mode control, plane enable, panning, color select and display enable still follow
the new preset. Explicit palette services remain effective.

AH=12h/BL=32h enables CPU video addressing with AL=0 and disables it with AL=1,
by writing 0Eh or 00h to the adapter control port at 46E8h. Valid selections
return AL=12h; others return AL=0 without accessing the device. Other registers
and flags are preserved, following the IBM adapter ROM (x96B–x97E).
The service changes no BIOS bookkeeping, display registers or VRAM. It neither
blanks output nor overrides the separate option-byte sleep and RAM-enable bits.
BIOS calls and direct guest writes therefore share one address-decoding state.

AH=12h/BL=33h enables grayscale summing with AL=0 and disables it with AL=1.
It changes future mode-palette loads and explicit BIOS DAC writes (AX=1010h/1012h),
without recoloring the current palette. Direct guest port writes remain raw.
The controls persist in BDA 489h across mode and scanline-profile changes.
Bit 3 disables defaults and bit 1 enables summing; IBM's independent monochrome
display bit 2 also enables summing. Preserving a mode palette takes precedence
over conversion. Valid calls return AL=12h; other AL values return AL=0 without
changing state or flags. These details follow the same IBM ROM (x955–x994,
x1C2B–x1C77 and x2CC2–x2CF4).

`PaletteLoad` owns the preserve/color/grayscale policy. Presets and explicit BIOS
color writes share its DAC transfer path and the same fixed-point conversion.

AH=12h/BL=36h enables screen output with AL=0 and disables it with AL=1.
It changes only Sequencer Clocking Mode bit 5 and leaves the sequencer index at
01h. CPU memory access, palette data and raster timing continue while output is
blanked; captures retain guest writes for the next visible image. Mode setting
restores the preset's enabled output. Other AL values leave device state alone.
All selections return AL=12h and preserve flags, following the IBM VGA ROM
(xA1B–xA2A and x127A–x128F). Screen output uses the existing register capture and
blanking shader, with no additional BIOS display state.

BIOS adapter identification is separate from the current video mode.
AH=12h/BL=10h reports the configuration in BDA 487h/488h: adapter color/mono
assignment, memory-size code, feature bits and compatibility switch settings.
Startup describes a 256-KiB color VGA with raw cursor shapes (cursor emulation
disabled). AX=1A00h returns active/alternate display codes; AX=1A01h selects an
entry in the live display-combination table through BDA 48Ah. With two descriptions,
the equipment word chooses the active one. Unknown combinations return FFh codes.
These calls preserve VGA registers and VRAM; describing another adapter does
not create one. Mode changes preserve this metadata, including monochrome modes
07h/0Fh, which still run on the same VGA. The behavior follows the
[IBM VGA ROM](https://github.com/jeffpar/pcjs/blob/master/machines/pcx86/ibm/video/vga/1986-10-27/IBM-VGA.asm)
(x870 and xA2D–xAD2).

BDA 4A8h points to the 28-byte video save-pointer table. Its first pointer selects
the VGA mode parameter table; the fifth selects a 26-byte secondary table whose
first pointer selects the display-combination table. BIOS initialization publishes
these structures in ROM, using the layouts in the
[IBM BIOS Interface reference](https://bitsavers.org/pdf/ibm/pc/ps2/PS2_and_PC_BIOS_Interface_Technical_Reference_Apr87.pdf)
(April 1987, figures 3-20/21). Guests can copy them to RAM and replace their
pointers. Services follow the current pointers, with 16-bit offset wrapping.

Mode setting reads a 64-byte record containing geometry, SR01h–04h, Miscellaneous
Output, CR00h–18h, AC00h–13h and GC00h–08h. The existing presets produce the ROM
records; one register programmer handles both defaults and guest replacements,
including cursor shape and display-start fields. The default DAC palette and ROM
font still follow the BIOS mode/profile. IBM's record order is retained: 350-line
text uses records 19–22, 400-line text uses 23–25, and modes 11h–13h use 26–28.
This 256-KiB VGA uses records 17/18 for modes 0Fh/10h; unused EGA-only slots
8–12 and 15/16 are zero. INT 1Dh separately exposes the PC BIOS's 88-byte legacy
6845 parameter table; it does not select VGA mode records.

Custom records change hardware and BIOS geometry. Graphics BIOS pixel and page
addressing still use the standard mode layouts; arbitrary register-programmed
graphics layouts require direct guest VRAM access. Text BIOS addressing already
uses the programmed BIOS geometry. Optional override pointers are initially null.

Mode setting applies primary and secondary font overrides whose FFh-terminated
mode lists include the requested mode. The primary text descriptor specifies
font height, bank, glyph range, source pointer and row count. It reuses the AH=11h
font loader and geometry calculation; an explicit row count replaces BDA 484h
after that calculation, without changing the computed page allocation. Loading
a primary bank does not select it: SR03 from the parameter record still chooses
normal font B. The secondary descriptor loads all 256 glyphs and selects only
alternate font A, preserving B, when its height matches the current BIOS height.

The graphics descriptor changes INT 43h and BIOS font height/rows without loading
plane two or changing raster timing. Font sources and headers wrap their 16-bit
offsets. The shared loaders accept banks 0–7 and heights 1–32; invalid ranges
leave the mode defaults. Malformed mode lists stop after one segment rather than
looping forever.

The primary table's second pointer optionally selects a 256-byte dynamic palette
save area. BIOS mode-palette loads, AH=0Bh color changes and AH=10h attribute
palette writes maintain its first 17 bytes: entries 00h–0Fh followed by overscan.
They store the requested bytes before hardware masking and preserve bytes
17–255. Palette-loading suppression also suppresses mode-set writes to this
area. Direct guest I/O and hardware-only AH=1Ch restore do not update BIOS RAM.
The BIOS attribute writer owns this bookkeeping; raw attribute access remains
available to hardware-state services.

The secondary table's palette profile pointer selects a 20-byte header followed
by a mode list. Matching profiles apply after fonts, loading an Attribute
Controller range and a DAC range through the existing BIOS palette writers.
Logical attribute entry 16 is overscan; a partial palette range never writes
Mode Control or an unrequested overscan entry. DAC colors obey grayscale summing
and wrap the DAC index. Sources and headers wrap their segment offsets.
Attribute ranges must fit entries 0–16; DAC descriptors accept starting indices
0–255 and counts 0–256. Invalid ranges and zero counts perform no transfer.

BL=31h suppresses both palette transfers but not the profile's underline flag.
A positive flag writes current font height minus one to CRTC Underline Location;
a negative flag writes 31, and zero leaves the mode's underline setting alone.
As in the IBM ROM, this writes the whole register, so profiles should request it
only in modes where that underline/addressing configuration is intended.

AH=1Bh/BX=0000h writes IBM's 64-byte functionality/state report to ES:DI and
returns AL=1Bh. It includes the BIOS mode, geometry, pages and cursors, the same
active/alternate display codes as AX=1A00h, and live font-bank and blink settings.
Text page counts follow the current font allocation. As in IBM's ROM
(xAFC–xC52), the scanline field describes the BIOS profile rather than custom
CRTC timing; the row field is a count, not BDA 484h's last-row index. The buffer
wraps at ES:FFFF. Unsupported BX values return AL=0 without changing the buffer.

The report points to a guest-readable ROM capability table advertising only
implemented modes and services. It reports two simultaneously visible font
banks and eight total banks. Miscellaneous capability bits cover font loading,
EGA/DAC palettes, grayscale summing, default palette loading, cursor emulation,
palette paging, blink, display combination and save/restore. The table also
advertises all six save-pointer capabilities: primary and alternate text fonts,
graphics fonts, dynamic palette saves, palette profiles and DCC table replacement.
Report byte 32h reports installed override pointers, even when their
mode lists do not match. It marks a DCC replacement when its segment differs
from the BIOS segment, following the IBM ROM; the default table does not set this
bit. Unsupported light-pen services are not advertised. Reserved state fields
remain zero. The BIOS information module owns this ABI encoding and reuses
adapter metadata and page validation; it adds no device or renderer state.

AH=1Ch saves and restores selected video-state components through ES:BX.
CX bits 0, 1 and 2 select hardware registers/latches, BIOS data, and DAC colors
respectively; higher bits are ignored. AL=0 returns the required number of
64-byte blocks in BX, AL=1 saves the components, and AL=2 restores them. Valid
nonempty requests return AL=1Ch, while an empty mask or unsupported operation
returns AL=0 without side effects. Other registers and flags are preserved.
A restore reads the selected components through the saved header pointers, so
a subset can be restored from a larger save.

The buffer follows the [IBM ROM](https://github.com/jeffpar/pcjs/blob/master/machines/pcx86/ibm/video/vga/1986-10-27/IBM-VGA.asm)
(xCB9–x1200) and [RBIL's corrected DAC layout](https://fd.lod.bz/rbil/interrup/video/101c.html):
a 20h-byte header has absolute ES offsets at +0/+2/+4 for hardware (46h bytes),
BIOS data (3Ah), and DAC data (304h). Only selected header entries and blocks are
written; offset arithmetic wraps at ES:FFFF. Color Select is the last DAC byte,
not an overlapping palette entry as in the early IBM ROM. BIOS data includes
the save pointer at 4A8h and vectors 05h, 1Dh, 1Fh and 43h.

This BIOS buffer does not include VRAM or fonts. Saving or restoring hardware
uses AFFFFh as latch scratch space, overwriting the last byte of each physical
plane as described in IBM VGA Function p. 2-98. The ABI has no attribute flip-flop
or DAC RGB-component field; DAC transfers resume at red. These operations use
ports and memory accesses; renderer captures retain their existing immutable state.
Hardware restore unlocks protected CRTC timing, programs registers through their
normal ports, and restores register indices and display enable. Display-start changes
still latch at vertical retrace. BIOS-only restore changes metadata and vectors
without setting a mode; DAC restore bypasses grayscale and default-palette policy.

Firmware can also save complete device programming through `core_state()`,
`dac_state()` and `dispi_state()`. Their `VgaCoreState`, `DacState` and `DispiState`
values own fixed byte encodings with validated `from_bytes` constructors. VGA core
state includes sequencer reset, register access phases, the latched CRTC display start,
adapter decoding and CPU latches. DAC state includes the exact RGB transfer
position and Color Select; DISPI state includes staged geometry, active layout,
enable flags, bank and register index. These values contain no framebuffer bytes.
`restore_state(context, core, dac, dispi)` applies selected components together after
servicing elapsed captures. It preserves VRAM and unselected components, and uses
the same timing reconciliation as port programming. DAC-only restoration leaves
raster timing intact. The scoped I/O context also publishes any changed status
port routing immediately. Stopped host callers obtain it through `IoBus::with_context`.
These encodings describe Relocade's current adapter;
firmware owns the surrounding guest buffer format and version. They are not
vendor BIOS save formats. The fixed IBM AH=1Ch buffer above remains independent.

At the start of vertical retrace the device captures a complete `DisplaySnapshot`
for the following active display: shared physical memory blocks, the active
scanout settings, and DAC colors and mask. VGA first latches its display start
and also captures sequencer/CRTC/graphics/attribute settings, font memory,
the effective start address, and both blink phases.
When these inputs change, the device stages the snapshot and its retrace timestamp.
Its async task synchronizes with host time before sending the snapshot.
Unchanged refreshes produce no new snapshot.
Late service coalesces missed boundaries; display mutations first service elapsed
time using the preceding state. A write exactly at a retrace edge applies after that latch.
The capture describes selected inputs, not pixels scanned earlier in the frame.
Other registers are sampled at that same edge. In particular, a preset-row
update made after retrace starts appears in the next capture. The snapshot
approximation does not reproduce the within-blanking ordering used by
raster-synchronized smooth scrolling.

`DisplaySnapshot` owns shared `DacSnapshot` colors and mask alongside a typed
`Scanout`. `Scanout::Vga(VgaDisplayState)` retains raw display register values,
display enable, the effective start-address latch and both blink phases. CPU
register indices, access phases and DAC transfer position remain on the live
device; `Vga::registers()` copies them for independent inspection. Capture creation
copies display values without advancing any port or memory-access state.
`CrtcRegisters` keeps programmed CRTC bytes and field decoding together in both
the live device and its captured VGA payload. Indexed port access, write
protection and display-start latching remain on the live `Crtc`.
`Scanout::Svga(SvgaLayout)` owns a validated packed layout with size, byte pitch
and physical display origin. It carries no legacy VGA registers or blink phases.
The scanout variant is read-only so it stays paired with its captured memory extent.

Skipping captures loses no inputs required by the selected one. `Vga::snapshot()`
captures those same inputs immediately without publishing a frame, forcing a
display-start latch, or changing CPU access phases. `DisplaySnapshot::geometry()`
returns common image dimensions and explicit pixel aspect ratio. VGA row
collapsing and nominal 4:3 proportions belong to `VgaDisplayState::geometry()`,
whose `VgaGeometry` also records physical scanlines per output row.
SVGA owns its square pixel aspect explicitly. All pixel interpretation uses
captured display settings and VRAM directly in the shader.

Snapshots share immutable 64-KiB blocks with the device: four for VGA, and enough
blocks to cover the packed viewport's final byte for SVGA. Capturing or cloning
a snapshot copies references and register values, not VRAM bytes. A guest write
copies a shared block only when its final physical byte changes; VGA routing,
write modes and masks apply before that comparison. Subsequent writes reuse the
device's private block until the next capture. Retaining many distinct captures
can therefore retain many versions of a block.

`Vga::frames()` returns an `async_channel::Receiver<DisplaySnapshot>` for a
capacity-one queue. Sending replaces an older queued snapshot, so a slow consumer
receives recent state without a backlog. Use one consumer; cloned receivers compete
for the same snapshots. The consumer owns any snapshot it retains. Snapshots have
no RGBA image and can cross workers and survive device destruction.
Waiting on `recv()` observes changed inputs captured at a refresh boundary,
not every hardware refresh, pixel output or presentation. Comparison uses block
identities and inputs consumed by the active display without scanning VRAM.
VGA includes display registers and blink phases; SVGA uses its layout and the
shared DAC. Hidden captured memory can conservatively cause publication;
CPU port indices and transfer positions do not. Hardware
timing and status-port behavior continue through unchanged refreshes, while the
CPU waits for input and while the app is not drawing. After its task stops,
destroying VGA closes the queue and wakes waiters. Any queued snapshot can still
be received before `recv()` reports closure.

The CPU owner's VGA task calls `clock.sync_to_host(boundary).await` when it takes
a pending snapshot. This uses the mapping at task processing time and the
configured host timer. Host-clock captures are already due; paced guest
captures may be ahead of the host. Without an active mapping, the wait completes
immediately. The task keeps one capture through its wait while newer captures
replace one pending slot, merging their dirty pages. Newer captures cannot
postpone the in-flight deadline. A pause or resume before the task takes a pending
snapshot can change its mapping; an already-started wait keeps its deadline.
A newly occupied slot wakes the task even when
the guest timer has not delivered its notification. No device borrow spans a wait.
Keep the host timer service running while execution tasks have pending waits.

Captured memory and display settings are renderer inputs. An embedding can
read them directly or use the optional GPU renderer below. Device-only users do
not need GPU dependencies. `Vga::run` services retraces and publishes synchronized
captures; the embedding owns redraw scheduling, request coalescing, dimensions and presentation.
Closing the source ends capture production without invalidating retained captures
or requiring the embedding's GPU resources to close.

`geometry` defines common output dimensions and aspect; `scanout/vga` owns VGA
register interpretation and row collapsing. `VgaRenderer` owns the common VRAM buffer
separately from captured registers and palette.
`memory/pages` tracks physical writes across 64 blocks in a fixed 512-byte bitmap.
`renderer/memory` uploads those pages when their captured baseline matches the
queued GPU contents, otherwise it restores the affected block.
`renderer/state` uploads mode-specific settings separately from the shared DAC.
`renderer/pipelines` builds two cached pipelines with one explicit input layout;
the uploaded scanout selects which decoder `encode` records, even when geometry
stays the same. The packed pipeline selects pixel decoding
from the captured format.
Both shaders share physical VRAM reads, DAC lookup and fullscreen drawing.
VGA alone owns raw register decoding, blink, blanking, fonts and row interpretation.
Of the Attribute Controller's address byte, only display enable is captured.
`shaders/vga` dispatches pixels to `text`, `indexed256` or `graphics16`. All use
the scanline and physical-address logic in `addressing`. Graphics layouts also
share load alignment and chained output in
`serializer`, independently of CRTC address counting. The vertex stage decodes
frame-constant settings and the 16 attribute
colors, passing them flat to fragments in the same draw. There is no preparation
pass or intermediate GPU buffer. Further graphics decoders can read the same physical
buffer without mode-specific upload tracking or application changes.
`shaders/packed` reads pixels using byte pitch, display origin and pixel format.
Indexed pixels use the shared DAC mask and palette; RGB565 and BGRX decode their
stored channels directly, with opaque output. CPU bank selection does not scroll
the image, and legacy VGA blanking, panning and reset do not affect it.
The generic `relocade-renderer` owns image storage and attachment drawing and knows
nothing about VGA, BIOS or DOS.

Cursor and attribute blinking follow separate divisions of the existing retrace
counter. The cursor toggles every eight retraces; text and graphics attributes
toggle every sixteen. These are the 50% duty-cycle VGA rates in IBM's
[Video Subsystems Technical Reference](https://ardent-tool.com/docs/pdf/42G2193_PS2_Hardware_Interface_Technical_Reference_Video_Subsystems_Sep92.pdf),
September 1992, figure 3-3 (p. 3-16). Both phases start on as an emulator convention.
Each VGA payload owns its `cursor_blink_on` and `attribute_blink_on` values, so delayed
service and retained captures preserve the appropriate phases without another timer.

Four-bit graphics blinking uses the same captured attribute phase as text.
After Color Plane Enable masks the pixel index, Attribute Mode Control bit 3
enables replacement of palette address bit 3. Color graphics toggle that bit
for pixels whose masked bit 3 was set; other pixels use the upper palette half.
Monochrome graphics (Mode Control bit 1) toggle it for every pixel. The remaining
three index bits, attribute palette, Color Select and DAC mask work normally.
This follows the [V7 VGA Technical Reference](https://oummg.com/manual/700-0242_V7_VGA_Technical_Reference_Manual_Jun88.pdf),
pages 3-78–3-79. Eight-bit graphics bypass this attribute palette operation;
text keeps its foreground/background blink behavior. Phase changes update the
sixteen vertex-decoded colors without copying VRAM or changing pixel addressing.

`Vga::initialize(runner).await` creates VGA on the session clock, registers its
memory aperture and ports, and starts display capture. It returns the device
handle with its mappings already installed on the runner's bus.
Manually constructed devices use the same wiring in their connection scope:

```rust
runner.connect(vga.clone(), |io| Vga::configure_io(io, &vga)).await;
```

`configure_io` installs the full aperture and fixed port bindings once; mode
changes reuse those bindings and prepared handlers.

DOS supplies this same snapshot queue through `dos.frames()`. Construct
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
if let Ok(snapshot) = frames.try_recv() {
    let mut encoder = device.create_command_encoder(&Default::default());
    vga_renderer.upload(&queue, &snapshot);
    let image = vga_renderer.encode(&mut encoder).unwrap();
    let destination = relocade_renderer::fit_image(&image, output_bounds);
    {
        let mut pass = encoder.begin_render_pass(&output_pass);
        renderer.draw_image(&mut pass, &image, destination);
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
nine-dot text width, 13 flat inter-stage variables, two uniform buffers and one
4-MiB fragment storage buffer. Uploads cover only captured blocks. The settings
buffer is 128 bytes; packed scanout uses two words. The shared DAC buffer holds
a four-byte mask and 768 palette bytes at offset 16. With
the renderer feature enabled, final physical writes mark fixed 1-KiB pages.
Consecutive dirty pages in each block become one upload. Character,
attribute and font planes update independently. Hidden memory is included, so
changing the display start or font bank requires no residency bookkeeping.

Snapshots remain complete. Their private transfer hints contain dirty pages and
the immutable memory baseline those changes describe. Sampling clears the device's
local bitmap; immediate snapshots leave it alone. Snapshots coalesced before the
output task takes them merge their hints. Replacing a snapshot already in the
queue does not merge hints. Retained snapshots stay immutable, and baseline block
references retain no history of earlier snapshot objects. The renderer compares
block identities and walks the bitmap without scanning VRAM or maintaining a
separate CPU shadow.
Settings, DAC mask and palette are compared and uploaded independently when
changed. Blink, cursor or geometry updates do not resend the palette;
palette-only updates do not resend settings or font memory.

If a captured block already matches the last queued GPU version, no memory upload
is needed. If its change baseline matches, the renderer uploads dirty pages.
If the baseline differs, it restores the affected 64-KiB block. This handles
dropped queued snapshots without a sequence number or dropped-frame notification.
New renderers, other sources, immediate snapshots and reverse playback can also
have different baselines. Consecutive snapshots keep sparse transfers; skipping
queued snapshots can require larger uploads. Receiving requires no GPU-completion
acknowledgement.

The device never waits for GPU mapping or completion. Snapshot storage still
uses shared CPU blocks with copy-on-write, so a changed retained block can copy
64 KiB even for a one-byte guest write. Dirty pages limit GPU transfers; they do
not change CPU copy-on-write granularity. Device-only builds omit transfer hints
and dirty tracking along with the optional renderer.

`upload(queue, snapshot)` synchronizes persistent input resources without decoding
or presenting, and returns whether rendering inputs or output geometry changed.
Uploads can share the submission that decodes and draws the image.
`encode(encoder)` decodes the latest uploaded inputs and returns `None` before
the first upload. It always records
a decode, including retries after an encoder was discarded. Uploading inputs does
not imply that image pixels have been generated.

The main thread can coalesce new-frame notifications and resize events into one
animation-frame redraw request. On a redraw request the graphics worker selects
the latest capture, uploads and decodes its inputs if necessary, draws and presents.
A resize with no new inputs reuses the decoded image. Scheduling remains with the
embedding; VGA creates no worker, redraw queue or presentation loop.

Submit or discard a decode encoder before uploading another capture. Queue writes
run before submitted commands, and the persistent input resources hold one version.
Neither operation reads live VRAM or converts CPU pixels; the caller creates
encoders and submits work.

`encode` returns the renderer's `Image` directly, including its dimensions and
pixel aspect ratio. Submit the update before consuming its contents in later batches.
Image storage is reused while geometry stays the same; retaining it does not
retain historical pixels. Geometry changes allocate new output storage. Retain
the image and call `Renderer::draw_image` at a different destination for a
resize-only redraw, with no new VGA decode.

VGA owns the pass that writes its private image. The caller owns the output pass,
including its attachments, placement and load/clear policy, and submits the whole
command batch. Neither renderer creates a surface, worker, timer or background
task. Production rendering records commands without waiting for GPU completion.

`VgaDisplay` connects a snapshot receiver to a host through
`relocade_frontend::display::DisplayRenderer`. Its `service(queue)` waits on
`frames.recv()`, uploads the snapshot, submits those writes and requests a redraw.
The VGA task has already synchronized each publication with host time.
Unchanged refreshes leave the receive wait asleep.

`encode_image(queue, encoder)` uses only the last uploaded state, recording a
decode when rendering inputs changed. It does not receive captures or wait for
their timing. Redraws during synchronization keep the previous image; a resize
with unchanged inputs reuses it. Uploads and capture publication continue
independently of host redraws, while decoding remains demand-driven. Closing the
source preserves the image, available from `current_image()`.

Construct the adapter inside the graphics host's factory so it shares the host's
GPU environment and remains on the graphics owner:

```rust,ignore
let mut graphics = relocade_app::graphics::Graphics::spawn(
    surface_setup,
    move |device, _queue| {
        relocade_vga::VgaDisplay::new(device, frames).map_err(|e| e.to_string())
    },
    notify_redraw_needed,
);
```

The adapter retains the source and the last uploaded capture.
`device_lost()` drops its GPU resources and cached output without changing guest
VGA state. `attach_device(device, queue)` creates fresh resources and uploads the
last uploaded capture. The next
`encode_image` decodes that state, including after the source closes. Captures
still awaiting host time remain with the execution task. Automatic restoration
is VGA's policy, not a requirement of other display backends.

The host submits each recorded presentation batch before servicing the adapter
again. `VgaDisplay` relies on that submission contract for its cache. Callers that
need to discard and retry recorded commands
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
restoration. Guest-to-GPU checks run COM programs that set modes 04h–06h and
0Dh–13h, write pixels through their memory apertures, change palette colors
and restore text. Packed checks compare complete images against CPU-visible
banked byte patterns and retain palette-only updates without VRAM writes.
Mode X guests unchain the memory and flip pages without rewriting pixels.
Serializer guests combine four-clock loads with byte/pel panning and a split,
and two-clock loads with chain-4 writes and divided CRTC counting. They also
restore ordinary BIOS graphics/text modes.
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

## Banked SVGA device

The device implements a restricted banked subset of the
[Bochs DISPI display interface](https://github.com/bochs-emu/VGABIOS/blob/master/vgabios/vbe_display_api.txt).
The ID2 register subset uses the 01CEh index and 01CFh data ports.
The ID is fixed: ID writes are ignored, so this subset does not support the
ID negotiation used by stock Bochs VGABIOS or SeaBIOS. It is not a drop-in
Bochs/QEMU device or an emulation of a historical SVGA chipset.
Both ports require word transfers: a word at 01CEh selects one complete register
index, rather than writing two adjacent byte registers. Byte/dword reads return
all ones and writes are ignored. Unknown register reads return FFFFh; unknown
register writes and unsupported requests leave the state unchanged.

| Register | Readback and accepted writes |
| --- | --- |
| 0: ID | Fixed B0C2h; writes are ignored |
| 1–3: X, Y, bits per pixel | Stage a mode while disabled; active writes are ignored |
| 4: Enable | 0 disables; 1 or 81h enables a complete supported packed mode |
| 5: Bank | 0–63 selects a 64-KiB bank in 4 MiB; out-of-range writes are ignored |
| 6–7: Virtual width and height | Active logical width in pixels; read-only full-row count in VRAM |
| 8–9: X and Y offsets | Active viewport origin in pixels; out-of-bounds writes are ignored |

Entering a valid mode selects bank zero and clears all VRAM unless enable bit 7
requests memory preservation. Readback retains the accepted enable word. Repeating enable
while active preserves VRAM and the current bank. Disabling preserves VRAM,
legacy registers, DAC contents and VGA read latches. Unsupported enable flags,
including LFB combined with no-clear, are ignored as a whole. The restricted ID2
profile supports 8-, 16- and 32-bit pixels and no-clear mode entry. The 15-/24-bit
formats and linear aperture remain unsupported.

`SvgaLayout::with_pitch` and `with_origin` validate immutable candidate layouts.
Pitch changes retain pixel coordinates and fail if the viewport would no longer
fit. Only complete logical rows count toward virtual height. Layout writes do
not clear VRAM, change the CPU bank or restart raster timing. Mode entry resets
to the default pitch and origin. Inactive layout reads return zero and writes
are ignored. Each retrace captures the layout in effect at that boundary.

`SvgaMode` combines a `SvgaResolution` with a `PixelFormat`; its default
`SvgaLayout` and checked layout changes keep byte strides distinct from pixel
coordinates. The three resolutions have fixed DMT rasters, independent of pixel
format. DISPI accepts indexed eight-bit, little-endian RGB565 and BGRX8888 modes
at each resolution. The packed shader selects decoding from the captured format;
direct colors bypass the DAC palette and mask, and BGRX output is opaque.
BIOS metadata uses the same layouts; VESA mode numbers belong to firmware.

### Banked VBE profile

The BIOS reports VBE 1.2. Its `video::vbe` module provides `INT 10h/4F00h`
controller discovery and `4F01h` information for nine banked modes,
`4F02h`/`4F03h` mode control,
`4F04h` state save/restore, `4F05h` bank control, `4F06h` logical scanline control,
`4F07h` display-start control and `4F08h` DAC-format control, using the VBE 1.2
register and 256-byte buffer layouts with selected VBE 2.0 extensions. It reports
the 4 MiB adapter, a terminated ROM mode list, one readable and writable 64 KiB
window at A000h, and the number of additional complete display pages at the
reported pitch. No callable bank function or BIOS output functions
are advertised for these modes. Unimplemented functions return AL != 4Fh, while
failed supported requests return AX=014Fh, AX=024Fh for an excessively wide
scanline, or AX=034Fh for DAC-format control in direct color. All VBE results
preserve flags.

| VBE mode | Resolution | Pixel format | Default pitch | Additional image pages |
| --- | --- | --- | --- | --- |
| 0101h | 640×480 | Indexed8 | 640 bytes | 12 |
| 0103h | 800×600 | Indexed8 | 800 bytes | 7 |
| 0105h | 1024×768 | Indexed8 | 1024 bytes | 4 |
| 0111h | 640×480 | RGB565 | 1280 bytes | 5 |
| 0114h | 800×600 | RGB565 | 1600 bytes | 3 |
| 0117h | 1024×768 | RGB565 | 2048 bytes | 1 |
| 0142h | 640×480 | BGRX8888 | 2560 bytes | 2 |
| 0143h | 800×600 | BGRX8888 | 3200 bytes | 1 |
| 0144h | 1024×768 | BGRX8888 | 4096 bytes | 0 |

RGB565 stores a little-endian word with red at bits 11–15, green at 5–10 and
blue at 0–4. BGRX stores blue, green, red and an unused byte in increasing byte
addresses. That unused byte can hold application data and never controls alpha.
Direct-color mode information reports memory model 6, the channel and reserved
bit masks, and a fixed color ramp. The 32-bit mode numbers follow
[Bochs VGABIOS's OEM assignments](https://github.com/bochs-emu/VGABIOS/blob/master/vgabios/vbe.h);
the common 24-bit numbers 0112h, 0115h and 0118h remain unsupported. Guests should
enumerate the mode list and inspect mode information to choose a pixel format.

The OEM string and mode list reside in ROM and remain valid across mode changes,
query-buffer reuse and guest exit. A null `WinFuncPtr` selects INT 10h/4F05h bank
control. Controller information remains the 256-byte 1.x structure even when
the caller supplies a `VBE2` signature. Capability bits advertise VGA compatibility
and a fixed six-bit DAC. The 15-/24-bit formats, eight-bit DAC components, linear
framebuffer access and protected-mode VBE entry points are not advertised.

State service `4F04h` selects hardware, BIOS data, DAC and SVGA through CX bits
0–3. DL=0 reports the required allocation in 64-byte blocks; DL=1 saves to ES:BX
and DL=2 restores from it. The opaque buffer uses Relocade's versioned format,
not the byte layout of a vendor BIOS. Relative offsets support relocation and
subset restoration; guest offsets wrap within ES. It contains no framebuffer
bytes. All requested device components are validated before restoration;
invalid buffers, missing classes, empty or reserved masks,
and unsupported operations fail without applying state. Capture and restore
preserve unselected classes and never use scratch VRAM or clear the framebuffer.
The BIOS-data component shares the existing IBM metadata/vector implementation.
The hardware, DAC and SVGA components use `VgaCoreState`, `DacState` and
`DispiState`, respectively. DISPI's saved pitch remains a byte count in every
pixel format; the component encoding stays 18 bytes. IBM AH=1Ch retains its
separate fixed buffer format and behavior.

Logical scanlines support setting pixels or bytes, querying the current stride,
and querying the maximum stride that accommodates the visible height. Byte
requests round up to a whole pixel, and requests below the visible width round
up to that width; changes that invalidate the current viewport fail. Successful
calls return byte pitch in BX, pixel width in CX and complete rows in VRAM in DX.
DISPI's virtual-width register likewise uses pixels. `4F01h`
reports the active mode's current pitch and other modes' default pitches.

DAC-format service `4F08h` reports the device's fixed six-bit color components
in legacy and indexed modes. Direct-color modes return AX=034Fh for this service.
Set requests of six bits or more select six, following VBE's next-lower-width
rule; requests below six and unsupported selectors fail without changing the
palette or an in-progress DAC transfer. The controller capability bit remains
clear. Legacy palette services and direct port access share the same colors and
pixel mask in VGA and indexed SVGA. Direct-color mode entry preserves the DAC;
subsequent DAC writes do not change its displayed colors. Returning to indexed
rendering uploads palette changes that were skipped while displaying direct color.

Native and browser GPU checks cover RGB565 channel scaling, BGRX padding and
opacity, bank edges, panning to the final VRAM byte, and retained captures.
A native DOS guest draws both formats through byte stores and VBE bank calls,
pans with a wider scanline, saves and restores all state classes around an indexed
mode change, then returns to usable text. Full image comparisons verify both
restoration and retained output after the device is destroyed.

Display-start control supports immediate updates, position queries and BL=80h
retrace synchronization. Requests validate the whole visible rectangle;
invalid coordinates or reserved selectors leave state unchanged. The retrace
variant uses `Vga::wait_for_retrace` on the same device passed by `Pc` to BIOS
interrupt registration. The method prepares an owned future from VGA's clock
and programming notifications; BIOS releases the device borrow before awaiting
it. `RetraceEvent::Started` reports a retrace occurrence, while `Reprogrammed`
asks the caller to revalidate and prepare another wait. BIOS keeps that short
async loop and validates its mode/layout after every completion. Register access
continues through the existing synchronous `IoBus`.

Mode, layout, status-routing or adapter-decoding changes wake the wait to
revalidate the request and recalculate its deadline; index and bank writes do
not. Disabled VGA cannot prepare a wait. Suppressed sync waits for
programming to restore the signal, while internal capture boundaries continue.
Both capture activity and the retrace wait use the same timing and
notification mechanism. Waiting works without a running capture task and through
unchanged-frame suppression.

The wait reports a retrace occurrence even when a late host timer resumes after
the narrow status pulse. Revalidation services that completed boundary before
the origin changes; the following retrace captures the new origin. Immediate
updates appear at the next capture boundary. Neither path changes the CPU bank,
VRAM or an already published capture. Image-page capacity depends on pitch and
visible height, independently of the current origin.

Mode sets validate the request before changing hardware or BIOS data. Bit 15
preserves VRAM; every successful packed mode set resets the CPU window to bank
zero. Get-mode returns the mode number without that flag, as specified by VBE
1.2. Window A supports positions 0–63 while packed mode is active; window B,
unsupported operations and out-of-range positions fail without changing the bank.
The BIOS stores a 7Fh legacy mode marker so existing text, font and pixel services
avoid accessing packed VRAM: scrolling does nothing, and output/font requests fail.
Legacy mode 3 restores their normal operation. VBE mode setting also accepts
supported legacy modes and translates its preservation flag.

A supported legacy BIOS mode set disables SVGA after validating the request and
before loading fonts or writing the VGA aperture. Unsupported mode requests leave
the active SVGA mode and its memory untouched. BIOS palette calls continue to
program the same DAC through ordinary VGA ports.

While enabled, A0000h–AFFFFh reads and writes consecutive packed bytes in the
selected bank; other VGA windows return FFh and ignore writes. Multi-byte
accesses crossing AFFFFh do not spill into the next bank. Adapter setup and
Miscellaneous Output memory gating still apply. The existing six-bit VGA DAC
ports program the shared palette, with their existing byte-transfer rules.
Embeddings install both VGA and DISPI through `Vga::configure_io` and mark
`MEMORY_RANGE` as MMIO in the physical map.

Each CPU transfer resolves its adapter gates and addressing mode into `VramAccess`:
disabled, legacy VGA registers, or a validated `SvgaBankMapping` supplied by DISPI.
`VideoMemory` applies that policy to every byte, retaining ownership of VGA
latches, aperture bounds and shared storage. The device applies explicit
`WriteEffect` actions from DISPI, including `ClearVram` on mode entry. Both
register banks remain stored on the device across addressing-mode changes.

Packed byte addresses interleave physical VGA planes: addresses 0, 1, 2 and 3
are byte zero of planes 0, 1, 2 and 3; address 4 is byte one of plane zero.
That mapping repeats in each 256-KiB group throughout VRAM. Legacy VGA accesses
remain within the first group, including their existing chain-4 address mapping.
Changing addressing modes never repacks memory. Packed accesses bypass VGA
latches and write masks. Immutable 64-KiB storage blocks preserve retained
VGA snapshots and avoid allocating unused extended memory until written.

Immutable memory snapshots retain the blocks covering a display's physical
memory extent, rounded to complete groups of four interleaved blocks. Legacy
VGA captures retain the first four blocks; 640×480 packed captures retain eight.
Dirty pages are tracked throughout
VRAM; an upload uses sparse ranges only when its prior block matches the
capture's baseline, and uploads a complete block when that baseline is absent.

`Vga::snapshot()` captures active display inputs immediately; retained captures
render after mode changes or device destruction. Automatic publication uses the
same shared frame source for both modes.

SVGA uses the VESA DMT 60-Hz rasters. The 640×480 mode uses 25.175 MHz,
800×525 total dots, and retrace lines 490–492; 800×600 uses 40 MHz,
1056×628 totals and lines 601–605; 1024×768 uses 65 MHz, 1344×806 totals
and lines 771–777. Retrace ends are exclusive. Input Status 1 and capture
deadlines use the same raster.
Legacy clock selection, sequencer reset, CRTC totals and sync controls remain
stored but do not affect this mode. The legacy display-start latch updates only
at VGA retraces.

Entering or leaving SVGA first services elapsed time under the old mode, before
clearing memory or changing scanout. The new raster begins at dot zero at the
mode-switch time, while the lifetime retrace count stays monotonic. The previous
capture remains readable until changed inputs are published at the new mode's
first retrace. Repeating enable, changing the CPU bank or rejected requests do
not restart timing. Disabling restores timing from the stored VGA registers,
including a stopped clock or held reset. Mode changes wake the device activity
to replace its timer deadline. Slow consumers take the latest complete capture
with accumulated dirty pages; worker and presentation scheduling stay shared.
