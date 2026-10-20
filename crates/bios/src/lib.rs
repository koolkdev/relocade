//! BIOS services for a real-mode PC, reached through ordinary runtime hooks.
//!
//! INT 10h supplies basic mode-3 text output, cursors, pages and scrolling.
//! INT 16h/AH=00h/01h/02h reads, peeks and queries the shared host keyboard.
//! INT 1Ah/AH=00h reads ticks and midnight rollover from the BIOS data area at
//! 0040:006C and 0040:0070. A machine timer must update these fields.
#![forbid(unsafe_code)]

mod keyboard;
mod screen;
mod text_renderer;
mod video;

pub use keyboard::{KeyStroke, Keyboard, ShiftState};
use relocade_machine::Memory;
use relocade_runtime::{real_mode_interrupt, Hook, HostCall};
use relocade_x86::ExecutionExit;
pub use screen::{
    write_text, TextCell, TextCursor, TextScreen, TEXT_MEMORY_BYTES, TEXT_MEMORY_START,
};
use std::ops::ControlFlow;
pub use text_renderer::TextRenderer;

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
/// Color text memory at [`TEXT_MEMORY_START`] must also have RAM backing.
pub fn initialize(memory: &mut Memory<'_>) {
    screen::initialize(memory);
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
/// The environment lends the same keyboard used by its console input.
pub fn hooks<T: AsRef<Keyboard> + 'static>() -> [Hook<T>; 3] {
    [
        Hook::new(CLOCK_ADDRESS, clock),
        Hook::new(KEYBOARD_ADDRESS, keyboard::interrupt),
        Hook::new(VIDEO_ADDRESS, video::interrupt),
    ]
}

fn clock<T>(call: &mut HostCall<'_, T>) -> ControlFlow<ExecutionExit> {
    if call.cpu().ah() == 0 {
        let mut data = [0; 5];
        call.memory()
            .read(CLOCK_TICKS, &mut data)
            .expect("BIOS clock data");
        let mut cpu = call.cpu();
        cpu.set_dx(u16::from_le_bytes([data[0], data[1]]));
        cpu.set_cx(u16::from_le_bytes([data[2], data[3]]));
        cpu.set_al(data[4]);
        call.memory()
            .write(MIDNIGHT_ROLLOVER, &[0])
            .expect("BIOS midnight rollover");
    } else {
        // Unsupported BIOS function.
        call.cpu().set_ah(0x86);
        real_mode_interrupt::set_carry(call, true);
    }
    real_mode_interrupt::return_from_interrupt(call)
}
