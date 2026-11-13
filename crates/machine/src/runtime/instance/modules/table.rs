//! Publishes real-mode CS:IP entries into one callable table.

use crate::{DispatchEntry, FarPointer, Result};
use relocade_compiler::abi::Function;
use relocade_wasm_runtime::{ExportFunction, FunctionRef, Import, Table};
use relocade_x86::{abi, EntryTable};
use std::collections::BTreeMap;

struct Entry<B: ExportFunction<abi::ExecutionSignature>> {
    slot: u32,
    function: FunctionRef<B, abi::ExecutionSignature>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Clone for Entry<B> {
    fn clone(&self) -> Self {
        Self {
            slot: self.slot,
            function: self.function.clone(),
        }
    }
}

pub(super) struct DispatchTable<B: ExportFunction<abi::ExecutionSignature>> {
    memory: B::Memory,
    functions: Table<B, abi::ExecutionSignature>,
    entries: BTreeMap<u32, Entry<B>>,
    interpreter: Option<FunctionRef<B, abi::ExecutionSignature>>,
    next_slot: u32,
    capacity: u32,
}

impl<B: ExportFunction<abi::ExecutionSignature>> DispatchTable<B> {
    pub(super) fn new(backend: &mut B) -> Result<Self> {
        let memory = backend.create_memory(abi::EXECUTION_ENTRIES)?;
        backend
            .memory(&memory)
            .write(0, &EntryTable::default().to_bytes());
        Ok(Self {
            memory,
            functions: backend.create_table(abi::EXECUTION_FUNCTIONS, None)?,
            entries: BTreeMap::new(),
            interpreter: None,
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
            capacity: abi::EXECUTION_FUNCTIONS.minimum,
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

    pub(super) fn resolve(&self, key: u32) -> Option<&FunctionRef<B, abi::ExecutionSignature>> {
        self.entries.get(&key).map(|entry| &entry.function)
    }

    pub(super) fn contains(&self, key: u32) -> bool {
        self.resolve(key).is_some()
    }

    /// Failed Real16 candidates suppress profiling only for their own CS:IP.
    pub(super) fn install_interpreter_fallback(
        &mut self,
        backend: &mut B,
        entry: FarPointer,
    ) -> Result<()> {
        let key = entry.packed();
        if self.contains(key) {
            return Ok(());
        }
        let mut entries = self.entries.clone();
        entries.insert(
            key,
            Entry {
                slot: EntryTable::INTERPRETER_SLOT,
                function: self
                    .interpreter
                    .as_ref()
                    .expect("interpreter is installed before execution")
                    .clone(),
            },
        );
        self.publish(backend, entries, Vec::new())
    }

    pub(super) fn install_interpreter(
        &mut self,
        backend: &mut B,
        instance: &B::Instance,
    ) -> Result<()> {
        let function = backend.function(instance, abi::RUN)?;
        let function = backend.import_function(&function);
        self.functions
            .set(backend, EntryTable::INTERPRETER_SLOT, &function)?;
        self.interpreter = Some(function);
        Ok(())
    }

    pub(super) fn install(
        &mut self,
        backend: &mut B,
        instance: &B::Instance,
        exports: &[DispatchEntry<'_>],
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
                .map(|entry| entry.slot)
                .filter(|&slot| slot >= EntryTable::FIRST_DYNAMIC_SLOT)
                .unwrap_or_else(|| {
                    let slot = next_slot;
                    next_slot = next_slot
                        .checked_add(1)
                        .expect("Wasm function table capacity");
                    slot
                });
            let function = backend.import_function(&function);
            entries.insert(
                entry.key,
                Entry {
                    slot,
                    function: function.clone(),
                },
            );
            functions.push((slot, function));
        }
        self.publish(backend, entries, functions)
    }

    fn publish(
        &mut self,
        backend: &mut B,
        entries: BTreeMap<u32, Entry<B>>,
        functions: Vec<(u32, FunctionRef<B, abi::ExecutionSignature>)>,
    ) -> Result<()> {
        let mut slots = EntryTable::default();
        for (&key, entry) in &entries {
            slots.insert(key, entry.slot);
        }
        let bytes = slots.to_bytes();
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
        backend.ensure_memory_size(&self.memory, bytes.len() as u64)?;
        for (slot, function) in functions {
            self.functions.set(backend, slot, &function)?;
        }
        backend.memory(&self.memory).write(0, &bytes);
        self.entries = entries;
        self.next_slot = next_slot;
        Ok(())
    }
}
