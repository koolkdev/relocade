//! BIOS interrupt registration and shared return handling.

use crate::{clock, keyboard, video, Keyboard, BIOS_SEGMENT};
use relocade_machine::{
    real_mode_interrupt::{self, interrupt_handler, Interrupts},
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

const KEYBOARD_IRQ_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff40,
};

/// Registers BIOS vectors and their handlers alongside an environment's interrupts.
/// The keyboard is retained by its handler and may be shared with console input.
/// Software services enable live IF. Ready services finish in one poll; pending
/// services allow the driver to deliver device IRQs.
pub fn register_interrupts(interrupts: &mut Interrupts<'_>, keyboard: Keyboard) {
    let irq_keyboard = keyboard.clone();
    interrupts.register_interrupt(0x09, KEYBOARD_IRQ_ENTRY, async move |call| {
        if let Some(byte) = relocade_at_keyboard::read_scan_code(call.io) {
            irq_keyboard.scan_code(byte);
        }
        relocade_pic::end_of_interrupt(call.io);
    });
    register_interrupt(interrupts, 0x08, TIMER_ENTRY, async |call| {
        clock::tick(&mut call.memory);
        relocade_pic::end_of_interrupt(call.io);
    });
    register_interrupt(
        interrupts,
        0x1a,
        CLOCK_ENTRY,
        interrupt_handler!(clock::dispatch),
    );
    register_interrupt(
        interrupts,
        0x16,
        KEYBOARD_ENTRY,
        interrupt_handler!(keyboard::dispatch(&keyboard)),
    );
    register_interrupt(
        interrupts,
        0x10,
        VIDEO_ENTRY,
        interrupt_handler!(video::dispatch),
    );
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
