//! BIOS Attribute Controller palette entries, border color and blink control.
use crate::video::{input_status_1_port, tables::Pointer};
use relocade_machine::{
    real_mode_interrupt::{interrupt, FarPointer},
    IoBus, MemoryView,
};
use relocade_vga::registers::{attribute, ATTRIBUTE_ADDRESS_PORT, ATTRIBUTE_DATA_READ_PORT};

#[interrupt(context(memory, io), args(index = bl, value = bh),
    returns { true => {}, false => crate::interrupt::unsupported },
)]
pub(in crate::video) fn set_register(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    index: u8,
    value: u8,
) -> bool {
    if index > attribute::COLOR_SELECT {
        return false;
    }
    write_attribute(memory, io, index, value);
    true
}

#[interrupt(context(io), args(index = bl), returns {
    Some(value) => { bh = value }, None => crate::interrupt::unsupported,
})]
pub(in crate::video) fn register(io: &IoBus, index: u8) -> Option<u8> {
    (index <= attribute::COLOR_SELECT).then(|| read_attribute(io, index))
}

#[interrupt(context(memory, io), args(color = bh))]
pub(in crate::video) fn set_overscan(memory: &mut MemoryView<'_>, io: &IoBus, color: u8) {
    write_attribute(memory, io, attribute::OVERSCAN_COLOR, color);
}

#[interrupt(context(io), returns { color => { bh = color } })]
pub(in crate::video) fn overscan(io: &IoBus) -> u8 {
    read_attribute(io, attribute::OVERSCAN_COLOR)
}

#[interrupt(context(memory, io), args(source = ptr(es, dx)))]
pub(in crate::video) fn load_palette(memory: &mut MemoryView<'_>, io: &IoBus, source: FarPointer) {
    for (offset, index) in palette_registers().enumerate() {
        let value = memory
            .read_u8(source.wrapping_add(offset).address())
            .expect("attribute palette buffer");
        write_attribute(memory, io, index, value);
    }
}

#[interrupt(context(memory, io), args(destination = ptr(es, dx)))]
pub(in crate::video) fn read_palette(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    destination: FarPointer,
) {
    for (offset, index) in palette_registers().enumerate() {
        memory
            .write_u8(
                destination.wrapping_add(offset).address(),
                read_attribute(io, index),
            )
            .expect("attribute palette buffer");
    }
}

// The 17-byte BIOS buffer contains sixteen palette entries followed by overscan.
fn palette_registers() -> impl Iterator<Item = u8> {
    (0..16).chain(std::iter::once(attribute::OVERSCAN_COLOR))
}

/// Write an Attribute Controller register and save palette/overscan values in
/// the optional BIOS dynamic palette area.
pub(in crate::video) fn write_attribute(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    index: u8,
    value: u8,
) {
    write_raw_attribute(io, index, value);
    let offset = match index {
        0..=15 => usize::from(index),
        attribute::OVERSCAN_COLOR => 16,
        _ => return,
    };
    if let Some(area) = Pointer::DynamicPalette.read(memory) {
        // Store the requested byte, before VGA register masking. Bytes 17–255
        // are reserved; overscan belongs at 16, not at its hardware index 11h.
        memory
            .write_u8(area.wrapping_add(offset).address(), value)
            .expect("dynamic palette save area");
    }
}

#[interrupt(context(io), args(selection = bl),
    returns { true => {}, false => crate::interrupt::unsupported },
)]
pub(in crate::video) fn set_blink(io: &IoBus, selection: u8) -> bool {
    if selection > 1 {
        return false;
    }
    let mode = read_attribute(io, attribute::MODE_CONTROL);
    write_raw_attribute(io, attribute::MODE_CONTROL, (mode & !8) | (selection << 3));
    true
}

fn select_attribute(io: &IoBus, address: u8) {
    io.read_port(input_status_1_port(io), 1); // Reset the address/data flip-flop.
    io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(address));
}

pub(in crate::video) fn read_attribute(io: &IoBus, index: u8) -> u8 {
    let saved_address = io.read_port(ATTRIBUTE_ADDRESS_PORT, 1) as u8;
    select_attribute(io, index);
    let value = io.read_port(ATTRIBUTE_DATA_READ_PORT, 1) as u8;
    select_attribute(io, saved_address);
    value
}

/// Write only the hardware register, leaving the BIOS palette save area untouched.
pub(in crate::video) fn write_raw_attribute(io: &IoBus, index: u8, value: u8) {
    let saved_address = io.read_port(ATTRIBUTE_ADDRESS_PORT, 1) as u8;
    // Selecting a palette index clears PAS so Type-1 VGA accepts the write.
    select_attribute(io, index);
    io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(value));
    select_attribute(io, saved_address); // Preserve display enable and index.
}
