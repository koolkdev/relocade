//! Banked, eight-bit subset of the Bochs DISPI display interface.
//!
//! Register numbers and word transfers follow the Bochs VBE Display API:
//! https://github.com/bochs-emu/VGABIOS/blob/master/vgabios/vbe_display_api.txt
//! This adapter exposes the ID1 register subset at the modern 01CE/01CF ports.
//! It accepts three indexed modes and the no-clear enable flag from ID2. Newer interface
//! IDs, other enable flags and registers are unsupported. Invalid requests are ignored.

use crate::{SvgaLayout, SvgaMode, VIDEO_MEMORY_BYTES};
use std::ops::RangeInclusive;

pub const INDEX_PORT: u16 = 0x01ce;
pub const DATA_PORT: u16 = 0x01cf;
pub const PORT_RANGE: RangeInclusive<u16> = INDEX_PORT..=DATA_PORT;

pub const ID: u16 = 0;
pub const X_RESOLUTION: u16 = 1;
pub const Y_RESOLUTION: u16 = 2;
pub const BITS_PER_PIXEL: u16 = 3;
pub const ENABLE: u16 = 4;
pub const BANK: u16 = 5;
pub const VIRTUAL_WIDTH: u16 = 6;
pub const VIRTUAL_HEIGHT: u16 = 7;
pub const X_OFFSET: u16 = 8;
pub const Y_OFFSET: u16 = 9;

pub const INTERFACE_ID: u16 = 0xb0c1;
pub const DISABLED: u16 = 0;
pub const ENABLED: u16 = 1;
pub const NO_CLEAR_MEMORY: u16 = 0x80;
pub const BANK_BYTES: u32 = 65536;
pub const BANK_START: u32 = 0xa0000;
pub const BANK_RANGE: RangeInclusive<u32> = BANK_START..=BANK_START + BANK_BYTES - 1;

/// Maps the fixed CPU window to a validated SVGA bank in physical VRAM.
/// Only DISPI constructs these values, after validating its bank register.
#[derive(Clone, Copy)]
pub(crate) struct SvgaBankMapping {
    vram_start: u32,
}

impl SvgaBankMapping {
    /// Other VGA apertures are closed; accesses never spill into the next bank.
    pub fn memory_offset(self, address: u32) -> Option<u32> {
        BANK_RANGE
            .contains(&address)
            .then(|| self.vram_start + address - BANK_START)
    }
}

/// An action the device must apply after the register write.
#[must_use = "DISPI write effects must be applied by the device"]
pub(crate) enum WriteEffect {
    None,
    ClearVram,
}

/// Geometry writes stage a request while disabled. Only a supported complete
/// request can enable the packed aperture; active geometry writes are ignored.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Dispi {
    index: u16,
    width: u16,
    height: u16,
    bits_per_pixel: u16,
    active: Option<ActiveMode>,
    bank: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveMode {
    layout: SvgaLayout,
    flags: u16,
}

impl Dispi {
    pub fn to_state_bytes(&self) -> [u8; 18] {
        let layout = self.display_layout();
        let [x, y] = layout.map_or([0, 0], SvgaLayout::origin);
        let words = [
            self.index,
            self.width,
            self.height,
            self.bits_per_pixel,
            self.active.as_ref().map_or(0, |active| active.flags),
            self.bank,
            layout.map_or(0, |layout| layout.pitch() as u16),
            x as u16,
            y as u16,
        ];
        let mut bytes = [0; 18];
        for (target, word) in bytes.as_chunks_mut::<2>().0.iter_mut().zip(words) {
            *target = word.to_le_bytes();
        }
        bytes
    }

