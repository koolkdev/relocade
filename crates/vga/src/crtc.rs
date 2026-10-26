use crate::{CRTC_DATA_PORT, CRTC_INDEX_PORT};

/// The supported CRTC registers, in the initial fixed text addressing mode.
#[derive(Default)]
pub(super) struct Crtc {
    index: u8,
    pub start_address: u16,
    pub cursor_address: u16,
    pub cursor_start: u8,
    pub cursor_end: u8,
}

impl Crtc {
    pub fn read(&self, port: u16) -> u8 {
        match port {
            CRTC_INDEX_PORT => self.index,
            CRTC_DATA_PORT => match self.index {
                0x0a => self.cursor_start,
                0x0b => self.cursor_end,
                0x0c => (self.start_address >> 8) as u8,
                0x0d => self.start_address as u8,
                0x0e => (self.cursor_address >> 8) as u8,
                0x0f => self.cursor_address as u8,
                _ => 0xff,
            },
            _ => 0xff,
        }
    }

    pub fn write(&mut self, port: u16, value: u8) {
        match port {
            CRTC_INDEX_PORT => self.index = value & 0x1f,
            CRTC_DATA_PORT => match self.index {
                0x0a => self.cursor_start = value & 0x3f,
                0x0b => self.cursor_end = value & 0x7f,
                0x0c => {
                    self.start_address = (self.start_address & 0x00ff) | (u16::from(value) << 8)
                }
                0x0d => self.start_address = (self.start_address & 0xff00) | u16::from(value),
                0x0e => {
                    self.cursor_address = (self.cursor_address & 0x00ff) | (u16::from(value) << 8)
                }
                0x0f => self.cursor_address = (self.cursor_address & 0xff00) | u16::from(value),
                _ => {}
            },
            _ => {}
        }
    }
}
