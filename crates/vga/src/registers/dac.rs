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
    pub(crate) fn select(&mut self, address: u8, reading: bool) {
        self.address = address;
        self.component = 0;
        self.reading = reading;
    }

    pub fn address(&self) -> u8 {
        self.address
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
        self.colors[usize::from(self.address)][self.component] = value & 0x3f;
        self.step();
    }
}
