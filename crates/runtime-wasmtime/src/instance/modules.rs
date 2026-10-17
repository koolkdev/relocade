//! Module instantiation, shared imports and callable-table installation.

use super::*;
use wasmtime::{Caller, MemoryType, Ref, RefType, TableType};

impl<D: Environment + 'static> WasmtimeInstance<D> {
    pub(crate) fn new(
        engine: &Engine,
        module: &PreparedModule,
        dispatcher: &PreparedModule,
        imports: Imports<D>,
    ) -> wasmtime::Result<Self> {
        let mapping_name = imports.memory_map().import_name();
        let mapping_pages = (imports.memory_map().metadata_len() as u64).div_ceil(65536) as u32;
        let guest_pages = imports.memory_size().div_ceil(65536).max(1) as u32;
        let mut store = Store::new(engine, imports);
        let context = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        context.write(&mut store, 0, &ExecutionContext::default().to_bytes())?;
        let guest = Memory::new(&mut store, MemoryType::new(guest_pages, None))?;
        let mapping = Memory::new(
            &mut store,
            MemoryType::new(mapping_pages, Some(mapping_pages)),
        )?;
        let entries = Memory::new(&mut store, MemoryType::new(1, None))?;
        entries.write(&mut store, 0, &EntryTable::default().to_bytes())?;
        let functions = Table::new(
            &mut store,
            TableType::new(RefType::FUNCREF, 2, None),
            Ref::Func(None),
        )?;
        let mut linker = Linker::new(engine);
        linker.define(&store, "wasm86", "executionFunctions", functions)?;
        linker.define(&store, "wasm86", "executionContext", context)?;
        linker.define(&store, "wasm86", "guest", guest)?;
        linker.define(&store, "wasm86", mapping_name, mapping)?;
        linker.define(&store, "wasm86", "executionEntries", entries)?;
        linker.func_wrap(
            "wasm86",
            "readPort",
            move |mut caller: Caller<'_, Imports<D>>, port: u32, bytes: u32| {
                memory::access(&mut caller, guest, mapping, |imports, memory| {
                    imports.read_port(memory, port as u16, bytes as u8)
                })
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "writePort",
            move |mut caller: Caller<'_, Imports<D>>, port: u32, bytes: u32, value: u32| {
                memory::access(&mut caller, guest, mapping, |imports, memory| {
                    imports.write_port(memory, port as u16, bytes as u8, value)
                })
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "readMmio",
            move |mut caller: Caller<'_, Imports<D>>, address: u32, bytes: u32| {
                memory::access(&mut caller, guest, mapping, |imports, memory| {
                    imports.read_mmio(memory, address, bytes as u8)
                })
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "writeMmio",
            move |mut caller: Caller<'_, Imports<D>>, address: u32, bytes: u32, value: u64| {
                memory::access(&mut caller, guest, mapping, |imports, memory| {
                    imports.write_mmio(memory, address, bytes as u8, value)
                })
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "resolveSegment",
            |mut caller: Caller<'_, Imports<D>>, segment: u32, selector: u32| {
                wasm86_runtime::backend::resolve_segment(caller.data_mut(), segment, selector)
            },
        )?;
        linker.func_wrap(
            "wasm86",
            "querySegmentDescriptor",
            |mut caller: Caller<'_, Imports<D>>, selector: u32| {
                wasm86_runtime::backend::query_segment_descriptor(caller.data_mut(), selector)
            },
        )?;
        let dispatch_instance = linker.instantiate(&mut store, &dispatcher.module)?;
        let entry = dispatch_instance.get_typed_func(&mut store, &dispatcher.entry)?;
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
            context,
            guest,
            mapping,
            entries,
            functions,
            linker,
            entry,
        })
    }
}

impl<D: Environment + 'static> WasmtimeInstance<D> {
    pub(super) fn install(
        &mut self,
        module: &PreparedModule,
        first_slot: u32,
        exports: &[String],
    ) -> Result<()> {
        let instance = self
            .linker
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
