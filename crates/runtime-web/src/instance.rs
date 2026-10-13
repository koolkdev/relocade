use crate::{
    imports::BoundImports, js_error, memory, memory::MemoryAccess, set_property, PreparedModule,
};
use js_sys::{Function, Object, Reflect, WebAssembly};
use std::{cell::RefCell, rc::Rc};
use wasm86_runtime::{
    backend::{EntryRequest, EntryResult, GuestMemory, Imports, Instance},
    Devices, Result,
};
use wasm86_x86::{CpuState, EntryTable};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Per-machine JavaScript memories, entry and retained synchronous callbacks.
pub struct WebInstance<D> {
    state: Rc<RefCell<Imports<D>>>,
    cpu: WebAssembly::Memory,
    budget: WebAssembly::Memory,
    entries: WebAssembly::Memory,
    memory: MemoryAccess,
    entry: Function,
    imports: BoundImports,
    functions: WebAssembly::Table,
}

impl<D: Devices + 'static> WebInstance<D> {
    pub(crate) async fn new(
        module: &PreparedModule,
        dispatcher: &PreparedModule,
        imports: Imports<D>,
    ) -> Result<Self> {
        let state = Rc::new(RefCell::new(imports));
        let cpu = memory::new(1)?;
        let memory = MemoryAccess {
            guest: memory::new(65536)?,
            physical: memory::new(1)?,
        };
        let budget = memory::new(1)?;
        let entries = memory::new(65536)?;
        let descriptor = Object::new();
        set_property(&descriptor, "element", &"anyfunc".into());
        set_property(&descriptor, "initial", &2.into());
        let functions = WebAssembly::Table::new(&descriptor)
            .map_err(|error| js_error("create execution functions", error))?;
        let imports = BoundImports::new(&state, &cpu, &budget, &entries, &memory, &functions);
        let dispatcher_instance = instantiate(dispatcher, &imports.object).await?;
        let entry = export(&dispatcher_instance, &dispatcher.entry)?;
        functions
            .set(0, &entry)
            .map_err(|error| js_error("install dispatcher", error))?;
        imports.bind_dispatcher(&dispatcher_instance.exports());
        let interpreter = instantiate(module, &imports.object).await?;
        let interpreter = export(&interpreter, &module.entry)?;
        functions
            .set(1, &interpreter)
            .map_err(|error| js_error("install interpreter", error))?;
        Ok(Self {
            state,
            cpu,
            budget,
            entries,
            memory,
            entry,
            imports,
            functions,
        })
    }
}

impl<D: Devices + 'static> Instance<D> for WebInstance<D> {
    type Module = PreparedModule;

    async fn install_module(
        &mut self,
        module: &PreparedModule,
        first_slot: u32,
        exports: &[String],
    ) -> Result<()> {
        let instance = instantiate(module, &self.imports.object).await?;
        let required = first_slot + exports.len() as u32;
        let current = self.functions.length();
        if required > current {
            self.functions
                .grow(required - current)
                .map_err(|error| js_error("grow execution functions", error))?;
        }
        for (index, name) in exports.iter().enumerate() {
            let function = export(&instance, name)?;
            self.functions
                .set(first_slot + index as u32, &function)
                .map_err(|error| js_error("install execution function", error))?;
        }
        Ok(())
    }

    fn enter(&mut self, request: EntryRequest) -> Result<EntryResult> {
        let counter = std::mem::offset_of!(CpuState, instruction_count) as u32;
        let read_u32 = |memory: &WebAssembly::Memory, offset: u32| {
            let mut bytes = [0; 4];
            memory::read(memory, offset, &mut bytes);
            u32::from_le_bytes(bytes)
        };
        let before = read_u32(&self.cpu, counter);
        memory::write(&self.budget, 0, &request.to_bytes(before));
        memory::write(
            &self.entries,
            EntryTable::OBSERVE_OFFSET,
            &u32::from(request.observe).to_le_bytes(),
        );
        // No borrow of state crosses the entry: imports borrow it synchronously.
        let word = self
            .entry
            .call0(&JsValue::UNDEFINED)
            .map_err(|error| js_error("execute Wasm", error))?;
        // Wasm i64 values cross the JS boundary as BigInt, never Number.
        let word = i64::try_from(word).map_err(|error| js_error("read Wasm exit", error))? as u64;
        Ok(EntryResult {
            word,
            remaining_work: read_u32(&self.budget, 0),
            completed: read_u32(&self.cpu, counter).wrapping_sub(before),
        })
    }

    fn set_entries(&mut self, entries: &EntryTable) -> Result<()> {
        let bytes = entries.to_bytes();
        memory::grow(
            &self.entries,
            u64::from(EntryTable::DATA_OFFSET) + bytes.len() as u64,
        )?;
        memory::write(&self.entries, EntryTable::DATA_OFFSET, &bytes);
        Ok(())
    }

    fn cpu(&self) -> CpuState {
        let mut bytes = [0; CpuState::BYTE_LEN];
        memory::read(&self.cpu, 0, &mut bytes);
        CpuState::from_bytes(bytes)
    }

    fn set_cpu(&mut self, cpu: &CpuState) {
        memory::write(&self.cpu, 0, &cpu.to_bytes());
    }

    fn inspect<R>(&self, inspect: impl FnOnce(&Imports<D>) -> R) -> R {
        inspect(&self.state.borrow())
    }

    fn with_memory<R>(
        &mut self,
        access: impl FnOnce(&mut Imports<D>, &mut dyn GuestMemory) -> R,
    ) -> R {
        access(&mut self.state.borrow_mut(), &mut self.memory)
    }

    fn grow_guest(&mut self, bytes: u64) -> Result<()> {
        memory::grow(&self.memory.guest, bytes)
    }
}

async fn instantiate(module: &PreparedModule, imports: &Object) -> Result<WebAssembly::Instance> {
    JsFuture::from(WebAssembly::instantiate_module(&module.module, imports))
        .await
        .map_err(|error| js_error("instantiate Wasm", error))?
        .dyn_into()
        .map_err(|error| js_error("read Wasm instance", error))
}

fn export(instance: &WebAssembly::Instance, name: &str) -> Result<Function> {
    Reflect::get(&instance.exports(), &name.into())
        .map_err(|error| js_error("read callable export", error))?
        .dyn_into()
        .map_err(|error| js_error("read callable export", error))
}
