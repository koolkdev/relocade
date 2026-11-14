//! DISPI capabilities supplied to the shared VBE information buffers.
use super::{active_layout, available, Mode};
use crate::video::firmware::vbe::{self, ModeInformation, WindowAccess};
use relocade_machine::MemoryView;
use relocade_vga::DispiAdapter;
use std::cell::RefCell;

pub(in super::super) fn initialize(memory: &mut MemoryView<'_>) {
    vbe::initialize(
        memory,
        b"Relocade banked SVGA\0",
        Mode::ALL.map(|mode| mode.number),
    );
}

pub(in crate::video::firmware) fn mode(
    card: &RefCell<DispiAdapter>,
    number: u16,
) -> Option<ModeInformation> {
    let mode = Mode::from_number(number).filter(|_| available(card))?;
    let layout = active_layout(card)
        .filter(|layout| {
            layout.size() == mode.display.size() && layout.format() == mode.display.format()
        })
        .unwrap_or_else(|| {
            mode.display
                .layout(card.borrow().video_memory_bytes())
                .expect("supported mode fits VRAM")
        });
    Some(ModeInformation {
        layout,
        windows: [WindowAccess::ReadWrite, WindowAccess::Absent],
    })
}