    pub fn from_state_bytes(bytes: &[u8; 18]) -> Option<Self> {
        let words = std::array::from_fn(|index| {
            u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]])
        });
        let [index, width, height, bits_per_pixel, flags, bank, pitch, x, y] = words;
        if u32::from(bank) >= VIDEO_MEMORY_BYTES / BANK_BYTES {
            return None;
        }
        let active = if flags == DISABLED {
            // Geometry staging is deliberately unrestricted while disabled.
            if [pitch, x, y] != [0; 3] {
                return None;
            }
            None
        } else {
            if flags & !NO_CLEAR_MEMORY != ENABLED {
                return None;
            }
            let layout = SvgaMode::from_registers(width, height, bits_per_pixel)?
                .layout()
                .with_pitch(u32::from(pitch))?
                .with_origin([u32::from(x), u32::from(y)])?;
            Some(ActiveMode { layout, flags })
        };
        Some(Self {
            index,
            width,
            height,
            bits_per_pixel,
            active,
            bank,
        })
    }

    fn enabled(&self) -> bool {
        self.active.is_some()
    }

    pub fn display_layout(&self) -> Option<SvgaLayout> {
        self.active.as_ref().map(|active| active.layout)
    }

    pub fn bank_mapping(&self) -> Option<SvgaBankMapping> {
        self.enabled().then_some(SvgaBankMapping {
            vram_start: u32::from(self.bank) * BANK_BYTES,
        })
    }

    pub fn read_port(&self, port: u16, bytes: u8) -> u32 {
        // A word at the index port is one register, not adjacent byte writes.
        if bytes != 2 {
            return if bytes == 1 { 0xff } else { u32::MAX };
        }
        u32::from(match port {
            INDEX_PORT => self.index,
            DATA_PORT => match self.index {
                ID => INTERFACE_ID,
                X_RESOLUTION => self.width,
                Y_RESOLUTION => self.height,
                BITS_PER_PIXEL => self.bits_per_pixel,
                ENABLE => self.active.as_ref().map_or(DISABLED, |active| active.flags),
                BANK => self.bank,
                VIRTUAL_WIDTH => self
                    .display_layout()
                    .map_or(0, |layout| layout.pitch() as u16),
                VIRTUAL_HEIGHT => self
                    .display_layout()
                    .map_or(0, |layout| layout.virtual_height() as u16),
                X_OFFSET => self
                    .display_layout()
                    .map_or(0, |layout| layout.origin()[0] as u16),
                Y_OFFSET => self
                    .display_layout()
                    .map_or(0, |layout| layout.origin()[1] as u16),
                _ => u16::MAX,
            },
            _ => u16::MAX,
        })
    }

    /// Entering a valid mode clears shared video memory unless no-clear is set.
    /// Repeated enable writes do not clear memory or reset the bank.
    pub fn write_port(&mut self, port: u16, bytes: u8, value: u32) -> WriteEffect {
        if bytes != 2 {
            return WriteEffect::None;
        }
        let value = value as u16;
        if port == INDEX_PORT {
            self.index = value;
            return WriteEffect::None;
        }
        match self.index {
            X_RESOLUTION if !self.enabled() => self.width = value,
            Y_RESOLUTION if !self.enabled() => self.height = value,
            BITS_PER_PIXEL if !self.enabled() => self.bits_per_pixel = value,
            ENABLE if value == DISABLED => self.active = None,
            ENABLE if value & !NO_CLEAR_MEMORY == ENABLED && !self.enabled() => {
                let Some(mode) =
                    SvgaMode::from_registers(self.width, self.height, self.bits_per_pixel)
                else {
                    return WriteEffect::None;
                };
                self.active = Some(ActiveMode {
                    layout: mode.layout(),
                    flags: value,
                });
                self.bank = 0;
                return if value & NO_CLEAR_MEMORY == 0 {
                    WriteEffect::ClearVram
                } else {
                    WriteEffect::None
                };
            }
            BANK if u32::from(value) < VIDEO_MEMORY_BYTES / BANK_BYTES => self.bank = value,
            VIRTUAL_WIDTH | X_OFFSET | Y_OFFSET => {
                if let Some(active) = &mut self.active {
                    let layout = active.layout;
                    let value = u32::from(value);
                    let next = match self.index {
                        VIRTUAL_WIDTH => layout.with_pitch(value),
                        X_OFFSET => layout.with_origin([value, layout.origin()[1]]),
                        Y_OFFSET => layout.with_origin([layout.origin()[0], value]),
                        _ => unreachable!(),
                    };
                    if let Some(next) = next {
                        active.layout = next;
                    }
                }
            }
            _ => {}
        }
        WriteEffect::None
    }
}
