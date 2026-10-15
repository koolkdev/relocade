use crate::{
    imports::BoundImports, js_error, memory, memory::MemoryAccess, set_property, PreparedModule,
};
use js_sys::{Function, Object, Reflect, WebAssembly};
use std::{cell::RefCell, mem::offset_of, rc::Rc};
use wasm86_machine::Environment;
use wasm86_runtime::{
    backend::{EntryRequest, GuestMemory, Imports, Instance},
    Result,
};
use wasm86_x86::{CpuState, EntryTable, ExecutionContext};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Per-machine JavaScript memories, entry and retained synchronous callbacks.
pub struct WebInstance<D> {
    state: Rc<RefCell<Imports<D>>>,
    context: WebAssembly::Memory,
    entries: WebAssembly::Memory,
    memory: MemoryAccess,
    entry: Function,
    imports: BoundImports,
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
        let context = memory::new(1, 1)?;
        memory::write(&context, 0, &ExecutionContext::default().to_bytes());
        let memory = MemoryAccess {
            guest: memory::new(1, 65536)?,
            mapping: memory::new(mapping_pages, mapping_pages)?,
        };
        let entries = memory::new(1, 65536)?;
        let descriptor = Object::new();
        set_property(&descriptor, "element", &"anyfunc".into());
        set_property(&descriptor, "initial", &2.into());
        let functions = WebAssembly::Table::new(&descriptor)
            .map_err(|error| js_error("create execution functions", error))?;
        let imports = BoundImports::new(&state, &context, &entries, &memory, &functions);
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
            context,
            entries,
            memory,
            entry,
            imports,
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

    fn enter(&mut self, request: EntryRequest) -> Result<u64> {
        memory::write(
            &self.context,
            offset_of!(ExecutionContext, instruction_target) as u32,
            &request.instruction_target.to_le_bytes(),
        );
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
        i64::try_from(word)
            .map(|word| word as u64)
            .map_err(|error| js_error("read Wasm exit", error))
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
        memory::read(
            &self.context,
            offset_of!(ExecutionContext, cpu) as u32,
            &mut bytes,
        );
        CpuState::from_bytes(bytes)
    }

    fn set_cpu(&mut self, cpu: &CpuState) {
        memory::write(
            &self.context,
            offset_of!(ExecutionContext, cpu) as u32,
            &cpu.to_bytes(),
        );
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
