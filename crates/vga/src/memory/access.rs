//! CPU access selected from adapter gates and the active addressing mechanism.

use crate::registers::{
    dispi::{BankedWindow, Dispi},
    setup::Setup,
    Registers,
};

/// One transfer's access policy. Both register banks remain on the device;
/// this value only selects how the CPU reaches their shared video memory.
#[derive(Clone, Copy)]
pub(crate) enum CpuAccess<'a> {
    Disabled,
    LegacyVga(&'a Registers),
    Banked(BankedWindow),
}

impl<'a> CpuAccess<'a> {
    pub fn new(setup: &Setup, registers: &'a Registers, dispi: &Dispi) -> Self {
        if !setup.enabled() || !registers.video_memory_enabled() {
            Self::Disabled
        } else if let Some(window) = dispi.banked_window() {
            Self::Banked(window)
        } else {
            Self::LegacyVga(registers)
        }
    }
}
