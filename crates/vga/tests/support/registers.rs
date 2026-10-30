//! Guest-visible register programming shared by display fixtures.
use relocade_machine::Device;

pub fn indexed(vga: &mut impl Device, port: u16, index: u8, value: u8) {
    vga.write_port(port, 2, u32::from(u16::from_le_bytes([index, value])));
}

pub fn attribute(vga: &mut impl Device, index: u8, value: u8) {
    vga.read_port(0x3da, 1);
    vga.write_port(0x3c0, 1, u32::from(index));
    vga.write_port(0x3c0, 1, u32::from(value));
    vga.write_port(0x3c0, 1, 0x20);
}

pub fn palette(vga: &mut impl Device, index: u8, rgb: [u8; 3]) {
    vga.write_port(0x3c8, 1, u32::from(index));
    for component in rgb {
        vga.write_port(0x3c9, 1, u32::from(component));
    }
}
