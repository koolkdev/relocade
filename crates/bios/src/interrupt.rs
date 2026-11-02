//! BIOS interrupt registration and shared return handling.

use crate::{clock, keyboard, video, Keyboard, BIOS_SEGMENT};
use relocade_machine::{
    real_mode_interrupt::{self, Interrupts},
    FarPointer,
};

const CLOCK_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff00,
};
const KEYBOARD_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff10,
};
const VIDEO_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff20,
};

/// Registers BIOS vectors and their handlers alongside an environment's interrupts.
/// The keyboard is retained by its handler and may be shared with console input.
pub fn register_interrupts(interrupts: &mut Interrupts<'_>, keyboard: Keyboard) {
    interrupts.register_interrupt(0x1a, CLOCK_ENTRY, clock::dispatch);
    interrupts.register_interrupt(0x16, KEYBOARD_ENTRY, async move |call| {
        keyboard::dispatch(call, &keyboard).await;
    });
    interrupts.register_interrupt(0x10, VIDEO_ENTRY, video::dispatch);
}

#[real_mode_interrupt::interrupt(returns { status => { ah = status, cf = true } })]
pub(crate) fn unsupported() -> u8 {
    0x86
}
