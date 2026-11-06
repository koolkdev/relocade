//! Six-bit RGB palette and sequential CPU access, IBM VGA Function pp. 2-104–106.

use super::text_mode;

/// DAC colors, pixel mask and CPU palette-transfer position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dac {
    pub mask: u8,
    /// Six-bit RGB components in DAC address order.
    pub colors: [[u8; 3]; 256],
    address: u8,
    component: usize,
    reading: bool,
}

impl Default for Dac {
    fn default() -> Self {
        Self {
            mask: 0xff,
            colors: text_mode::DAC_PALETTE,
            address: 0,
            component: 0,
            reading: false,
        }
    }
}

impl Dac {
    /// This VGA-compatible DAC has a fixed width in every display mode.
    pub const BITS_PER_COMPONENT: u8 = 6;

    pub(crate) fn to_state_bytes(&self) -> [u8; 772] {
        let mut bytes = [0; 772];
        bytes[..4].copy_from_slice(&[
            self.mask,
            self.address,
            self.component as u8,
            u8::from(self.reading),
        ]);
        bytes[4..].copy_from_slice(self.colors.as_flattened());
        bytes
    }

    pub(crate) fn from_state_bytes(bytes: &[u8; 772]) -> Option<Self> {
        if bytes[2] >= 3 || bytes[3] > 1 || bytes[4..].iter().any(|&value| value >= 64) {
            return None;
        }
        let mut colors = [[0; 3]; 256];
        colors.as_flattened_mut().copy_from_slice(&bytes[4..]);
        Some(Self {
            mask: bytes[0],
            address: bytes[1],
            component: usize::from(bytes[2]),
            reading: bytes[3] != 0,
            colors,
        })
    }

    pub(crate) fn select(&mut self, address: u8, reading: bool) {
        self.address = address;
        self.component = 0;
        self.reading = reading;
    }

    /// Port 3C8h reports the next index during a read transfer, and the current
    /// index during a write transfer (V7 VGA Technical Reference pp. 3-12–13).
    pub fn address(&self) -> u8 {
        self.address.wrapping_add(u8::from(self.reading))
    }

    pub(crate) fn state(&self) -> u8 {
        if self.reading {
            3
        } else {
            0
        }
    }

    fn step(&mut self) {
        self.component += 1;
        if self.component == 3 {
            self.component = 0;
            self.address = self.address.wrapping_add(1);
        }
    }

    pub(crate) fn read(&mut self) -> u8 {
        let value = self.colors[usize::from(self.address)][self.component];
        self.step();
        value
    }

    pub(crate) fn write(&mut self, value: u8) {
        self.colors[usize::from(self.address)][self.component] =
            value & ((1 << Self::BITS_PER_COMPONENT) - 1);
        self.step();
    }
}
