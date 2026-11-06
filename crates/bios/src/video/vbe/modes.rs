//! Mode transitions and bank selection through guest-visible DISPI registers.
use super::{available, read_register, write_register};
use crate::video::{
    modes as legacy, palette::PaletteLoad, presets, ACTIVE_PAGE, CHARACTER_HEIGHT, COLUMNS,
    CURSORS, LAST_ROW, MODE, PAGE_OFFSET, PAGE_SIZE, VIDEO_CONTROL,
};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};
use relocade_vga::{
    registers::{dispi, MISC_OUTPUT_READ_PORT, MISC_OUTPUT_WRITE_PORT},
    VIDEO_MEMORY_BYTES,
};

// Outside the supported legacy modes: their output/font paths reject this marker.
const VBE_MODE_MARKER: u8 = 0x7f;

/// BIOS mode numbers and geometry belong here; DISPI owns hardware validation.
pub(super) struct Mode {
    pub(super) number: u16,
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) bits_per_pixel: u8,
}

impl Mode {
    pub(super) const INDEXED_640_480: Self = Self {
        number: 0x101,
        width: 640,
        height: 480,
        bits_per_pixel: 8,
    };

    pub(super) fn from_number(number: u16) -> Option<Self> {
        (number == Self::INDEXED_640_480.number).then_some(Self::INDEXED_640_480)
    }

    fn activate(self, memory: &mut MemoryView<'_>, io: &IoBus, preserve: bool) {
        // A new BIOS mode set re-enters the device mode, even if its geometry is unchanged.
        write_register(io, dispi::ENABLE, dispi::DISABLED);
        write_register(io, dispi::X_RESOLUTION, self.width);
        write_register(io, dispi::Y_RESOLUTION, self.height);
        write_register(io, dispi::BITS_PER_PIXEL, u16::from(self.bits_per_pixel));
        let flags = dispi::ENABLED | if preserve { dispi::NO_CLEAR_MEMORY } else { 0 };
        write_register(io, dispi::ENABLE, flags);
        let miscellaneous = io.read_port(MISC_OUTPUT_READ_PORT, 1);
        io.write_port(MISC_OUTPUT_WRITE_PORT, 1, miscellaneous | 2); // Enable CPU VRAM access.
        presets::INDEXED256.load_palette(io, PaletteLoad::for_mode_set(memory));

        memory.write_u8(MODE, VBE_MODE_MARKER).expect("video mode");
        memory
            .write_u16(COLUMNS, self.width / 8)
            .expect("video columns");
        memory
            .write_u8(LAST_ROW, (self.height / 16 - 1) as u8)
            .expect("video rows");
        memory.write_u16(CHARACTER_HEIGHT, 16).expect("font height");
        // The packed frame exceeds a 16-bit legacy page size; no legacy pages exist.
        memory.write_u16(PAGE_SIZE, 0).expect("video page size");
        memory.write_u16(PAGE_OFFSET, 0).expect("video page offset");
        memory.write_u8(ACTIVE_PAGE, 0).expect("active video page");
        memory.write(CURSORS, &[0; 16]).expect("video cursors");
        let control = memory.read_u8(VIDEO_CONTROL).expect("video control");
        memory
            .write_u8(
                VIDEO_CONTROL,
                (control & 0x7f) | if preserve { 0x80 } else { 0 },
            )
            .expect("video control");
    }
}

#[interrupt(context(memory, io), args(request = bx),
    returns { true => { ax = 0x004f }, false => { ax = 0x014f } },
)]
pub(super) fn set(memory: &mut MemoryView<'_>, io: &IoBus, request: u16) -> bool {
    if !available(io) {
        return false;
    }
    let number = request & 0x7fff;
    let preserve = request & 0x8000 != 0;
    if let Some(mode) = Mode::from_number(number) {
        mode.activate(memory, io, preserve);
        true
    } else if number < 0x80 {
        legacy::set_mode(memory, io, number as u8 | if preserve { 0x80 } else { 0 }).is_some()
    } else {
        false
    }
}

#[interrupt(context(memory, io), returns {
    Some(number) => { ax = 0x004f, bx = number }, None => { ax = 0x014f },
})]
pub(super) fn current(memory: &MemoryView<'_>, io: &IoBus) -> Option<u16> {
    if !available(io) {
        return None;
    }
    if read_register(io, dispi::ENABLE) & dispi::ENABLED != 0 {
        Some(Mode::INDEXED_640_480.number)
    } else {
        // VBE 1.2 returns the mode alone, without the memory-preservation flag.
        let number = legacy::mode(memory).mode & 0x7f;
        (number != VBE_MODE_MARKER).then_some(u16::from(number))
    }
}

#[interrupt(context(io), args(operation = bx, position = dx), returns {
    Some(position) => { ax = 0x004f, dx = position }, None => { ax = 0x014f },
})]
pub(super) fn window(io: &IoBus, operation: u16, position: u16) -> Option<u16> {
    if !available(io) || read_register(io, dispi::ENABLE) & dispi::ENABLED == 0 {
        return None;
    }
    match operation {
        0 if u32::from(position) < VIDEO_MEMORY_BYTES / dispi::BANK_BYTES => {
            write_register(io, dispi::BANK, position);
            Some(position)
        }
        0x100 => Some(read_register(io, dispi::BANK)),
        _ => None,
    }
}
