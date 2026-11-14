//! ET4000 capabilities and programming used by the shared VBE services.
pub(in crate::video::firmware) mod modes;
use super::modes::Mode;
use crate::video::firmware::vbe::{ModeInformation, WindowAccess};
use relocade_vga::Et4000Ax;
use std::cell::RefCell;

pub(in crate::video::firmware) fn mode(
    card: &RefCell<Et4000Ax>,
    number: u16,
) -> Option<ModeInformation> {
    let layout = {
        let card = card.borrow();
        let mode = Mode::from_vbe_number(number).filter(|_| card.ports_enabled())?;
        let layout = mode.layout(card.video_memory_bytes())?;
        card.packed_layout()
            .filter(|active| active.size() == layout.size())
            .unwrap_or(layout)
    };
    // Window A controls writes, B controls reads; both use the A000h aperture.
    Some(ModeInformation {
        layout,
        windows: [WindowAccess::Write, WindowAccess::Read],
    })
}
