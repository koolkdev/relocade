//! Text font loading, with optional display geometry changes.

use super::{layout, rom, upload};
use crate::{screen, video::write_indexed};
use relocade_machine::{
    real_mode_interrupt::{interrupt, FarPointer},
    IoBus, MemoryView,
};
use relocade_vga::registers::{sequencer, SEQUENCER_INDEX_PORT};

pub(in crate::video) struct GuestFont {
    source: FarPointer,
    height: u8,
    first: u16,
    count: u16,
}

enum FontSource {
    Guest(GuestFont),
    Builtin(&'static rom::Font),
}

/// Loads glyphs without changing the current text geometry.
#[interrupt(context(memory, io),
    args(bank = bl, font = GuestFont { source: ptr(es, bp), height: bh, first: dx, count: cx }),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub(in crate::video) fn load_font(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    bank: u8,
    font: GuestFont,
) -> Option<()> {
    load(memory, io, bank, FontSource::Guest(font), false)
}

/// Loads glyphs and adjusts text geometry to the supplied character height.
#[interrupt(context(memory, io),
    args(bank = bl, font = GuestFont { source: ptr(es, bp), height: bh, first: dx, count: cx }),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub(in crate::video) fn set_font(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    bank: u8,
    font: GuestFont,
) -> Option<()> {
    load(memory, io, bank, FontSource::Guest(font), true)
}

#[interrupt(context(memory, io), args(bank = bl), returns { Some(()) => {}, None => crate::interrupt::unsupported })]
pub(in crate::video) fn load_8x14(memory: &mut MemoryView<'_>, io: &IoBus, bank: u8) -> Option<()> {
    let font = FontSource::Builtin(rom::for_height(14));
    load(memory, io, bank, font, false)
}

#[interrupt(context(memory, io), args(bank = bl), returns { Some(()) => {}, None => crate::interrupt::unsupported })]
pub(in crate::video) fn load_8x8(memory: &mut MemoryView<'_>, io: &IoBus, bank: u8) -> Option<()> {
    let font = FontSource::Builtin(rom::for_height(8));
    load(memory, io, bank, font, false)
}

#[interrupt(context(memory, io), args(bank = bl), returns { Some(()) => {}, None => crate::interrupt::unsupported })]
pub(in crate::video) fn load_8x16(memory: &mut MemoryView<'_>, io: &IoBus, bank: u8) -> Option<()> {
    let font = FontSource::Builtin(rom::for_height(16));
    load(memory, io, bank, font, false)
}

#[interrupt(context(memory, io), args(bank = bl), returns { Some(()) => {}, None => crate::interrupt::unsupported })]
pub(in crate::video) fn set_8x14(memory: &mut MemoryView<'_>, io: &IoBus, bank: u8) -> Option<()> {
    let font = FontSource::Builtin(rom::for_height(14));
    load(memory, io, bank, font, true)
}

#[interrupt(context(memory, io), args(bank = bl), returns { Some(()) => {}, None => crate::interrupt::unsupported })]
pub(in crate::video) fn set_8x8(memory: &mut MemoryView<'_>, io: &IoBus, bank: u8) -> Option<()> {
    let font = FontSource::Builtin(rom::for_height(8));
    load(memory, io, bank, font, true)
}

#[interrupt(context(memory, io), args(bank = bl), returns { Some(()) => {}, None => crate::interrupt::unsupported })]
pub(in crate::video) fn set_8x16(memory: &mut MemoryView<'_>, io: &IoBus, bank: u8) -> Option<()> {
    let font = FontSource::Builtin(rom::for_height(16));
    load(memory, io, bank, font, true)
}

#[interrupt(context(memory, io), args(maps = bl), returns { Some(()) => {}, None => crate::interrupt::unsupported })]
pub(in crate::video) fn select_maps(memory: &MemoryView<'_>, io: &IoBus, maps: u8) -> Option<()> {
    if !screen::is_text_mode(memory) {
        return None;
    }
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::CHARACTER_MAP_SELECT,
        maps,
    );
    Some(())
}

fn load(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    bank: u8,
    source: FontSource,
    resize: bool,
) -> Option<()> {
    if !screen::is_text_mode(memory) {
        return None;
    }
    let (height, first, count) = match &source {
        FontSource::Guest(font) => (font.height, font.first, font.count),
        FontSource::Builtin(font) => (font.height, 0, 256),
    };
    if !(1..=32).contains(&height) || bank >= 8 || u32::from(first) + u32::from(count) > 256 {
        return None;
    }
    let layout = if resize {
        Some(layout::Layout::for_font(memory, io, height)?)
    } else {
        None
    };
    if count != 0 {
        // Read the source before exposing plane two: it can itself address VGA.
        let mut guest_bytes;
        let bytes = match source {
            FontSource::Builtin(font) => font.bytes,
            FontSource::Guest(font) => {
                guest_bytes = vec![0; usize::from(count) * usize::from(height)];
                memory
                    .read_wrapping(font.source, &mut guest_bytes)
                    .expect("font buffer");
                &guest_bytes
            }
        };
        upload::write_glyphs(memory, io, bank, first, bytes.chunks(usize::from(height)));
    }
    if let Some(layout) = layout {
        layout.apply(memory, io);
    }
    Some(())
}
