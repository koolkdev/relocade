//! Guest-visible programming of banked DISPI modes.

pub fn set(io: &relocade_machine::IoBus, index: u16, value: u16) {
    io.write_port(0x1ce, 2, u32::from(index));
    io.write_port(0x1cf, 2, u32::from(value));
}

pub fn get(io: &relocade_machine::IoBus, index: u16) -> u32 {
    io.write_port(0x1ce, 2, u32::from(index));
    io.read_port(0x1cf, 2)
}

pub fn enable(io: &relocade_machine::IoBus) {
    set_mode(io, 640, 480, 8);
}

pub fn set_mode(io: &relocade_machine::IoBus, width: u16, height: u16, bits_per_pixel: u16) {
    set(io, 4, 0);
    for (index, value) in [(1, width), (2, height), (3, bits_per_pixel), (4, 1)] {
        set(io, index, value);
    }
    assert_eq!(get(io, 4), 1);
}

pub fn write_byte(io: &relocade_machine::IoBus, address: u32, value: u8) {
    set(io, 5, (address / 65536) as u16);
    io.write_mmio(0xa0000 + address % 65536, 1, u64::from(value));
}
