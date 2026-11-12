//! Complete programming state for firmware save/restore, without framebuffer bytes.
//! Device-owned encodings describe this adapter's programming. Firmware owns the
//! guest buffer format and version; decoding validates each component before use.
mod vga_core;
pub use vga_core::VgaCoreState;

use relocade_machine::IoContext;

use crate::{
    effects::Effects,
    registers::{attribute, dac::Dac, dispi::Dispi},
    Vga,
};

/// DAC colors, pixel mask, exact transfer position and Attribute Color Select.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DacState {
    dac: Dac,
    color_select: u8,
}

impl DacState {
    pub const BYTE_LEN: usize = 773;

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        bytes[..772].copy_from_slice(&self.dac.to_state_bytes());
        bytes[772] = self.color_select;
        bytes
    }

    pub fn from_bytes(bytes: &[u8; Self::BYTE_LEN]) -> Option<Self> {
        if bytes[772] > 15 {
            return None;
        }
        Some(Self {
            dac: Dac::from_state_bytes(bytes[..772].try_into().unwrap())?,
            color_select: bytes[772],
        })
    }
}

/// Staged DISPI mode, active layout, enable flags, CPU bank and register index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DispiState(Dispi);

impl DispiState {
    pub const BYTE_LEN: usize = 18;

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        self.0.to_state_bytes()
    }

    pub fn from_bytes(bytes: &[u8; Self::BYTE_LEN]) -> Option<Self> {
        Dispi::from_state_bytes(bytes).map(Self)
    }
}

impl Vga {
    /// Capture the VGA core without reading ports or disturbing VRAM/latches.
    pub fn core_state(&self) -> VgaCoreState {
        VgaCoreState::capture(self)
    }

    pub fn dac_state(&self) -> DacState {
        DacState {
            dac: self.registers.dac.clone(),
            color_select: self.registers.attribute.values[attribute::COLOR_SELECT as usize],
        }
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
        let effects = if core.is_some() {
            Effects::CONFIGURATION_AND_IO
        } else if dispi.is_some() {
            Effects::CONFIGURATION
        } else {
            Effects::IMAGE
        };
        self.with_effects(context, effects, |vga| {
            if let Some(core) = core {
                core.restore(vga);
            }
            if let Some(dac) = dac {
                vga.registers.dac = dac.dac;
                vga.registers.attribute.values[attribute::COLOR_SELECT as usize] = dac.color_select;
            }
            if let Some(dispi) = dispi {
                // Restoring state is not an enable command and never clears VRAM.
                vga.dispi = dispi.0;
            }
        });
    }
}
