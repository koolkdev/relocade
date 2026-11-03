//! BIOS interrupt registration and shared return handling.

use crate::{clock, keyboard, video, Keyboard, BIOS_SEGMENT};
use relocade_machine::{
    real_mode_interrupt::{self, Interrupts},
    CpuContext, FarPointer,
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
const TIMER_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff30,
};

/// Registers BIOS vectors and their handlers alongside an environment's interrupts.
/// The keyboard is retained by its handler and may be shared with console input.
/// Entries enable live IF during service execution. Ready services finish in one
/// poll; pending services allow the driver to deliver device IRQs.
pub fn register_interrupts(interrupts: &mut Interrupts<'_>, keyboard: Keyboard) {
    register_interrupt(interrupts, 0x08, TIMER_ENTRY, async |call| {
        clock::tick(&mut call.memory);
    });
    register_interrupt(interrupts, 0x1a, CLOCK_ENTRY, clock::dispatch);
    register_interrupt(interrupts, 0x16, KEYBOARD_ENTRY, async move |call| {
        keyboard::dispatch(call, &keyboard).await;
    });
    register_interrupt(interrupts, 0x10, VIDEO_ENTRY, video::dispatch);
}

fn register_interrupt<F>(interrupts: &mut Interrupts<'_>, vector: u8, entry: FarPointer, handler: F)
where
    F: AsyncFn(&mut CpuContext<'_>) + 'static,
{
    interrupts.register_interrupt(vector, entry, async move |call| {
        real_mode_interrupt::with_interrupts_enabled(call.cpu, handler(call)).await;
    });
}

#[real_mode_interrupt::interrupt(returns { status => { ah = status, cf = true } })]
pub(crate) fn unsupported() -> u8 {
    0x86
}
