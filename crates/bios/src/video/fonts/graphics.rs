//! Graphics font selection changes BIOS pointers and geometry, not VGA font memory.
use super::rom;
use crate::video::{unsupported, CHARACTER_HEIGHT, LAST_ROW};

pub(super) fn select(call: &mut crate::BiosContext<'_>) {
    let guest_pointer =
        (u32::from(call.cpu.segments().es.selector) << 16) | u32::from(call.cpu.bp());
    if call.cpu.al() == 0x20 {
        call.memory
            .write_u32(0x1f * 4, guest_pointer)
            .expect("high character font vector");
        return;
    }
    let (pointer, height) = match call.cpu.al() {
        0x21 => (guest_pointer, call.cpu.cx()),
        function => {
            let height = match function {
                0x22 => 14,
                0x23 => 8,
                0x24 => 16,
                _ => unreachable!("graphics font function"),
            };
            (rom::for_height(height).pointer(), u16::from(height))
        }
    };
    // Keep the same supported glyph heights as the existing BIOS font writer.
    if !(1..=32).contains(&height) {
        return unsupported(call);
    }
    let rows: u8 = match call.cpu.bl() {
        0 => call.cpu.dl(),
        1 => 14,
        3 => 43,
        _ => 25, // IBM also treats row selectors above three as 25 rows.
    };
    call.memory
        .write_u32(0x43 * 4, pointer)
        .expect("graphics font vector");
    call.memory
        .write_u16(CHARACTER_HEIGHT, height)
        .expect("font height");
    call.memory
        .write_u8(LAST_ROW, rows.wrapping_sub(1))
        .expect("video rows");
}
