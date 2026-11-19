//! Banked subset of the Bochs DISPI display interface.
//!
//! Register numbers and word transfers follow the Bochs VBE Display API:
//! https://github.com/bochs-emu/VGABIOS/blob/master/vgabios/vbe_display_api.txt
//! This adapter exposes a banked ID3 subset at the 01CE/01CF ports: three resolutions
//! in indexed eight-bit, RGB555, RGB565, BGR888 and BGRX8888 formats, with no-clear entry.
//! ID0–ID3 negotiation, capability queries and eight-bit DAC control are supported.
//! Linear access and newer flags are unsupported.
//! Invalid requests are ignored.

use super::DispiMode;

use crate::{memory::VramWindow, PackedLayout, Timing};
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

pub const INTERFACE_ID: u16 = 0xb0c3;
/// Physical memory installed on the emulated DISPI card.
pub const VIDEO_MEMORY_BYTES: u32 = 4 * 1024 * 1024;
pub const DISABLED: u16 = 0;
pub const ENABLED: u16 = 1;
pub const GET_CAPABILITIES: u16 = 2;
pub const EIGHT_BIT_DAC: u16 = 0x20;
pub const NO_CLEAR_MEMORY: u16 = 0x80;
pub const BANK_BYTES: u32 = 65536;
/// Physical start of the banked window (VBE Window A).
pub const BANK_START: u32 = 0xa0000;
pub const BANK_RANGE: RangeInclusive<u32> = BANK_START..=BANK_START + BANK_BYTES - 1;

/// An action the device must apply after the register write.
#[must_use = "DISPI write actions must be applied by the device"]
pub(crate) enum WriteAction {
    None,
    Enable {
        clear_memory: bool,
        dac_bits_per_component: u8,
    },
}

/// Geometry writes stage a request while disabled. Only a supported complete
/// request enables packed graphics. Geometry writes are ignored while active.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Dispi {
    index: u16,
    width: u16,
    height: u16,
    bits_per_pixel: u16,
    active: Option<ActiveMode>,
    bank: u16,
    interface_id: u16,
    querying_capabilities: bool,
}

