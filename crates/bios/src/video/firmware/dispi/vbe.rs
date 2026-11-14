//! DISPI capabilities and programming used by the shared VBE services.
pub(in crate::video::firmware) mod information;
pub(in crate::video::firmware) mod modes;
pub(in crate::video::firmware) mod state;

use relocade_vga::{DispiAdapter, PackedLayout};
use std::cell::RefCell;

pub(super) use information::initialize;
use modes::Mode;

fn available(card: &RefCell<DispiAdapter>) -> bool {
    card.borrow().ports_enabled()
}

fn active_layout(card: &RefCell<DispiAdapter>) -> Option<PackedLayout> {
    available(card)
        .then(|| card.borrow().packed_layout())
        .flatten()
}
