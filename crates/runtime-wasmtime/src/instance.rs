use crate::{memory, PreparedModule};
use std::mem::offset_of;
use wasm86_machine::Environment;
use wasm86_runtime::{
    backend::{EntryRequest, GuestMemory, Imports, Instance},
    Error, Result,
};
use wasm86_x86::{CpuState, EntryTable, ExecutionContext};
use wasmtime::{Engine, Linker, Memory, Store, Table, TypedFunc};

mod modules;

/// Per-machine Wasmtime store and execution handles.
pub struct WasmtimeInstance<D> {
    store: Store<Imports<D>>,
    context: Memory,
    guest: Memory,
    mapping: Memory,
    entries: Memory,
    functions: Table,
    linker: Linker<Imports<D>>,
    entry: TypedFunc<(), i64>,
}

impl<D: Environment + 'static> Instance<D> for WasmtimeInstance<D> {
    type Module = PreparedModule;

    async fn install_module(
        &mut self,
        module: &PreparedModule,
        first_slot: u32,
        exports: &[String],
    ) -> Result<()> {
        self.install(module, first_slot, exports)
    }

    fn enter(&mut self, request: EntryRequest) -> Result<u64> {
        self.context
            .write(
                &mut self.store,
                offset_of!(ExecutionContext, instruction_target),
                &request.instruction_target.to_le_bytes(),
            )
            .expect("instruction target has fixed backing");
        self.entries
            .write(
                &mut self.store,
                EntryTable::OBSERVE_OFFSET as usize,
                &u32::from(request.observe).to_le_bytes(),
            )
            .expect("dispatch state has valid backing");
        self.entry
            .call(&mut self.store, ())
            .map(|word| word as u64)
            .map_err(|error| Error::backend("execute Wasm", error))
    }
    fn set_entries(&mut self, entries: &EntryTable) -> Result<()> {
        let bytes = entries.to_bytes();
        let pages = (u64::from(EntryTable::DATA_OFFSET) + bytes.len() as u64).div_ceil(65536);
        let current = self.entries.size(&self.store);
        if pages > current {
            self.entries
                .grow(&mut self.store, pages - current)
                .map_err(|error| Error::backend("grow execution entries", error))?;
        }
        self.entries
            .write(&mut self.store, EntryTable::DATA_OFFSET as usize, &bytes)
            .expect("entry backing was grown before publication");
        Ok(())
    }
    fn cpu(&self) -> CpuState {
        let offset = offset_of!(ExecutionContext, cpu);
        CpuState::from_bytes(
            self.context.data(&self.store)[offset..offset + CpuState::BYTE_LEN]
                .try_into()
                .unwrap(),
        )
    }
    fn set_cpu(&mut self, cpu: &CpuState) {
        self.context
            .write(
                &mut self.store,
                offset_of!(ExecutionContext, cpu),
                &cpu.to_bytes(),
            )
            .expect("CPU image has fixed valid backing");
    }
    fn inspect<R>(&self, inspect: impl FnOnce(&Imports<D>) -> R) -> R {
        inspect(self.store.data())
    }
    fn with_memory<R>(
        &mut self,
        access: impl FnOnce(&mut Imports<D>, &mut dyn GuestMemory) -> R,
    ) -> R {
        memory::access(&mut self.store, self.guest, self.mapping, access)
    }
    fn grow_guest(&mut self, bytes: u64) -> Result<()> {
        let pages = bytes.div_ceil(65536);
        let current = self.guest.size(&self.store);
        if pages > current {
            self.guest
                .grow(&mut self.store, pages - current)
                .map_err(|error| Error::backend("grow guest memory", error))?;
        }
        Ok(())
    }
}
