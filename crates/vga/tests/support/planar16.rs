//! Direct four-plane, 640-by-480 programming, independent of BIOS mode tables.
use super::registers::{attribute, indexed};
use relocade_machine::Device;

pub fn program(vga: &mut impl Device) {
    vga.write_port(0x3c2, 1, 0xe3);
    for (index, value) in [(0, 1), (1, 1), (2, 15), (3, 0), (4, 6)] {
        indexed(vga, 0x3c4, index, value);
    }
    for (index, value) in [
        (0, 0),
        (1, 0),
        (2, 0),
        (3, 0),
        (4, 0),
        (5, 0),
        (6, 5),
        (7, 15),
        (8, 255),
    ] {
        indexed(vga, 0x3ce, index, value);
    }
    indexed(vga, 0x3d4, 0x11, 0);
    for (index, value) in [
        (0, 0x5f),
        (1, 79),
        (2, 0x50),
        (3, 0x82),
        (4, 0x54),
        (5, 0x80),
        (6, 0x0b),
        (7, 0x3e),
        (8, 0),
        (9, 0x40),
        (12, 0),
        (13, 0),
        (0x10, 0xea),
        (0x12, 0xdf),
        (0x13, 40),
        (0x14, 0),
        (0x15, 0xe7),
        (0x16, 4),
        (0x17, 0xe3),
        (0x18, 255),
    ] {
        indexed(vga, 0x3d4, index, value);
    }
    for color in 0..16 {
        attribute(vga, color, color);
    }
    for (index, value) in [(0x10, 1), (0x11, 0), (0x12, 15), (0x13, 0), (0x14, 0)] {
        attribute(vga, index, value);
    }
    vga.write_port(0x3c6, 1, 255);
    indexed(vga, 0x3c4, 0, 3);
}

/// Pack a chosen sequence of four-bit pixel indices into the physical planes.
/// One CPU byte supplies eight horizontal pixels, high bit first.
pub fn write_pixels(vga: &mut impl Device, indices: &[u8]) {
    assert_eq!(indices.len() % 8, 0);
    for plane in 0..4 {
        indexed(vga, 0x3c4, 2, 1 << plane);
        for (offset, group) in indices.as_chunks::<8>().0.iter().enumerate() {
            let mut bits = 0u8;
            for (pixel, index) in group.iter().enumerate() {
                bits |= ((index >> plane) & 1) << (7 - pixel);
            }
            vga.write_mmio(0xa0000 + offset as u32, 1, u64::from(bits));
        }
    }
    indexed(vga, 0x3c4, 2, 15);
}
