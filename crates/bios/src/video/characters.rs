//! BIOS character output and recognition at the stored cursor.

use crate::screen;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};

/// Character at a page's stored cursor. Graphics reads return attribute zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterCell {
    pub character: u8,
    pub attribute: u8,
}

/// Reads a text cell or recognizes a graphics glyph from live video memory.
/// Graphics returns the first matching character, or zero if no glyph matches.
/// Unsupported modes and invalid pages return None.
#[interrupt(context(memory, io), args(page = bh), returns {
    Some(cell) => { al = cell.character, ah = cell.attribute },
    None => crate::interrupt::unsupported,
})]
pub fn read_character(memory: &MemoryView<'_>, io: &IoBus, page: u8) -> Option<CharacterCell> {
    if !screen::valid_page(memory, page) {
        return None;
    }
    let [character, attribute] = screen::read_cell(memory, io, page)?;
    Some(CharacterCell {
        character,
        attribute,
    })
}

/// Repeated character output at a page's stored cursor, without moving it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CharacterRun {
    pub page: u8,
    pub character: u8,
    /// Text attribute or graphics foreground color, including the XOR bit.
    pub color: u8,
    pub count: u16,
}

/// Writes characters and text attributes, as for INT 10h/AH=09h.
/// Graphics modes use the shared mode-specific color and XOR rules.
#[interrupt(
    context(memory, io),
    args(
        run = CharacterRun {
            page: bh, character: al, color: bl, count: cx,
        },
    ),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub fn write_characters_with_attribute(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    run: CharacterRun,
) -> Option<()> {
    write_characters(memory, io, run, true)
}

/// Writes characters while preserving text attributes, as for INT 10h/AH=0Ah.
/// Graphics modes still use the run's foreground color and XOR bit.
#[interrupt(
    context(memory, io),
    args(
        run = CharacterRun {
            page: bh, character: al, color: bl, count: cx,
        },
    ),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub fn write_characters_preserving_attributes(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    run: CharacterRun,
) -> Option<()> {
    write_characters(memory, io, run, false)
}

/// Writes a repeated character. `replace_attribute` selects AH=09h behavior;
/// false selects AH=0Ah, preserving text attributes. Both use color in graphics
/// modes, including the existing mode-11h white/XOR rules. Invalid pages return None.
pub fn write_characters(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    run: CharacterRun,
    replace_attribute: bool,
) -> Option<()> {
    let mut output = screen::Output::new(memory, io, run.page)?;
    output.write_repeated(run.character, run.color, run.count, replace_attribute);
    Some(())
}

/// Writes one teletype character to the active page and commits its cursor.
/// Text keeps cell attributes; graphics uses color. Controls, wrapping and
/// scrolling match INT 10h/AH=0Eh. Invalid firmware page state returns None.
#[interrupt(
    context(memory, io), args(character = al, color = bl),
    returns { Some(()) => {}, None => crate::interrupt::unsupported },
)]
pub fn teletype(memory: &mut MemoryView<'_>, io: &IoBus, character: u8, color: u8) -> Option<()> {
    let page = screen::active_page(memory);
    let mut output = screen::Output::new(memory, io, page)?;
    output.teletype(character, color);
    output.commit_cursor();
    Some(())
}
