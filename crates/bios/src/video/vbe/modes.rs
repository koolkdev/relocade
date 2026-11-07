//! Mode transitions and bank selection through guest-visible DISPI registers.
use super::{available, read_register, write_register};
use crate::video::{
    modes as legacy, palette::PaletteLoad, presets, ACTIVE_PAGE, CHARACTER_HEIGHT, COLUMNS,
    CURSORS, LAST_ROW, MODE, PAGE_OFFSET, PAGE_SIZE, VIDEO_CONTROL,
};
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};
use relocade_vga::{
    registers::{dispi, MISC_OUTPUT_READ_PORT, MISC_OUTPUT_WRITE_PORT},
    PixelFormat, SvgaMode, SvgaResolution, VIDEO_MEMORY_BYTES,
};

// Outside the supported legacy modes: their output/font paths reject this marker.
const VBE_MODE_MARKER: u8 = 0x7f;

/// Firmware maps VESA mode numbers to the device's supported display modes.
#[derive(Clone, Copy)]
pub(super) struct Mode {
    pub(super) number: u16,
    pub(super) display: SvgaMode,
}

impl Mode {
    pub(super) const ALL: [Self; 9] = [
        Self {
            number: 0x101,
            display: SvgaMode::new(SvgaResolution::R640x480, PixelFormat::Indexed8),
        },
        Self {
            number: 0x103,
            display: SvgaMode::new(SvgaResolution::R800x600, PixelFormat::Indexed8),
        },
        Self {
            number: 0x105,
            display: SvgaMode::new(SvgaResolution::R1024x768, PixelFormat::Indexed8),
        },
        Self {
            number: 0x111,
            display: SvgaMode::new(SvgaResolution::R640x480, PixelFormat::Rgb565),
        },
        Self {
            number: 0x114,
            display: SvgaMode::new(SvgaResolution::R800x600, PixelFormat::Rgb565),
        },
        Self {
            number: 0x117,
            display: SvgaMode::new(SvgaResolution::R1024x768, PixelFormat::Rgb565),
        },
        // Bochs OEM numbers identify 32-bit storage without reusing 24-bit modes.
        Self {
            number: 0x142,
            display: SvgaMode::new(SvgaResolution::R640x480, PixelFormat::Bgrx8888),
        },
        Self {
            number: 0x143,
            display: SvgaMode::new(SvgaResolution::R800x600, PixelFormat::Bgrx8888),
        },
        Self {
            number: 0x144,
            display: SvgaMode::new(SvgaResolution::R1024x768, PixelFormat::Bgrx8888),
        },
    ];

    pub(super) fn from_number(number: u16) -> Option<Self> {
        Self::ALL.into_iter().find(|mode| mode.number == number)
    }

    pub(super) fn active(io: &IoBus) -> Option<Self> {
        if read_register(io, dispi::ENABLE) & dispi::ENABLED == 0 {
            return None;
        }
        let display = SvgaMode::from_registers(
            read_register(io, dispi::X_RESOLUTION),
            read_register(io, dispi::Y_RESOLUTION),
            read_register(io, dispi::BITS_PER_PIXEL),
        )?;
        Self::ALL.into_iter().find(|mode| mode.display == display)
    }

    fn activate(self, memory: &mut MemoryView<'_>, io: &IoBus, preserve: bool) {
        let [width, height] = self.display.size().map(|value| value as u16);
        // A new BIOS mode set re-enters the device mode, even if its geometry is unchanged.
        write_register(io, dispi::ENABLE, dispi::DISABLED);
        write_register(io, dispi::X_RESOLUTION, width);
        write_register(io, dispi::Y_RESOLUTION, height);
        write_register(
            io,
            dispi::BITS_PER_PIXEL,
            u16::from(self.display.bits_per_pixel()),
        );
        let flags = dispi::ENABLED | if preserve { dispi::NO_CLEAR_MEMORY } else { 0 };
        write_register(io, dispi::ENABLE, flags);
        let miscellaneous = io.read_port(MISC_OUTPUT_READ_PORT, 1);
        io.write_port(MISC_OUTPUT_WRITE_PORT, 1, miscellaneous | 2); // Enable CPU VRAM access.
        if self.display.format() == PixelFormat::Indexed8 {
            presets::INDEXED256.load_palette(io, PaletteLoad::for_mode_set(memory));
        }

        memory.write_u8(MODE, VBE_MODE_MARKER).expect("video mode");
        memory.write_u16(COLUMNS, width / 8).expect("video columns");
        memory
            .write_u8(LAST_ROW, (height / 16 - 1) as u8)
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
    if let Some(mode) = Mode::active(io) {
        Some(mode.number)
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
