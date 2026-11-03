//! BIOS palette operations through the guest-visible VGA ports.
pub(super) mod attributes;
pub(super) mod compatibility;
pub(super) mod dac;
mod guest_tables;
pub(super) mod load;
pub(super) mod paging;

pub(super) use attributes::{read_attribute, write_attribute, write_raw_attribute};
pub(super) use compatibility::initialize as initialize_compatibility;
pub(super) use guest_tables::apply as apply_guest_tables;
pub(super) use load::PaletteLoad;
