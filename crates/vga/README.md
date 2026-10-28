# VGA text display

Register storage and port access live in `src/registers/`. CRTC indices, write
protection and decoded counter fields belong to `crtc.rs`; sequencer reset and
clocking fields belong to `sequencer.rs`. The public `registers` module gives
firmware the same port addresses and register names used by the device. Its
`text_mode` preset supplies both device defaults and BIOS mode programming.
`timing.rs` combines decoded fields into raster counts and owns exact
time/character-clock conversion. `scanout.rs` tracks
the live raster position and produces Input Status 1.

Fractional character progress is retained between advances. Elapsed nanoseconds
multiply the oscillator frequency; division by the scaled character width gives
whole character clocks and a remainder. Changing the character width rescales
that remainder to preserve the fractional position. Rounded sleep periods never
feed back into the counters.

`Vga` is the emulated device. The execution worker owns its ordinary mutable
video memory and hardware registers. It implements `relocade_machine::Device`.
System RAM contains BIOS bookkeeping, not the framebuffer. The board maps
`B8000..BFFFF` as MMIO and connects
the same device to that aperture and `PORT_RANGES`. Those ranges include the
color/monochrome CRTC aliases, Input Status 1, and sequencer/clock registers.

The initial supported mode is 80 by 25 characters with fixed 8-by-8 CP437 glyphs,
PC colors, attribute blinking and a hardware cursor. The color text aperture
provides 32 KiB, enough for eight BIOS pages. Odd/even CPU bytes select character
and attribute planes. Scanout uses the full 16-bit CRTC cell address into two
64-KiB logical planes, including starts within a page and wraparound at FFFF.

CRTC indices 0A/0B control cursor shape, visibility and skew; 0C/0D control display
start; 0E/0F control cursor location. Byte and paired index/data port writes work.
Horizontal/vertical totals, display-enable ends, vertical retrace start/end and
counter overflow bits determine raster timing. CRTC 11 protects indices 00–07,
except the line-compare overflow bit. CRTC 17 controls sync output and the
vertical-counter clock divisor. Maximum character height and double scanning do
not multiply totals that already count physical scan lines.

Miscellaneous Output selects the 25.175 or 28.322 MHz oscillator and color or
monochrome I/O addresses. Sequencer 00 controls reset; 01 selects 8/9-dot character
clocks, dot-clock division and screen-off. Screen-off preserves sync; its bit is
stored, but pixel blanking is not yet applied to rendered snapshots.
External/reserved clock selections have no connected oscillator.
Input Status 1 reports inverted display enable (bit 0) and vertical retrace (bit 3)
from the current device clock, including during a guest's synchronous polling loop.

The annotated preset in `registers/text_mode.rs` supplies the initial
register bytes: 800 dots per line and 449 lines per frame at 25.175 MHz,
approximately 70.086 Hz, with 400 active lines and double-scanned 8-row characters.
The rasterizer still produces the existing logical 640-by-200 text image.
Programming timing does not yet change its text geometry, glyphs or palette.
Horizontal blanking/retrace and vertical blanking registers retain their values;
their effect on individual output pixels is not implemented. Unsupported register
indices read FF and ignore writes.

BIOS services maintain their BDA mode, page and per-page cursor data and program
these registers. Direct guest port writes affect the device without updating the
BDA. Direct guest writes, BIOS output and DOS output all reach the same VRAM.

`Vga::with_clock(Rc<C>)` accepts the shared session clock. Guest I/O advances
raster counters before reading or writing
registers. Clock changes account for elapsed time under the old settings and
preserve the live counter position. Reset clears the counters; an unconnected
oscillator freezes them. The timebase is host elapsed time, not CPU cycles.

Retain an `Rc<RefCell<Vga>>` on the execution worker, independently of the CPU's
current run. `screen()` copies VRAM and registers into an owned `TextScreen`.
Release the device borrow before polling execution or awaiting other work.

```rust,ignore
use relocade_vga::TextRenderer;

// On the execution worker, including while the CPU awaits input:
let screen = dos.screen();
// Transfer this owned screen to the renderer worker.
let mut text = TextRenderer::new();
let rgba = text.render(&screen, elapsed);
renderer.render([TextRenderer::WIDTH, TextRenderer::HEIGHT], rgba, &output_view)?;
```

The snapshot stays stable across later device writes. Device callbacks borrow
mutable state only for the synchronous transfer; BIOS operations spanning several
transfers may be observed between them. The live device cannot cross threads.

The renderer worker can own the wgpu device, queue, surface and presentation loop.
The embedding supplies platform surface setup, transfers owned screen snapshots,
and forwards resize and lifecycle messages. These crates do not create an
application event loop.

The shared [worker integration check](../../tests/browser/src/vga.rs) exercises
guest MMIO and CRTC writes, snapshot delivery while input is pending, and snapshots
that remain readable after CPU teardown on native threads and browser workers.

The text geometry, memory aperture and CRTC register definitions follow the
[IBM VGA/XGA Technical Reference](https://bitsavers.trailing-edge.com/pdf/ibm/pc/cards/IBM_VGA_XGA_Technical_Reference_Manual_May92.pdf),
VGA Function pages 2-12, 2-24–2-26, 2-42–2-50 and 2-55–2-76.
The supported subset does not yet implement every feature of a VGA adapter.
