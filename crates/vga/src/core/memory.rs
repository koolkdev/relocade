//! VGA CPU access semantics over memory supplied by the complete card.
use super::VgaCore;
use crate::{
    memory::VideoMemory,
    registers::{graphics, sequencer},
    Registers,
};
use std::ops::Range;

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

    pub(crate) fn read_memory(&mut self, memory: &VideoMemory, address: u32, bytes: u8) -> u64 {
        let mut value = [0; 8];
        for (index, byte) in value[..usize::from(bytes)].iter_mut().enumerate() {
            *byte = self.read_vga_byte(memory, address + index as u32);
        }
        u64::from_le_bytes(value)
    }

    pub(crate) fn direct_memory_writes(&self) -> bool {
        self.memory_controls.write.passes_bytes_through()
    }

    /// The card may install the direct variant while the controls pass host
    /// bytes through unchanged. Both variants apply addressing and plane masks.
    pub(crate) fn write_memory<const DIRECT: bool>(
        &mut self,
        memory: &mut VideoMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        let controls = &self.memory_controls;
        let latches = u32::from_le_bytes(self.latches);
        for (index, byte) in value.to_le_bytes()[..usize::from(bytes)].iter().enumerate() {
            let address = address + index as u32;
            if !controls.window.contains(&address) {
                continue;
            }
            let offset = address - controls.window.start;
            let planes = controls.write_planes[(offset & 3) as usize];
            let address = u32::from(controls.addressing.index(offset)) * 4;
            let values = if DIRECT {
                [*byte; 4]
            } else {
                controls.write.apply(*byte, latches).to_le_bytes()
            };
            for (plane, value) in values.into_iter().enumerate() {
                if planes & (1 << plane) != 0 {
                    memory.write_physical(address + plane as u32, value);
                }
            }
        }
    }

    fn read_vga_byte(&mut self, memory: &VideoMemory, address: u32) -> u8 {
        let controls = &self.memory_controls;
        if !controls.window.contains(&address) {
            return 0xff;
        }
        let offset = address - controls.window.start;
        let index = u32::from(controls.addressing.index(offset));
        self.latches = std::array::from_fn(|plane| memory.read_physical(index * 4 + plane as u32));
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
    write_planes: [u8; 4],
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
            write_planes: std::array::from_fn(|offset| {
                registers.sequencer.write_plane_mask()
                    & if chain_four {
                        1 << offset
                    } else if registers.sequencer.odd_even_writes() {
                        0x05 << (offset & 1)
                    } else {
                        0x0f
                    }
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
