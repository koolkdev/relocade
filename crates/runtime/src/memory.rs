use crate::{machine::State, Devices};
use std::fmt;
use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};
use wasmtime::{AsContextMut, Linker, Memory, MemoryType, Store};

/// Host memory setup/access failures, independent of guest CPU exceptions.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MemoryError {
    AddressRange,
    InsufficientBacking { required: u64, available: usize },
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddressRange => {
                f.write_str("physical transfer exceeds the real-mode address range")
            }
            Self::InsufficientBacking {
                required,
                available,
            } => {
                write!(f,
                "physical map requires {required} backing bytes; only {available} are allocated")
            }
        }
    }
}

impl std::error::Error for MemoryError {}

/// Physical backing and routing available during a synchronous device callback.
/// CPU backing and guest execution are deliberately absent. Mapping changes are
/// installed before the generated interpreter's next physical transfer.
pub struct DeviceMemory<'a> {
    pub(crate) backing: &'a mut [u8],
    pub(crate) map: &'a mut PhysicalMemoryMap,
    pub(crate) changed: &'a mut bool,
}

impl DeviceMemory<'_> {
    /// Raw backing bytes. These offsets are independent of physical bus addresses.
    pub fn backing(&self) -> &[u8] {
        self.backing
    }

    /// Raw backing for device-managed buffers or ROM initialization.
    pub fn backing_mut(&mut self) -> &mut [u8] {
        self.backing
    }

    pub fn map(&self) -> &PhysicalMemoryMap {
        self.map
    }

    pub fn set_a20_enabled(&mut self, enabled: bool) {
        self.map.set_a20_enabled(enabled);
        *self.changed = true;
    }

    /// Changes routing using already allocated backing. To grow backing, replace
    /// the map through [`crate::Machine::set_memory_map`] between entries.
    pub fn set_map(&mut self, map: PhysicalMemoryMap) -> Result<(), MemoryError> {
        let required = map.required_backing_bytes();
        if required > self.backing.len() as u64 {
            return Err(MemoryError::InsufficientBacking {
                required,
                available: self.backing.len(),
            });
        }
        *self.map = map;
        *self.changed = true;
        Ok(())
    }
}

pub(crate) fn check_range(address: u32, bytes: usize) -> Result<(), MemoryError> {
    let limit = PhysicalMemoryMap::PAGE_COUNT as u64 * u64::from(PhysicalMemoryMap::PAGE_BYTES);
    if u64::from(address) > limit || bytes as u64 > limit.saturating_sub(u64::from(address)) {
        return Err(MemoryError::AddressRange);
    }
    Ok(())
}

pub(crate) fn backing_pages(map: &PhysicalMemoryMap) -> u64 {
    map.required_backing_bytes().div_ceil(65536).max(1)
}

/// Owns backing allocation and publication of the routing table used by Wasm.
#[derive(Clone, Copy)]
pub(crate) struct MemoryBus {
    guest: Memory,
    physical: Memory,
}

impl MemoryBus {
    pub(crate) fn new<D>(store: &mut Store<State<D>>) -> wasmtime::Result<Self> {
        let pages = backing_pages(&store.data().map);
        let guest = Memory::new(&mut *store, MemoryType::new(pages as u32, None))?;
        let physical = Memory::new(&mut *store, MemoryType::new(1, Some(1)))?;
        let image = store.data().map.to_bytes();
        physical.write(&mut *store, 0, &image)?;
        Ok(Self { guest, physical })
    }

    pub(crate) fn define_imports<D>(
        &self,
        linker: &mut Linker<State<D>>,
        store: &Store<State<D>>,
    ) -> wasmtime::Result<()> {
        linker.define(store, "wasm86", "guest", self.guest)?;
        linker.define(store, "wasm86", "physicalMap", self.physical)?;
        Ok(())
    }

    pub(crate) fn set_map<D>(
        &self,
        store: &mut Store<State<D>>,
        map: PhysicalMemoryMap,
    ) -> wasmtime::Result<()> {
        let pages = backing_pages(&map);
        let current = self.guest.size(&*store);
        if pages > current {
            self.guest.grow(&mut *store, pages - current)?;
        }
        self.physical.write(&mut *store, 0, &map.to_bytes())?;
        store.data_mut().map = map;
        store.data_mut().map_changed = false;
        Ok(())
    }

    pub(crate) fn access<D, R>(
        &self,
        mut store: impl AsContextMut<Data = State<D>>,
        access: impl FnOnce(&mut D, &mut DeviceMemory<'_>) -> R,
    ) -> R {
        let mut store = store.as_context_mut();
        let (backing, state) = self.guest.data_and_store_mut(&mut store);
        let result = access(
            &mut state.devices,
            &mut DeviceMemory {
                backing,
                map: &mut state.map,
                changed: &mut state.map_changed,
            },
        );
        if store.data().map_changed {
            let image = store.data().map.to_bytes();
            self.physical
                .write(&mut store, 0, &image)
                .expect("physical table has fixed valid backing");
            store.data_mut().map_changed = false;
        }
        result
    }

    pub(crate) fn read<D: Devices>(
        &self,
        store: &mut Store<State<D>>,
        address: u32,
        bytes: &mut [u8],
    ) -> Result<(), MemoryError> {
        check_range(address, bytes.len())?;
        self.access(store, |devices, memory| {
            for (offset, byte) in bytes.iter_mut().enumerate() {
                let address = address + offset as u32;
                *byte = match memory.map.get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => {
                        memory.backing[backing_offset as usize]
                    }
                    PhysicalMapping::Unmapped => 0xff,
                    PhysicalMapping::Mmio => {
                        let address = memory.map.bus_address(address);
                        devices.read_mmio(memory, address, 1) as u8
                    }
                };
            }
        });
        Ok(())
    }

    pub(crate) fn write<D: Devices>(
        &self,
        store: &mut Store<State<D>>,
        address: u32,
        bytes: &[u8],
    ) -> Result<(), MemoryError> {
        check_range(address, bytes.len())?;
        self.access(store, |devices, memory| {
            for (offset, &byte) in bytes.iter().enumerate() {
                let address = address + offset as u32;
                match memory.map.get(address) {
                    PhysicalMapping::Ram { backing_offset } => {
                        memory.backing[backing_offset as usize] = byte
                    }
                    PhysicalMapping::Rom { .. } | PhysicalMapping::Unmapped => {}
                    PhysicalMapping::Mmio => {
                        let address = memory.map.bus_address(address);
                        devices.write_mmio(memory, address, 1, u64::from(byte))
                    }
                }
            }
        });
        Ok(())
    }
}
