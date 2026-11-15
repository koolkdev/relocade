//! RGB palette and sequential CPU access, IBM VGA Function pp. 2-104–106.

use super::text_mode;

/// DAC colors, pixel mask and CPU palette-transfer position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dac {
    pub mask: u8,
    /// RGB components in the active precision, in address order.
    pub colors: [[u8; 3]; 256],
    bits_per_component: u8,
    address: u8,
    component: usize,
    reading: bool,
}

impl Default for Dac {
    fn default() -> Self {
        Self {
            mask: 0xff,
            colors: text_mode::DAC_PALETTE,
            bits_per_component: 6,
            address: 0,
            component: 0,
            reading: false,
        }
    }
}

impl Dac {
    /// Standard VGA component width before a card enables extended precision.
    pub const BITS_PER_COMPONENT: u8 = 6;

    pub fn bits_per_component(&self) -> u8 {
        self.bits_per_component
    }

    /// Changing precision preserves the transfer position and shifts stored
    /// components by two bits. Reducing precision discards the low two bits.
    /// Accepts six or eight bits per component.
    pub fn set_bits_per_component(&mut self, bits: u8) {
        assert!(
            matches!(bits, 6 | 8),
            "DAC components must have six or eight bits"
        );
        if self.bits_per_component == bits {
            return;
        }
        for component in self.colors.as_flattened_mut() {
            *component = if bits == 8 {
                *component << 2
            } else {
                *component >> 2
            };
        }
        self.bits_per_component = bits;
    }

    pub(crate) fn to_state_bytes(&self) -> [u8; 772] {
        let mut bytes = [0; 772];
        bytes[..4].copy_from_slice(&[
            self.mask,
            self.address,
            self.component as u8,
            u8::from(self.reading) | (u8::from(self.bits_per_component == 8) << 1),
        ]);
        bytes[4..].copy_from_slice(self.colors.as_flattened());
        bytes
    }

    pub(crate) fn from_state_bytes(bytes: &[u8; 772]) -> Option<Self> {
        let bits_per_component = if bytes[3] & 2 != 0 { 8 } else { 6 };
        if bytes[2] >= 3
            || bytes[3] & !3 != 0
            || (bits_per_component == 6 && bytes[4..].iter().any(|&value| value > 63))
        {
            return None;
        }
        let mut colors = [[0; 3]; 256];
        colors.as_flattened_mut().copy_from_slice(&bytes[4..]);
        Some(Self {
            mask: bytes[0],
            address: bytes[1],
            component: usize::from(bytes[2]),
            reading: bytes[3] & 1 != 0,
            colors,
            bits_per_component,
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
        let mask = u8::MAX >> (8 - self.bits_per_component);
        self.colors[usize::from(self.address)][self.component] = value & mask;
        self.step();
    }

    /// A complete color transfer has the same masking and final position as
    /// sequential writes through the DAC address and data ports.
    pub(crate) fn write_colors(&mut self, first: u8, colors: &[[u8; 3]]) {
        if colors.is_empty() {
            return;
        }
        self.select(first, false);
        for &component in colors.as_flattened() {
            self.write(component);
        }
    }
}

#[cfg(test)]
mod tests;
