//! The 64-byte VGA mode records reached through the BIOS save-pointer table.
use super::{
    crtc_port, input_status_1_port,
    palette::{self, PaletteLoad},
    tables, write_indexed,
};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::{
    attribute, crtc, ATTRIBUTE_ADDRESS_PORT, GRAPHICS_INDEX_PORT, MISC_OUTPUT_WRITE_PORT,
    SEQUENCER_INDEX_PORT,
};

pub(super) const SIZE: usize = 64;

pub(super) struct Parameters {
    pub columns: u8,
    pub last_row: u8,
    pub character_height: u8,
    pub page_bytes: u16,
    pub sequencer: [u8; 4],
    pub miscellaneous: u8,
    pub crtc: [u8; 25],
    pub attribute: [u8; 20],
    pub graphics: [u8; 9],
}

impl Parameters {
    pub fn read(memory: &MemoryView<'_>, index: usize) -> Option<Self> {
        let pointer = tables::Pointer::Parameters.read(memory)?;
        let mut bytes = [0; SIZE];
        memory
            .read_wrapping(pointer.wrapping_add(index * SIZE), &mut bytes)
            .expect("video mode parameters");
        Some(Self {
            columns: bytes[0],
            last_row: bytes[1],
            character_height: bytes[2],
            page_bytes: u16::from_le_bytes([bytes[3], bytes[4]]),
            sequencer: bytes[5..9].try_into().unwrap(),
            miscellaneous: bytes[9],
            crtc: bytes[10..35].try_into().unwrap(),
            attribute: bytes[35..55].try_into().unwrap(),
            graphics: bytes[55..64].try_into().unwrap(),
        })
    }

    pub fn encode(&self) -> [u8; SIZE] {
        let mut bytes = [0; SIZE];
        bytes[..3].copy_from_slice(&[self.columns, self.last_row, self.character_height]);
        bytes[3..5].copy_from_slice(&self.page_bytes.to_le_bytes());
        bytes[5..9].copy_from_slice(&self.sequencer);
        bytes[9] = self.miscellaneous;
        bytes[10..35].copy_from_slice(&self.crtc);
        bytes[35..55].copy_from_slice(&self.attribute);
        bytes[55..].copy_from_slice(&self.graphics);
        bytes
    }

    pub fn cursor_shape(&self) -> u16 {
        u16::from_be_bytes([
            self.crtc[crtc::CURSOR_START as usize],
            self.crtc[crtc::CURSOR_END as usize],
        ])
    }

    pub fn program(&self, memory: &mut MemoryView<'_>, io: &IoBus, palette: PaletteLoad) {
        io.write_port(MISC_OUTPUT_WRITE_PORT, 1, u32::from(self.miscellaneous));
        for (index, &value) in self.sequencer.iter().enumerate() {
            write_indexed(io, SEQUENCER_INDEX_PORT, index as u8 + 1, value);
        }
        for (index, &value) in self.graphics.iter().enumerate() {
            write_indexed(io, GRAPHICS_INDEX_PORT, index as u8, value);
        }
        let port = crtc_port(io);
        write_indexed(io, port, crtc::VERTICAL_RETRACE_END, 0);
        for (index, &value) in self.crtc.iter().enumerate() {
            // Restore protection only after all timing registers have been written.
            if index != usize::from(crtc::VERTICAL_RETRACE_END) {
                write_indexed(io, port, index as u8, value);
            }
        }
        write_indexed(io, port, crtc::VERTICAL_RETRACE_END, self.crtc[0x11]);
        for (index, &value) in self.attribute.iter().enumerate() {
            if !palette.loads_colors() && (index < 16 || index == 0x11) {
                continue;
            }
            palette::attributes::write_bios_attribute(memory, io, index as u8, value);
        }
        // Color Select is a VGA extension absent from the EGA-compatible record.
        palette::write_attribute(io, attribute::COLOR_SELECT, 0);
        io.read_port(input_status_1_port(io), 1);
        io.write_port(
            ATTRIBUTE_ADDRESS_PORT,
            1,
            u32::from(attribute::PALETTE_ADDRESS_SOURCE),
        );
    }
}
