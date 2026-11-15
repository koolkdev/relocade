//! Real-mode entries own their current binding and incoming call-table slots.

use super::{calls::CallSite, Calls, ModuleId};
use crate::{DispatchEntry, FarPointer, Result};
use relocade_compiler::abi::Function;
use relocade_wasm_runtime::{ExportFunction, FunctionRef, Import, Table};
use relocade_x86::{abi, EntryTable};
use std::collections::BTreeMap;

struct Entry<B: ExportFunction<abi::ExecutionSignature>> {
    target: Target<B>,
    callers: BTreeMap<ModuleId, CallSite<B>>,
}

impl<B: ExportFunction<abi::ExecutionSignature>> Entry<B> {
    fn unresolved() -> Self {
        Self {
            target: Target::Unresolved,
            callers: BTreeMap::new(),
        }
    }
}

enum Target<B: ExportFunction<abi::ExecutionSignature>> {
    Unresolved,
    Interpreter,
    Export {
        module: ModuleId,
        slot: u32,
        function: FunctionRef<B, abi::ExecutionSignature>,
    },
}

impl<B: ExportFunction<abi::ExecutionSignature>> Target<B> {
    fn slot(&self) -> Option<u32> {
        match self {
            Self::Unresolved => None,
            Self::Interpreter => Some(EntryTable::INTERPRETER_SLOT),
            Self::Export { slot, .. } => Some(*slot),
        }
    }

    fn function<'a>(
        &'a self,
        interpreter: Option<&'a FunctionRef<B, abi::ExecutionSignature>>,
    ) -> Option<&'a FunctionRef<B, abi::ExecutionSignature>> {
        match self {
            Self::Unresolved => None,
            Self::Interpreter => Some(interpreter.expect("installed interpreter")),
            Self::Export { function, .. } => Some(function),
        }
    }
}

pub(super) struct DispatchTable<B: ExportFunction<abi::ExecutionSignature>> {
    memory: B::Memory,
    functions: Table<B, abi::ExecutionSignature>,
    entries: BTreeMap<u32, Entry<B>>,
    interpreter: Option<FunctionRef<B, abi::ExecutionSignature>>,
    next_slot: u32,
    free_slots: Vec<u32>,
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
            free_slots: Vec::new(),
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

    pub(super) fn contains(&self, key: u32) -> bool {
        self.entries
            .get(&key)
            .is_some_and(|entry| entry.target.slot().is_some())
    }

    pub(super) fn owner(&self, key: u32) -> Option<ModuleId> {
        match &self.entries.get(&key)?.target {
            Target::Export { module, .. } => Some(*module),
            _ => None,
        }
    }

    pub(super) fn register_calls(
        &mut self,
        backend: &mut B,
        module: ModuleId,
        calls: &Calls<B>,
    ) -> Result<()> {
        // Each module uses one call-table slot per distinct target.
        for (key, site) in calls.sites() {
            let entry = self.entries.entry(key).or_insert_with(Entry::unresolved);
            site.bind(backend, entry.target.function(self.interpreter.as_ref()))?;
            entry.callers.insert(module, site);
        }
        Ok(())
    }

    pub(super) fn unregister_calls(&mut self, module: ModuleId, calls: &Calls<B>) {
        for key in &calls.targets {
            let entry = self.entries.get_mut(key).expect("registered call target");
            entry.callers.remove(&module);
            if entry.target.slot().is_none() && entry.callers.is_empty() {
                self.entries.remove(key);
            }
        }
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
        self.publish(backend, BTreeMap::from([(key, Target::Interpreter)]))
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
        module: ModuleId,
        instance: &B::Instance,
        exports: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let mut updates = BTreeMap::new();
        let mut next_slot = self.next_slot;
        let mut free_slots = self.free_slots.clone();
        for entry in exports {
            let function = backend.function(
                instance,
                Function::<abi::ExecutionSignature>::new("", entry.export),
            )?;
            let slot = self
                .entries
                .get(&entry.key)
                .and_then(|entry| entry.target.slot())
                .filter(|&slot| slot >= EntryTable::FIRST_DYNAMIC_SLOT)
                .or_else(|| free_slots.pop())
                .unwrap_or_else(|| {
                    let slot = next_slot;
                    next_slot = next_slot
                        .checked_add(1)
                        .expect("Wasm function table capacity");
                    slot
                });
            updates.insert(
                entry.key,
                Target::Export {
                    module,
                    slot,
                    function: backend.import_function(&function),
                },
            );
        }
        self.publish(backend, updates)?;
        self.free_slots = free_slots;
        Ok(())
    }

