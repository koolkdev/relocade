use crate::registers::dac::Dac;

/// DAC colors, pixel mask, exact transfer position and Attribute Color Select.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DacState {
    pub(super) dac: Dac,
    pub(super) color_select: u8,
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
