//! Module instantiation, shared imports and callable-table installation.

use super::*;
use wasmtime::{Caller, MemoryType, Ref, RefType, TableType};

impl<D: Devices + 'static> WasmtimeInstance<D> {
    pub(crate) fn new(
        engine: &Engine,
        module: &PreparedModule,
        dispatcher: &PreparedModule,
        imports: Imports<D>,
    ) -> wasmtime::Result<Self> {
        let mut store = Store::new(engine, imports);
        let cpu = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        let guest = Memory::new(&mut store, MemoryType::new(1, None))?;
        let physical = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        let budget = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        let entries = Memory::new(&mut store, MemoryType::new(1, None))?;
        let functions = Table::new(
            &mut store,
            TableType::new(RefType::FUNCREF, 2, None),
            Ref::Func(None),
        )?;
        let mut linker = Linker::new(engine);
        linker.define(&store, "wasm86", "executionFunctions", functions)?;
        linker.define(&store, "wasm86", "cpuState", cpu)?;
        linker.define(&store, "wasm86", "guest", guest)?;
        linker.define(&store, "wasm86", "physicalMap", physical)?;
        linker.define(&store, "wasm86", "executionBudget", budget)?;
        linker.define(&store, "wasm86", "executionEntries", entries)?;
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
        let dispatch_instance = linker.instantiate(&mut store, &dispatcher.module)?;
        let entry = dispatch_instance.get_typed_func(&mut store, &dispatcher.entry)?;
        functions.set(&mut store, 0, Ref::Func(Some(*entry.func())))?;
        for name in ["dispatch", "interpret"] {
            let function = dispatch_instance
                .get_func(&mut store, name)
                .expect("dispatcher export exists");
            linker.define(&store, "wasm86", name, function)?;
        }
        let instance = linker.instantiate(&mut store, &module.module)?;
        let interpreter = instance
            .get_func(&mut store, &module.entry)
            .expect("compiled interpreter exports its entry");
        functions.set(&mut store, 1, Ref::Func(Some(interpreter)))?;
        Ok(Self {
            store,
            cpu,
            guest,
            physical,
            budget,
            entries,
            functions,
            linker,
            entry,
        })
    }
}

impl<D: Devices + 'static> WasmtimeInstance<D> {
    pub(super) fn install(
        &mut self,
        module: &PreparedModule,
        first_slot: u32,
        exports: &[String],
        callbacks: Vec<HostCallback<D>>,
    ) -> Result<()> {
        let mut linker = self.linker.clone();
        let cpu = self.cpu;
        let guest = self.guest;
        let physical = self.physical;
        for (index, callback) in callbacks.into_iter().enumerate() {
            let binding = self.store.data_mut().bind_host_call(callback);
            linker
                .func_wrap(
                    "hooks",
                    &format!("entry_{index}"),
                    move |mut caller: Caller<'_, Imports<D>>| {
                        let mut state = CpuState::from_bytes(
                            cpu.data(&caller)[..CpuState::BYTE_LEN].try_into().unwrap(),
                        );
                        let action =
                            memory::access(&mut caller, guest, physical, |imports, memory| {
                                imports.host_call(&mut state, memory, binding)
                            });
                        cpu.write(&mut caller, 0, &state.to_bytes())
                            .expect("CPU backing is valid");
                        action
                    },
                )
                .map_err(|error| Error::backend("bind module import", error))?;
        }
        let instance = linker
            .instantiate(&mut self.store, &module.module)
            .map_err(|error| Error::backend("instantiate callable module", error))?;
        let required = u64::from(first_slot) + exports.len() as u64;
        let current = self.functions.size(&self.store);
        if required > current {
            self.functions
                .grow(&mut self.store, required - current, Ref::Func(None))
                .map_err(|error| Error::backend("grow execution functions", error))?;
        }
        for (index, name) in exports.iter().enumerate() {
            let function = instance
                .get_typed_func::<(), i64>(&mut self.store, name)
                .map_err(|error| Error::backend("read callable export", error))?;
            self.functions
                .set(
                    &mut self.store,
                    u64::from(first_slot) + index as u64,
                    Ref::Func(Some(*function.func())),
                )
                .map_err(|error| Error::backend("install execution function", error))?;
        }
        Ok(())
    }
}
