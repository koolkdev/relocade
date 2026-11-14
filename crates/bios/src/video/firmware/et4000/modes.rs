//! BIOS modes for the EV8913 clocks, 1 MiB of DRAM and SC11483 RGB555 output.
use crate::video::{fonts, modes::complete_mode, palette::PaletteLoad, presets, write_indexed};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::{
    registers::{attribute, SEQUENCER_INDEX_PORT},
    Et4000Ax, PackedLayout, PixelFormat,
};
use std::cell::RefCell;

pub(super) struct Mode {
    pub bios_number: Option<u8>,
    pub vbe_number: u16,
    pub size: [u16; 2],
    pub format: PixelFormat,
    miscellaneous: u8,
    compatibility: u8,
    overflow: u8,
    crtc: [u8; 25],
}

const INDEXED_640X480: Mode = Mode {
    bios_number: Some(0x2e),
    vbe_number: 0x101,
    size: [640, 480],
    format: PixelFormat::Indexed8,
    miscellaneous: 0xe3,
    compatibility: 8,
    overflow: 0,
    crtc: [
        0x5f, 0x4f, 0x50, 0x82, 0x54, 0x80, 0x0b, 0x3e, 0, 0x40, 0, 0, 0, 0, 0, 0, 0xea, 0x8c,
        0xdf, 0x50, 0x60, 0xe7, 4, 0xab, 0xff,
    ],
};

const INDEXED_800X600: Mode = Mode {
    bios_number: Some(0x30),
    vbe_number: 0x103,
    size: [800, 600],
    format: PixelFormat::Indexed8,
    miscellaneous: 0xef,
    compatibility: 8,
    overflow: 0,
    crtc: [
        0x7f, 0x63, 0x64, 2, 0x64, 0x17, 0x77, 0xf0, 0, 0x60, 0, 0, 0, 0, 0, 0, 0x60, 0x82, 0x57,
        0x64, 0x60, 0x5b, 0x75, 0xab, 0xff,
    ],
};

const INDEXED_1024X768_INTERLACED: Mode = Mode {
    bios_number: Some(0x38),
    vbe_number: 0x105,
    size: [1024, 768],
    format: PixelFormat::Indexed8,
    miscellaneous: 0x2f,
    compatibility: 0x0a,
    overflow: 0x80,
    crtc: [
        0x99, 0x7f, 0x7f, 0x1d, 0x83, 0x17, 0x2f, 0xf5, 0, 0, 0, 0, 0, 0, 0, 0, 8, 0x80, 0xff,
        0x80, 0x60, 0xff, 0x30, 0xab, 0xff,
    ],
};

// Word output requires MCLK/2. Interlace keeps the indexed preset's field rate
// using the board's existing 25.175/40-MHz clocks. No higher oscillator is fitted.
const RGB555_640X480_INTERLACED: Mode = Mode {
    bios_number: None,
    vbe_number: 0x110,
    format: PixelFormat::Rgb555,
    overflow: 0x80,
    crtc: {
        let mut crtc = INDEXED_640X480.crtc;
        crtc[0x13] = 0xa0; // 1280 bytes per row.
        crtc
    },
    ..INDEXED_640X480
};

const RGB555_800X600_INTERLACED: Mode = Mode {
    bios_number: None,
    vbe_number: 0x113,
    format: PixelFormat::Rgb555,
    overflow: 0x80,
    crtc: {
        let mut crtc = INDEXED_800X600.crtc;
        crtc[0x13] = 0xc8; // 1600 bytes per row; 960000 bytes fit in 1 MiB.
        crtc
    },
    ..INDEXED_800X600
};

impl Mode {
    pub const ALL: [Self; 5] = [
        INDEXED_640X480,
        INDEXED_800X600,
        INDEXED_1024X768_INTERLACED,
        RGB555_640X480_INTERLACED,
        RGB555_800X600_INTERLACED,
    ];

    pub fn from_bios_number(number: u8) -> Option<&'static Self> {
        Self::ALL
            .iter()
            .find(|mode| mode.bios_number == Some(number))
    }

    pub fn from_vbe_number(number: u16) -> Option<&'static Self> {
        Self::ALL.iter().find(|mode| mode.vbe_number == number)
    }

    pub fn layout(&self, memory_bytes: u32) -> Option<PackedLayout> {
        PackedLayout::new(
            self.size.map(u32::from),
            self.format,
            u32::from(self.size[0]) * self.format.bytes_per_pixel(),
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
        if self.format == PixelFormat::Rgb555 {
            // ATC 16h word output, its required MCLK/2, and dual-edge RAMDAC input.
            write_indexed(io, SEQUENCER_INDEX_PORT, 7, 0xfc);
            io.read_port(0x3da, 1);
            io.write_port(0x3c0, 1, 0x36);
            io.write_port(0x3c0, 1, 0x30);
            io.with_context(|context| card.borrow_mut().program_dac_command(context, 0x80));
        }
        complete_mode(
            memory,
            io,
            self.bios_number.unwrap_or(0x7f) | if preserve { 0x80 } else { 0 },
            &parameters,
            |_| io.with_context(|context| card.borrow_mut().clear_video_memory(context)),
        );
    }
}
