//! BIOS palette operations through the guest-visible VGA ports.
pub(super) mod attributes;
pub(super) mod compatibility;
pub(super) mod dac;
pub(super) mod load;
pub(super) mod paging;
mod profile;

pub(super) use attributes::{read_attribute, write_attribute, write_raw_attribute};
pub(super) use compatibility::initialize as initialize_compatibility;
pub(super) use load::PaletteLoad;
pub(super) use profile::apply as apply_profile;
