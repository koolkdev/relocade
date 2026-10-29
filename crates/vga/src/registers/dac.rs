//! Six-bit RGB palette and sequential CPU access, IBM VGA Function pp. 2-104–106.

pub(crate) struct Dac {
    pub mask: u8,
    colors: [[u8; 3]; 256],
    address: u8,
    component: usize,
    reading: bool,
}

impl Default for Dac {
    fn default() -> Self {
        let mut colors = [[0; 3]; 256];
        // The initial EGA-compatible palette pairs with the text attribute palette.
        for (index, color) in colors[..64].iter_mut().enumerate() {
            *color = [
                (((index >> 2) & 1) * 42 + ((index >> 5) & 1) * 21) as u8,
                (((index >> 1) & 1) * 42 + ((index >> 4) & 1) * 21) as u8,
                ((index & 1) * 42 + ((index >> 3) & 1) * 21) as u8,
            ];
        }
        Self {
            mask: 0xff,
            colors,
            address: 0,
            component: 0,
            reading: false,
        }
    }
}

impl Dac {
    pub fn select(&mut self, address: u8, reading: bool) {
        self.address = address;
        self.component = 0;
        self.reading = reading;
    }

    pub fn address(&self) -> u8 {
        self.address
    }

    pub fn state(&self) -> u8 {
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

    pub fn read(&mut self) -> u8 {
        let value = self.colors[usize::from(self.address)][self.component];
        self.step();
        value
    }

    pub fn write(&mut self, value: u8) {
        self.colors[usize::from(self.address)][self.component] = value & 0x3f;
        self.step();
    }
}