    /// Only withdraw exports still owned by the retiring installation.
    pub(super) fn remove(
        &mut self,
        backend: &mut B,
        exports: impl IntoIterator<Item = (ModuleId, u32)>,
    ) -> Result<Vec<u32>> {
        let updates: BTreeMap<_, _> = exports
            .into_iter()
            .filter(|&(module, key)| self.owner(key) == Some(module))
            .map(|(_, key)| (key, Target::Unresolved))
            .collect();
        let keys: Vec<_> = updates.keys().copied().collect();
        let slots: Vec<_> = keys
            .iter()
            .map(|key| self.entries[key].target.slot().expect("published export"))
            .collect();
        self.publish(backend, updates)?;
        self.free_slots.extend(slots);
        Ok(keys)
    }

    /// Publish binding changes and update their callers in the same operation.
    /// Unresolved entries remain registered while they have incoming calls.
    fn publish(&mut self, backend: &mut B, updates: BTreeMap<u32, Target<B>>) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }
        let bytes = self.lookup_after(&updates).to_bytes();
        let next_slot = self.reserve(backend, &updates, bytes.len())?;
        for (key, target) in updates {
            self.bind(backend, key, target)?;
        }
        backend.memory(&self.memory).write(0, &bytes);
        self.next_slot = next_slot;
        Ok(())
    }

    fn lookup_after(&self, updates: &BTreeMap<u32, Target<B>>) -> EntryTable {
        let mut slots = EntryTable::default();
        for (&key, entry) in &self.entries {
            if updates.contains_key(&key) {
                continue;
            }
            if let Some(slot) = entry.target.slot() {
                slots.insert(key, slot);
            }
        }
        for (&key, target) in updates {
            if let Some(slot) = target.slot() {
                slots.insert(key, slot);
            }
        }
        slots
    }

    /// Grow both tables before replacing any live binding.
    fn reserve(
        &mut self,
        backend: &mut B,
        updates: &BTreeMap<u32, Target<B>>,
        lookup_bytes: usize,
    ) -> Result<u32> {
        let next_slot = updates
            .values()
            .filter_map(Target::slot)
            .fold(self.next_slot, |next, slot| next.max(slot + 1));
        if next_slot > self.capacity {
            let capacity = next_slot.max(self.capacity.saturating_mul(2));
            self.functions.ensure_size(backend, capacity, None)?;
            self.capacity = capacity;
        }
        backend.ensure_memory_size(&self.memory, lookup_bytes as u64)?;
        Ok(next_slot)
    }

    /// Update the function-table slot and registered callers for one entry.
    fn bind(&mut self, backend: &mut B, key: u32, target: Target<B>) -> Result<()> {
        let entry = self.entries.entry(key).or_insert_with(Entry::unresolved);
        if let Target::Export { slot, function, .. } = &target {
            self.functions.set(backend, *slot, function)?;
        } else if let Target::Export { slot, .. } = &entry.target {
            self.functions.set(
                backend,
                *slot,
                self.interpreter.as_ref().expect("installed interpreter"),
            )?;
        }
        for caller in entry.callers.values() {
            caller.bind(backend, target.function(self.interpreter.as_ref()))?;
        }
        entry.target = target;
        if entry.target.slot().is_none() && entry.callers.is_empty() {
            self.entries.remove(&key);
        }
        Ok(())
    }
}
