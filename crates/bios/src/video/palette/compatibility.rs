//! The legacy BIOS color service programs VGA Attribute Controller registers.
use super::{modes, unsupported, write_attribute};
use crate::screen;
use crate::video::CRTC_PORT;
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::{attribute, MONO_CRTC_INDEX_PORT};

// CGA register shadows in the BIOS data area, not emulated CGA I/O ports.
const MODE_SHADOW: u32 = 0x465;
const COLOR_SHADOW: u32 = 0x466;

pub(in crate::video) fn initialize(memory: &mut MemoryView<'_>, mode: u8) {
    let (control, color) = match mode {
        modes::TEXT_40X25 => (0x2c, 0x30),
        modes::COLOR_TEXT_40X25 => (0x28, 0x30),
        modes::TEXT_80X25 => (0x2d, 0x30),
        modes::COLOR_TEXT_80X25 | modes::MONO_TEXT_80X25 => (0x29, 0x30),
        modes::GRAPHICS_320X200_4 => (0x2a, 0x30),
        modes::GRAPHICS_320X200_4_ALT => (0x2e, 0x30),
        modes::GRAPHICS_640X200_2 => (0x1e, 0x3f),
        _ => return, // IBM VGA mode sets above 07h retain these shadows.
    };
    memory
        .write(MODE_SHADOW, &[control, color])
        .expect("video compatibility shadows");
}

pub(in crate::video) fn interrupt(call: &mut crate::BiosContext<'_>) {
    // IBM's legacy CGA color service does nothing with monochrome CRTC routing.
    if call.memory.read_u16(CRTC_PORT).expect("CRTC port") == MONO_CRTC_INDEX_PORT {
        return;
    }
    let value = call.cpu.bl();
    let previous = call
        .memory
        .read_u8(COLOR_SHADOW)
        .expect("video color shadow");
    let text = screen::is_text_mode(&call.memory);
    match call.cpu.bh() {
        0 => {
            let shadow = (previous & 0xe0) | (value & 31);
            call.memory
                .write_u8(COLOR_SHADOW, shadow)
                .expect("video color shadow");
            let color = (value & 7) | ((value & 8) << 1);
            write_attribute(call.io, attribute::OVERSCAN_COLOR, color);
            if !text {
                write_attribute(call.io, 0, color);
                foreground_palette(call.io, shadow);
            }
        }
        1 if text => {} // Palette selection is a complete no-op in text modes.
        1 => {
            let shadow = (previous & !0x20) | ((value & 1) << 5);
            call.memory
                .write_u8(COLOR_SHADOW, shadow)
                .expect("video color shadow");
            foreground_palette(call.io, shadow);
        }
        _ => unsupported(call),
    }
}

fn foreground_palette(io: &IoBus, shadow: u8) {
    let selection = (shadow >> 5) & 1;
    let intensity = shadow & 0x10;
    for index in 1..=3 {
        write_attribute(io, index, (index * 2) | selection | intensity);
    }
}
