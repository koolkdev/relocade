//! Installation of callable Wasm functions at guest linear addresses.

use crate::{wasm::WasmBackend, Result};
use relocade_compiler::abi::Function;
use relocade_x86::{abi, EntryTable};

/// An ordinary `() -> i64` export to publish at a guest linear address.
pub struct DispatchEntry<'a> {
    pub address: u32,
    pub export: &'a str,
}

/// Owns the guest-address map and shared Wasm function table.
pub(crate) struct DispatchTable<B: WasmBackend> {
    pub memory: B::Memory,
    pub functions: B::Table,
    addresses: EntryTable,
    next_slot: u32,
    capacity: u32,
}

impl<B: WasmBackend> DispatchTable<B> {
    pub fn new(backend: &mut B) -> Result<Self> {
        let memory = backend.create_memory(1, None)?;
        let addresses = EntryTable::default();
        backend.write_memory(&memory, 0, &addresses.to_bytes());
        Ok(Self {
            memory,
            functions: backend.create_table(abi::EXECUTION_FUNCTIONS.minimum)?,
            addresses,
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
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
        let mut addresses = self.addresses.clone();
        let mut next_slot = self.next_slot;
        let mut functions = Vec::with_capacity(entries.len());
        for entry in entries {
            let function = backend.function(instance, Function::new("", entry.export))?;
            let slot = addresses.get(entry.address).unwrap_or_else(|| {
                let slot = next_slot;
                next_slot = next_slot
                    .checked_add(1)
                    .expect("Wasm function table capacity");
                slot
            });
            addresses.insert(entry.address, slot);
            functions.push((slot, function));
        }
        let bytes = addresses.to_bytes();
        // Reserve both resources before replacing any live function slots.
        if next_slot > self.capacity {
            let capacity = next_slot.max(self.capacity.saturating_mul(2));
            backend.ensure_table_size(&self.functions, capacity)?;
            self.capacity = capacity;
        }
        backend.ensure_memory_size(&self.memory, bytes.len() as u64)?;
        for (slot, function) in functions {
            backend.set_table_function(&self.functions, slot, &function)?;
        }
        // All functions exist before their addresses become visible to dispatch.
        backend.write_memory(&self.memory, 0, &bytes);
        self.addresses = addresses;
        self.next_slot = next_slot;
        Ok(())
    }
}
