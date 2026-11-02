//! BIOS window scrolling shared by text and graphics modes.

use super::{ScrollDirection, Window};
use crate::screen;
use relocade_machine::{real_mode_interrupt::interrupt, IoBus, MemoryView};

/// Scrolls the active page's window upward, as for INT 10h/AH=06h.
/// Zero lines clears the window.
#[interrupt(
    context(memory, io),
    args(
        window = Window { top: ch, left: cl, bottom: dh, right: dl },
        lines = al,
        color = bh,
    ),
)]
pub fn scroll_up(memory: &mut MemoryView<'_>, io: &IoBus, window: Window, lines: u8, color: u8) {
    scroll(memory, io, window, lines, color, ScrollDirection::Up);
}

/// Scrolls the active page's window downward, as for INT 10h/AH=07h.
/// Zero lines clears the window.
#[interrupt(
    context(memory, io),
    args(
        window = Window { top: ch, left: cl, bottom: dh, right: dl },
        lines = al,
        color = bh,
    ),
)]
pub fn scroll_down(memory: &mut MemoryView<'_>, io: &IoBus, window: Window, lines: u8, color: u8) {
    scroll(memory, io, window, lines, color, ScrollDirection::Down);
}

/// Scrolls the active page's inclusive character window. Zero lines clears it;
/// clipping and text attributes/graphics fill match INT 10h/AH=06h and AH=07h.
pub fn scroll(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    window: Window,
    lines: u8,
    color: u8,
    direction: ScrollDirection,
) {
    let page = screen::active_page(memory);
    screen::scroll(memory, io, page, window, lines, color, direction);
}
