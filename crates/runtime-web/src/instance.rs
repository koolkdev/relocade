use crate::{
    imports::{BoundImports, HostImports},
    js_error, memory,
    memory::MemoryAccess,
    set_property, PreparedModule,
};
use js_sys::{Function, Object, Reflect, WebAssembly};
use std::{cell::RefCell, rc::Rc};
use wasm86_machine::Environment;
use wasm86_runtime::{
    backend::{EntryRequest, EntryResult, GuestMemory, HostCallback, Imports, Instance},
    Result,
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
    host_imports: Vec<HostImports>,
    functions: WebAssembly::Table,
}

impl<D: Environment + 'static> WebInstance<D> {
    pub(crate) async fn new(
        module: &PreparedModule,
        dispatcher: &PreparedModule,
        imports: Imports<D>,
    ) -> Result<Self> {
        let mapping_pages = (imports.memory_map().metadata_len() as u64).div_ceil(65536) as u32;
        let state = Rc::new(RefCell::new(imports));
        let cpu = memory::new(1, 1)?;
        let memory = MemoryAccess {
            guest: memory::new(1, 65536)?,
            mapping: memory::new(mapping_pages, mapping_pages)?,
        };
        let budget = memory::new(1, 1)?;
        let entries = memory::new(1, 65536)?;
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
            host_imports: Vec::new(),
            functions,
        })
    }
}

impl<D: Environment + 'static> Instance<D> for WebInstance<D> {
    type Module = PreparedModule;

    async fn install_module(
        &mut self,
        module: &PreparedModule,
        first_slot: u32,
        exports: &[String],
        callbacks: Vec<HostCallback<D>>,
    ) -> Result<()> {
        let imports = HostImports::new(
            &self.imports.object,
            &self.state,
            &self.cpu,
            &self.memory,
            callbacks,
        );
        let instance = instantiate(module, &imports.object).await?;
        // Once functions enter the table, every referenced closure must stay alive.
        self.host_imports.push(imports);
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
        let read_count = || {
            let mut bytes = [0; 8];
            memory::read(&self.cpu, counter, &mut bytes);
            u64::from_le_bytes(bytes)
        };
        let before = read_count();
        memory::write(&self.budget, 0, &request.to_bytes());
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
        let mut remaining_work = [0; 8];
        memory::read(&self.budget, 0, &mut remaining_work);
        Ok(EntryResult {
            word,
            remaining_work: i64::from_le_bytes(remaining_work),
            completed: read_count().wrapping_sub(before),
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

    fn restart_entry(&mut self) {
        memory::write(&self.entries, 0, &[0; EntryTable::OBSERVE_OFFSET as usize]);
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
