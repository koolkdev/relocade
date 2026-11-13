//! Complete programming state for firmware save/restore, without framebuffer bytes.
//! Device-owned encodings describe this adapter's programming. Firmware owns the
//! guest buffer format and version; decoding validates each component before use.

use relocade_machine::IoContext;

use super::registers::Dispi;
use crate::{
    effects::{CardEffects, Effects},
    DacState, DispiAdapter, VgaCoreState,
};

/// Staged DISPI mode, active layout, enable flags, CPU bank and register index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispiState(Dispi);

impl DispiState {
    pub const BYTE_LEN: usize = 18;

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        self.0.to_state_bytes()
    }

    pub fn from_bytes(bytes: &[u8; Self::BYTE_LEN]) -> Option<Self> {
        Dispi::from_state_bytes(bytes, crate::dispi::VIDEO_MEMORY_BYTES).map(Self)
    }
}

impl DispiAdapter {
    /// Capture the VGA core without reading ports or disturbing VRAM/latches.
    pub fn core_state(&self) -> VgaCoreState {
        self.vga.core_state()
    }

    pub fn dac_state(&self) -> DacState {
        self.vga.dac_state()
    }

    pub fn dispi_state(&self) -> DispiState {
        DispiState(self.dispi.clone())
    }

    /// Apply already-validated state classes together. Unselected classes and all
    /// framebuffer bytes are preserved; completed captures retain their old inputs.
    pub fn restore_state(
        &mut self,
        context: IoContext<'_>,
        core: Option<VgaCoreState>,
        dac: Option<DacState>,
        dispi: Option<DispiState>,
    ) {
        if core.is_none() && dac.is_none() && dispi.is_none() {
            return;
        }
        let mut effects = if core.is_some() {
            Effects::CONFIGURATION_AND_IO
        } else if dispi.is_some() {
            Effects::CONFIGURATION_AND_MEMORY
        } else {
            Effects::IMAGE
        };
        // DAC state also includes the VGA Attribute color-select register.
        effects.scanout = true;
        self.with_effects(context, effects, |card| {
            if let Some(core) = core {
                core.restore(&mut card.vga);
            }
            if let Some(dac) = dac {
                card.vga.restore_dac(dac);
            }
            if let Some(dispi) = dispi {
                // Restoring state is not an enable command and never clears VRAM.
                card.dispi = dispi.0;
            }
        });
    }
}
