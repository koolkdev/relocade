//! VGA CPU access semantics over memory supplied by the complete card.
use super::VgaCore;
use crate::{
    memory::VideoMemory,
    registers::{graphics, sequencer},
    Registers,
};
use std::ops::Range;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryWrite {
    Disabled,
    Graphics,
    MaskedCopy,
    Chain4,
    OnePlane,
}

impl VgaCore {
    pub(crate) fn write_sequencer(&mut self, value: u8) {
        if self.registers.sequencer.write(value)
            && matches!(
                self.registers.sequencer.index(),
                sequencer::MAP_MASK | sequencer::MEMORY_MODE
            )
        {
            self.update_memory_controls();
        }
    }

    pub(crate) fn write_graphics(&mut self, value: u8) {
        if self.registers.graphics.write(value) {
            self.update_memory_controls();
        }
    }

    pub(super) fn update_memory_controls(&mut self) {
        self.memory_controls = MemoryControls::new(&self.registers);
    }

    pub(crate) fn memory_window(&self) -> Range<u32> {
        self.memory_controls.window.clone()
    }

    /// Select the operation when controls change, before accepting more MMIO.
    pub(crate) fn memory_write(&self) -> MemoryWrite {
        let planes = self.registers.sequencer.write_plane_mask();
        if planes == 0 {
            MemoryWrite::Disabled
        } else if !self.memory_controls.write.passes_bytes_through() {
            MemoryWrite::Graphics
        } else if self.registers.sequencer.chain_four() {
            if planes == 15 {
                MemoryWrite::Chain4
            } else {
                MemoryWrite::MaskedCopy
            }
        } else if self.registers.sequencer.odd_even_writes() {
            MemoryWrite::MaskedCopy
        } else if planes.is_power_of_two() {
            MemoryWrite::OnePlane
        } else {
            MemoryWrite::MaskedCopy
        }
    }

    /// The card's bindings restrict the complete transfer to the active window.
    pub(crate) fn read_memory(&mut self, memory: &VideoMemory, address: u32, bytes: u8) -> u64 {
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.read_vga_byte(memory, address + index as u32);
        }
        u64::from_le_bytes(value)
    }

    /// General graphics operations and copies using the prepared plane masks.
    /// The complete transfer must lie within the card's active bindings.
    pub(crate) fn write_memory<const COPY: bool>(
        &mut self,
        memory: &mut VideoMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        let controls = &self.memory_controls;
        let latches = u32::from_le_bytes(self.latches);
        let offset = address - controls.window.start;
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            let offset = offset + index as u32;
            let mask = controls.write_masks[(offset & 3) as usize];
            if mask == 0 {
                continue;
            }
            let address = u32::from(controls.addressing.index(offset)) * 4;
            let values = if COPY {
                u32::from(*byte) * 0x0101_0101
            } else {
                controls.write.apply(*byte, latches)
            };
            memory.write_physical_masked_word(address, values, mask);
        }
    }

    /// With all planes enabled, the address selects the byte for each chain-4 store.
    pub(crate) fn write_chain4(
        &self,
        memory: &mut VideoMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        let controls = &self.memory_controls;
        let mut offset = address - controls.window.start;
        let physical = u32::from(controls.addressing.index(offset)) * 4;
        if bytes == 1 {
            memory.write_physical(physical + (offset & 3), value as u8);
        } else if bytes == 4 && offset & 3 == 0 {
            memory.write_physical_word(physical, value as u32);
        } else {
            let values = value.to_le_bytes();
            let mut values = &values[..usize::from(bytes)];
            while !values.is_empty() {
                let plane = offset & 3;
                let count = values.len().min((4 - plane) as usize);
                let physical = u32::from(controls.addressing.index(offset)) * 4 + plane;
                memory.write_physical_bytes(physical, &values[..count]);
                values = &values[count..];
                offset += count as u32;
            }
        }
    }

    pub(crate) fn write_one_plane(
        &self,
        memory: &mut VideoMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        let controls = &self.memory_controls;
        let offset = address - controls.window.start;
        if bytes == 1 {
            let physical = u32::from(controls.addressing.index(offset)) * 4 + controls.write_plane;
            memory.write_physical(physical, value as u8);
            return;
        }
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            let physical = u32::from(controls.addressing.index(offset + index as u32)) * 4
                + controls.write_plane;
            memory.write_physical(physical, *byte);
        }
    }

    fn read_vga_byte(&mut self, memory: &VideoMemory, address: u32) -> u8 {
        let controls = &self.memory_controls;
        let offset = address - controls.window.start;
        let index = u32::from(controls.addressing.index(offset));
        self.latches = memory.read_physical_word(index * 4).to_le_bytes();
        if let Some(value) = controls.read.compare(self.latches) {
            return value;
        }
        let plane = controls.read_plane_base | (offset as u8 & controls.read_plane_mask);
        self.latches[usize::from(plane & 3)]
    }
}

/// Prepared on relevant register changes and restoration, never on MMIO access.
pub(super) struct MemoryControls {
    window: Range<u32>,
    addressing: Addressing,
    write_masks: [u32; 4],
    write_plane: u32,
    read_plane_base: u8,
    read_plane_mask: u8,
    read: graphics::ReadControls,
    write: graphics::WriteControls,
}

impl MemoryControls {
    pub(super) fn new(registers: &Registers) -> Self {
        let chain_four = registers.sequencer.chain_four();
        let read = registers.graphics.read_controls();
        Self {
            window: registers.graphics.memory_window(),
            addressing: Addressing::new(registers),
            write_plane: registers.sequencer.write_plane_mask().trailing_zeros(),
            write_masks: std::array::from_fn(|offset| {
                let planes = registers.sequencer.write_plane_mask()
                    & if chain_four {
                        1 << offset
                    } else if registers.sequencer.odd_even_writes() {
                        0x05 << (offset & 1)
                    } else {
                        0x0f
                    };
                u32::from_le_bytes(std::array::from_fn(|plane| {
                    if planes & (1 << plane) == 0 {
                        0
                    } else {
                        0xff
                    }
                }))
            }),
            read_plane_base: if chain_four {
                0
            } else if read.odd_even {
                read.plane & 2
            } else {
                read.plane
            },
            read_plane_mask: if chain_four {
                3
            } else if read.odd_even {
                1
            } else {
                0
            },
            read,
            write: registers.graphics.write_controls(),
        }
    }
}

struct Addressing {
    low_mask: u32,
    shift: u32,
    mask: u16,
}

impl Addressing {
    fn new(registers: &Registers) -> Self {
        // Chain-4 routes A14/A15 onto the plane address's low two bits.
        // Chained odd/even routes A16 onto its low bit; only the 128-KiB
        // aperture exposes A16. Unchained addressing leaves all bits intact.
        let (low_mask, shift) = if registers.sequencer.chain_four() {
            (3, 14)
        } else if registers.graphics.chain_odd_even() {
            (1, 16)
        } else {
            (0, 0)
        };
        Self {
            low_mask,
            shift,
            mask: if registers.sequencer.extended_memory() {
                0xffff
            } else {
                0x3fff
            },
        }
    }

    fn index(&self, offset: u32) -> u16 {
        let offset = (offset & !self.low_mask) | (offset.wrapping_shr(self.shift) & self.low_mask);
        offset as u16 & self.mask
    }
}
