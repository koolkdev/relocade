//! Banked, eight-bit subset of the Bochs DISPI display interface.
//!
//! Register numbers and word transfers follow the Bochs VBE Display API:
//! https://github.com/bochs-emu/VGABIOS/blob/master/vgabios/vbe_display_api.txt
//! This adapter exposes the ID0 register subset at the modern 01CE/01CF ports.
//! It accepts only 640x480x8; newer interface IDs, enable flags and registers are
//! unsupported. Invalid requests are ignored instead of trapping the host.

use crate::VIDEO_MEMORY_BYTES;
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

pub const INTERFACE_ID: u16 = 0xb0c0;
pub const DISABLED: u16 = 0;
pub const ENABLED: u16 = 1;
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
#[derive(Default)]
pub(crate) struct Dispi {
    index: u16,
    width: u16,
    height: u16,
    bits_per_pixel: u16,
    enabled: bool,
    bank: u16,
}

impl Dispi {
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub fn bank_mapping(&self) -> Option<SvgaBankMapping> {
        self.enabled.then_some(SvgaBankMapping {
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
                ENABLE => u16::from(self.enabled),
                BANK => self.bank,
                _ => u16::MAX,
            },
            _ => u16::MAX,
        })
    }

    /// Entering a valid mode requests a clear of the shared video memory.
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
            X_RESOLUTION if !self.enabled => self.width = value,
            Y_RESOLUTION if !self.enabled => self.height = value,
            BITS_PER_PIXEL if !self.enabled => self.bits_per_pixel = value,
            ENABLE if value == DISABLED => self.enabled = false,
            ENABLE
                if value == ENABLED
                    && !self.enabled
                    && (self.width, self.height, self.bits_per_pixel) == (640, 480, 8) =>
            {
                self.enabled = true;
                self.bank = 0;
                return WriteEffect::ClearVram;
            }
            BANK if u32::from(value) < VIDEO_MEMORY_BYTES / BANK_BYTES => self.bank = value,
            _ => {}
        }
        WriteEffect::None
    }
}
