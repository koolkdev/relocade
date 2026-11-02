//! DAC block with Color Select at 303h (the corrected IBM format in RBIL).
use super::super::palette;
use relocade_machine::IoBus;
use relocade_vga::registers::{
    attribute, DAC_DATA_PORT, DAC_MASK_PORT, DAC_READ_ADDRESS_PORT, DAC_WRITE_ADDRESS_PORT,
};

pub(super) const SIZE: usize = 0x304;

pub(super) fn save(io: &IoBus) -> [u8; SIZE] {
    let mut bytes = [0; SIZE];
    bytes[0] = io.read_port(DAC_READ_ADDRESS_PORT, 1) as u8 & 1;
    // In read mode the address register is one entry ahead of the RGB data.
    bytes[1] = (io.read_port(DAC_WRITE_ADDRESS_PORT, 1) as u8).wrapping_sub(bytes[0]);
    bytes[2] = io.read_port(DAC_MASK_PORT, 1) as u8;
    io.write_port(DAC_READ_ADDRESS_PORT, 1, 0);
    for component in &mut bytes[3..0x303] {
        *component = io.read_port(DAC_DATA_PORT, 1) as u8;
    }
    bytes[0x303] = palette::read_attribute(io, attribute::COLOR_SELECT);
    select_transfer(io, &bytes);
    bytes
}

fn select_transfer(io: &IoBus, bytes: &[u8; SIZE]) {
    let port = if bytes[0] & 1 != 0 {
        DAC_READ_ADDRESS_PORT
    } else {
        DAC_WRITE_ADDRESS_PORT
    };
    // The ABI has no RGB component field; transfers restart at red.
    io.write_port(port, 1, u32::from(bytes[1]));
}
