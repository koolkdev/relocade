use super::{code::CodeRegistry, Backend, DispatchEntry, RuntimeMemory};
use crate::Result;
use relocade_compiler::abi::Function as FunctionDeclaration;
use relocade_wasm_runtime::{Function, Import, Memory, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, EntryTable};
use std::sync::{Arc, Mutex};

/// Owns callable slots; callbacks can retire their address registrations.
pub(super) struct DispatchTable {
    memory: Memory,
    functions: <Backend as WasmBackend>::Table,
    pub(super) code: Arc<Mutex<CodeRegistry>>,
    next_slot: u32,
    capacity: u32,
}

impl DispatchTable {
    pub(super) fn new(backend: &mut Backend, bindings: &RuntimeMemory) -> Result<Self> {
        let memory = backend.create_memory(abi::EXECUTION_ENTRIES)?;
        backend
            .memory(&memory)
            .write(0, &EntryTable::default().to_bytes());
        let code = CodeRegistry::new(
            bindings.guest.code.clone(),
            bindings.mapping.clone(),
            memory.clone(),
            bindings.guest.configuration().memory_map().clone(),
        );
        Ok(Self {
            memory,
            functions: backend.create_table(abi::EXECUTION_FUNCTIONS.minimum)?,
            code: Arc::new(Mutex::new(code)),
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
            capacity: abi::EXECUTION_FUNCTIONS.minimum,
        })
    }

    pub(super) fn imports<'a>(&'a self, backend: &Backend) -> [Import<'a, Backend>; 2] {
        [
            backend.bind(abi::EXECUTION_ENTRIES, &self.memory),
            backend.bind(abi::EXECUTION_FUNCTIONS, &self.functions),
        ]
    }

    pub(super) fn install_interpreter(
        &self,
        backend: &mut Backend,
        instance: &<Backend as WasmBackend>::Instance,
    ) -> Result<()> {
        let function = backend.function(instance, abi::RUN)?;
        backend.set_table_function(&self.functions, EntryTable::INTERPRETER_SLOT, &function)?;
        Ok(())
    }

    pub(super) fn sync(
        &self,
        backend: &mut Backend,
        interpret: &Function<Backend, abi::ExecutionSignature>,
    ) {
        let mut code = self.code.lock().unwrap();
        code.sync(backend);
        // Address removal happens synchronously in callbacks. Release function
        // references at the next backend boundary, when no guest call is active.
        for slot in code.retired.drain(..) {
            backend
                .set_table_function(&self.functions, slot, interpret)
                .expect("allocated callable slot");
        }
    }

    pub(super) fn install(
        &mut self,
        backend: &mut Backend,
        instance: &<Backend as WasmBackend>::Instance,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let mut code = self.code.lock().unwrap();
        // Resolve exports and reserve storage before changing live registrations.
        let mut addresses = code.addresses.clone();
        let mut next_slot = self.next_slot;
        let mut free_slots = code.free_slots.clone();
        let mut functions = Vec::with_capacity(entries.len());
        for entry in entries {
            let function = backend.function(
                instance,
                FunctionDeclaration::<abi::ExecutionSignature>::new("", entry.export),
            )?;
            let slot = addresses.get(entry.address).unwrap_or_else(|| {
                free_slots.pop().unwrap_or_else(|| {
                    let slot = next_slot;
                    next_slot = next_slot
                        .checked_add(1)
                        .expect("Wasm function table capacity");
                    slot
                })
            });
            addresses.insert(entry.address, slot);
            functions.push((entry.address, slot, function));
        }
        if next_slot > self.capacity {
            let capacity = next_slot.max(self.capacity.saturating_mul(2));
            backend.ensure_table_size(&self.functions, capacity)?;
            self.capacity = capacity;
        }
        backend.ensure_memory_size(&self.memory, addresses.to_bytes().len() as u64)?;
        code.free_slots = free_slots;
        code.replace(
            &entries
                .iter()
                .map(|entry| entry.address)
                .collect::<Vec<_>>(),
        );
        for (address, slot, function) in functions {
            backend.set_table_function(&self.functions, slot, &function)?;
            code.free_slots.retain(|free| *free != slot);
            code.retired.retain(|retired| *retired != slot);
            code.addresses.insert(address, slot);
        }
        code.publish(backend);
        self.next_slot = next_slot;
        Ok(())
    }
}
