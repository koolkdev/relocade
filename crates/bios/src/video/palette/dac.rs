//! BIOS DAC color transfers and IBM VGA grayscale conversion.
use super::PaletteLoad;
use relocade_machine::{
    real_mode_interrupt::{interrupt, FarPointer},
    IoBus, MemoryView,
};
use relocade_vga::registers::{
    DAC_DATA_PORT, DAC_MASK_PORT, DAC_READ_ADDRESS_PORT, DAC_WRITE_ADDRESS_PORT,
};

#[interrupt(context(memory, io), args(index = bl, red = dh, green = ch, blue = cl))]
pub(in crate::video) fn set_color(
    memory: &MemoryView<'_>,
    io: &IoBus,
    index: u8,
    red: u8,
    green: u8,
    blue: u8,
) {
    PaletteLoad::for_color_write(memory).write(io, index, [[red, green, blue]]);
}

#[interrupt(context(io), args(index = bl), returns {
    [red, green, blue] => { dh = red, ch = green, cl = blue },
})]
pub(in crate::video) fn color(io: &IoBus, index: u8) -> [u8; 3] {
    io.write_port(DAC_READ_ADDRESS_PORT, 1, u32::from(index));
    std::array::from_fn(|_| io.read_port(DAC_DATA_PORT, 1) as u8)
}

#[interrupt(context(memory, io), args(first = bl, count = cx, source = ptr(es, dx)))]
pub(in crate::video) fn load_palette(
    memory: &MemoryView<'_>,
    io: &IoBus,
    first: u8,
    count: u16,
    source: FarPointer,
) {
    let colors = (0..count).map(|index| {
        let mut color = [0; 3];
        memory
            .read_wrapping(source.wrapping_add(usize::from(index) * 3), &mut color)
            .expect("DAC palette buffer");
        color
    });
    PaletteLoad::for_color_write(memory).write(io, first, colors);
}

#[interrupt(context(memory, io), args(first = bl, count = cx, destination = ptr(es, dx)))]
pub(in crate::video) fn read_palette(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    first: u8,
    count: u16,
    destination: FarPointer,
) {
    // Zero-length transfers do not disturb DAC access state.
    if count == 0 {
        return;
    }
    io.write_port(DAC_READ_ADDRESS_PORT, 1, u32::from(first));
    for offset in 0..usize::from(count) * 3 {
        memory
            .write_u8(
                destination.wrapping_add(offset).address(),
                io.read_port(DAC_DATA_PORT, 1) as u8,
            )
            .expect("DAC palette buffer");
    }
}

#[interrupt(context(io), args(mask = bl))]
pub(in crate::video) fn set_mask(io: &IoBus, mask: u8) {
    io.write_port(DAC_MASK_PORT, 1, u32::from(mask));
}

#[interrupt(context(io), returns { mask => { bl = mask } })]
pub(in crate::video) fn mask(io: &IoBus) -> u8 {
    io.read_port(DAC_MASK_PORT, 1) as u8
}

pub(in crate::video) fn write(io: &IoBus, index: u8, colors: impl IntoIterator<Item = [u8; 3]>) {
    let mut colors = colors.into_iter().peekable();
    if colors.peek().is_none() {
        return;
    }
    io.write_port(DAC_WRITE_ADDRESS_PORT, 1, u32::from(index));
    for component in colors.flatten() {
        io.write_port(DAC_DATA_PORT, 1, u32::from(component));
    }
}

pub(super) fn grayscale(color: [u8; 3]) -> [u8; 3] {
    let [red, green, blue] = color.map(|component| u32::from(component & 63));
    // IBM VGA ROM x2DF0: 15-bit weights for 30% red, 59% green, 11% blue,
    // rounded by adding half a unit. The masked inputs keep the result within 0..63.
    let intensity = ((9830 * red + 19333 * green + 3604 * blue + 16384) >> 15) as u8;
    [intensity; 3]
}

#[interrupt(context(io), args(first = bl, count = cx))]
pub(in crate::video) fn grayscale_range(io: &IoBus, first: u8, count: u16) {
    // A zero count preserves even an unfinished DAC transfer, like the other
    // BIOS block operations. The eight-bit DAC index wraps independently.
    for offset in 0..count {
        let index = first.wrapping_add(offset as u8);
        let color = color(io, index);
        write(io, index, [grayscale(color)]);
    }
}
