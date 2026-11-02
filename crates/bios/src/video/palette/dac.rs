//! BIOS DAC color transfers and IBM VGA grayscale conversion.
use relocade_machine::IoBus;
use relocade_vga::registers::{DAC_DATA_PORT, DAC_READ_ADDRESS_PORT, DAC_WRITE_ADDRESS_PORT};

pub(super) fn read(io: &IoBus, index: u8) -> [u8; 3] {
    io.write_port(DAC_READ_ADDRESS_PORT, 1, u32::from(index));
    std::array::from_fn(|_| io.read_port(DAC_DATA_PORT, 1) as u8)
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

pub(super) fn grayscale_range(io: &IoBus, first: u8, count: u16) {
    // A zero count preserves even an unfinished DAC transfer, like the other
    // BIOS block operations. The eight-bit DAC index wraps independently.
    for offset in 0..count {
        let index = first.wrapping_add(offset as u8);
        let color = read(io, index);
        write(io, index, [grayscale(color)]);
    }
}
