//! BIOS palette-loading policy lives in the guest-visible mode-set control byte.
use super::dac;
use crate::video::MODESET_CONTROL;
use relocade_machine::{IoBus, MemoryView};

const DEFAULT_PALETTE_DISABLED: u8 = 8;
const GRAYSCALE_SUMMING: u8 = 2;
const MONOCHROME_DISPLAY: u8 = 4;

#[derive(Clone, Copy)]
pub(in crate::video) enum PaletteLoad {
    Preserve,
    Color,
    Grayscale,
}

impl PaletteLoad {
    pub fn for_mode_set(memory: &MemoryView<'_>) -> Self {
        let control = memory.read_u8(MODESET_CONTROL).expect("mode-set control");
        if control & DEFAULT_PALETTE_DISABLED != 0 {
            Self::Preserve
        } else {
            Self::for_color_write(memory)
        }
    }

    pub fn for_color_write(memory: &MemoryView<'_>) -> Self {
        let control = memory.read_u8(MODESET_CONTROL).expect("mode-set control");
        if control & (GRAYSCALE_SUMMING | MONOCHROME_DISPLAY) != 0 {
            Self::Grayscale
        } else {
            Self::Color
        }
    }

    pub fn loads_colors(self) -> bool {
        !matches!(self, Self::Preserve)
    }

    pub fn write(self, io: &IoBus, first: u8, colors: impl IntoIterator<Item = [u8; 3]>) {
        match self {
            Self::Preserve => {}
            Self::Color => dac::write(io, first, colors),
            Self::Grayscale => dac::write(io, first, colors.into_iter().map(dac::grayscale)),
        }
    }
}

/// AL=0 enables default palettes on mode changes; AL=1 preserves the current palette.
/// Returns whether the selection was accepted; invalid selections leave the policy unchanged.
#[relocade_machine::real_mode_interrupt::interrupt(
    context(memory), args(selection = al),
    returns { true => { al = 0x12 }, false => { al = 0 } }
)]
pub(in crate::video) fn set_palette_loading(memory: &mut MemoryView<'_>, selection: u8) -> bool {
    if selection > 1 {
        return false;
    }
    set_control_flag(memory, DEFAULT_PALETTE_DISABLED, selection == 1);
    true
}

/// AL=0 enables grayscale conversion of BIOS palette writes; AL=1 disables it.
/// Returns whether the selection was accepted; invalid selections leave the policy unchanged.
#[relocade_machine::real_mode_interrupt::interrupt(
    context(memory), args(selection = al),
    returns { true => { al = 0x12 }, false => { al = 0 } }
)]
pub(in crate::video) fn set_grayscale_summing(memory: &mut MemoryView<'_>, selection: u8) -> bool {
    if selection > 1 {
        return false;
    }
    set_control_flag(memory, GRAYSCALE_SUMMING, selection == 0);
    true
}

fn set_control_flag(memory: &mut MemoryView<'_>, flag: u8, value: bool) {
    let control = memory.read_u8(MODESET_CONTROL).expect("mode-set control");
    memory
        .write_u8(
            MODESET_CONTROL,
            (control & !flag) | if value { flag } else { 0 },
        )
        .expect("mode-set control");
}
