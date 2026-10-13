use crate::{imports::BoundImports, js_error, memory, memory::MemoryAccess, PreparedModule};
use js_sys::{Function, Reflect, WebAssembly};
use std::{cell::RefCell, rc::Rc};
use wasm86_runtime::{
    backend::{EntryResult, GuestMemory, Imports, Instance},
    Devices, Result,
};
use wasm86_x86::CpuState;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

/// Per-machine JavaScript memories, entry and retained synchronous callbacks.
pub struct WebInstance<D> {
    state: Rc<RefCell<Imports<D>>>,
    cpu: WebAssembly::Memory,
    budget: WebAssembly::Memory,
    memory: MemoryAccess,
    entry: Function,
    _imports: BoundImports,
}

impl<D: Devices + 'static> WebInstance<D> {
    pub(crate) async fn new(module: &PreparedModule, imports: Imports<D>) -> Result<Self> {
        let state = Rc::new(RefCell::new(imports));
        let cpu = memory::new(1)?;
        let memory = MemoryAccess {
            guest: memory::new(65536)?,
            physical: memory::new(1)?,
        };
        let budget = memory::new(1)?;
        let imports = BoundImports::new(&state, &cpu, &budget, &memory);
        let instance: WebAssembly::Instance = JsFuture::from(WebAssembly::instantiate_module(
            &module.module,
            &imports.object,
        ))
        .await
        .map_err(|error| js_error("instantiate Wasm", error))?
        .dyn_into()
        .map_err(|error| js_error("read Wasm instance", error))?;
        let entry = Reflect::get(&instance.exports(), &module.entry.clone().into())
            .map_err(|error| js_error("read Wasm entry", error))?
            .dyn_into()
            .map_err(|error| js_error("read Wasm entry", error))?;
        Ok(Self {
            state,
            cpu,
            budget,
            memory,
            entry,
            _imports: imports,
        })
    }
}

impl<D: Devices> Instance<D> for WebInstance<D> {
    fn enter(&mut self, work: u32) -> Result<EntryResult> {
        let counter = std::mem::offset_of!(CpuState, instruction_count) as u32;
        let read_u32 = |memory: &WebAssembly::Memory, offset: u32| {
            let mut bytes = [0; 4];
            memory::read(memory, offset, &mut bytes);
            u32::from_le_bytes(bytes)
        };
        let before = read_u32(&self.cpu, counter);
        memory::write(&self.budget, 0, &work.to_le_bytes());
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
        self.memory.grow(bytes)
    }
}
