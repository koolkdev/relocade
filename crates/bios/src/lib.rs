//! BIOS services for a real-mode PC, reached through ordinary runtime hooks.
//!
//! INT 10h sets modes 03h and 13h, with text output, cursors, pages and scrolling
//! in mode 03h. Mode 13h programs VGA for direct guest pixel and palette writes.
//! Mode setting honors AL bit 7 to preserve video memory; AH=0Fh reports the
//! mode, that flag, character columns and active page. Other video services in
//! graphics mode return AH=86h with carry set. [`write_text`] ignores graphics
//! output until BIOS glyph drawing in graphics modes is implemented.
//! INT 16h/AH=00h/01h/02h reads, peeks and queries the shared host keyboard.
//! INT 1Ah/AH=00h reads ticks and midnight rollover from the BIOS data area at
//! 0040:006C and 0040:0070. A machine timer must update these fields.
#![forbid(unsafe_code)]

mod keyboard;
mod screen;
mod video;

pub use keyboard::{KeyStroke, Keyboard, ShiftState};
use relocade_machine::{real_mode_interrupt, CpuContext, Hook, HookResult};
use relocade_machine::{IoBus, MemoryView};
pub use screen::write_text;

const BIOS_SEGMENT: u16 = 0xf000;
const CLOCK_ENTRY: u16 = 0xff00;
const CLOCK_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + CLOCK_ENTRY as u32;
const KEYBOARD_ENTRY: u16 = 0xff10;
const KEYBOARD_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + KEYBOARD_ENTRY as u32;
const VIDEO_ENTRY: u16 = 0xff20;
const VIDEO_ADDRESS: u32 = ((BIOS_SEGMENT as u32) << 4) + VIDEO_ENTRY as u32;
const CLOCK_TICKS: u32 = 0x46c;
const MIDNIGHT_ROLLOVER: u32 = 0x470;

/// Initializes the clock data, BIOS vectors and their ROM return instructions.
/// The machine must map low memory as RAM and F0000..FFFFF as identity-backed ROM.
/// Map [`relocade_vga::MEMORY_RANGE`] as MMIO and connect VGA to that memory
/// range and [`relocade_vga::PORT_RANGES`] on the supplied bus. Font loading uses
/// the A0000 aperture before mode setup restores the color text aperture.
pub fn initialize(memory: &mut MemoryView<'_>, io: &IoBus) {
    video::set_mode(memory, io, 3).expect("initial text mode");
    memory.write(CLOCK_TICKS, &[0; 5]).expect("BIOS clock data");
    let [segment_lo, segment_hi] = BIOS_SEGMENT.to_le_bytes();
    for (vector, offset) in [
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
pub fn hooks<T: 'static>(keyboard: Keyboard) -> [Hook<T>; 3] {
    [
        Hook::new(CLOCK_ADDRESS, clock),
        Hook::new(KEYBOARD_ADDRESS, async move |call| {
            keyboard::interrupt(call, &keyboard).await
        }),
        Hook::new(VIDEO_ADDRESS, video::interrupt),
    ]
}

async fn clock<T>(call: &mut CpuContext<'_, T>) -> HookResult {
    if call.cpu.ah() == 0 {
        let mut data = [0; 5];
        call.memory
            .read(CLOCK_TICKS, &mut data)
            .expect("BIOS clock data");
        let cpu = &mut call.cpu;
        cpu.set_dx(u16::from_le_bytes([data[0], data[1]]));
        cpu.set_cx(u16::from_le_bytes([data[2], data[3]]));
        cpu.set_al(data[4]);
        call.memory
            .write(MIDNIGHT_ROLLOVER, &[0])
            .expect("BIOS midnight rollover");
    } else {
        // Unsupported BIOS function.
        call.cpu.set_ah(0x86);
        real_mode_interrupt::set_carry(call, true);
    }
    real_mode_interrupt::return_from_interrupt(call)
}
