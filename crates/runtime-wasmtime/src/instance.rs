use crate::{memory, PreparedModule};
use wasm86_runtime::{
    backend::{EntryResult, GuestMemory, Imports, Instance},
    Devices, Error, Result,
};
use wasm86_x86::CpuState;
use wasmtime::{Caller, Engine, Linker, Memory, MemoryType, Store, TypedFunc};

/// Per-machine Wasmtime store and execution handles.
pub struct WasmtimeInstance<D> {
    store: Store<Imports<D>>,
    cpu: Memory,
    guest: Memory,
    physical: Memory,
    budget: Memory,
    entry: TypedFunc<(), i64>,
}

impl<D: Devices + 'static> WasmtimeInstance<D> {
    pub(crate) fn new(
        engine: &Engine,
        module: &PreparedModule,
        imports: Imports<D>,
    ) -> wasmtime::Result<Self> {
        let mut store = Store::new(engine, imports);
        let cpu = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        let guest = Memory::new(&mut store, MemoryType::new(1, None))?;
        let physical = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        let budget = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        let mut linker = Linker::new(engine);
        linker.define(&store, "wasm86", "cpuState", cpu)?;
        linker.define(&store, "wasm86", "guest", guest)?;
        linker.define(&store, "wasm86", "physicalMap", physical)?;
        linker.define(&store, "wasm86", "executionBudget", budget)?;
        linker.func_wrap(
            "wasm86",
            "dispatch",
            |mut caller: Caller<'_, Imports<D>>, _: u32| caller.data_mut().dispatch(),
        )?;
        linker.func_wrap(
            "wasm86",
            "readPort",
            move |mut caller: Caller<'_, Imports<D>>, port: u32, bytes: u32| {
                memory::access(&mut caller, guest, physical, |imports, memory| {
                    imports.read_port(memory, port as u16, bytes as u8)
                })
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "writePort",
            move |mut caller: Caller<'_, Imports<D>>, port: u32, bytes: u32, value: u32| {
                memory::access(&mut caller, guest, physical, |imports, memory| {
                    imports.write_port(memory, port as u16, bytes as u8, value)
                })
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "readMmio",
            move |mut caller: Caller<'_, Imports<D>>, address: u32, bytes: u32| {
                memory::access(&mut caller, guest, physical, |imports, memory| {
                    imports.read_mmio(memory, address, bytes as u8)
                })
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "writeMmio",
            move |mut caller: Caller<'_, Imports<D>>, address: u32, bytes: u32, value: u64| {
                memory::access(&mut caller, guest, physical, |imports, memory| {
                    imports.write_mmio(memory, address, bytes as u8, value)
                })
            },
        )?;
        let instance = linker.instantiate(&mut store, &module.module)?;
        let entry = instance.get_typed_func(&mut store, &module.entry)?;
        Ok(Self {
            store,
            cpu,
            guest,
            physical,
            budget,
            entry,
        })
    }
}

impl<D: Devices> Instance<D> for WasmtimeInstance<D> {
    fn enter(&mut self, work: u32) -> Result<EntryResult> {
        let counter = std::mem::offset_of!(CpuState, instruction_count);
        let read_u32 = |memory: Memory, store: &Store<Imports<D>>, offset: usize| {
            u32::from_le_bytes(memory.data(store)[offset..offset + 4].try_into().unwrap())
        };
        let before = read_u32(self.cpu, &self.store, counter);
        self.budget.data_mut(&mut self.store)[..4].copy_from_slice(&work.to_le_bytes());
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
