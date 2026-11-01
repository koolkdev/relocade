//! BIOS raster presets share timings; each mode overrides its scanline and address controls.
use super::super::write_indexed;
use relocade_machine::IoBus;
use relocade_vga::registers::{crtc, text_mode};

#[derive(Clone, Copy)]
pub(super) struct CrtcPreset([u8; 0x19]);

impl CrtcPreset {
    // Keep the startup profile shared with the device's power-on defaults.
    pub const COLOR_TEXT_200: Self = Self([0; 0x19]).with(text_mode::CRTC_REGISTERS);

    pub const fn with(mut self, registers: &[(u8, u8)]) -> Self {
        let mut index = 0;
        while index < registers.len() {
            let (register, value) = registers[index];
            self.0[register as usize] = value;
            index += 1;
        }
        self
    }

    pub fn program(&self, io: &IoBus, port: u16) {
        write_indexed(io, port, crtc::VERTICAL_RETRACE_END, 0);
        for (register, &value) in self.0.iter().enumerate() {
            let register = register as u8;
            // Mode setup initializes the cursor and display start separately.
            // Restore timing protection after all timing writes are complete.
            if matches!(
                register,
                crtc::CURSOR_START..=crtc::CURSOR_ADDRESS_LOW | crtc::VERTICAL_RETRACE_END
            ) {
                continue;
            }
            write_indexed(io, port, register, value);
        }
        write_indexed(
            io,
            port,
            crtc::VERTICAL_RETRACE_END,
            self.0[crtc::VERTICAL_RETRACE_END as usize],
        );
    }
}

pub(super) const HORIZONTAL_40: &[(u8, u8)] = &[
    (crtc::HORIZONTAL_TOTAL, 0x2d),
    (crtc::HORIZONTAL_DISPLAY_END, 0x27),
    (crtc::HORIZONTAL_BLANK_START, 0x28),
    (crtc::HORIZONTAL_BLANK_END, 0x90),
    (crtc::HORIZONTAL_RETRACE_START, 0x2b),
    (crtc::HORIZONTAL_RETRACE_END, 0x80),
];

pub(super) const VERTICAL_350: &[(u8, u8)] = &[
    (crtc::VERTICAL_TOTAL, 0xbf),
    (crtc::OVERFLOW, 0x1f),
    (crtc::VERTICAL_RETRACE_START, 0x83),
    (crtc::VERTICAL_DISPLAY_END, 0x5d),
    (crtc::VERTICAL_BLANK_START, 0x63),
    (crtc::VERTICAL_BLANK_END, 0xba),
    (crtc::VERTICAL_RETRACE_END, 0x85),
];

pub(super) const VERTICAL_480: &[(u8, u8)] = &[
    (crtc::VERTICAL_TOTAL, 0x0b),
    (crtc::OVERFLOW, 0x3e),
    (crtc::VERTICAL_RETRACE_START, 0xea),
    (crtc::VERTICAL_DISPLAY_END, 0xdf),
    (crtc::VERTICAL_BLANK_START, 0xe7),
    (crtc::VERTICAL_BLANK_END, 4),
    (crtc::VERTICAL_RETRACE_END, 0x8c),
];
