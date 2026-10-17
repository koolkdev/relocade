use crate::{memory::check_range, Environment, GuestMemory, MemoryAccess, MemoryError, MemoryMap};
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

/// Shared implementation of the generated interpreter's host imports. Backends
/// bind these methods to Wasm imports and supply scoped access to guest backing.
/// Each device call publishes changed routing before returning to generated code.
pub struct Imports<D> {
    environment: D,
    map: MemoryMap,
}

impl<D: Environment> Imports<D> {
    pub fn environment(&self) -> &D {
        &self.environment
    }
    pub fn environment_mut(&mut self) -> &mut D {
        &mut self.environment
    }
    pub fn memory_map(&self) -> &MemoryMap {
        &self.map
    }

    pub(crate) fn new(map: MemoryMap, environment: D) -> Self {
        Self { map, environment }
    }

    pub fn read_port(&mut self, memory: &mut dyn GuestMemory, port: u16, bytes: u8) -> u32 {
        self.access(memory, |environment, memory| {
            environment.read_port(memory, port, bytes)
        })
    }

    pub fn write_port(&mut self, memory: &mut dyn GuestMemory, port: u16, bytes: u8, value: u32) {
        self.access(memory, |environment, memory| {
            environment.write_port(memory, port, bytes, value)
        })
    }

    pub fn read_mmio(&mut self, memory: &mut dyn GuestMemory, address: u32, bytes: u8) -> u64 {
        self.access(memory, |environment, memory| {
            environment.read_mmio(memory, address, bytes)
        })
    }

    pub fn write_mmio(
        &mut self,
        memory: &mut dyn GuestMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        self.access(memory, |environment, memory| {
            environment.write_mmio(memory, address, bytes, value)
        })
    }

    pub fn publish_map(&self, memory: &mut dyn GuestMemory) {
        memory.publish_map(&self.map.to_bytes());
    }

    pub fn access<R>(
        &mut self,
        backing: &mut dyn GuestMemory,
        access: impl FnOnce(&mut D, &mut MemoryAccess<'_>) -> R,
    ) -> R {
        let mut changed = false;
        let result = access(
            &mut self.environment,
            &mut MemoryAccess {
                backing,
                map: &mut self.map,
                changed: &mut changed,
            },
        );
        if changed {
            self.publish_map(backing);
        }
        result
    }

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
                match memory.map {
                    MemoryMap::Physical(map) => match map.get(address) {
                        PhysicalMapping::Ram { backing_offset }
                        | PhysicalMapping::Rom { backing_offset } => {
                            memory.backing.read(backing_offset, &mut bytes[..count]);
                        }
                        PhysicalMapping::Unmapped => bytes[..count].fill(0xff),
                        PhysicalMapping::Mmio => {
                            count = 1;
                            let bus_address = map.bus_address(address);
                            bytes[0] = environment.read_mmio(memory, bus_address, 1) as u8;
                        }
                    },
                    MemoryMap::Paged(map) => {
                        let mapping = map.get(address).ok_or(MemoryError::Unmapped { address })?;
                        memory
                            .backing
                            .read(mapping.backing_offset, &mut bytes[..count]);
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
                match memory.map {
                    MemoryMap::Physical(map) => match map.get(address) {
                        PhysicalMapping::Ram { backing_offset } => {
                            memory.backing.write(backing_offset, &bytes[..count]);
                        }
                        PhysicalMapping::Rom { .. } | PhysicalMapping::Unmapped => {}
                        PhysicalMapping::Mmio => {
                            count = 1;
                            let bus_address = map.bus_address(address);
                            environment.write_mmio(memory, bus_address, 1, u64::from(bytes[0]));
                        }
                    },
                    MemoryMap::Paged(map) => {
                        let mapping = map.get(address).ok_or(MemoryError::Unmapped { address })?;
                        if !mapping.writable {
                            return Err(MemoryError::ReadOnly { address });
                        }
                        memory
                            .backing
                            .write(mapping.backing_offset, &bytes[..count]);
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
