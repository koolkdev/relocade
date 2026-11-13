//! ET4000AX Rev E extensions. Indices remain owned by the VGA register banks.
//! Tseng data book pp. 102, 111, 129–144, 158 and the Rev E addendum.

pub(super) const SEGMENT_SELECT: u16 = 0x3cd;
pub(super) const HERCULES_CONTROL: u16 = 0x3bf;
pub(super) const VIDEO_ENABLE: u16 = 0x3c3;
pub(super) const EXTENDED_START: u8 = 0x33;
pub(super) const COMPATIBILITY: u8 = 0x34;
pub(super) const OVERFLOW: u8 = 0x35;
pub(super) const CONFIGURATION: u8 = 0x36;
pub(super) const AUXILIARY: u8 = 7;
pub(super) const ATTRIBUTE_MISC: u8 = 0x16;

pub(super) struct Extensions {
    crtc: [u8; 6],
    sequencer: [u8; 2],
    pub attribute: u8,
    hercules: u8,
    mode: u8,
    segment_access: bool,
    pub segments: u8,
}

impl Default for Extensions {
    fn default() -> Self {
        Self {
            // Post-firmware VGA preset: 46E8 enable, 32-bit DRAM, 256Kx4 depth.
            crtc: [0, 0, 8, 0, 0x40, 0x0b],
            sequencer: [0, 0xbc],
            attribute: 0,
            hercules: 0,
            mode: 0,
            segment_access: false,
            segments: 0,
        }
    }
}

impl Extensions {
    pub fn unlocked(&self) -> bool {
        self.hercules == 3 && self.mode & 0xa0 == 0xa0
    }

    pub fn write_hercules(&mut self, value: u8) {
        self.hercules = value & 3;
        self.segment_access |= self.unlocked();
    }

    pub fn write_mode(&mut self, value: u8) {
        self.mode = if self.hercules & 2 != 0 {
            value
        } else {
            value & 0x7f
        };
        self.segment_access |= self.unlocked();
    }

    pub fn read_mode(&self, color: bool) -> u8 {
        let mask = if color { 0x3f } else { 0xab };
        (self.mode & mask) | ((self.hercules & 2) << 5)
    }

    pub fn read_segments(&self) -> u8 {
        if self.segment_access {
            self.segments
        } else {
            0xff
        }
    }

    pub fn write_segments(&mut self, value: u8) {
        if self.segment_access {
            self.segments = value;
        }
    }

    pub fn synchronous_reset(&mut self) {
        self.segment_access = false;
    }

    pub fn crtc(&self, index: u8) -> Option<u8> {
        index
            .checked_sub(0x32)
            .and_then(|i| self.crtc.get(usize::from(i)))
            .copied()
    }

    pub fn write_crtc(&mut self, index: u8, value: u8, protected: bool) {
        let mask = match index {
            EXTENDED_START => 0x0f,
            OVERFLOW if protected => 0x90,
            OVERFLOW => 0xff,
            0x32 | COMPATIBILITY | CONFIGURATION | 0x37 if self.unlocked() => 0xff,
            _ => return,
        };
        let register = &mut self.crtc[usize::from(index - 0x32)];
        *register = if index == OVERFLOW {
            (*register & !mask) | (value & mask)
        } else {
            value & mask
        };
    }

    pub fn sequencer(&self, index: u8) -> Option<u8> {
        index
            .checked_sub(6)
            .and_then(|i| self.sequencer.get(usize::from(i)))
            .copied()
    }

    pub fn write_sequencer(&mut self, index: u8, value: u8) {
        if self.unlocked() {
            match index {
                6 => self.sequencer[0] = value & 6,
                AUXILIARY => self.sequencer[1] = value,
                _ => {}
            }
        }
    }

    pub fn write_attribute(&mut self, index: u8, value: u8) -> bool {
        if index != ATTRIBUTE_MISC {
            return false;
        }
        if self.unlocked() {
            self.attribute = value & 0xf0;
        }
        true
    }

    pub fn uses_46e8(&self) -> bool {
        self.crtc[usize::from(COMPATIBILITY - 0x32)] & 8 != 0
    }
}
