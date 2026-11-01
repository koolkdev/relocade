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

pub(in crate::video) fn configure_load(call: &mut crate::BiosContext<'_>) {
    let selection = call.cpu.al();
    if selection > 1 {
        // IBM's recognized alternate functions return AL=0 for an invalid selection.
        call.cpu.set_al(0);
        return;
    }
    let (mask, enabled) = match call.cpu.bl() {
        0x31 => (DEFAULT_PALETTE_DISABLED, selection == 1),
        0x33 => (GRAYSCALE_SUMMING, selection == 0),
        _ => unreachable!("palette loading function"),
    };
    let control = call
        .memory
        .read_u8(MODESET_CONTROL)
        .expect("mode-set control");
    call.memory
        .write_u8(
            MODESET_CONTROL,
            (control & !mask) | if enabled { mask } else { 0 },
        )
        .expect("mode-set control");
    call.cpu.set_al(0x12);
}
