//! CPU address decoding for the PC/AT VGA adapter, independent of scanout.

const SETUP_MODE: u8 = 0x10;
const ADDRESSING_ENABLE: u8 = 0x08;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Setup {
    control: u8,
    option: u8,
}

impl Default for Setup {
    fn default() -> Self {
        // Like the register banks, start in the state following firmware setup.
        Self {
            control: ADDRESSING_ENABLE,
            option: 1,
        }
    }
}

impl Setup {
    pub(crate) fn to_bytes(&self) -> [u8; 2] {
        [self.control, self.option]
    }

    pub(crate) fn from_bytes([control, option]: [u8; 2]) -> Option<Self> {
        (control & !(SETUP_MODE | ADDRESSING_ENABLE) == 0 && option <= 1)
            .then_some(Self { control, option })
    }

    pub(crate) fn enabled(&self) -> bool {
        self.control == ADDRESSING_ENABLE && self.option != 0
    }

    pub(crate) fn write_control(&mut self, value: u8) {
        // ROM bank selection is outside this device's memory aperture.
        self.control = value & (SETUP_MODE | ADDRESSING_ENABLE);
    }

    pub(crate) fn read_option(&self) -> u8 {
        if self.control & SETUP_MODE != 0 {
            self.option
        } else {
            0xff
        }
    }

    pub(crate) fn write_option(&mut self, value: u8) {
        // Our setup-priority convention permits IBM's adapter initialization
        // sequence: 46E8h=16h, 0102h=01h, then 46E8h=0Eh. See the README.
        if self.control & SETUP_MODE != 0 {
            self.option = value & 1;
        }
    }
}
