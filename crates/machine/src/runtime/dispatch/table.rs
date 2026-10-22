use super::{DispatchEntry, EntryKind};
use crate::Result;
use relocade_compiler::abi::Function;
use relocade_wasm_runtime::{Import, WasmBackend};
use relocade_x86::{abi, EntryTable};
use std::collections::BTreeMap;

/// Owns the guest-address map and Wasm function table for one CPU.
pub(super) struct DispatchTable<B: WasmBackend> {
    memory: B::Memory,
    functions: B::Table,
    addresses: EntryTable,
    next_slot: u32,
    capacity: u32,
    hooks: BTreeMap<u32, u32>,
    generation: Option<u64>,
    free_slots: Vec<u32>,
}

impl<B: WasmBackend> DispatchTable<B> {
    pub(super) fn new(backend: &mut B) -> Result<Self> {
        let memory = backend.create_memory(abi::EXECUTION_ENTRIES)?;
        let addresses = EntryTable::default();
        backend.write_memory(&memory, 0, &addresses.to_bytes());
        Ok(Self {
            memory,
            functions: backend.create_table(abi::EXECUTION_FUNCTIONS.minimum)?,
            addresses,
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
            capacity: abi::EXECUTION_FUNCTIONS.minimum,
            hooks: BTreeMap::new(),
            generation: None,
            free_slots: Vec::new(),
        })
    }

    pub(super) fn imports<'a>(&'a self, backend: &B) -> [Import<'a, B>; 2]
    where
        B: 'a,
    {
        [
            backend.bind(abi::EXECUTION_ENTRIES, &self.memory),
            backend.bind(abi::EXECUTION_FUNCTIONS, &self.functions),
        ]
    }

    pub(super) fn install_interpreter(
        &self,
        backend: &mut B,
        instance: &B::Instance,
    ) -> Result<()> {
        let function = backend.function(instance, abi::RUN)?;
        backend.set_table_function(&self.functions, EntryTable::INTERPRETER_SLOT, &function)?;
        Ok(())
    }

    pub(super) fn install(
        &mut self,
        backend: &mut B,
        instance: &B::Instance,
        entries: &[DispatchEntry<'_>],
        kind: EntryKind,
    ) -> Result<()> {
        // Resolve every export before changing existing callable slots.
        let mut addresses = self.addresses.clone();
        let mut next_slot = self.next_slot;
        let mut free_slots = self.free_slots.clone();
        let mut functions = Vec::with_capacity(entries.len());
        for entry in entries {
            let function = backend.function(instance, Function::new("", entry.export))?;
            let slot = addresses.get(entry.address).unwrap_or_else(|| {
                if let Some(slot) = free_slots.pop() {
                    return slot;
                }
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
        self.free_slots = free_slots;
        for entry in entries {
            match kind {
                EntryKind::Generated => {
                    self.hooks.remove(&entry.address);
                }
                EntryKind::Hook => {
                    self.hooks.insert(
                        entry.address,
                        self.addresses.get(entry.address).expect("installed entry"),
                    );
                }
            }
        }
        Ok(())
    }

    /// Discards generated entries after a mapping change, retaining hooks.
    /// Called before execution or code publication.
    pub(super) fn synchronize(&mut self, backend: &mut B, generation: u64) {
        if self.generation == Some(generation) {
            return;
        }
        let mut addresses = EntryTable::default();
        self.free_slots.extend(
            self.addresses
                .iter()
                .filter_map(|(address, slot)| (!self.hooks.contains_key(&address)).then_some(slot)),
        );
        for (&address, &slot) in &self.hooks {
            addresses.insert(address, slot);
        }
        backend.write_memory(&self.memory, 0, &addresses.to_bytes());
        self.addresses = addresses;
        self.generation = Some(generation);
    }
}
