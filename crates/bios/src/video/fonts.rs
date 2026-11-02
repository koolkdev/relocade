//! INT 10h/AH=11h: text font memory, graphics font vectors and font information.
mod glyph;
pub(super) mod graphics;
mod layout;
mod rom;
pub(super) mod text;
mod upload;

use super::{CHARACTER_HEIGHT, LAST_ROW};
pub(crate) use glyph::Glyph;
use relocade_machine::{
    real_mode_interrupt::{interrupt, FarPointer},
    IoBus, MemoryView,
};
pub(super) use rom::initialize;

pub(super) struct FontInformation {
    pub pointer: FarPointer,
    pub height: u16,
    pub last_row: u8,
}

#[interrupt(context(memory), args(selector = bh), returns {
    Some(info) => { ptr(es, bp) = info.pointer, cx = info.height, dl = info.last_row },
    None => crate::interrupt::unsupported,
})]
pub(super) fn information(memory: &MemoryView<'_>, selector: u8) -> Option<FontInformation> {
    Some(FontInformation {
        pointer: rom::query(memory, selector)?,
        // Geometry describes the current display even when another font is requested.
        height: memory.read_u16(CHARACTER_HEIGHT).expect("font height"),
        last_row: memory.read_u8(LAST_ROW).expect("video rows"),
    })
}

pub(super) fn set_mode(memory: &mut MemoryView<'_>, io: &IoBus, height: u8, text: bool) {
    let font = rom::for_height(height);
    memory
        .write_u32(0x43 * 4, font.pointer().packed())
        .expect("graphics font vector");
    if text {
        // Mode setting resets complete slots. AH=11h instead preserves all
        // bytes below each supplied glyph, even when loading a built-in font.
        let glyphs = font.bytes.chunks(usize::from(height)).map(|glyph| {
            let mut slot = [0; 32];
            slot[..glyph.len()].copy_from_slice(glyph);
            slot
        });
        upload::write_glyphs(memory, io, 0, 0, glyphs);
    }
}
