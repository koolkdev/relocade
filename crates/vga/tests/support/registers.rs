//! Guest-visible register programming shared by display fixtures.

pub fn indexed(io: &relocade_machine::IoBus, port: u16, index: u8, value: u8) {
    io.write_port(port, 2, u32::from(u16::from_le_bytes([index, value])));
}

pub fn cursor(io: &relocade_machine::IoBus, address: u16, start: u8, end: u8) {
    let [high, low] = address.to_be_bytes();
    for (index, value) in [(0x0e, high), (0x0f, low), (0x0a, start), (0x0b, end)] {
        indexed(io, 0x3d4, index, value);
    }
}

pub fn attribute(io: &relocade_machine::IoBus, index: u8, value: u8) {
    io.read_port(0x3da, 1);
    io.write_port(0x3c0, 1, u32::from(index));
    io.write_port(0x3c0, 1, u32::from(value));
    io.write_port(0x3c0, 1, 0x20);
}

pub fn palette(io: &relocade_machine::IoBus, index: u8, rgb: [u8; 3]) {
    io.write_port(0x3c8, 1, u32::from(index));
    for component in rgb {
        io.write_port(0x3c9, 1, u32::from(component));
    }
}
