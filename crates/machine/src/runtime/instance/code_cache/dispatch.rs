//! Dispatch entries own their current binding and incoming call-table slots.

use super::calls::{CallSite, Calls};
use crate::memory::code::ModuleId;
use crate::{runtime::WasmRuntime, DispatchEntry, FarPointer, Result};
use relocade_compiler::abi::Function;
use relocade_wasm_runtime::{
    Context, FunctionRef, Import, Memory, Table, WasmBackend, WasmMemoryAccess,
};
use relocade_x86::{abi, EntryTable};
use std::collections::BTreeMap;

struct Entry {
    target: Target,
    callers: BTreeMap<ModuleId, CallSite>,
}

impl Entry {
    fn unresolved() -> Self {
        Self {
            target: Target::Unresolved,
            callers: BTreeMap::new(),
        }
    }
}

enum Target {
    Unresolved,
    Interpreter,
    Export {
        module: ModuleId,
        slot: u32,
        function: FunctionRef<WasmRuntime, abi::ExecutionSignature>,
    },
}

impl Target {
    fn slot(&self) -> Option<u32> {
        match self {
            Self::Unresolved => None,
            Self::Interpreter => Some(EntryTable::INTERPRETER_SLOT),
            Self::Export { slot, .. } => Some(*slot),
        }
    }

    fn function<'a>(
        &'a self,
        interpreter: Option<&'a FunctionRef<WasmRuntime, abi::ExecutionSignature>>,
    ) -> Option<&'a FunctionRef<WasmRuntime, abi::ExecutionSignature>> {
        match self {
            Self::Unresolved => None,
            Self::Interpreter => Some(interpreter.expect("installed interpreter")),
            Self::Export { function, .. } => Some(function),
        }
    }
}

pub(in crate::runtime::instance) struct DispatchTable {
    memory: Memory,
    functions: Table<WasmRuntime, abi::ExecutionSignature>,
    entries: BTreeMap<u32, Entry>,
    interpreter: Option<FunctionRef<WasmRuntime, abi::ExecutionSignature>>,
    next_slot: u32,
    free_slots: Vec<u32>,
    capacity: u32,
}

impl DispatchTable {
    pub(in crate::runtime::instance) fn new(wasm: &mut WasmRuntime) -> Result<Self> {
        let memory = wasm.create_memory(abi::EXECUTION_ENTRIES)?;
        wasm.memory(&memory)
            .write(0, &EntryTable::default().to_bytes());
        Ok(Self {
            memory,
            functions: wasm.create_table(abi::EXECUTION_FUNCTIONS, None)?,
            entries: BTreeMap::new(),
            interpreter: None,
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
            free_slots: Vec::new(),
            capacity: abi::EXECUTION_FUNCTIONS.minimum,
        })
    }

