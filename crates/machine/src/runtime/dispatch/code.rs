//! Callable registrations and code dependencies visible to memory callbacks.

use crate::memory::CodePages;
use relocade_wasm_runtime::{Memory, WasmMemoryAccess};
use relocade_x86::{EntryTable, PhysicalMemoryMap};
#[cfg(feature = "worker")]
use std::collections::BTreeSet;
use std::{collections::BTreeMap, sync::Arc};

#[cfg(feature = "worker")]
pub(super) struct CompiledEntry {
    pub(super) address: u32,
    pub(super) byte_len: u32,
    pub(super) pages: BTreeSet<u32>,
}

struct Dependency {
    byte_len: u32,
    pages: BTreeMap<u32, u64>,
}

pub(crate) struct CodeRegistry {
    pub(super) addresses: EntryTable,
    pub(super) free_slots: Vec<u32>,
    pub(super) retired: Vec<u32>,
    entries: BTreeMap<u32, Dependency>,
    pages: Arc<CodePages>,
    mapping: Memory,
    dispatch: Memory,
    map: PhysicalMemoryMap,
    map_bytes: [u8; PhysicalMemoryMap::BYTE_LEN],
}

impl CodeRegistry {
    pub(super) fn new(
        pages: Arc<CodePages>,
        mapping: Memory,
        dispatch: Memory,
        map: PhysicalMemoryMap,
    ) -> Self {
        let map_bytes = map.to_bytes();
        Self {
            addresses: EntryTable::default(),
            free_slots: Vec::new(),
            retired: Vec::new(),
            entries: BTreeMap::new(),
            pages,
            mapping,
            dispatch,
            map,
            map_bytes,
        }
    }

    /// Replacing or adding an entry must also invalidate compiled fallthrough
    /// over that address, so newly registered hooks remain execution boundaries.
    pub(super) fn replace(&mut self, addresses: &[u32]) {
        let invalid: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(&start, dependency)| {
                addresses
                    .iter()
                    .any(|address| address.wrapping_sub(start) < dependency.byte_len)
                    .then_some(start)
            })
            .collect();
        for address in invalid {
            self.remove(address);
        }
    }

    #[cfg(feature = "worker")]
    pub(super) fn conflicts(&self, entry: &CompiledEntry) -> bool {
        self.addresses
            .range(entry.address..entry.address + entry.byte_len)
            .next()
            .is_some()
    }

    #[cfg(feature = "worker")]
    pub(super) fn track(&mut self, entries: Vec<CompiledEntry>) {
        for entry in entries {
            let pages = self.pages.retain(entry.pages);
            assert!(self
                .entries
                .insert(
                    entry.address,
                    Dependency {
                        byte_len: entry.byte_len,
                        pages
                    }
                )
                .is_none());
        }
    }

    fn remove(&mut self, address: u32) {
        if let Some(dependency) = self.entries.remove(&address) {
            self.pages.release(&dependency.pages);
            if let Some(slot) = self.addresses.remove(address) {
                self.free_slots.push(slot);
                self.retired.push(slot);
            }
        }
    }

    pub(super) fn clear(&mut self) {
        let addresses: Vec<_> = self.entries.keys().copied().collect();
        for address in addresses {
            self.remove(address);
        }
    }

    pub(crate) fn write(
        &mut self,
        access: &mut impl WasmMemoryAccess<Memory = Memory>,
        address: u32,
        bytes: u32,
    ) {
        self.pages.write(address, bytes as usize);
        self.sync(access);
    }

    /// Runs before every guest entry and before returning from a device callback.
    /// No registry lock or host-memory view is retained across guest execution.
    pub(crate) fn sync(&mut self, access: &mut impl WasmMemoryAccess<Memory = Memory>) {
        let revisions = self.pages.revisions();
        let invalid: Vec<_> = self
            .entries
            .iter()
            .filter_map(|(&address, dependency)| {
                dependency
                    .pages
                    .iter()
                    .any(|(page, revision)| revisions.get(page) != Some(revision))
                    .then_some(address)
            })
            .collect();
        if !invalid.is_empty() {
            for address in invalid {
                self.remove(address);
            }
            self.publish(access);
        } else {
            self.update_map(access);
        }
    }

    pub(super) fn publish(&mut self, access: &mut impl WasmMemoryAccess<Memory = Memory>) {
        access
            .memory(&self.dispatch)
            .write(0, &self.addresses.to_bytes());
        self.update_map(access);
    }

    fn update_map(&mut self, access: &mut impl WasmMemoryAccess<Memory = Memory>) {
        let revisions = self.pages.revisions();
        let bytes = self
            .map
            .to_bytes_with_code(|page| revisions.contains_key(&page));
        if bytes != self.map_bytes {
            access.memory(&self.mapping).write(0, &bytes);
            self.map_bytes = bytes;
        }
    }
}

impl Drop for CodeRegistry {
    fn drop(&mut self) {
        self.clear();
    }
}
