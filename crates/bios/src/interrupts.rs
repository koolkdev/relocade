//! BIOS interrupt registration and shared return handling.

use crate::{clock, information, keyboard, mouse::Mouse, video, Keyboard, BIOS_SEGMENT};
use relocade_machine::{
    real_mode_interrupt::{self, interrupt_handler, Interrupts},
    CpuContext, FarPointer,
};
use relocade_vga::Vga;
use relocade_x86::Exception;
use std::{cell::RefCell, rc::Rc};

/// Default PC IRQ0 vector. Guest PIC remapping can select a different vector.
pub const TIMER_IRQ: u8 = 0x08;
/// Default PC IRQ1 vector. Guest PIC remapping can select a different vector.
pub const KEYBOARD_IRQ: u8 = 0x09;
/// Default PC IRQ12 vector.
pub const MOUSE_IRQ: u8 = 0x74;
/// Video services (INT 10h).
pub const VIDEO: u8 = 0x10;
/// Installed equipment (INT 11h).
pub const EQUIPMENT: u8 = 0x11;
/// Conventional memory size (INT 12h).
pub const MEMORY_SIZE: u8 = 0x12;
/// System services, including the PS/2 pointing device (INT 15h/C2h).
pub const SYSTEM: u8 = 0x15;
/// Keyboard services (INT 16h).
pub const KEYBOARD: u8 = 0x16;
/// Time-of-day services (INT 1Ah).
pub const TIME_OF_DAY: u8 = 0x1a;
/// Ctrl-Break callback invoked by the keyboard IRQ handler (INT 1Bh).
pub const CONTROL_BREAK: u8 = 0x1b;
/// Timer callback invoked by the timer IRQ handler (INT 1Ch).
pub const TIMER_TICK: u8 = 0x1c;
/// IVT data pointer to the upper half of the 8x8 graphics font (INT 1Fh).
pub const HIGH_FONT: u8 = 0x1f;
/// IVT data pointer to the active graphics font (INT 43h).
pub const GRAPHICS_FONT: u8 = 0x43;

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
const TIMER_CALLBACK_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff50,
};
const SYSTEM_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff60,
};
const MOUSE_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff70,
};
const EQUIPMENT_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff80,
};
const MEMORY_SIZE_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff90,
};

const KEYBOARD_IRQ_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xff40,
};
const BREAK_CALLBACK_ENTRY: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xffa0,
};
const EXCEPTION_ENTRIES: FarPointer = FarPointer {
    segment: BIOS_SEGMENT,
    offset: 0xffb0,
};

/// Registers BIOS vectors and their handlers alongside an environment's interrupts.
/// The keyboard is retained by its handler and may be shared with console input.
/// Video retains the same VGA device connected to the CPU's I/O bus.
/// An optional mouse owns BIOS state and waits on the connected PS/2 controller.
/// BIOS services and IRQ handlers enable live IF while their work runs. IRQ
/// completion disables IF and acknowledges the PIC before returning through IRET.
pub fn register_interrupts(
    interrupts: &mut Interrupts<'_>,
    keyboard: Keyboard,
    vga: Rc<RefCell<Vga>>,
    mouse: Option<Mouse>,
) {
    for (index, exception) in [
        Exception::DivideError,
        Exception::BoundRangeExceeded,
        Exception::InvalidOpcode,
    ]
    .into_iter()
    .enumerate()
    {
        interrupts.register_unhandled_exception(
            exception,
            EXCEPTION_ENTRIES.wrapping_add(index * real_mode_interrupt::HOOK_STUB.len()),
        );
    }
    if let Some(mouse) = mouse {
        let mouse = Rc::new(mouse);
        let irq_mouse = mouse.clone();
        register_irq(
            interrupts,
            MOUSE_IRQ,
            relocade_pic::MOUSE_IRQ,
            MOUSE_ENTRY,
            async move |call| {
                irq_mouse.interrupt(call).await;
            },
        );
        register_service(
            interrupts,
            SYSTEM,
            SYSTEM_ENTRY,
            interrupt_handler!(mouse.dispatch),
        );
    } else {
        register_service(
            interrupts,
            SYSTEM,
            SYSTEM_ENTRY,
            interrupt_handler!(unsupported),
        );
    }
    let irq_keyboard = keyboard.clone();
    register_irq(
        interrupts,
        KEYBOARD_IRQ,
        relocade_pic::KEYBOARD_IRQ,
        KEYBOARD_IRQ_ENTRY,
        async move |call| {
            keyboard::interrupt(call, &irq_keyboard).await;
        },
    );
    interrupts.register_interrupt(CONTROL_BREAK, BREAK_CALLBACK_ENTRY, async |_| {});
    register_irq(
        interrupts,
        TIMER_IRQ,
        relocade_pic::TIMER_IRQ,
        TIMER_ENTRY,
        clock::timer,
    );
    interrupts.register_interrupt(TIMER_TICK, TIMER_CALLBACK_ENTRY, async |_| {});
    register_service(
        interrupts,
        EQUIPMENT,
        EQUIPMENT_ENTRY,
        interrupt_handler!(information::equipment),
    );
    register_service(
        interrupts,
        MEMORY_SIZE,
        MEMORY_SIZE_ENTRY,
        interrupt_handler!(information::conventional_memory),
    );
    register_service(
        interrupts,
        TIME_OF_DAY,
        CLOCK_ENTRY,
        interrupt_handler!(clock::dispatch),
    );
    register_service(
        interrupts,
        KEYBOARD,
        KEYBOARD_ENTRY,
        interrupt_handler!(keyboard::dispatch(&keyboard)),
    );
    register_service(
        interrupts,
        VIDEO,
        VIDEO_ENTRY,
        interrupt_handler!(video::dispatch(&vga)),
    );
}

/// Completes a BIOS IRQ only after its handler and any guest callbacks return.
fn register_irq<F>(
    interrupts: &mut Interrupts<'_>,
    vector: u8,
    irq: u8,
    entry: FarPointer,
    handler: F,
) where
    F: AsyncFn(&mut CpuContext<'_>) + 'static,
{
    interrupts.register_interrupt(vector, entry, async move |call| {
        real_mode_interrupt::with_interrupts_enabled(call.cpu, handler(call)).await;
        // Guest handlers can chain to the BIOS with IF set. Keep IRQ completion
        // masked until the ordinary IRET restores the interrupted caller's flags.
        call.cpu.set_interrupts_enabled(false);
        relocade_pic::end_of_interrupt(call.io, irq);
    });
}

fn register_service<F>(interrupts: &mut Interrupts<'_>, vector: u8, entry: FarPointer, handler: F)
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
