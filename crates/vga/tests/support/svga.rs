//! Guest-visible programming of banked DISPI modes.
use relocade_machine::Device;

pub fn set(vga: &mut impl Device, index: u16, value: u16) {
    vga.write_port(0x1ce, 2, u32::from(index));
    vga.write_port(0x1cf, 2, u32::from(value));
}

pub fn get(vga: &mut impl Device, index: u16) -> u32 {
    vga.write_port(0x1ce, 2, u32::from(index));
    vga.read_port(0x1cf, 2)
}

pub fn enable(vga: &mut impl Device) {
    set_mode(vga, 640, 480);
}

pub fn set_mode(vga: &mut impl Device, width: u16, height: u16) {
    set(vga, 4, 0);
    for (index, value) in [(1, width), (2, height), (3, 8), (4, 1)] {
        set(vga, index, value);
    }
    assert_eq!(get(vga, 4), 1);
}

pub fn write_byte(vga: &mut impl Device, address: u32, value: u8) {
    set(vga, 5, (address / 65536) as u16);
    vga.write_mmio(0xa0000 + address % 65536, 1, u64::from(value));
}
