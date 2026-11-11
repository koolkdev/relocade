//! Video memory access selected from adapter gates and the active addressing mechanism.

use crate::registers::{
    dispi::{self, SvgaBankMapping},
    setup, LegacyRegisters,
};

/// One transfer's access policy. Both register banks remain on the device;
/// this value only selects how the CPU reaches their shared video memory.
#[derive(Clone, Copy)]
pub(crate) enum VramAccess<'a> {
    Disabled,
    Vga(&'a LegacyRegisters),
    SvgaBank(SvgaBankMapping),
}

impl<'a> VramAccess<'a> {
    pub fn new(setup: &setup::State, registers: &'a LegacyRegisters, dispi: &dispi::State) -> Self {
        if !setup.enabled() || !registers.video_memory_enabled() {
            Self::Disabled
        } else if let Some(mapping) = dispi.bank_mapping() {
            Self::SvgaBank(mapping)
        } else {
            Self::Vga(registers)
        }
    }
}
