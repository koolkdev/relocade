//! BIOS services for a real-mode PC, reached through ordinary runtime hooks.
//!
//! INT 11h/12h return the equipment word and conventional-memory size directly
//! from guest RAM at 0040:0010 and 0040:0013, initialized from PC configuration.
//! INT 10h sets text modes 00h–03h and 07h with output, cursors, pages and scrolling.
//! Mode 07h uses the VGA monochrome palette, nine-dot cells and B0000 aperture.
//! Graphics modes 04h–06h and 0Dh–13h program VGA for direct guest pixel
//! and palette writes, including CGA-compatible packed storage at B8000.
//! Mode setting honors AL bit 7 to preserve video memory; AH=0Fh reports the
//! mode, that flag, character columns and active page. AH=0Ch/0Dh write/read
//! graphics pixels, with XOR in packed/planar modes and eight, four or two pages
//! in modes 0Dh, 0Eh or 0Fh/10h respectively.
//! Graphics page/cursor and scroll services use that same layout. AH=09h/0Ah
//! draw characters from live BIOS font vectors; AH=0Eh and [`write_text`] share
//! teletype wrapping and scrolling in text and graphics modes.
//! AH=13h writes strings with fixed or inline attributes to a requested page,
//! sharing those controls and optionally updating the stored cursor.
//! AH=0Bh sets legacy colors through VGA attributes; AH=10h reads/writes
//! attribute and DAC palettes, blink, paging and the DAC mask.
//! AX=101Bh converts a DAC range to grayscale using IBM VGA's fixed-point weights.
//! AH=11h loads text fonts, selects font banks and reports ROM font pointers.
//! Its AL=20h–24h services select graphics font vectors and BIOS character geometry.
//! AH=12h/BL=30h selects the next text mode set's scanline profile.
//! BL=31h controls default palette loading; BL=33h controls grayscale conversion
//! for mode palettes and explicit BIOS DAC writes.
//! BL=34h controls legacy cursor-shape emulation for AH=01h.
//! BL=36h blanks or restores screen output through the VGA sequencer.
//! AH=12h/BL=10h reports adapter configuration; AX=1A00h/1A01h get/set the
//! BIOS display-combination description without changing hardware or rendering.
//! AH=1Bh/BX=0 writes the functionality/state report to ES:DI; other BX values
//! return AL=0 without writing the buffer. Other unsupported video services or
//! pixel coordinates/pages return AH=86h with carry set.
//! INT 09h consumes controller scan bytes and translates the default US layout into
//! the guest-visible BIOS keyboard ring at 0040:001E. INT 16h provides legacy
//! AH=00h/01h/02h, enhanced AH=10h/11h/12h, and AH=05h buffer insertion. DOS console
//! input uses enhanced reads from that same ring and modifier flags. Guest IRQ1 handlers
//! may replace or chain the default handler; port reads consume its input bytes.
//! Ctrl-Break empties the ring, sets bit 7 at 0040:0071, and awaits INT 1Bh.
//! After that callback returns, the BIOS inserts a zero keystroke and completes
//! IRQ1. The default callback returns without taking action.
//! INT 1Ah/AH=00h reads ticks and midnight rollover from the BIOS data area at
//! 0040:006C and 0040:0070. The IRQ0 handler advances them once per delivered tick.
//! Timer hardware supplies vector 08h. After advancing the clock, the BIOS invokes
//! the guest's INT 1Ch callback, then acknowledges the PIC and returns. The default
//! callback simply returns; guests may replace it or chain to it. The Rust timer
//! handler awaits the guest callback through the machine's ordinary resume hook.
//! These service entries enable live IF. Pending handlers admit device IRQs;
//! ready handlers finish in one poll. Guest IRET preserves the caller's saved IF,
//! while abandonment leaves live CPU state untouched.
#![forbid(unsafe_code)]

pub mod clock;
mod information;
mod interrupt;
mod keyboard;
mod screen;
pub mod video;

pub use information::Configuration;
pub use interrupt::register_interrupts;
pub use keyboard::{KeyMode, KeyStroke, Keyboard, PhysicalKeyboard, ShiftState};
use relocade_machine::{IoBus, MemoryView};
pub use screen::write_text;

const BIOS_SEGMENT: u16 = 0xf000;

/// Initializes machine/clock/video data and ROM fonts. Use [`register_interrupts`] to bind
/// BIOS interrupt vectors and their handlers to a CPU.
/// The machine must map low memory as RAM and F0000..FFFFF as identity-backed ROM.
/// Map [`relocade_vga::MEMORY_RANGE`] as MMIO and connect VGA to that memory
/// range and [`relocade_vga::PORT_RANGES`] on the supplied bus. Font loading uses
/// the A0000 aperture before mode setup restores the color text aperture.
pub fn initialize(memory: &mut MemoryView<'_>, io: &IoBus, configuration: Configuration) {
    information::initialize(memory, configuration);
    video::initialize(memory, io);
    clock::initialize(memory);
}
