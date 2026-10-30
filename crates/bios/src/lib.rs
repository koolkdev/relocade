//! BIOS services for a real-mode PC, reached through ordinary runtime hooks.
//!
//! INT 10h supplies basic mode-3 text output, cursors, pages and scrolling.
//! INT 16h/AH=00h/01h/02h reads, peeks and queries the shared host keyboard.
//! INT 1Ah/AH=00h reads ticks and midnight rollover from the BIOS data area at
//! 0040:006C and 0040:0070. IRQ0 advances these fields at the default timer rate.
#![forbid(unsafe_code)]

mod clock;
mod keyboard;
mod screen;
mod video;

pub use keyboard::{KeyStroke, Keyboard, ShiftState};
use relocade_machine::{real_mode_interrupt, Hook};
use relocade_machine::{IoBus, MemoryView};
pub use screen::write_text;

const BIOS_SEGMENT: u16 = 0xf000;
const CLOCK_ENTRY: u16 = 0xff00;
const CLOCK_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + CLOCK_ENTRY as u32;
const KEYBOARD_ENTRY: u16 = 0xff10;
const KEYBOARD_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + KEYBOARD_ENTRY as u32;
const VIDEO_ENTRY: u16 = 0xff20;
const VIDEO_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + VIDEO_ENTRY as u32;
const TIMER_ENTRY: u16 = 0xff30;
const TIMER_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + TIMER_ENTRY as u32;

/// Initializes the clock data, BIOS vectors and their ROM return instructions.
/// The machine must map low memory as RAM and F0000..FFFFF as identity-backed ROM.
/// VGA text memory must be mapped as MMIO, with the VGA device connected to
/// the supplied bus at its color text aperture and [`relocade_vga::PORT_RANGES`].
pub fn initialize(memory: &mut MemoryView<'_>, io: &IoBus) {
    screen::initialize(memory, io);
    clock::initialize(memory);
    let [segment_lo, segment_hi] = BIOS_SEGMENT.to_le_bytes();
    for (vector, offset) in [
        (0x08, TIMER_ENTRY),
        (0x1a, CLOCK_ENTRY),
        (0x16, KEYBOARD_ENTRY),
        (0x10, VIDEO_ENTRY),
    ] {
        let [offset_lo, offset_hi] = offset.to_le_bytes();
        memory
            .write(vector * 4, &[offset_lo, offset_hi, segment_lo, segment_hi])
            .expect("BIOS interrupt vector");
        let address = ((BIOS_SEGMENT as u32) << 4) + u32::from(offset);
        memory
            .write_backing(address, &real_mode_interrupt::HOOK_STUB)
            .expect("BIOS ROM backing");
    }
}

/// Returns hooks for installation alongside another environment's hooks.
/// The keyboard is captured by the hooks and may be shared with console input.
pub fn hooks<T: 'static>(keyboard: Keyboard) -> [Hook<T>; 4] {
    [
        real_mode_interrupt::hook(CLOCK_ADDRESS, async |call| call.with(clock::read)),
        real_mode_interrupt::hook(TIMER_ADDRESS, async |call| call.with(clock::tick)),
        real_mode_interrupt::hook(KEYBOARD_ADDRESS, async move |call| {
            keyboard::interrupt(call, &keyboard).await
        }),
        real_mode_interrupt::hook(VIDEO_ADDRESS, async |call| call.with(video::interrupt)),
    ]
}