impl Default for Dispi {
    fn default() -> Self {
        Self {
            index: 0,
            width: 0,
            height: 0,
            bits_per_pixel: 0,
            active: None,
            bank: 0,
            interface_id: INTERFACE_ID,
            querying_capabilities: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActiveMode {
    layout: PackedLayout,
    flags: u16,
}

impl Dispi {
    pub fn to_state_bytes(&self) -> [u8; 20] {
        let layout = self.display_layout();
        let [x, y] = layout.map_or([0, 0], PackedLayout::origin);
        let words = [
            self.index,
            self.width,
            self.height,
            self.bits_per_pixel,
            // DAC precision belongs to DacState, not the SVGA state class.
            self.enable_flags(6),
            self.bank,
            layout.map_or(0, |layout| layout.pitch() as u16),
            x as u16,
            y as u16,
            self.interface_id,
        ];
        let mut bytes = [0; 20];
        for (target, word) in bytes.as_chunks_mut::<2>().0.iter_mut().zip(words) {
            *target = word.to_le_bytes();
        }
        bytes
    }

    pub fn from_state_bytes(bytes: &[u8; 20], memory_bytes: u32) -> Option<Self> {
        let words = std::array::from_fn(|index| {
            u16::from_le_bytes([bytes[index * 2], bytes[index * 2 + 1]])
        });
        let [index, width, height, bits_per_pixel, flags, bank, pitch, x, y, interface_id] = words;
        if u32::from(bank) >= memory_bytes / BANK_BYTES
            || !(0xb0c0..=INTERFACE_ID).contains(&interface_id)
        {
            return None;
        }
        let querying_capabilities = flags & GET_CAPABILITIES != 0;
        let flags = flags & !GET_CAPABILITIES;
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
            let layout = DispiMode::from_registers(width, height, bits_per_pixel)?
                .layout(memory_bytes)?
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
            interface_id,
            querying_capabilities,
        })
    }

    fn enabled(&self) -> bool {
        self.active.is_some()
    }

    pub fn display_layout(&self) -> Option<PackedLayout> {
        self.active.as_ref().map(|active| active.layout)
    }

    pub(crate) fn timing(&self) -> Option<Timing> {
        self.active.as_ref()?;
        Some(DispiMode::from_registers(self.width, self.height, self.bits_per_pixel)?.timing())
    }

    /// The bank register is validated on writes and restoration, so the whole
    /// CPU window is backed by physical VRAM.
    pub fn banked_window(&self) -> Option<VramWindow> {
        self.enabled().then_some(VramWindow {
            cpu_start: BANK_START,
            vram_start: u32::from(self.bank) * BANK_BYTES,
            bytes: BANK_BYTES,
        })
    }

    pub fn index(&self) -> u16 {
        self.index
    }

    pub fn bank(&self) -> u16 {
        self.bank
    }

    pub fn select(&mut self, index: u16) {
        self.index = index;
    }

    pub fn enable_flags(&self, dac_bits_per_component: u8) -> u16 {
        self.active.as_ref().map_or(0, |active| active.flags)
            | if self.querying_capabilities {
                GET_CAPABILITIES
            } else {
                0
            }
            | if dac_bits_per_component == 8 {
                EIGHT_BIT_DAC
            } else {
                0
            }
    }

    pub fn read(&self, dac_bits_per_component: u8) -> u16 {
        match self.index {
            ID => self.interface_id,
            X_RESOLUTION if self.querying_capabilities => 1024,
            Y_RESOLUTION if self.querying_capabilities => 768,
            BITS_PER_PIXEL if self.querying_capabilities => 32,
            X_RESOLUTION => self.width,
            Y_RESOLUTION => self.height,
            BITS_PER_PIXEL => self.bits_per_pixel,
            ENABLE => self.enable_flags(dac_bits_per_component),
            BANK => self.bank,
            VIRTUAL_WIDTH => self
                .display_layout()
                .map_or(0, |layout| layout.virtual_width() as u16),
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
        }
    }

    /// Entering a valid mode clears shared video memory unless no-clear is set.
    /// Repeated enable writes do not clear memory or reset the bank.
    pub fn write_register(&mut self, index: u16, value: u16, memory_bytes: u32) -> WriteAction {
        match index {
            ID if (0xb0c0..=INTERFACE_ID).contains(&value) => self.interface_id = value,
            X_RESOLUTION if !self.enabled() => self.width = value,
            Y_RESOLUTION if !self.enabled() => self.height = value,
            BITS_PER_PIXEL if !self.enabled() => self.bits_per_pixel = value,
            ENABLE => return self.write_enable(value, memory_bytes),
            BANK if u32::from(value) < memory_bytes / BANK_BYTES => self.bank = value,
            VIRTUAL_WIDTH | X_OFFSET | Y_OFFSET => {
                if let Some(active) = &mut self.active {
                    let layout = active.layout;
                    let value = u32::from(value);
                    let next = match index {
                        VIRTUAL_WIDTH => layout.with_virtual_width(value),
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
        WriteAction::None
    }

    fn write_enable(&mut self, value: u16, memory_bytes: u32) -> WriteAction {
        let mode_flags = value & !(GET_CAPABILITIES | EIGHT_BIT_DAC);
        if mode_flags != DISABLED && mode_flags & !NO_CLEAR_MEMORY != ENABLED {
            return WriteAction::None;
        }
        let mut clear_memory = false;
        if mode_flags == DISABLED {
            self.active = None;
        } else if !self.enabled() {
            let Some(mode) =
                DispiMode::from_registers(self.width, self.height, self.bits_per_pixel)
            else {
                return WriteAction::None;
            };
            self.active = Some(ActiveMode {
                layout: mode
                    .layout(memory_bytes)
                    .expect("supported DISPI mode fits VRAM"),
                flags: mode_flags,
            });
            self.bank = 0;
            clear_memory = mode_flags & NO_CLEAR_MEMORY == 0;
        }
        self.querying_capabilities = value & GET_CAPABILITIES != 0;
        WriteAction::Enable {
            clear_memory,
            dac_bits_per_component: if value & EIGHT_BIT_DAC != 0 { 8 } else { 6 },
        }
    }
}
