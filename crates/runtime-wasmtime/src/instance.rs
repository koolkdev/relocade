use crate::{memory, PreparedModule};
use wasm86_runtime::{
    backend::{EntryRequest, EntryResult, GuestMemory, HostCallback, Imports, Instance},
    Devices, Error, Result,
};
use wasm86_x86::{CpuState, EntryTable};
use wasmtime::{Engine, Linker, Memory, Store, Table, TypedFunc};

mod modules;

/// Per-machine Wasmtime store and execution handles.
pub struct WasmtimeInstance<D> {
    store: Store<Imports<D>>,
    cpu: Memory,
    guest: Memory,
    physical: Memory,
    budget: Memory,
    entries: Memory,
    functions: Table,
    linker: Linker<Imports<D>>,
    entry: TypedFunc<(), i64>,
}

impl<D: Devices + 'static> Instance<D> for WasmtimeInstance<D> {
    type Module = PreparedModule;

    async fn install_module(
        &mut self,
        module: &PreparedModule,
        first_slot: u32,
        exports: &[String],
        callbacks: Vec<HostCallback<D>>,
    ) -> Result<()> {
        self.install(module, first_slot, exports, callbacks)
    }

    fn enter(&mut self, request: EntryRequest) -> Result<EntryResult> {
        let counter = std::mem::offset_of!(CpuState, instruction_count);
        let read_u32 = |memory: Memory, store: &Store<Imports<D>>, offset: usize| {
            u32::from_le_bytes(memory.data(store)[offset..offset + 4].try_into().unwrap())
        };
        let before = read_u32(self.cpu, &self.store, counter);
        self.budget.data_mut(&mut self.store)[..12].copy_from_slice(&request.to_bytes(before));
        self.entries
            .write(
                &mut self.store,
                EntryTable::OBSERVE_OFFSET as usize,
                &u32::from(request.observe).to_le_bytes(),
            )
            .expect("dispatch state has valid backing");
        let word = self
            .entry
            .call(&mut self.store, ())
            .map_err(|error| Error::backend("execute Wasm", error))? as u64;
        Ok(EntryResult {
            word,
            remaining_work: read_u32(self.budget, &self.store, 0),
            completed: read_u32(self.cpu, &self.store, counter).wrapping_sub(before),
        })
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
    fn restart_entry(&mut self) {
        self.entries.data_mut(&mut self.store)[..24].fill(0);
    }
    fn cpu(&self) -> CpuState {
        CpuState::from_bytes(
            self.cpu.data(&self.store)[..CpuState::BYTE_LEN]
                .try_into()
                .unwrap(),
        )
    }
    fn set_cpu(&mut self, cpu: &CpuState) {
        self.cpu
            .write(&mut self.store, 0, &cpu.to_bytes())
            .expect("CPU image has fixed valid backing");
    }
    fn inspect<R>(&self, inspect: impl FnOnce(&Imports<D>) -> R) -> R {
        inspect(self.store.data())
    }
    fn with_memory<R>(
        &mut self,
        access: impl FnOnce(&mut Imports<D>, &mut dyn GuestMemory) -> R,
    ) -> R {
        memory::access(&mut self.store, self.guest, self.physical, access)
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
