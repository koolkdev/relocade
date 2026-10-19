//! Installation of callable Wasm functions at guest linear addresses.

use crate::{wasm::WasmBackend, Result};
use relocade_compiler::abi::Function;
use relocade_x86::{abi, EntryTable};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

/// An ordinary `() -> i64` export to publish at a guest linear address.
pub struct DispatchEntry<'a> {
    pub address: u32,
    pub export: &'a str,
}

/// Owns the guest-address map and shared Wasm function table.
pub(crate) struct DispatchTable<B: WasmBackend> {
    pub memory: B::Memory,
    pub functions: B::Table,
    pub entries: Arc<Mutex<Entries>>,
    capacity: u32,
}

/// Publication and slot allocation share one registry with code invalidation.
#[derive(Clone)]
pub(crate) struct Entries {
    pub addresses: EntryTable,
    free: BTreeSet<u32>,
    next: u32,
}

impl Entries {
    pub fn remove(&mut self, address: u32) {
        if let Some(slot) = self.addresses.remove(address) {
            self.free.insert(slot);
        }
    }

    fn slot(&mut self, address: u32) -> u32 {
        if let Some(slot) = self.addresses.get(address) {
            return slot;
        }
        let slot = self.free.pop_first().unwrap_or_else(|| {
            let slot = self.next;
            self.next = self
                .next
                .checked_add(1)
                .expect("Wasm function table capacity");
            slot
        });
        self.addresses.insert(address, slot);
        slot
    }
}

impl<B: WasmBackend> DispatchTable<B> {
    pub fn new(backend: &mut B) -> Result<Self> {
        let memory = backend.create_memory(abi::EXECUTION_ENTRIES)?;
        let addresses = EntryTable::default();
        backend.write_memory(&memory, 0, &addresses.to_bytes());
        Ok(Self {
            memory,
            functions: backend.create_table(abi::EXECUTION_FUNCTIONS.minimum)?,
            entries: Arc::new(Mutex::new(Entries {
                addresses,
                free: BTreeSet::new(),
                next: EntryTable::FIRST_DYNAMIC_SLOT,
            })),
            capacity: abi::EXECUTION_FUNCTIONS.minimum,
        })
    }

    pub fn install(
        &mut self,
        backend: &mut B,
        instance: &B::Instance,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        // Resolve every export before changing existing callable slots.
        let mut registry = self.entries.lock().unwrap();
        let mut next = registry.clone();
        let mut functions = Vec::with_capacity(entries.len());
        for entry in entries {
            let function = backend.function(instance, Function::new("", entry.export))?;
            let slot = next.slot(entry.address);
            functions.push((slot, function));
        }
        let bytes = next.addresses.to_bytes();
        // Reserve both resources before replacing any live function slots.
        if next.next > self.capacity {
            let capacity = next.next.max(self.capacity.saturating_mul(2));
            backend.ensure_table_size(&self.functions, capacity)?;
            self.capacity = capacity;
        }
        backend.ensure_memory_size(&self.memory, bytes.len() as u64)?;
        for (slot, function) in functions {
            backend.set_table_function(&self.functions, slot, &function)?;
        }
        // All functions exist before their addresses become visible to dispatch.
        backend.write_memory(&self.memory, 0, &bytes);
        *registry = next;
        Ok(())
    }
}
