//! Publishes block identities and address-bound overrides into one callable table.

use crate::{
    runtime::{EntryKey, ModuleEntry},
    FarPointer, Result,
};
use relocade_compiler::abi::Function;
use relocade_wasm_runtime::{ExportFunction, FunctionRef, Import, Table};
use relocade_x86::{abi, EntryTable, ExecutionProfile};
use std::collections::BTreeMap;

pub(super) struct DispatchTable<B: ExportFunction<abi::ExecutionSignature>> {
    memory: B::Memory,
    real_mode: Option<B::Memory>,
    functions: Table<B, abi::ExecutionSignature>,
    entries: BTreeMap<EntryKey, u32>,
    next_slot: u32,
    capacity: u32,
}

impl<B: ExportFunction<abi::ExecutionSignature>> DispatchTable<B> {
    pub(super) fn new(backend: &mut B, profile: ExecutionProfile) -> Result<Self> {
        let memory = backend.create_memory(abi::EXECUTION_ENTRIES)?;
        backend
            .memory(&memory)
            .write(0, &EntryTable::default().to_bytes());
        let real_mode = if profile == ExecutionProfile::Real16 {
            let memory = backend.create_memory(abi::REAL_MODE_BLOCKS)?;
            backend
                .memory(&memory)
                .write(0, &EntryTable::default().to_bytes());
            Some(memory)
        } else {
            None
        };
        Ok(Self {
            memory,
            real_mode,
            functions: backend.create_table(abi::EXECUTION_FUNCTIONS, None)?,
            entries: BTreeMap::new(),
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
            capacity: abi::EXECUTION_FUNCTIONS.minimum,
        })
    }

    pub(super) fn imports<'a>(&'a self, backend: &B) -> Vec<Import<'a, B>>
    where
        B: 'a,
    {
        let mut imports = vec![
            backend.bind(abi::EXECUTION_ENTRIES, &self.memory),
            backend.bind(abi::EXECUTION_FUNCTIONS, &self.functions),
        ];
        if let Some(memory) = &self.real_mode {
            imports.push(backend.bind(abi::REAL_MODE_BLOCKS, memory));
        }
        imports
    }

    pub(super) fn contains(&self, key: EntryKey) -> bool {
        self.entries.contains_key(&EntryKey::Address(key.address()))
            || self.entries.contains_key(&key)
    }

    /// Failed Real16 candidates suppress profiling only for their own CS:IP.
    pub(super) fn interpret_at(&mut self, backend: &mut B, entry: FarPointer) -> Result<()> {
        let key = EntryKey::RealMode(entry.packed());
        if self.contains(key) {
            return Ok(());
        }
        let mut entries = self.entries.clone();
        entries.insert(key, EntryTable::INTERPRETER_SLOT);
        self.publish(backend, entries, Vec::new())
    }

    pub(super) fn install_interpreter(
        &self,
        backend: &mut B,
        instance: &B::Instance,
    ) -> Result<()> {
        let function = backend.function(instance, abi::RUN)?;
        let function = backend.import_function(&function);
        self.functions
            .set(backend, EntryTable::INTERPRETER_SLOT, &function)?;
        Ok(())
    }

    pub(super) fn install(
        &mut self,
        backend: &mut B,
        instance: &B::Instance,
        exports: &[ModuleEntry<'_>],
    ) -> Result<()> {
        let mut entries = self.entries.clone();
        let mut next_slot = self.next_slot;
        let mut functions = Vec::with_capacity(exports.len());
        for entry in exports {
            let function = backend.function(
                instance,
                Function::<abi::ExecutionSignature>::new("", entry.export),
            )?;
            let slot = entries
                .get(&entry.key)
                .copied()
                .filter(|&slot| slot >= EntryTable::FIRST_DYNAMIC_SLOT)
                .unwrap_or_else(|| {
                    let slot = next_slot;
                    next_slot = next_slot
                        .checked_add(1)
                        .expect("Wasm function table capacity");
                    slot
                });
            if let EntryKey::RealMode(_) = entry.key {
                // Explicit compilation replaces an address hook. Other CS:IP
                // aliases keep their own compiled entries when it is removed.
                entries.remove(&EntryKey::Address(entry.key.address()));
            }
            entries.insert(entry.key, slot);
            functions.push((slot, backend.import_function(&function)));
        }
        self.publish(backend, entries, functions)
    }

    fn publish(
        &mut self,
        backend: &mut B,
        entries: BTreeMap<EntryKey, u32>,
        functions: Vec<(u32, FunctionRef<B, abi::ExecutionSignature>)>,
    ) -> Result<()> {
        let mut addresses = EntryTable::default();
        let mut blocks = EntryTable::default();
        for (&key, &slot) in &entries {
            match key {
                EntryKey::Address(address) => {
                    addresses.insert(address, slot);
                }
                EntryKey::RealMode(key) => {
                    blocks.insert(key, slot);
                }
            }
        }
        let addresses = addresses.to_bytes();
        let blocks = blocks.to_bytes();
        let next_slot = functions
            .iter()
            .map(|(slot, _)| slot + 1)
            .max()
            .unwrap_or(self.next_slot)
            .max(self.next_slot);
        if next_slot > self.capacity {
            let capacity = next_slot.max(self.capacity.saturating_mul(2));
            self.functions.ensure_size(backend, capacity, None)?;
            self.capacity = capacity;
        }
        backend.ensure_memory_size(&self.memory, addresses.len() as u64)?;
        if let Some(memory) = &self.real_mode {
            backend.ensure_memory_size(memory, blocks.len() as u64)?;
        }
        for (slot, function) in functions {
            self.functions.set(backend, slot, &function)?;
        }
        backend.memory(&self.memory).write(0, &addresses);
        if let Some(memory) = &self.real_mode {
            backend.memory(memory).write(0, &blocks);
        }
        self.entries = entries;
        self.next_slot = next_slot;
        Ok(())
    }
}
