//! Host byte transfers follow the same address routing as CPU accesses.

use super::*;
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

impl<D: Environment> Imports<D> {
    pub fn read(
        &mut self,
        backing: &mut dyn GuestMemory,
        mut address: u32,
        mut bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        if !check_range(address, bytes.len(), self.map.address_limit()) {
            return Err(MemoryError::AddressRange);
        }
        self.access(backing, |environment, memory| {
            while !bytes.is_empty() {
                let mut count = page_span(address, bytes.len());
                match memory.map.resolve(address, false)? {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => {
                        memory.backing.read(backing_offset, &mut bytes[..count])
                    }
                    PhysicalMapping::Unmapped => bytes[..count].fill(0xff),
                    PhysicalMapping::Mmio => {
                        count = 1;
                        bytes[0] =
                            environment.read_mmio(memory, memory.map.bus_address(address), 1) as u8;
                    }
                }
                address = address.wrapping_add(count as u32);
                bytes = &mut std::mem::take(&mut bytes)[count..];
            }
            Ok(())
        })
    }

    pub fn write(
        &mut self,
        backing: &mut dyn GuestMemory,
        mut address: u32,
        mut bytes: &[u8],
    ) -> Result<(), MemoryError> {
        if !check_range(address, bytes.len(), self.map.address_limit()) {
            return Err(MemoryError::AddressRange);
        }
        self.access(backing, |environment, memory| {
            while !bytes.is_empty() {
                let mut count = page_span(address, bytes.len());
                match memory.map.resolve(address, true)? {
                    PhysicalMapping::Ram { backing_offset } => {
                        memory.backing.write(backing_offset, &bytes[..count])
                    }
                    PhysicalMapping::Rom { .. } | PhysicalMapping::Unmapped => {}
                    PhysicalMapping::Mmio => {
                        count = 1;
                        environment.write_mmio(
                            memory,
                            memory.map.bus_address(address),
                            1,
                            u64::from(bytes[0]),
                        );
                    }
                }
                address = address.wrapping_add(count as u32);
                bytes = &bytes[count..];
            }
            Ok(())
        })
    }
}

// A20 and physical routing can change only at a page boundary unless a device
// callback runs. MMIO therefore transfers one byte before resolving routing again.
fn page_span(address: u32, remaining: usize) -> usize {
    let page = PhysicalMemoryMap::PAGE_BYTES;
    remaining.min((page - address % page) as usize)
}
