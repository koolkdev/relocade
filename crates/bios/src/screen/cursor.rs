//! BIOS cursor records, emulation policy and the corresponding CRTC writes.
use super::{active_page, geometry::Geometry, is_text_mode};
use crate::video::{write_crtc_word, CHARACTER_HEIGHT, CURSORS, CURSOR_SHAPE, VIDEO_CONTROL};
use relocade_machine::{IoBus, MemoryView};
use relocade_vga::registers::crtc;

const EMULATION_DISABLED: u8 = 1;
const ADAPTER_INACTIVE: u8 = 8;

pub(crate) fn cursor_position(memory: &MemoryView<'_>, page: u8) -> u16 {
    memory
        .read_u16(CURSORS + u32::from(page) * 2)
        .expect("video cursor")
}

pub(crate) fn set_cursor_position(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    page: u8,
    position: u16,
) {
    memory
        .write_u16(CURSORS + u32::from(page) * 2, position)
        .expect("video cursor");
    if page == active_page(memory) {
        update_cursor(memory, io, page);
    }
}

/// AL=0 enables emulation; AL=1 disables it for subsequent cursor-shape writes.
pub(crate) fn set_cursor_emulation(memory: &mut MemoryView<'_>, selection: u8) -> bool {
    if selection > 1 {
        return false;
    }
    let control = memory.read_u8(VIDEO_CONTROL).expect("video control");
    memory
        .write_u8(VIDEO_CONTROL, (control & !EMULATION_DISABLED) | selection)
        .expect("video control");
    true
}

/// Applies the BIOS emulation policy to an AH=01h cursor-shape request.
pub(crate) fn set_cursor_shape(memory: &mut MemoryView<'_>, io: &IoBus, shape: u16) {
    let control = memory.read_u8(VIDEO_CONTROL).expect("video control");
    let hardware_shape = if control & (EMULATION_DISABLED | ADAPTER_INACTIVE) == 0 {
        let height = memory.read_u8(CHARACTER_HEIGHT).expect("font height");
        emulated_shape(shape, height)
    } else {
        // Preserve raw shapes, including hide and skew bits, when emulation is off.
        shape
    };
    write_cursor_shape(memory, io, shape, hardware_shape);
}

/// Font layouts and mode presets already supply native scanline positions.
pub(crate) fn set_native_cursor_shape(memory: &mut MemoryView<'_>, io: &IoBus, shape: u16) {
    write_cursor_shape(memory, io, shape, shape);
}

fn write_cursor_shape(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    stored_shape: u16,
    hardware_shape: u16,
) {
    // AH=03h reports the requested shape, even when hardware needs a translation.
    memory
        .write_u16(CURSOR_SHAPE, stored_shape)
        .expect("video cursor shape");
    write_crtc_word(io, crtc::CURSOR_START, hardware_shape);
}

pub(super) fn update_cursor(memory: &MemoryView<'_>, io: &IoBus, page: u8) {
    // Graphics cursors are BIOS bookkeeping only.
    if !is_text_mode(memory) {
        return;
    }
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let position = Geometry::read(memory).cursor_address(page, u16::from(row), u16::from(column));
    write_crtc_word(io, crtc::CURSOR_ADDRESS_HIGH, position);
}

// IBM VGA ROM x1EA9–x1F2E: translate legacy shapes for the current font height.
fn emulated_shape(shape: u16, height: u8) -> u16 {
    let [start, end] = shape.to_be_bytes();
    if start & 0x60 == 0x20 {
        return 0x1e00; // IBM represents a hidden cursor with an empty scanline range.
    }
    if shape & 0xe0e0 != 0 {
        return shape;
    }
    let last = height.wrapping_sub(1);
    if end < start {
        return if end == 0 {
            shape
        } else {
            u16::from_be_bytes([end, last])
        };
    }
    // The ROM uses OR for this range test, which matters for non-power-of-two heights.
    if (start | end) < height && (end == last || start == last.wrapping_sub(1)) {
        return shape;
    }
    if end <= 3 {
        return shape;
    }
    if start + 2 >= end {
        let bottom = u16::from_be_bytes([last.wrapping_sub(end - start), last]);
        // Match the ROM's signed byte comparison, including guest-written BDA heights.
        return if (height as i8) >= 14 {
            bottom.wrapping_sub(0x0101)
        } else {
            bottom
        };
    }
    let start = if start <= 2 { start } else { height / 2 };
    u16::from_be_bytes([start, last])
}
