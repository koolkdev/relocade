//! Direct port programming for the standard 320-by-200, 256-color layout.
//! Kept independent of BIOS mode tables so those tables are tested separately.
use relocade_machine::Device;

use super::registers::{attribute, indexed};

pub fn program(vga: &mut impl Device) {
    vga.write_port(0x3c2, 1, 0x63);
    for (index, value) in [(0, 1), (1, 1), (2, 15), (3, 0), (4, 14)] {
        indexed(vga, 0x3c4, index, value);
    }
    for (index, value) in [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 0x40),
        (6, 5),
        (7, 15),
        (8, 255),
    ] {
        indexed(vga, 0x3ce, index, value);
    }
    indexed(vga, 0x3d4, 0x11, 0);
    for (index, value) in [
        (1, 79),
        (6, 0xbf),
        (7, 0x1f),
        (8, 0),
        (9, 0x41),
        (12, 0),
        (13, 0),
        (0x10, 0x9c),
        (0x12, 0x8f),
        (0x13, 40),
        (0x14, 0x40),
        (0x15, 0x96),
        (0x16, 0xb9),
        (0x17, 0xa3),
        (0x18, 0xff),
    ] {
        indexed(vga, 0x3d4, index, value);
    }
    for color in 0..16 {
        attribute(vga, color, color);
    }
    for (index, value) in [(0x10, 0x41), (0x11, 0), (0x12, 15), (0x13, 0), (0x14, 0)] {
        attribute(vga, index, value);
    }
    vga.write_port(0x3c6, 1, 0xff);
    indexed(vga, 0x3c4, 0, 3);
}

/// Unchain the CPU aperture and switch the CRTC to byte addresses. Four
/// consecutive pixels occupy the four planes at one address, including above 16 KiB.
pub fn unchain(vga: &mut impl Device) {
    indexed(vga, 0x3c4, 4, 6);
    indexed(vga, 0x3d4, 0x14, 0);
    indexed(vga, 0x3d4, 0x17, 0xe3);
}
