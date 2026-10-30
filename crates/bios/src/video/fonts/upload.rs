//! Font writes temporarily expose plane two through the A0000 aperture.
use super::super::{read_indexed, write_indexed};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::{
    graphics, sequencer, GRAPHICS_INDEX_PORT, MISC_OUTPUT_READ_PORT, MISC_OUTPUT_WRITE_PORT,
    SEQUENCER_INDEX_PORT,
};

pub(super) fn write_glyphs(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    bank: u8,
    first: u16,
    glyphs: impl IntoIterator<Item = impl AsRef<[u8]>>,
) {
    let sequencer = [(sequencer::MAP_MASK, 4), (sequencer::MEMORY_MODE, 6)];
    let graphics = [
        (graphics::ENABLE_SET_RESET, 0),
        (graphics::DATA_ROTATE, 0),
        (graphics::MODE, 0),
        (graphics::MISCELLANEOUS, 5),
        (graphics::BIT_MASK, 0xff),
    ];
    let seq_index = io.read_port(SEQUENCER_INDEX_PORT, 1);
    let gc_index = io.read_port(GRAPHICS_INDEX_PORT, 1);
    let saved_seq =
        sequencer.map(|(index, _)| (index, read_indexed(io, SEQUENCER_INDEX_PORT, index)));
    let saved_gc = graphics.map(|(index, _)| (index, read_indexed(io, GRAPHICS_INDEX_PORT, index)));
    let miscellaneous = io.read_port(MISC_OUTPUT_READ_PORT, 1);
    io.write_port(MISC_OUTPUT_WRITE_PORT, 1, miscellaneous | 2); // Enable VRAM access.
    for (index, value) in sequencer {
        write_indexed(io, SEQUENCER_INDEX_PORT, index, value);
    }
    for (index, value) in graphics {
        write_indexed(io, GRAPHICS_INDEX_PORT, index, value);
    }
    // BIOS font-bank numbering puts bit two below bits zero and one.
    let bank_offset = (u32::from(bank & 3) << 14) | (u32::from(bank & 4) << 11);
    let start = 0xa0000 + bank_offset + u32::from(first) * 32;
    for (index, glyph) in glyphs.into_iter().enumerate() {
        memory
            .write(start + index as u32 * 32, glyph.as_ref())
            .expect("text font");
    }
    for (index, value) in saved_gc {
        write_indexed(io, GRAPHICS_INDEX_PORT, index, value);
    }
    for (index, value) in saved_seq {
        write_indexed(io, SEQUENCER_INDEX_PORT, index, value);
    }
    io.write_port(MISC_OUTPUT_WRITE_PORT, 1, miscellaneous);
    io.write_port(SEQUENCER_INDEX_PORT, 1, seq_index);
    io.write_port(GRAPHICS_INDEX_PORT, 1, gc_index);
}