    pub(in crate::runtime::instance) fn imports<'a>(
        &'a self,
        wasm: &WasmRuntime,
    ) -> [Import<'a, WasmRuntime>; 2] {
        [
            wasm.bind(abi::EXECUTION_ENTRIES, &self.memory),
            wasm.bind(abi::EXECUTION_FUNCTIONS, &self.functions),
        ]
    }

    pub(super) fn resources(&self) -> (Memory, Table<WasmRuntime, abi::ExecutionSignature>) {
        (self.memory.clone(), self.functions.clone())
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
        wasm: &mut WasmRuntime,
        module: ModuleId,
        calls: &Calls,
    ) -> Result<()> {
        // Each module uses one call-table slot per distinct target.
        for (key, site) in calls.sites() {
            let entry = self.entries.entry(key).or_insert_with(Entry::unresolved);
            site.bind(wasm, entry.target.function(self.interpreter.as_ref()))?;
            entry.callers.insert(module, site);
        }
        Ok(())
    }

    pub(super) fn unregister_calls(&mut self, module: ModuleId, calls: &Calls) {
        for key in &calls.targets {
            let entry = self.entries.get_mut(key).expect("registered call target");
            entry.callers.remove(&module);
            if entry.target.slot().is_none() && entry.callers.is_empty() {
                self.entries.remove(key);
            }
        }
    }

    /// An interpreter binding suppresses profiling for its own CS:IP. Preserve newer bindings.
    pub(super) fn bind_interpreter_if_vacant(
        &mut self,
        wasm: &mut WasmRuntime,
        entries: impl IntoIterator<Item = FarPointer>,
    ) -> Result<()> {
        let updates = entries
            .into_iter()
            .map(FarPointer::packed)
            .filter(|&key| !self.contains(key))
            .map(|key| (key, Target::Interpreter))
            .collect();
        self.publish(wasm, updates)
    }

    pub(in crate::runtime::instance) fn install_interpreter(
        &mut self,
        wasm: &mut WasmRuntime,
        instance: &<WasmRuntime as WasmBackend>::Instance,
    ) -> Result<()> {
        let function = wasm.function(instance, abi::RUN)?;
        let function = wasm.import_function(&function);
        self.functions
            .set(wasm, EntryTable::INTERPRETER_SLOT, &function)?;
        self.interpreter = Some(function);
        Ok(())
    }

    pub(super) fn register_exports(
        &mut self,
        wasm: &mut WasmRuntime,
        module: ModuleId,
        instance: &<WasmRuntime as WasmBackend>::Instance,
        entries: &[DispatchEntry<'_>],
    ) -> Result<Vec<u32>> {
        // Only the last export at each key needs to exist in the Wasm instance.
        let mut exports = BTreeMap::new();
        for entry in entries {
            exports.insert(entry.key, entry.export);
        }
        let mut updates = BTreeMap::new();
        let mut next_slot = self.next_slot;
        let mut free_slots = self.free_slots.clone();
        for (&key, &export) in &exports {
            let function = wasm.function(
                instance,
                Function::<abi::ExecutionSignature>::new("", export),
            )?;
            let slot = self
                .entries
                .get(&key)
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
                key,
                Target::Export {
                    module,
                    slot,
                    function: wasm.import_function(&function),
                },
            );
        }
        self.publish(wasm, updates)?;
        self.free_slots = free_slots;
        Ok(exports.into_keys().collect())
    }

    /// Only withdraw exports still owned by the retiring installation.
    pub(super) fn remove(
        &mut self,
        context: &mut Context<'_>,
        exports: impl IntoIterator<Item = (ModuleId, u32)>,
    ) -> Result<()> {
        let updates: BTreeMap<_, _> = exports
            .into_iter()
            .filter(|&(module, key)| self.owner(key) == Some(module))
            .map(|(_, key)| (key, Target::Unresolved))
            .collect();
        if updates.is_empty() {
            return Ok(());
        }
        let slots: Vec<_> = updates
            .keys()
            .map(|key| self.entries[key].target.slot().expect("published export"))
            .collect();
        // Withdrawal only shrinks the lookup and reuses existing function slots.
        let bytes = self.lookup_after(&updates).to_bytes();
        self.apply(context, updates, &bytes)?;
        self.free_slots.extend(slots);
        Ok(())
    }

    /// Publish binding changes and update their callers in the same operation.
    /// Unresolved entries remain registered while they have incoming calls.
    fn publish(&mut self, wasm: &mut WasmRuntime, updates: BTreeMap<u32, Target>) -> Result<()> {
        if updates.is_empty() {
            return Ok(());
        }
        let bytes = self.lookup_after(&updates).to_bytes();
        let next_slot = self.reserve(wasm, &updates, bytes.len())?;
        self.apply(&mut wasm.context(), updates, &bytes)?;
        self.next_slot = next_slot;
        Ok(())
    }

    fn apply(
        &mut self,
        context: &mut Context<'_>,
        updates: BTreeMap<u32, Target>,
        lookup: &[u8],
    ) -> Result<()> {
        for (key, target) in updates {
            self.bind(context, key, target)?;
        }
        context.memory(&self.memory).write(0, lookup);
        Ok(())
    }

    fn lookup_after(&self, updates: &BTreeMap<u32, Target>) -> EntryTable {
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
        wasm: &mut WasmRuntime,
        updates: &BTreeMap<u32, Target>,
        lookup_bytes: usize,
    ) -> Result<u32> {
        let next_slot = updates
            .values()
            .filter_map(Target::slot)
            .fold(self.next_slot, |next, slot| next.max(slot + 1));
        if next_slot > self.capacity {
            let capacity = next_slot.max(self.capacity.saturating_mul(2));
            self.functions.ensure_size(wasm, capacity, None)?;
            self.capacity = capacity;
        }
        wasm.ensure_memory_size(&self.memory, lookup_bytes as u64)?;
        Ok(next_slot)
    }

    /// Update the function-table slot and registered callers for one entry.
    fn bind(&mut self, context: &mut Context<'_>, key: u32, target: Target) -> Result<()> {
        let entry = self.entries.entry(key).or_insert_with(Entry::unresolved);
        if let Target::Export { slot, function, .. } = &target {
            self.functions.set(context, *slot, function)?;
        } else if let Target::Export { slot, .. } = &entry.target {
            self.functions.set(
                context,
                *slot,
                self.interpreter.as_ref().expect("installed interpreter"),
            )?;
        }
        for caller in entry.callers.values() {
            caller.bind(context, target.function(self.interpreter.as_ref()))?;
        }
        entry.target = target;
        if entry.target.slot().is_none() && entry.callers.is_empty() {
            self.entries.remove(&key);
        }
        Ok(())
    }
}
