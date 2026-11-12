//! The video firmware selected by the machine composition.
pub mod dispi;

use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, IoBus, MemoryView,
};
use relocade_vga::DispiAdapter;
use std::{cell::RefCell, rc::Rc};

/// Firmware services for the connected card; VGA-only firmware has no VBE interface.
#[derive(Clone)]
pub enum VideoFirmware {
    Vga,
    Dispi(Rc<RefCell<DispiAdapter>>),
}

impl VideoFirmware {
    pub(crate) fn initialize(&self, memory: &mut MemoryView<'_>) {
        match self {
            Self::Vga => {}
            Self::Dispi(_) => dispi::initialize(memory),
        }
    }

    /// Prepare this card for a validated legacy mode, then program standard VGA.
    pub fn set_mode(&self, memory: &mut MemoryView<'_>, io: &IoBus, request: u8) -> Option<u8> {
        match self {
            Self::Vga => super::modes::set_mode(memory, io, request),
            Self::Dispi(_) => dispi::set_mode(memory, io, request),
        }
    }
}

#[interrupt(context(memory, io), args(request = al),
    returns { Some(value) => { al = value }, None => crate::interrupts::unsupported }
)]
pub(super) fn set_mode(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    firmware: &VideoFirmware,
    request: u8,
) -> Option<u8> {
    firmware.set_mode(memory, io, request)
}

#[interrupt(context(call))]
pub(super) async fn vbe(call: &mut CpuContext<'_>, firmware: &VideoFirmware) {
    interrupt_dispatch!(
        call,
        match firmware {
            VideoFirmware::Dispi(card) => dispi::vbe::dispatch(card),
            VideoFirmware::Vga => unsupported,
        }
    );
}

#[interrupt(returns { () => { ax = 0x0100 } })]
fn unsupported() {}
