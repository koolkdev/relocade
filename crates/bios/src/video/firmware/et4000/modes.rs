//! Indexed BIOS modes from the ET4000 data book's mode and timing tables.
//! The EV8913 clock wiring supplies 40 MHz and interlaced 44.9 MHz SVGA modes.
use crate::video::{fonts, modes::complete_mode, palette::PaletteLoad, presets, write_indexed};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::{
    registers::{attribute, SEQUENCER_INDEX_PORT},
    Et4000Ax, PackedLayout, PixelFormat,
};
use std::cell::RefCell;

pub(super) struct Mode {
    pub bios_number: u8,
    pub vbe_number: u16,
    pub size: [u16; 2],
    miscellaneous: u8,
    compatibility: u8,
    overflow: u8,
    crtc: [u8; 25],
}

const INDEXED_640X480: Mode = Mode {
    bios_number: 0x2e,
    vbe_number: 0x101,
    size: [640, 480],
    miscellaneous: 0xe3,
    compatibility: 8,
    overflow: 0,
    crtc: [
        0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0x0b, 0x3e, 0, 0x40, 0, 0, 0, 0, 0, 0, 0xea, 0x8c,
        0xdf, 0x50, 0x60, 0xe7, 4, 0xab, 0xff,
    ],
};

const INDEXED_800X600: Mode = Mode {
    bios_number: 0x30,
    vbe_number: 0x103,
    size: [800, 600],
    miscellaneous: 0xef,
    compatibility: 8,
    overflow: 0,
    crtc: [
        0x7f, 0x63, 0x64, 2, 0x64, 0x17, 0x77, 0xf0, 0, 0x60, 0, 0, 0, 0, 0, 0, 0x60, 0x82, 0x57,
        0x64, 0x60, 0x5b, 0x75, 0xab, 0xff,
    ],
};

const INDEXED_1024X768_INTERLACED: Mode = Mode {
    bios_number: 0x38,
    vbe_number: 0x105,
    size: [1024, 768],
    miscellaneous: 0x2f,
    compatibility: 0x0a,
    overflow: 0x80,
    crtc: [
        0x99, 0x7f, 0x7f, 0x1d, 0x83, 0x17, 0x2f, 0xf5, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0x80, 0xff,
        0x80, 0x60, 0xff, 0x30, 0xab, 0xff,
    ],
};

impl Mode {
    pub const ALL: [Self; 3] = [
        INDEXED_640X480,
        INDEXED_800X600,
        INDEXED_1024X768_INTERLACED,
    ];

    pub fn from_bios_number(number: u8) -> Option<&'static Self> {
        Self::ALL.iter().find(|mode| mode.bios_number == number)
    }

    pub fn from_vbe_number(number: u16) -> Option<&'static Self> {
        Self::ALL.iter().find(|mode| mode.vbe_number == number)
    }

    pub fn layout(&self, memory_bytes: u32) -> Option<PackedLayout> {
        PackedLayout::new(
            self.size.map(u32::from),
            PixelFormat::Indexed8,
            u32::from(self.size[0]),
            0,
            memory_bytes,
        )
    }

    pub fn activate(
        &self,
        memory: &mut MemoryView<'_>,
        io: &IoBus,
        card: &RefCell<Et4000Ax>,
        preserve: bool,
    ) {
        write_indexed(io, SEQUENCER_INDEX_PORT, 0, 1);
        io.with_context(|mut context| {
            let mut card = card.borrow_mut();
            card.prepare_legacy_mode(context.reborrow());
            card.program_crtc(context.reborrow(), 0x34, self.compatibility);
            card.program_crtc(context.reborrow(), 0x35, self.overflow);
        });
        fonts::set_mode(memory, io, 16, false);
        let palette = PaletteLoad::for_mode_set(memory);
        let mut parameters = presets::INDEXED256.parameters();
        parameters.columns = (self.size[0] / 8) as u8;
        parameters.last_row = (self.size[1] / 16 - 1) as u8;
        parameters.character_height = 16;
        parameters.page_bytes = 0;
        parameters.miscellaneous = self.miscellaneous;
        parameters.sequencer = [1, 0x0f, 0, 0x0e];
        parameters.crtc = self.crtc;
        // ET4000's full-byte serializer, rather than VGA's paired-nibble output.
        parameters.attribute[attribute::MODE_CONTROL as usize] = 1;
        parameters.attribute[attribute::HORIZONTAL_PANNING as usize] = 0;
        parameters.program(memory, io, palette);
        presets::INDEXED256.load_palette(io, palette);
        complete_mode(
            memory,
            io,
            self.bios_number | if preserve { 0x80 } else { 0 },
            &parameters,
            |_| io.with_context(|context| card.borrow_mut().clear_video_memory(context)),
        );
    }
}
