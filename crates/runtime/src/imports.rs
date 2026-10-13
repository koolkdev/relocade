use crate::{
    backend::GuestMemory, host::HostCallback, memory::check_physical_range, DeviceMemory, Devices,
    MemoryError,
};
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};

/// Shared implementation of the generated interpreter's host imports. Backends
/// bind these methods to Wasm imports and supply scoped access to guest backing.
/// Each device call publishes changed routing before returning to generated code.
pub struct Imports<D> {
    pub(crate) devices: D,
    pub(crate) callbacks: Vec<Option<HostCallback<D>>>,
    map: PhysicalMemoryMap,
}

impl<D: Devices> Imports<D> {
    pub(crate) fn new(map: PhysicalMemoryMap, devices: D) -> Self {
        Self {
            map,
            devices,
            callbacks: Vec::new(),
        }
    }

    pub fn read_port(&mut self, memory: &mut dyn GuestMemory, port: u16, bytes: u8) -> u32 {
        self.access(memory, |devices, memory| {
            devices.read_port(memory, port, bytes)
        })
    }

    pub fn write_port(&mut self, memory: &mut dyn GuestMemory, port: u16, bytes: u8, value: u32) {
        self.access(memory, |devices, memory| {
            devices.write_port(memory, port, bytes, value)
        })
    }

    pub fn read_mmio(&mut self, memory: &mut dyn GuestMemory, address: u32, bytes: u8) -> u64 {
        self.access(memory, |devices, memory| {
            devices.read_mmio(memory, address, bytes)
        })
    }

    pub fn write_mmio(
        &mut self,
        memory: &mut dyn GuestMemory,
        address: u32,
        bytes: u8,
        value: u64,
    ) {
        self.access(memory, |devices, memory| {
            devices.write_mmio(memory, address, bytes, value)
        })
    }

    pub(crate) fn publish_map(&self, memory: &mut dyn GuestMemory) {
        memory.publish_map(&self.map.to_bytes());
    }

    pub(crate) fn access<R>(
        &mut self,
        backing: &mut dyn GuestMemory,
        access: impl FnOnce(&mut D, &mut DeviceMemory<'_>) -> R,
    ) -> R {
        let mut changed = false;
        let result = access(
            &mut self.devices,
            &mut DeviceMemory {
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

    pub(crate) fn read(
        &mut self,
        backing: &mut dyn GuestMemory,
        mut address: u32,
        mut bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        check_physical_range(address, bytes.len())?;
        self.access(backing, |devices, memory| {
            while !bytes.is_empty() {
                let mut count = page_span(address, bytes.len());
                match memory.map.get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => {
                        memory.backing.read(backing_offset, &mut bytes[..count])
                    }
                    PhysicalMapping::Unmapped => bytes[..count].fill(0xff),
                    PhysicalMapping::Mmio => {
                        count = 1;
                        bytes[0] =
                            devices.read_mmio(memory, memory.map.bus_address(address), 1) as u8;
                    }
                }
                address += count as u32;
                bytes = &mut std::mem::take(&mut bytes)[count..];
            }
        });
        Ok(())
    }

    pub(crate) fn write(
        &mut self,
        backing: &mut dyn GuestMemory,
        mut address: u32,
        mut bytes: &[u8],
    ) -> Result<(), MemoryError> {
        check_physical_range(address, bytes.len())?;
        self.access(backing, |devices, memory| {
            while !bytes.is_empty() {
                let mut count = page_span(address, bytes.len());
                match memory.map.get(address) {
                    PhysicalMapping::Ram { backing_offset } => {
                        memory.backing.write(backing_offset, &bytes[..count])
                    }
                    PhysicalMapping::Rom { .. } | PhysicalMapping::Unmapped => {}
                    PhysicalMapping::Mmio => {
                        count = 1;
                        devices.write_mmio(
                            memory,
                            memory.map.bus_address(address),
                            1,
                            u64::from(bytes[0]),
                        );
                    }
                }
                address += count as u32;
                bytes = &bytes[count..];
            }
        });
        Ok(())
    }
}

// A20 and physical routing can change only at a page boundary unless a device
// callback runs. MMIO therefore transfers one byte before resolving routing again.
fn page_span(address: u32, remaining: usize) -> usize {
    let page = PhysicalMemoryMap::PAGE_BYTES;
    remaining.min((page - address % page) as usize)
}
