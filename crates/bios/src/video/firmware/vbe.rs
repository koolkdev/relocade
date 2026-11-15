//! Shared VBE services, specialized once for the selected card's driver.
mod driver;
mod information;
mod layout;
mod modes;
mod palette;
mod state;

use super::VideoFirmware;
pub(super) use driver::{SavedState, Vbe};
pub(super) use information::{initialize, ModeInformation, WindowAccess};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch, interrupt_handler, Interrupts},
    CpuContext, FarPointer,
};
use std::{cell::RefCell, rc::Rc};

// Follows the VBE OEM string and mode list in BIOS ROM.
const WINDOW_FUNCTION: FarPointer = FarPointer {
    segment: crate::BIOS_SEGMENT,
    offset: 0xb100,
};

#[interrupt(context(call))]
pub(in crate::video) async fn dispatch(call: &mut CpuContext<'_>, firmware: &VideoFirmware) {
    interrupt_dispatch!(
        call,
        match firmware {
            VideoFirmware::Vga => unsupported,
            VideoFirmware::Dispi(card) => dispatch_card(card.as_ref()),
            VideoFirmware::Et4000Ax(card) => dispatch_card(card.as_ref()),
        }
    );
}

#[interrupt(context(call), args(function = al))]
async fn dispatch_card<C: Vbe>(call: &mut CpuContext<'_>, card: &RefCell<C>, function: u8) {
    interrupt_dispatch!(
        call,
        match function {
            0x00 => information::controller(card),
            0x01 => information::mode(card),
            0x02 => modes::set(card),
            0x03 => modes::current(card),
            0x04 => state::dispatch(card),
            0x05 => modes::window(card),
            0x06 => layout::scanline(card),
            0x07 => layout::display_start(card),
            0x08 => palette::format(card),
            _ => unsupported,
        }
    );
}

// VBE reports status in AX and preserves flags, including carry.
#[interrupt(returns { () => { ax = 0x0100 } })]
fn unsupported() {}

impl VideoFirmware {
    pub(crate) fn register_procedures(&self, interrupts: &mut Interrupts<'_>) {
        match self {
            Self::Vga => {}
            Self::Dispi(card) => register_window_procedure(interrupts, card.clone()),
            Self::Et4000Ax(card) => register_window_procedure(interrupts, card.clone()),
        }
    }
}

fn register_window_procedure<C: Vbe + 'static>(
    interrupts: &mut Interrupts<'_>,
    card: Rc<RefCell<C>>,
) {
    // Hook the bank service directly; far callers supply BX/DX without an AX
    // function number, and the runtime provides the far-procedure return.
    interrupts.register_far_procedure(
        WINDOW_FUNCTION,
        interrupt_handler!(modes::window(card.as_ref())),
    );
}
