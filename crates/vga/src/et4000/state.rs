//! Validated card programming, independent of firmware buffer layout and VRAM.
use super::{ramdac::Sc11483, registers::Extensions, Et4000Ax};
use crate::{
    effects::{CardEffects, Effects},
    DacState, VgaCoreState,
};
use relocade_machine::IoContext;

/// ET4000AX extensions, high start latch and SC11483 command/access state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Et4000State {
    extensions: Extensions,
    display_start_high: u8,
    ramdac: Sc11483,
}

impl Et4000State {
    pub const BYTE_LEN: usize = 16;

    pub fn to_bytes(&self) -> [u8; Self::BYTE_LEN] {
        let mut bytes = [0; Self::BYTE_LEN];
        bytes[..13].copy_from_slice(&self.extensions.to_state_bytes());
        bytes[13] = self.display_start_high;
        bytes[14..].copy_from_slice(&self.ramdac.to_state_bytes());
        bytes
    }

    pub fn from_bytes(bytes: &[u8; Self::BYTE_LEN]) -> Option<Self> {
        if bytes[13] > 3 {
            return None;
        }
        Some(Self {
            extensions: Extensions::from_state_bytes(bytes[..13].try_into().unwrap())?,
            display_start_high: bytes[13],
            ramdac: Sc11483::from_state_bytes(bytes[14..].try_into().unwrap())?,
        })
    }
}

impl Et4000Ax {
    /// The card exposes six CRTC index bits, including indices outside VGA.
    pub fn decode_core_state(bytes: &[u8; VgaCoreState::BYTE_LEN]) -> Option<VgaCoreState> {
        VgaCoreState::from_bytes_with_crtc_mask(bytes, 0x3f)
    }

    pub fn dac_state(&self) -> DacState {
        self.vga.dac_state()
    }

    pub fn et4000_state(&self) -> Et4000State {
        Et4000State {
            extensions: self.extensions.clone(),
            display_start_high: self.display_start_high,
            ramdac: self.ramdac.clone(),
        }
    }

    /// Apply validated components in one effects operation. No register command
    /// is replayed, no memory is cleared, and unselected components survive.
    pub fn restore_state(
        &mut self,
        context: IoContext<'_>,
        core: Option<VgaCoreState>,
        dac: Option<DacState>,
        extensions: Option<Et4000State>,
    ) {
        if core.is_none() && dac.is_none() && extensions.is_none() {
            return;
        }
        let effects = if core.is_some() || extensions.is_some() {
            Effects::CONFIGURATION_AND_IO
        } else {
            Effects::IMAGE
        };
        self.with_effects(context, effects, |card| {
            if let Some(core) = core {
                core.restore(&mut card.vga);
            }
            if let Some(dac) = dac {
                card.vga.restore_dac(dac);
            }
            if let Some(state) = extensions {
                card.extensions = state.extensions;
                card.display_start_high = state.display_start_high;
                card.ramdac = state.ramdac;
            }
        });
    }
}
