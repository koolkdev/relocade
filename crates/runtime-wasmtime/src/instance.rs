use crate::memory;
use std::mem::offset_of;
use wasm86_machine::Environment;
use wasm86_runtime::{
    backend::{GuestMemory, Imports, Instance},
    Error, Result,
};
use wasm86_x86::{CpuState, ExecutionContext};
use wasmtime::{Engine, Memory, Store, TypedFunc};

mod modules;

/// Per-machine Wasmtime store and execution handles.
pub struct WasmtimeInstance<D> {
    store: Store<Imports<D>>,
    context: Memory,
    guest: Memory,
    mapping: Memory,
    entry: TypedFunc<(), i64>,
}

impl<D: Environment + 'static> Instance<D> for WasmtimeInstance<D> {
    fn run(&mut self, instruction_target: u64) -> Result<u64> {
        self.context
            .write(
                &mut self.store,
                offset_of!(ExecutionContext, instruction_target),
                &instruction_target.to_le_bytes(),
            )
            .expect("instruction target has fixed backing");
        self.entry
            .call(&mut self.store, ())
            .map(|word| word as u64)
            .map_err(|error| Error::backend("execute Wasm", error))
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
}
