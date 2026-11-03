//! Guest font tables override mode defaults through the BIOS save-pointer tables.
use super::{graphics, text};
use crate::{
    screen,
    video::{read_indexed, tables::Pointer, CHARACTER_HEIGHT, LAST_ROW, MODE},
};
use relocade_machine::{FarPointer, IoBus, MemoryView};
use relocade_vga::registers::{sequencer, SEQUENCER_INDEX_PORT};

pub(in crate::video) fn apply(memory: &mut MemoryView<'_>, io: &IoBus) {
    let mode = memory.read_u8(MODE).expect("video mode");
    if screen::is_text_mode(memory) {
        apply_primary(memory, io, mode);
        apply_alternate(memory, io, mode);
    } else if let Some(header) = Pointer::GraphicsFont.read_for_mode::<7>(memory, mode) {
        let source = FarPointer::from_packed(u32::from_le_bytes(header[3..7].try_into().unwrap()));
        let height = u16::from_le_bytes([header[1], header[2]]);
        let rows = graphics::Rows {
            selection: 0,
            custom: header[0],
        };
        let _ = graphics::set_font(memory, source, height, rows);
    }
}

fn apply_primary(memory: &mut MemoryView<'_>, io: &IoBus, mode: u8) {
    let Some(header) = Pointer::TextFont.read_for_mode::<11>(memory, mode) else {
        return;
    };
    let font = text::GuestFont {
        height: header[0],
        count: u16::from_le_bytes([header[2], header[3]]),
        first: u16::from_le_bytes([header[4], header[5]]),
        source: FarPointer::from_packed(u32::from_le_bytes(header[6..10].try_into().unwrap())),
    };
    // Loading a bank does not select it: SR03 in the mode record still chooses B.
    if text::set_font(memory, io, header[1], font).is_some() && header[10] != 0xff {
        // IBM applies this row count after calculating the allocation and timing.
        memory
            .write_u8(LAST_ROW, header[10].wrapping_sub(1))
            .expect("video rows");
    }
}

fn apply_alternate(memory: &mut MemoryView<'_>, io: &IoBus, mode: u8) {
    let Some(header) = Pointer::AlternateFont.read_for_mode::<7>(memory, mode) else {
        return;
    };
    if u16::from(header[0]) != memory.read_u16(CHARACTER_HEIGHT).expect("font height") {
        return;
    }
    let font = text::GuestFont {
        height: header[0],
        first: 0,
        count: 256,
        source: FarPointer::from_packed(u32::from_le_bytes(header[3..7].try_into().unwrap())),
    };
    let bank = header[1];
    if text::load_font(memory, io, bank, font).is_some() {
        let maps = read_indexed(io, SEQUENCER_INDEX_PORT, sequencer::CHARACTER_MAP_SELECT);
        let alternate = ((bank & 3) << 2) | ((bank & 4) << 3);
        let _ = text::select_maps(memory, io, (maps & 0x13) | alternate);
    }
}
