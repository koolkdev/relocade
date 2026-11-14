//! Display configuration selected by the DISPI adapter's accepted mode.
use super::DispiAdapter;
use crate::scanout::{DisplayConfiguration, PreparedScanout};

pub(super) fn configuration(card: &DispiAdapter) -> DisplayConfiguration {
    match card.dispi.display_layout() {
        Some(layout) => DisplayConfiguration {
            scanout: PreparedScanout::Packed(layout),
            blanked: false,
            blanking: Default::default(),
        },
        None => DisplayConfiguration::vga(&card.vga.registers),
    }
}
