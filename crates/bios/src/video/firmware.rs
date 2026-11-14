//! The video firmware selected by the machine composition.
pub mod dispi;
pub mod et4000;
mod vbe;

use super::{modes, COLOR_TEXT_80X25};
use relocade_machine::{
    real_mode_interrupt::{interrupt, interrupt_dispatch},
    CpuContext, IoBus, MemoryView,
};
use relocade_vga::{DispiAdapter, Et4000Ax};
use std::{cell::RefCell, rc::Rc};

/// Firmware services for the connected card; VGA-only firmware has no VBE interface.
#[derive(Clone)]
pub enum VideoFirmware {
    Vga,
    Dispi(Rc<RefCell<DispiAdapter>>),
    Et4000Ax(Rc<RefCell<Et4000Ax>>),
}

impl VideoFirmware {
    pub(crate) fn initialize(&self, memory: &mut MemoryView<'_>, io: &IoBus) {
        match self {
            Self::Vga => {
                // Standard VGA starts with 80×25 nine-dot cells and the 16-high font.
                assert!(
                    modes::select_scanlines(memory, 2),
                    "initial scanline profile"
                );
                modes::set_mode(memory, io, COLOR_TEXT_80X25).expect("initial VGA text mode");
            }
            Self::Dispi(card) => dispi::initialize(memory, io, card),
            Self::Et4000Ax(card) => et4000::initialize(memory, io, card),
        }
    }

    /// Prepare this card for a validated legacy mode, then program standard VGA.
    pub fn set_mode(&self, memory: &mut MemoryView<'_>, io: &IoBus, request: u8) -> Option<u8> {
        match self {
            Self::Vga => modes::set_mode(memory, io, request),
            Self::Dispi(card) => dispi::set_mode(memory, io, card, request),
            Self::Et4000Ax(card) => et4000::set_mode(memory, io, card, request),
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
            VideoFirmware::Et4000Ax(card) => et4000::vbe::dispatch(card),
            VideoFirmware::Vga => vbe_unavailable,
        }
    );
}

#[interrupt(returns { () => { ax = 0x0100 } })]
fn vbe_unavailable() {}
